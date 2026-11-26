//! Type-arena walkthrough: the shared, canonical type table the whole
//! compilation leans on.
//!
//! `glyim-type` had no consumer walkthrough so far: the arena is the
//! foundation every `Ty`/`Substitution` handle resolves through, and its
//! soundness model is the seam plan M3.4 tightens ("safe append-only arena
//! — replace `Box::into_raw`/`unsafe impl Send/Sync` with a vetted/own
//! safe-API structure"). This walkthrough builds the lifetime a consumer
//! actually makes: intern structures through a mutable view, keep the
//! handles across the `freeze()` boundary, then share the frozen view's
//! arena across real threads — appends and reads racing on the same
//! `&TypeArena`, every racer observing one canonical handle (the
//! interning property). The concurrent contract's test suite lives in
//! `src/tests/arena_concurrency.rs`. Run with:
//!
//! ```text
//! cargo run -p glyim-type --example type_arena_walkthrough
//! ```

use glyim_core::interner::Interner;
use glyim_core::primitives::IntTy;

use glyim_type::{TyCtxMut, TyKind};

fn main() {
    // One compilation, one canonical arena: every `TyCtx`/`TyCtxMut` view
    // holds the same `Arc<TypeArena>`, so handles minted by any view are
    // valid in every other (the fix for the cross-context aliasing class).
    let mut ctx = TyCtxMut::new(Interner::new());

    // Interning: `alloc_ty` maps a structure to its canonical handle. The
    // sentinel first entry (`Ty::ERROR`, raw handle 0) is already there, so
    // the first user type is raw handle 1 — and re-interning the same
    // structure returns the same handle instead of appending a twin.
    let i32_a = ctx.alloc_ty(TyKind::Int(IntTy::I32));
    let i32_b = ctx.alloc_ty(TyKind::Int(IntTy::I32));
    println!(
        "interning: i32_a={i32_a:?} i32_b={i32_b:?} equal={}",
        i32_a == i32_b
    );

    // Handles cross the freeze boundary without a deep copy: `freeze` only
    // clones the `Arc` to the same arena, and the frozen view resolves
    // handles through `ty_kind`.
    let frozen = ctx.freeze();
    println!("frozen read: i32_a resolves to {:?}", frozen.ty_kind(i32_a));

    // The soundness model is concurrent: share the frozen view across
    // scoped threads and race interning against reads. `barrier.wait` parks
    // all racers so the `alloc_ty` fast-path misses genuinely overlap; a
    // sound arena still hands every racer the same canonical handle.
    let frozen = std::sync::Arc::new(frozen);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let handles: Vec<glyim_type::Ty> = std::thread::scope(|scope| {
        let mut racers = Vec::new();
        for _ in 0..3 {
            let barrier = std::sync::Arc::clone(&barrier);
            let frozen = std::sync::Arc::clone(&frozen);
            racers.push(scope.spawn(move || {
                barrier.wait();
                // Intern through the shared frozen view: `mk_ty` computes the
                // flags and lands in the same arena the racers read.
                frozen.mk_ty(TyKind::Int(IntTy::I64))
            }));
        }
        racers
            .into_iter()
            .map(|r| r.join().expect("racer joined"))
            .collect()
    });
    let canonical = handles[0];
    let unanimous = handles.iter().all(|h| h == &canonical);
    println!("concurrent alloc: 3 racers -> canonical={canonical:?} unanimous={unanimous}");

    // Appends never move entries: a handle read before more appends still
    // resolves to the same structure afterwards — the allocation face is
    // `&self`, so the frozen view keeps allocating through the same table.
    let _ = frozen.mk_ty(TyKind::Int(IntTy::I16));
    println!(
        "append stability: i32_a still resolves to {:?}",
        frozen.ty_kind(i32_a)
    );
    let _ = canonical;
}
