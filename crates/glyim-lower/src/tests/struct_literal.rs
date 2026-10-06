use crate::lower::lower_body;
use crate::tests::mock_lower_ctx::TestLowerCtx;
use crate::tests::thir_builder::ThirBuilder;
use glyim_core::def_id::AdtId;
use glyim_core::primitives::IntTy;
use glyim_test::{assert_mir, test_ty_ctx};
use glyim_type::*;
use glyim_typeck::thir::{self, ExprKind, Literal};

#[test]
fn struct_literal_does_not_panic() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let adt_id = AdtId::from_raw(0);
    let subst = ctx_mut.intern_substitution(vec![]);
    let struct_ty = ctx_mut.mk_adt(adt_id, subst);
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let b = ThirBuilder::new(struct_ty, interner);
    let struct_expr = b.expr(
        ExprKind::Struct {
            adt_id,
            variant_idx: 0,
            fields: vec![
                (
                    b.make_name("x"),
                    b.expr(ExprKind::Literal(Literal::Int(1, None)), i32_ty),
                ),
                (
                    b.make_name("y"),
                    b.expr(ExprKind::Literal(Literal::Int(2, None)), i32_ty),
                ),
            ],
            spread: None,
        },
        struct_ty,
    );
    let body = b.into_body(vec![thir::Stmt::Expr { expr: struct_expr }], vec![]);
    let result = lower_body(&mock, &body);
    // Struct literal stubbed; should not panic
    assert_mir(&ctx, &result.body).block_count(1);
}

#[test]
fn struct_literal_with_spread_does_not_panic() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let adt_id = AdtId::from_raw(1);
    let subst = ctx_mut.intern_substitution(vec![]);
    let struct_ty = ctx_mut.mk_adt(adt_id, subst);
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let b = ThirBuilder::new(struct_ty, interner);
    let base = b.expr(ExprKind::Literal(Literal::Int(0, None)), struct_ty);
    let struct_expr = b.expr(
        ExprKind::Struct {
            adt_id,
            variant_idx: 0,
            fields: vec![(
                b.make_name("x"),
                b.expr(ExprKind::Literal(Literal::Int(42, None)), i32_ty),
            )],
            spread: Some(Box::new(base)),
        },
        struct_ty,
    );
    let body = b.into_body(vec![thir::Stmt::Expr { expr: struct_expr }], vec![]);
    let result = lower_body(&mock, &body);
    assert_mir(&ctx, &result.body).block_count(1);
}

/// T024-REGRESSION [LOW-2]: a struct literal whose fields appear out of
/// declaration order must store each operand at its declaration index, not
/// in source order. Without the fix, `P { y: 2, x: 1 }` stored 2 in the
/// `x` slot and 1 in the `y` slot — a silent swap visible through
/// field access.
#[test]
fn t024_out_of_order_struct_fields_use_declaration_index() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let adt_id = AdtId::from_raw(0);
    let subst = ctx_mut.intern_substitution(vec![]);
    let struct_ty = ctx_mut.mk_adt(adt_id, subst);
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mut mock = TestLowerCtx::new(&ctx);

    // Declare a two-field struct P { x: i32 (idx 0), y: i32 (idx 1) }.
    let name_x = {
        let resolver = ctx.resolver().clone();
        resolver.intern("x")
    };
    let name_y = {
        let resolver = ctx.resolver().clone();
        resolver.intern("y")
    };
    mock.add_field_index(adt_id, 0, name_x, glyim_type::FieldIdx::from_raw(0));
    mock.add_field_index(adt_id, 0, name_y, glyim_type::FieldIdx::from_raw(1));

    // Build `P { y: 2, x: 1 }` — source order is y then x.
    let b = ThirBuilder::new(struct_ty, interner);
    let struct_expr = b.expr(
        ExprKind::Struct {
            adt_id,
            variant_idx: 0,
            fields: vec![
                (
                    b.make_name("y"),
                    b.expr(ExprKind::Literal(Literal::Int(2, None)), i32_ty),
                ),
                (
                    b.make_name("x"),
                    b.expr(ExprKind::Literal(Literal::Int(1, None)), i32_ty),
                ),
            ],
            spread: None,
        },
        struct_ty,
    );
    let body = b.into_body(vec![thir::Stmt::Expr { expr: struct_expr }], vec![]);
    let result = lower_body(&mock, &body);

    // Inspect the single Aggregate rvalue in the resulting MIR: the operand
    // at declaration index 0 must be the literal for `x` (1) and index 1
    // must be the literal for `y` (2).
    let mut found = false;
    for bb in result.body.basic_blocks.iter() {
        for stmt in &bb.statements {
            if let glyim_mir::StatementKind::Assign(_, rv) = &stmt.kind
                && let glyim_mir::Rvalue::Aggregate(kind, ops) = rv
                && matches!(kind, glyim_mir::AggregateKind::Adt(..))
            {
                found = true;
                assert_eq!(ops.len(), 2, "expected 2 operands, got {}", ops.len());
                let as_int = |op: &glyim_mir::Operand| -> Option<i128> {
                    match op {
                        glyim_mir::Operand::Constant(c) => match c.kind {
                            glyim_mir::MirConstKind::Int(v) => Some(v),
                            _ => None,
                        },
                        _ => None,
                    }
                };
                assert_eq!(
                    as_int(&ops[0]),
                    Some(1),
                    "operand at field index 0 (x) must be x's value 1"
                );
                assert_eq!(
                    as_int(&ops[1]),
                    Some(2),
                    "operand at field index 1 (y) must be y's value 2"
                );
            }
        }
    }
    assert!(found, "no Adt Aggregate rvalue found in lowered body");
}
