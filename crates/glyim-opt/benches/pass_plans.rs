//! Opt-level pass-plan cost harness.
//!
//! Dependency-free (`harness = false`), mirroring the `glyim-type`
//! `builtin_trait_registry` bench style. Measures what the opt-level seam
//! introduced and what `-O0` now skips:
//!
//! 1. `PassPlan::for_opt_level` — the plan construction the pipeline does
//!    once per compile (`Phase::Opt` entry);
//! 2. `BuiltinMirPass::from_id` over the registry — the canonicalization a
//!    custom plan or a phase-profile row takes from a pass id string back
//!    to the dispatchable variant;
//! 3. `optimize_at_level(.., 0)` vs `optimize_at_level(.., 2)` on a
//!    synthetic branchy body — the per-body cost the minimal plan skips
//!    (const-prop, DCE, CFG simplify, unreachable-elim) versus the full
//!    pipeline, so the `-O0` compile-time saving is visible in CI logs.
//!
//! Run with:
//!
//! ```text
//! cargo bench -p glyim-opt --bench pass_plans
//! ```

use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use glyim_core::{IndexVec, Interner, Mutability};
use glyim_mir::*;
use glyim_opt::{BuiltinMirPass, PassPlan, optimize_at_level, optimize_with_bisect};
use glyim_span::Span;
use glyim_type::{Ty, TyCtxMut, TyKind};

/// Interned `i32` for the synthetic body.
fn ty_i32(ctx: &mut TyCtxMut) -> Ty {
    ctx.mk_ty(TyKind::Int(glyim_core::IntTy::I32))
}

/// A const `i128` operand (the shape const-prop folds).
fn const_int(val: i128, ty: Ty) -> Operand {
    Operand::Constant(MirConst {
        kind: MirConstKind::Int(val),
        ty,
        span: Span::DUMMY,
    })
}

/// Build a synthetic body shaped like a lowered user function: a chain of
/// `N` blocks, each carrying live assigns, a dead store (DCE fodder), and
/// a constant-foldable re-assignment (const-prop fodder), linked by
/// `Goto`s. One body, reused for both levels — the plan is the variable.
fn synthetic_body(ctx: &mut TyCtxMut, blocks: usize) -> Body {
    let i32_ty = ty_i32(ctx);
    // _0: return slot, _1: live value, _2: dead store target.
    let locals = vec![
        (i32_ty, Mutability::Mut),
        (i32_ty, Mutability::Mut),
        (i32_ty, Mutability::Mut),
    ];

    let assign = |place: u32, val: i128| Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_usize(place as usize)),
            Rvalue::Use(const_int(val, i32_ty)),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let storage_live = |l: u32| Statement {
        kind: StatementKind::StorageLive(LocalIdx::from_usize(l as usize)),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let storage_dead = |l: u32| Statement {
        kind: StatementKind::StorageDead(LocalIdx::from_usize(l as usize)),
        source_info: SourceInfo::new(Span::DUMMY),
    };

    let mut bbs = Vec::with_capacity(blocks);
    for i in 0..blocks {
        let last = i + 1 == blocks;
        // StorageLive/StorageDead are function-entry/exit statements in real
        // lowering — one pair per local, not one per block. Emitting them per
        // block would make `cfg_simplify`'s block merge violate the M5.5
        // validator's `DoubleStorageLive` check (a fixture bug: real bodies
        // never look like that).
        let mut stmts = Vec::new();
        if i == 0 {
            stmts.push(storage_live(1));
            stmts.push(storage_live(2));
            stmts.push(assign(2, 42)); // dead store — never read (DCE fodder)
        }
        stmts.push(assign(1, i as i128)); // live value
        stmts.push(assign(1, (i as i128) + 1)); // immediate overwrite (const-prop fodder)
        if last {
            stmts.push(storage_dead(1));
            stmts.push(storage_dead(2));
        }
        let term = if last {
            Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(Span::DUMMY),
            }
        } else {
            Terminator {
                kind: TerminatorKind::Goto {
                    target: BasicBlockIdx::from_usize(i + 1),
                },
                source_info: SourceInfo::new(Span::DUMMY),
            }
        };
        bbs.push(BasicBlockData {
            statements: stmts,
            terminator: term,
            is_cleanup: false,
        });
    }

    Body {
        owner: glyim_core::DefId::new(
            glyim_core::CrateId::from_usize(0),
            glyim_core::LocalDefId::from_usize(0),
        ),
        basic_blocks: IndexVec::from_vec(bbs),
        locals: IndexVec::from_vec(
            locals
                .into_iter()
                .map(|(ty, mutability)| LocalDecl {
                    ty,
                    mutability,
                    source_info: SourceInfo::new(Span::DUMMY),
                })
                .collect(),
        ),
        arg_count: 0,
        return_ty: i32_ty,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    }
}

fn main() {
    let mut ctx_mut = TyCtxMut::new(Interner::default());

    // 1. Plan construction — once per compile.
    let t0 = Instant::now();
    for level in 0u8..=3 {
        black_box(PassPlan::for_opt_level(level));
    }
    let plan_ctor = t0.elapsed();

    // 2. Registry canonicalization — every `from_id` resolves one of the
    //    six variants against a miss-heavy id space.
    let t1 = Instant::now();
    for _ in 0..10_000 {
        for id in [
            "slice_desugar",
            "constant_prop",
            "dce",
            "cfg_simplify",
            "unreachable_elim",
            "drop_elaboration",
            "not_a_pass",
        ] {
            black_box(BuiltinMirPass::from_id(id));
        }
    }
    let from_id = t1.elapsed();

    // 3. Per-body optimize cost: -O0 (minimal plan) vs -O2 (full
    //    pipeline) over a 512-block body.
    let body = synthetic_body(&mut ctx_mut, 512);
    let frozen = ctx_mut.freeze();
    let arc = Arc::new(body);

    let t2 = Instant::now();
    let o0 = optimize_at_level(&frozen, Arc::clone(&arc), 0).expect("optimize_at_level failed");
    let o0_us = t2.elapsed();

    let t3 = Instant::now();
    let o2 = optimize_at_level(&frozen, Arc::clone(&arc), 2).expect("optimize_at_level failed");
    let o2_us = t3.elapsed();

    println!("glyim-opt pass-plan cost (512-block synthetic body)");
    println!("  for_opt_level 0..=3        : {plan_ctor:12?}");
    println!("  from_id x70k               : {from_id:12?}");
    println!("  optimize_at_level(.., 0)   : {o0_us:12?}  (minimal plan: slice_desugar only)");
    println!("  optimize_at_level(.., 2)   : {o2_us:12?}  (full pipeline)");
    println!(
        "  -O2 / -O0 cost ratio       : {:.2}x",
        o2_us.as_secs_f64() / o0_us.as_secs_f64().max(f64::EPSILON)
    );
    println!(
        "  blocks after O0/O2         : {}/{}",
        o0.body.basic_blocks.len(),
        o2.body.basic_blocks.len()
    );

    // 4. M5.11 bisect/stats overhead: `optimize_with_bisect` at the full
    //    limit (one `PassStats` row per pass, verify-after-each-pass always
    //    on) versus the stats-free `optimize_at_level` path — the collection
    //    cost `--opt-bisect-limit` adds must stay negligible next to the
    //    passes themselves. The limit=1 row shows the skipped-pass cost a
    //    bisect probe pays (most passes run no work).
    let plan = PassPlan::for_opt_level(2);
    let full_len = plan.passes().len();

    let t4 = Instant::now();
    let bisected = optimize_with_bisect(&frozen, Arc::clone(&arc), &plan, full_len)
        .expect("bisect at full limit must succeed");
    let bisect_full_us = t4.elapsed();

    let t5 = Instant::now();
    let probe = optimize_with_bisect(&frozen, Arc::clone(&arc), &plan, 1)
        .expect("bisect at limit 1 must succeed");
    let bisect_probe_us = t5.elapsed();

    println!("glyim-opt bisect/stats cost (M5.11, 512-block synthetic body)");
    println!(
        "  optimize_with_bisect(limit={full_len}) : {bisect_full_us:12?}  (stats collected, {} rows)",
        bisected.1.len()
    );
    println!(
        "  optimize_with_bisect(limit=1)     : {bisect_probe_us:12?}  (probe: slice_desugar only, {} rows)",
        probe.1.len()
    );
    println!(
        "  stats overhead vs optimize_at_level: {:.2}x",
        bisect_full_us.as_secs_f64() / o2_us.as_secs_f64().max(f64::EPSILON)
    );
}
