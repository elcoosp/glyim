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
//! `TyCtxMut` hold a `&'static TypeArena`; `freeze`/`to_mut` merely copy that
//! pointer (no deep `Vec` clone), so any `Ty`/`Substitution` handle allocated
//! by any view of the compilation is valid in every other view. This is the
//! "canonical type interner" (de-stubbing plan P0): one source of truth for
//! type structure, stable handles across contexts.
//!
//! Soundness model: the type/flag/substitution tables live in heap storage
//! that is allocated once per compilation and never moved or freed for the
//! program's lifetime (`Box::into_raw`). Reads (`ty_kind`, `substitution_args`)
//! return references straight into that stable storage — no lock guard escapes
//! the function. Writes are serialized through `write_gate` and operate on the
//! same stable storage via `&mut`. This is the standard typed-arena pattern
//! (rustc's `Ty` arena works the same way).
//!
//! SAFETY: within a single compilation, type allocation is single-threaded and
//! sequential (typeck → lower → mono → codegen). Reads therefore never observe
//! a `Vec` reallocation racing a write, and references into a boxed entry stay
//! valid across any reallocation of the outer `Vec`. Each compilation owns its
//! own `TypeArena`; `nextest` runs distinct compilations on distinct threads,
//! so no `TypeArena` is shared across threads. The raw-pointer fields are thus
//! sound under `unsafe impl Send/Sync` given that invariant.

use crate::flags::TypeFlags;
use crate::substitution::*;
use crate::ty::*;
use smallvec::SmallVec;
use std::collections::HashMap;
use std::sync::{Mutex, RwLock};

/// A single compilation's canonical type table.
pub struct TypeArena {
    /// Stable heap storage for `TyKind` entries (boxed so addresses survive
    /// `Vec` reallocations). Never moved/freed after allocation.
    #[allow(clippy::vec_box)]
    types: *mut Vec<Box<TyKind>>,
    /// Parallel stable storage for type flags.
    type_flags: *mut Vec<TypeFlags>,
    /// Stable storage for substitution argument lists (boxed smallvecs).
    #[allow(clippy::vec_box)]
    substitution_data: *mut Vec<Box<SmallVec<[GenericArg; 4]>>>,
    /// Fast structural de-duplication so the same logical type maps to the same
    /// `Ty` handle (the "interning" property) within a compilation.
    /// `RwLock` (§1.1): readers (`TyCtx` queries) take `read()` locks; the single
    /// writer (`TyCtxMut::alloc_ty`) takes the write lock. This lets `&self` reads
    /// proceed without blocking behind writer serialization.
    type_index: RwLock<HashMap<TyKind, Ty>>,
    /// Parallel de-duplication table for substitutions (plan §2.1). `RwLock` (§1.1)
    /// for the same reader/writer split. The old linear `data.iter().position(...)`
    /// re-scan under the write gate (§1.2) has been removed — the HashMap alone is
    /// the single source of truth under the write gate, so a concurrent insert is
    /// impossible in the single-threaded-per-compilation model.
    subst_index: RwLock<HashMap<SmallVec<[GenericArg; 4]>, Substitution>>,
    /// Serializes appends to the three tables above.
    write_gate: Mutex<()>,
}

// SAFETY: see module docs. Each arena is used single-threaded (per
// compilation); the heap storage is stable for the program lifetime and writes
// are serialized. Distinct arenas are never shared across threads.
unsafe impl Send for TypeArena {}
unsafe impl Sync for TypeArena {}

impl TypeArena {
    pub(crate) fn new() -> Self {
        TypeArena {
            types: Box::into_raw(Box::new(Vec::new())),
            type_flags: Box::into_raw(Box::new(Vec::new())),
            substitution_data: Box::into_raw(Box::new(Vec::new())),
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
        let _gate = self.write_gate.lock().unwrap();
        // SAFETY: `types`/`type_flags` point to stable heap storage; we hold the
        // exclusive write gate, so no reader can observe a torn state.
        let types = unsafe { &mut *self.types };
        let type_flags = unsafe { &mut *self.type_flags };
        let raw = types.len() as u32;
        types.push(Box::new(kind.clone()));
        type_flags.push(flags);
        // Re-lock with write access (the read lock from the fast path is dropped).
        self.type_index.write().unwrap().insert(kind, Ty::from_raw(raw));
        Ty::from_raw(raw)
    }

    #[inline]
    pub fn ty_kind(&self, ty: Ty) -> &TyKind {
        // SAFETY: `types` is stable heap storage; this compilation is
        // single-threaded, so no concurrent write reallocates the `Vec` while
        // we hold the resulting `&TyKind`.
        #[allow(clippy::borrowed_box)]
        unsafe {
            let v: &Vec<Box<TyKind>> = &*self.types;
            let b: &Box<TyKind> = &v[ty.index()];
            b
        }
    }

    #[inline]
    pub fn ty_flags(&self, ty: Ty) -> TypeFlags {
        // SAFETY: same reasoning as `ty_kind`.
        unsafe {
            let v: &Vec<TypeFlags> = &*self.type_flags;
            v[ty.index()]
        }
    }

    /// Intern a substitution's argument list; returns its stable index.
    ///
    /// §1.2 fix: the old code did an O(n) linear re-scan (`data.iter().position(...)`)
    /// after taking the write gate. With `subst_index` in place (plan §2.1), if the
    /// fast-path read-lock found no match, the write-gate path can trust the HashMap —
    /// no other thread inserts concurrently (single-threaded per compilation). Removing
    /// the re-scan makes substitution interning O(1) amortized instead of O(n) per call
    /// → O(n²) across a compilation.
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
        let _gate = self.write_gate.lock().unwrap();
        // SAFETY: see `alloc_ty`.
        let data = unsafe { &mut *self.substitution_data };
        // §1.2: no O(n) re-scan here. The read-lock above already proved `small`
        // is absent from `subst_index`, and the write gate ensures no concurrent
        // insert, so the HashMap write-lock insert below is the single source of truth.
        let index = data.len() as u32;
        data.push(Box::new(small.clone()));
        // Re-lock with write access (the read lock from the fast path is dropped).
        self.subst_index.write().unwrap().insert(small, Substitution::from_raw(index, len));
        Substitution::from_raw(index, len)
    }

    #[inline]
    pub fn substitution_args(&self, sub: Substitution) -> &[GenericArg] {
        if sub.is_empty() {
            return &[];
        }
        // SAFETY: `substitution_data` is stable heap storage; single-threaded
        // per compilation.
        #[allow(clippy::borrowed_box)]
        unsafe {
            let v: &Vec<Box<SmallVec<[GenericArg; 4]>>> = &*self.substitution_data;
            let b: &Box<SmallVec<[GenericArg; 4]>> = &v[sub.index() as usize];
            b
        }
    }
}
