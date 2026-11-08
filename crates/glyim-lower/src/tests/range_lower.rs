use crate::lower::lower_body;
use crate::tests::mock_lower_ctx::TestLowerCtx;
use crate::tests::thir_builder::ThirBuilder;
use glyim_core::def_id::AdtId;
use glyim_core::primitives::IntTy;
use glyim_mir::{AggregateKind, Operand, Rvalue, StatementKind};
use glyim_test::{assert_mir, test_ty_ctx};
use glyim_type::*;
use glyim_typeck::thir::{self, ExprKind, Literal};

/// Build a `Range`/`RangeInclusive` THIR expr from two i32 endpoints and lower
/// it, returning the produced MIR aggregate ADT id + operand count.
fn lower_range(start: i128, end: i128, inclusive: bool) -> (AdtId, usize) {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let range_substs = ctx_mut.intern_substitution(vec![glyim_type::GenericArg::Ty(i32_ty)]);
    let range_ty = ctx_mut.mk_ty(TyKind::Adt(
        AdtId::from_raw(if inclusive { 1001 } else { 1000 }),
        range_substs,
    ));
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let b = ThirBuilder::new(i32_ty, interner);
    let start_expr = b.expr(ExprKind::Literal(Literal::Int(start, None)), i32_ty);
    let end_expr = b.expr(ExprKind::Literal(Literal::Int(end, None)), i32_ty);
    let range_expr = b.expr(
        ExprKind::Range {
            start: Some(Box::new(start_expr)),
            end: Some(Box::new(end_expr)),
            inclusive,
        },
        range_ty,
    );
    let body = b.into_body(vec![thir::Stmt::Expr { expr: range_expr }], vec![]);
    let result = lower_body(&mock, &body);
    let _ = assert_mir(&ctx, &result.body).block_count(1);

    // Find the Aggregate assignment and return its ADT id + operand count.
    for stmt in &result.body.basic_blocks[glyim_mir::BasicBlockIdx::from_raw(0)].statements {
        if let StatementKind::Assign(_, Rvalue::Aggregate(kind, ops)) = &stmt.kind {
            match kind {
                AggregateKind::Adt(adt_id, _, _) => return (*adt_id, ops.len()),
                _ => panic!("range lowered to a non-Adt aggregate"),
            }
        }
    }
    panic!("no Aggregate assignment found in lowered range body");
}

#[test]
fn range_exclusive_lowers_to_range_adt() {
    let (adt_id, n_ops) = lower_range(1, 5, false);
    assert_eq!(
        adt_id,
        AdtId::from_raw(1000),
        "exclusive range -> Range ADT"
    );
    assert_eq!(n_ops, 2, "range carries start + end operands");
}

#[test]
fn range_inclusive_lowers_to_range_inclusive_adt() {
    let (adt_id, n_ops) = lower_range(1, 5, true);
    assert_eq!(
        adt_id,
        AdtId::from_raw(1001),
        "inclusive range -> RangeInclusive ADT"
    );
    assert_eq!(n_ops, 2, "range carries start + end operands");
}

#[test]
fn range_endpoints_are_lowered_as_operands() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let range_substs = ctx_mut.intern_substitution(vec![glyim_type::GenericArg::Ty(i32_ty)]);
    let range_ty = ctx_mut.mk_ty(TyKind::Adt(AdtId::from_raw(1000), range_substs));
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let b = ThirBuilder::new(i32_ty, interner);
    let start_expr = b.expr(ExprKind::Literal(Literal::Int(10, None)), i32_ty);
    let end_expr = b.expr(ExprKind::Literal(Literal::Int(20, None)), i32_ty);
    let range_expr = b.expr(
        ExprKind::Range {
            start: Some(Box::new(start_expr)),
            end: Some(Box::new(end_expr)),
            inclusive: false,
        },
        range_ty,
    );
    let body = b.into_body(vec![thir::Stmt::Expr { expr: range_expr }], vec![]);
    let result = lower_body(&mock, &body);

    for stmt in &result.body.basic_blocks[glyim_mir::BasicBlockIdx::from_raw(0)].statements {
        if let StatementKind::Assign(_, Rvalue::Aggregate(AggregateKind::Adt(_, _, _), ops)) =
            &stmt.kind
        {
            assert_eq!(ops.len(), 2);
            // Each endpoint should be a lowered constant operand, not an empty
            // aggregate.
            for op in ops {
                assert!(
                    matches!(op, Operand::Constant(_)),
                    "range endpoint should be a lowered constant operand"
                );
            }
            return;
        }
    }
    panic!("no Aggregate assignment found in lowered range body");
}
