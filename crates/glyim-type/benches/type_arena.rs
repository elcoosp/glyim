//! Type-arena cost harness (plan §6 M3.4 — safe append-only arena).
//!
//! Dependency-free (`harness = false`), mirroring the
//! `builtin_trait_registry` bench style. Measures the operations the arena
//! redesign touched, so the instruction-count gate (`perf/budgets.toml`,
//! plan M2.6/M2.5) sees the real delta of replacing the raw-pointer tables:
//!
//! 1. `alloc_ty` cold — a fresh structure per call (fast-path miss, gate
//!    path, table append);
//! 2. `alloc_ty` hot — the same structure per call (fast-path hit; the
//!    interning property every structural-equality consumer leans on);
//! 3. `ty_kind` — the read face every `TyCtx` query bottoms out in;
//! 4. `intern_substitution` cold/hot and `substitution_args` — the
//!    substitution tables the monomorphizer and backends read.
//!
//! Run with:
//!
//! ```text
//! cargo bench -p glyim-type --bench type_arena
//! ```

use std::hint::black_box;
use std::time::Instant;

use glyim_core::interner::Interner;
use glyim_core::primitives::{FloatTy, IntTy, UintTy};

use glyim_type::substitution::GenericArg;
use glyim_type::{TyCtxMut, TyKind};

fn cold_kinds(step: usize) -> TyKind {
    // A distinct-but-cheap structure per step so every `alloc_ty` is a
    // genuine fast-path miss: primitives are interned by value, so cycling
    // the primitive width gives fresh structures without building ADTs.
    match step % 8 {
        0 => TyKind::Int(IntTy::I8),
        1 => TyKind::Int(IntTy::I16),
        2 => TyKind::Int(IntTy::I32),
        3 => TyKind::Int(IntTy::I64),
        4 => TyKind::Uint(UintTy::U8),
        5 => TyKind::Uint(UintTy::U16),
        6 => TyKind::Float(FloatTy::F32),
        _ => TyKind::Float(FloatTy::F64),
    }
}

fn main() {
    let mut ctx = TyCtxMut::new(Interner::new());

    // 1. Cold interning: every call appends.
    let steps = 20_000;
    let start = Instant::now();
    let mut last = None;
    for step in 0..steps {
        last = Some(ctx.alloc_ty(cold_kinds(step)));
    }
    let cold = start.elapsed();
    println!("alloc_ty cold : {cold:>10.1?} ({steps} fresh appends)");
    black_box(last);

    // 2. Hot interning: the same structure every call (fast-path hit).
    let hot_kind = TyKind::Int(IntTy::I32);
    let start = Instant::now();
    let mut handle = None;
    for _ in 0..steps {
        handle = Some(ctx.alloc_ty(hot_kind.clone()));
    }
    let hot = start.elapsed();
    println!("alloc_ty hot  : {hot:>10.1?} ({steps} fast-path hits)");
    black_box(handle);

    // 3. The read face: `ty_kind` on a pinned handle.
    let pinned = ctx.alloc_ty(TyKind::Int(IntTy::I64));
    let frozen = ctx.freeze();
    let start = Instant::now();
    let mut primitives = 0usize;
    for _ in 0..steps {
        if matches!(frozen.ty_kind(pinned), TyKind::Int(_)) {
            primitives += 1;
        }
    }
    let read = start.elapsed();
    println!("ty_kind       : {read:>10.1?} ({steps} reads, {primitives} primitive)");
    black_box(primitives);

    // 4. Substitution tables: cold intern, hot intern, read back.
    let args: Vec<GenericArg> = Vec::new();
    let start = Instant::now();
    let mut sub = None;
    for _ in 0..steps {
        sub = Some(ctx.intern_substitution(args.clone()));
    }
    println!(
        "intern_subst cold: {:>10.1?} ({steps} empty appends)",
        start.elapsed()
    );
    black_box(sub);

    let start = Instant::now();
    let mut read_args = 0usize;
    for _ in 0..steps {
        read_args += frozen.substitution_args(sub.expect("interned above")).len();
    }
    println!(
        "substitution_args: {:>10.1?} ({steps} reads, {read_args} args)",
        start.elapsed()
    );
    black_box(read_args);
}
