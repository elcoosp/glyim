//! Per-compilation canonical type interner.
//!
//! Historically `Ty` was a `u32` index into a **per-context** `Vec<TyKind>`.
//! Every `TyCtx`/`TyCtxMut` owned its own table, so a handle allocated in one
//! context was invalid in another — the root cause of the whole "aliasing
//! class" of bugs (drop-glue elaboration, monomorphization, re-freeze OOB
//! panics). `struct`-with-`Drop` programs and loop-built arrays panicked at
//! `ty_ctx.rs:59` because a type handle crossed a context boundary.
//!
//! The fix is a single **shared type arena** per compilation. Both `TyCtx` and
//! `TyCtxMut` hold a clone of the same `Arc<TypeArena>`; `freeze`/`to_mut`
//! merely copy that handle (no deep `Vec` clone), so any `Ty`/`Substitution`
//! handle allocated by any view of the compilation is valid in every other
//! view. This is the "canonical type interner" (de-stubbing plan P0): one
//! source of truth for type structure, stable handles across contexts.
//!
//! Soundness model (plan §6 M3.4 — "safe append-only arena"): the three
//! tables are [`boxcar::Vec`]s — the vetted, loom-tested, append-only vector
//! whose entries are allocated in fixed chunks that **never move or shrink**
//! and are freed when the arena drops. All raw-pointer plumbing the old
//! design needed (`Box::into_raw` storage, `unsafe impl Send/Sync`, an
//! `unsafe` `Drop` reconstructing the boxes) is gone: this crate now
//! contains no `unsafe` at all, and soundness no longer rests on the
//! single-threaded-per-compilation convention (see
//! `tests/arena_concurrency.rs`, which races appends against reads across
//! real threads).
//!
//! The trade the old model made for safety was memory: a `Box::leak`ed
//! `&'static` arena variant shipped here once and was reverted because it
//! retained the whole table for the process lifetime (~40 MB per stdlib
//! compile, OOMing long runs). boxcar keeps the free-at-last-`Arc`-drop
//! semantics while staying concurrent — the two constraints that killed
//! every pure-`std` alternative (a `RwLock` guard cannot hand out `&TyKind`
//! with the caller's lifetime, and leaking cannot be reclaimed).
//!
//! Interning: `type_index`/`subst_index` remain `RwLock`-guarded maps (the
//! reader/writer split of §1.1) and the `write_gate` still serializes the
//! multi-table append; the gate path **re-checks the index** after taking
//! the gate, so two racers that both missed the fast path observe one
//! canonical handle instead of splitting the type world.

use crate::flags::TypeFlags;
use crate::substitution::*;
use crate::ty::*;
use smallvec::SmallVec;
use std::collections::HashMap;
use std::sync::{Mutex, RwLock};

/// A single compilation's canonical type table.
///
/// Append-only and concurrent-sound: `alloc_ty`/`intern_substitution` are
/// `&self`, reads are lock-free into never-moving entries, and the whole
/// table is freed when the last view (`Arc`) of the compilation drops.
pub struct TypeArena {
    /// Canonical `TyKind` entries. Entries never move (chunked storage), so
    /// `&TyKind` views handed out by [`TypeArena::ty_kind`] stay valid across
    /// any number of later appends.
    types: boxcar::Vec<TyKind>,
    /// Parallel flag array, index-aligned with `types`.
    type_flags: boxcar::Vec<TypeFlags>,
    /// Interned substitution argument lists (boxed smallvecs' contents, in
    /// chunk storage that likewise never moves).
    substitution_data: boxcar::Vec<SmallVec<[GenericArg; 4]>>,
    /// Fast structural de-duplication so the same logical type maps to the same
    /// `Ty` handle (the "interning" property) within a compilation.
    /// `RwLock` (§1.1): readers (`TyCtx` queries) take `read()` locks; writers
    /// (`alloc_ty`) take the write lock. This lets `&self` reads proceed
    /// without blocking behind writer serialization.
    type_index: RwLock<HashMap<TyKind, Ty>>,
    /// Parallel de-duplication table for substitutions (plan §2.1). `RwLock`
    /// (§1.1) for the same reader/writer split. The old linear
    /// `data.iter().position(...)` re-scan under the write gate (§1.2) has
    /// been removed — the `HashMap` alone is the single source of truth under
    /// the write gate.
    subst_index: RwLock<HashMap<SmallVec<[GenericArg; 4]>, Substitution>>,
    /// Serializes the multi-table append + index insert of one `alloc`/
    /// `intern` step. Storage pushes are individually atomic (boxcar), but
    /// the *pair* (entry + flag / entry + index) must become visible
    /// together with the handle, or a racing reader could observe a handle
    /// whose flags row is not written yet.
    write_gate: Mutex<()>,
}

impl TypeArena {
    pub(crate) fn new() -> Self {
        TypeArena {
            types: boxcar::Vec::new(),
            type_flags: boxcar::Vec::new(),
            substitution_data: boxcar::Vec::new(),
            type_index: RwLock::new(HashMap::new()),
            subst_index: RwLock::new(HashMap::new()),
            write_gate: Mutex::new(()),
        }
    }

    /// Allocate a fresh type, interning by structure. Returns the canonical
    /// `Ty` handle for `kind`. `flags` must already be computed for `kind`.
    pub fn alloc_ty(&self, kind: TyKind, flags: TypeFlags) -> Ty {
        // Fast path: already interned. `RwLock::read()` — cheap shared read.
        {
            let idx = self.type_index.read().unwrap();
            if let Some(&t) = idx.get(&kind) {
                return t;
            }
        }
        // Do NOT recover from poisoning here. `write_gate` serializes the
        // entry+flag append and the index insert; a poisoned gate means a
        // previous writer panicked *mid-write*, so the tables may hold an
        // entry whose index row is missing. Recovering the guard
        // (`PoisonError::into_inner`) would let this writer proceed over
        // inconsistent state — unsound. Panicking keeps the failure loud and
        // local, which is the correct trade for an invariant this strong.
        // (Contrast the runtime's handle maps, which are plain `HashMap`s and
        // *do* recover — see `lock_recover` in `glyim-runtime`.)
        let _gate = self.write_gate.lock().unwrap();
        // Re-check under the gate: a racer may have interned `kind` between
        // our fast-path read and this lock. Without this, two racers push two
        // entries for one structure and hand out two distinct handles — the
        // type world splits (pinned by `tests::arena_concurrency`).
        if let Some(&t) = self.type_index.read().unwrap().get(&kind) {
            return t;
        }
        let raw = self.types.push(kind.clone()) as u32;
        self.type_flags.push(flags);
        self.type_index
            .write()
            .unwrap()
            .insert(kind, Ty::from_raw(raw));
        Ty::from_raw(raw)
    }

    #[inline]
    /// Ty Kind.
    pub fn ty_kind(&self, ty: Ty) -> &TyKind {
        &self.types[ty.index()]
    }

    #[inline]
    /// Ty Flags.
    pub fn ty_flags(&self, ty: Ty) -> TypeFlags {
        self.type_flags[ty.index()]
    }

    /// Intern a substitution's argument list; returns its stable index.
    ///
    /// §1.2 fix (retained): the old code did an O(n) linear re-scan
    /// (`data.iter().position(...)`) after taking the write gate. With
    /// `subst_index` in place (plan §2.1), the write-gate path can trust the
    /// `HashMap` — after the gate re-check, no other racer is inserting this
    /// list — so substitution interning is O(1) amortized instead of O(n) per
    /// call → O(n²) across a compilation.
    pub fn intern_substitution(&self, args: Vec<GenericArg>) -> Substitution {
        let small: SmallVec<[GenericArg; 4]> = args.into_iter().collect();
        let len = small.len() as u16;
        // Fast path: already interned. `RwLock::read()` — cheap shared read.
        {
            let idx = self.subst_index.read().unwrap();
            if let Some(&s) = idx.get(&small) {
                return s;
            }
        }
        // Do NOT recover from poisoning; see `alloc_ty`'s note.
        let _gate = self.write_gate.lock().unwrap();
        // Gate re-check (see `alloc_ty`): interning uniqueness under racers.
        if let Some(&s) = self.subst_index.read().unwrap().get(&small) {
            return s;
        }
        let index = self.substitution_data.push(small.clone()) as u32;
        self.subst_index
            .write()
            .unwrap()
            .insert(small, Substitution::from_raw(index, len));
        Substitution::from_raw(index, len)
    }

    #[inline]
    /// Substitution Args.
    pub fn substitution_args(&self, sub: Substitution) -> &[GenericArg] {
        if sub.is_empty() {
            return &[];
        }
        &self.substitution_data[sub.index() as usize]
    }
}
