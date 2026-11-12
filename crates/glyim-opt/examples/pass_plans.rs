//! Opt-level pass-plan consumer walkthrough.
//!
//! Demonstrates the net-new capability end to end: the same MIR body
//! optimized under the `-O0` (minimal) and `-O2` (full) plans, with the
//! pass registry consulted by id and a custom plan built from explicit
//! passes. This is the seam the pipeline drives from the CLI's
//! `--opt-level` (see `glyim_opt::PassPlan::for_opt_level`).
//!
//! Run with:
//!
//! ```text
//! cargo run -p glyim-opt --example pass_plans
//! ```

use std::sync::Arc;

use glyim_core::{IndexVec, Interner, Mutability};
use glyim_mir::*;
use glyim_opt::{BuiltinMirPass, MirPass, PassPlan, optimize_at_level, optimize_with_plan};
use glyim_span::Span;
use glyim_type::{Ty, TyCtxMut, TyKind};

/// A const `i128` operand.
fn const_int(val: i128, ty: Ty) -> Operand {
    Operand::Constant(MirConst {
        kind: MirConstKind::Int(val),
        ty,
        span: Span::DUMMY,
    })
}

/// The demo body (the same shape the `pass_plan` tests pin):
///
/// ```text
/// bb0: _2 = 42        // dead store — never read
///      _1 = 10
///      _0 = copy _1
///      return
/// ```
fn dead_store_body(ctx: &mut TyCtxMut) -> Body {
    let i32_ty = ctx.mk_ty(TyKind::Int(glyim_core::IntTy::I32));
    let locals = vec![
        (i32_ty, Mutability::Mut), // _0 return slot
        (i32_ty, Mutability::Mut), // _1 live value
        (i32_ty, Mutability::Mut), // _2 dead store target
    ];
    let assign = |place: u32, rv: Rvalue| Statement {
        kind: StatementKind::Assign(Place::new(LocalIdx::from_raw(place)), rv),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let block = BasicBlockData {
        statements: vec![
            assign(2, Rvalue::Use(const_int(42, i32_ty))),
            assign(1, Rvalue::Use(const_int(10, i32_ty))),
            assign(
                0,
                Rvalue::Use(Operand::Copy(Place::new(LocalIdx::from_raw(1)))),
            ),
        ],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    };
    Body {
        owner: glyim_core::DefId::new(
            glyim_core::CrateId::from_raw(0),
            glyim_core::LocalDefId::from_raw(0),
        ),
        basic_blocks: IndexVec::from_raw(vec![block]),
        locals: IndexVec::from_raw(
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

/// Total `Assign` statements in the body — the DCE-observable count.
fn assign_count(body: &Body) -> usize {
    body.basic_blocks
        .iter()
        .map(|bb| {
            bb.statements
                .iter()
                .filter(|s| matches!(s.kind, StatementKind::Assign(_, _)))
                .count()
        })
        .sum()
}

fn main() {
    let mut ctx_mut = TyCtxMut::new(Interner::default());
    let body = dead_store_body(&mut ctx_mut);
    let ctx = ctx_mut.freeze();
    let arc = Arc::new(body);

    println!(
        "pass registry ({} builtin passes):",
        BuiltinMirPass::ALL.len()
    );
    for pass in BuiltinMirPass::ALL {
        println!(
            "  {:<18} -> from_id(\"{}\") round-trips: {}",
            pass.id(),
            pass.id(),
            BuiltinMirPass::from_id(pass.id()) == Some(*pass)
        );
    }

    for level in [0u8, 2] {
        let plan = PassPlan::for_opt_level(level);
        println!(
            "\n-O{level} plan ({} passes): {:?}",
            plan.passes().len(),
            plan.ids()
        );
        let out = optimize_at_level(&ctx, &arc, level);
        println!(
            "  assign statements: {} -> {}",
            assign_count(&arc),
            assign_count(&out.body)
        );
    }

    // Custom plans are explicit and ordered; duplicates are rejected so a
    // pass can never silently run twice.
    let custom = PassPlan::from_passes(&[
        BuiltinMirPass::SliceDesugar,
        BuiltinMirPass::Dce,
        BuiltinMirPass::DropElaboration,
    ]);
    match custom {
        Some(plan) => {
            let out = optimize_with_plan(&ctx, &arc, &plan);
            println!(
                "\ncustom plan {:?}: {} assigns after",
                plan.ids(),
                assign_count(&out.body)
            );
        }
        None => println!("\ncustom plan rejected: duplicate pass"),
    }
}
