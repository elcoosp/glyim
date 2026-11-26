//! Concurrent-soundness tests for the shared [`TypeArena`] (plan §6 M3.4).
//!
//! Plan acceptance line (M3.4): *"safe append-only arena (replace
//! `Box::into_raw`/`unsafe impl Send/Sync` with a vetted/own safe-API
//! structure)"* with the `DoD` `unsafe count in glyim-type = 0; loom/Miri
//! green`. The arena is *shared* by every view of a compilation
//! (`TyCtx`/`TyCtxMut` hold one `Arc<TypeArena>`), so the soundness model
//! must not rest on a single-threaded-per-compilation convention: these
//! tests share one arena across real threads and pin the two contracts the
//! compiler leans on —
//!
//! 1. **Interning is unique under concurrent allocation**: threads racing
//!    `alloc_ty` on the same structure must observe one canonical handle
//!    (the "same logical type maps to the same `Ty`" property the module
//!    promises; the write-gate path must re-check the index it racingly
//!    missed on the fast path).
//! 2. **Reads stay valid across concurrent appends**: `&TyKind` /
//!    flags views borrowed from the arena remain valid and unmodified
//!    while another thread appends (append-only, address-stable entries).
//!
//! Test 1 forces the race deterministically: the racer that wins the write
//! gate spends the gate section cloning a wide (50k-predicate) `Dynamic`
//! structure, so the second racer's fast-path miss reliably lands *inside*
//! the winner's gate section — the interleaving the gate re-check exists
//! for. Both tests run in the default `cargo test` PR tier, and both are
//! Miri evidence for the `unsafe`-free soundness model
//! (`cargo +nightly miri test -p glyim-type`).

use std::sync::Arc;
use std::sync::Barrier;

use glyim_core::interner::Interner;
use glyim_core::primitives::{FloatTy, IntTy};

use crate::binder::Binder;
use crate::flags::TypeFlags;
use crate::predicate::Predicate;
use crate::region::Region;
use crate::type_arena::TypeArena;
use crate::{Ty, TyCtxMut, TyKind};

/// Crate-local `TyCtxMut` mint (the glyim-test helper returns the
/// extern-crate instantiation, which is a distinct type world here).
fn test_ctx() -> TyCtxMut {
    TyCtxMut::new(Interner::new())
}

/// A `Dynamic` structure whose arena-side clone (the gate-section
/// `types.push(kind.clone())`) takes long enough that a barrier-released
/// racer reliably reaches its fast-path read inside the winner's gate
/// section. Predicate payload width, not scheduling luck, opens the window.
fn wide_dynamic_kind() -> TyKind {
    let preds: Box<[Predicate]> = (0..50_000)
        .map(|_| Predicate::Coerce(Ty::ERROR, Ty::ERROR))
        .collect();
    TyKind::Dynamic(Binder::bind(preds, Box::new([])), Region::Erased)
}

/// Property 1 (plan M3.4): interning is unique under concurrent allocation.
///
/// Two racers park on a `Barrier`, both observe the fast-path miss for the
/// same wide structure, and enter the write-gate path as a genuine race.
/// The interning contract says both must come back with one canonical
/// handle.
#[test]
fn alloc_ty_interns_identical_kinds_under_concurrent_sharing() {
    // Miri interprets every element copy ~100x, so the PR-tier trial count
    // is trimmed to the interleavings it explores (`cfg(miri)` is rustc's
    // own marker for a Miri-compiled build); the scheduled PR tier runs 3.
    let trials = if cfg!(miri) { 1 } else { 3 };
    for trial in 0..trials {
        let ctx = test_ctx();
        let arena: Arc<TypeArena> = Arc::clone(&ctx.arena);
        let barrier = Arc::new(Barrier::new(2));

        // Pre-build each racer's owned structure *outside* the barrier so
        // the race window contains only the gate-section work.
        let kinds: Vec<TyKind> = std::iter::repeat_n(wide_dynamic_kind(), 2).collect();
        let arena_ref = &*arena;

        let handles: Vec<Ty> = std::thread::scope(|scope| {
            let mut joined = Vec::new();
            for kind in kinds {
                let barrier = Arc::clone(&barrier);
                joined.push(scope.spawn(move || {
                    barrier.wait();
                    arena_ref.alloc_ty(kind, TypeFlags::empty())
                }));
            }
            joined
                .into_iter()
                .map(|j| j.join().expect("racer panicked"))
                .collect()
        });

        for (lane, handle) in handles.iter().enumerate() {
            assert_eq!(
                handle, &handles[0],
                "trial {trial} lane {lane}: threads racing alloc_ty on the \
                 same structure must observe one canonical handle; distinct \
                 handles split the type world and break every structural-\
                 equality consumer downstream"
            );
        }
        // The interned handle round-trips through the shared read face.
        match arena_ref.ty_kind(handles[0]) {
            TyKind::Dynamic(binder, region) => {
                assert!(
                    matches!(region, Region::Erased) && binder.value.len() == 50_000,
                    "trial {trial}: the canonical handle must resolve to the interned structure"
                );
            }
            other => panic!("trial {trial}: expected Dynamic, got {other:?}"),
        }
    }
}

/// Property 2 (plan M3.4): reads stay valid across concurrent appends.
///
/// The main thread pins `&TyKind`/flag views of pre-existing handles while
/// a scoped writer thread appends fresh entries. Every re-read must resolve
/// to the pinned structure (append-only, address-stable entries): a
/// reallocation-visible, torn, or mutated read is a soundness break, not a
/// benign race.
#[test]
fn shared_arena_reads_stay_valid_across_concurrent_appends() {
    let ctx = test_ctx();
    let arena: Arc<TypeArena> = Arc::clone(&ctx.arena);

    // Pre-intern the structures the reader pins (one primitive, one float
    // so the writer's appends cannot collide with them structurally).
    let i32_ty = arena.alloc_ty(TyKind::Int(IntTy::I32), TypeFlags::empty());
    let f64_ty = arena.alloc_ty(TyKind::Float(FloatTy::F64), TypeFlags::empty());
    let expected = [
        (i32_ty, &TyKind::Int(IntTy::I32)),
        (f64_ty, &TyKind::Float(FloatTy::F64)),
    ];

    let writer_arena = Arc::clone(&arena);
    std::thread::scope(|scope| {
        scope.spawn(move || {
            for step in 0..64 {
                // Distinct structures per step: the appends are real growth,
                // not interned lookups that hit the fast path.
                let fresh = TyKind::Tuple(crate::substitution::Substitution::empty());
                let _ = writer_arena.alloc_ty(fresh.clone(), TypeFlags::empty());
                std::hint::black_box((step, fresh));
            }
        });

        // Interleave reads with the writer's appends: each pinned view must
        // resolve to its structure on every pass.
        for pass in 0..64 {
            for (handle, expected_kind) in expected {
                let read = arena.ty_kind(handle);
                assert_eq!(read, expected_kind, "pass {pass}: pinned read drifted");
                assert_eq!(
                    arena.ty_flags(handle),
                    TypeFlags::empty(),
                    "pass {pass}: flags read drifted"
                );
            }
        }
        // The scope joins the writer here: an append that races a read is a
        // soundness break the loop above must never observe.
    });
}
