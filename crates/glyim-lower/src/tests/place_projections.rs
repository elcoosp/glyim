use crate::lower::lower_body;
use crate::tests::mock_lower_ctx::TestLowerCtx;
use crate::tests::thir_builder::ThirBuilder;
use glyim_core::def_id::AdtId;
use glyim_core::primitives::IntTy;
use glyim_mir::ProjectionElem;
use glyim_test::test_ty_ctx;
use glyim_type::*;
use glyim_typeck::thir::{self, ExprKind};

/// S20-T06: Place expressions generate valid projection chains
#[test]
fn nested_field_access_creates_projection_chain() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let inner_adt_id = AdtId::from_usize(1);
    let outer_adt_id = AdtId::from_usize(2);
    let inner_subst = ctx_mut.intern_substitution(vec![]);
    let outer_subst = ctx_mut.intern_substitution(vec![]);
    let inner_ty = ctx_mut.mk_adt(inner_adt_id, inner_subst);
    let outer_ty = ctx_mut.mk_adt(outer_adt_id, outer_subst);
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();

    let mut mock = TestLowerCtx::new(&ctx);
    let field_inner = interner.intern("inner");
    let field_val = interner.intern("val");
    mock.add_field_index(outer_adt_id, 0, field_inner, FieldIdx::from_usize(0));
    mock.add_field_index(inner_adt_id, 0, field_val, FieldIdx::from_usize(0));

    let mut b = ThirBuilder::new(i32_ty, interner.clone());
    let mut stmts = Vec::new();
    b.add_let_binding("o", outer_ty, None, &mut stmts);

    // o.inner
    let inner_access = b.expr(
        ExprKind::Field {
            receiver: Box::new(b.var_ref_expr("o", outer_ty)),
            field: field_inner,
            ty: inner_ty,
        },
        inner_ty,
    );
    // o.inner.val
    let val_access = b.expr(
        ExprKind::Field {
            receiver: Box::new(inner_access),
            field: field_val,
            ty: i32_ty,
        },
        i32_ty,
    );
    stmts.push(thir::Stmt::Expr { expr: val_access });

    let body = b.into_body(stmts, vec![]);
    let result = lower_body(&mock, &body);

    // Verify there's a place with at least one Field projection
    let found_field_proj = result.body.basic_blocks.iter().any(|bb| {
        bb.statements.iter().any(|stmt| {
            if let glyim_mir::StatementKind::Assign(_, rvalue) = &stmt.kind
                && let glyim_mir::Rvalue::Use(operand) = rvalue
                && let glyim_mir::Operand::Copy(place) | glyim_mir::Operand::Move(place) = operand
            {
                return place
                    .projection
                    .iter()
                    .any(|elem| matches!(elem, ProjectionElem::Field(_)));
            }
            false
        })
    });
    assert!(found_field_proj, "expected Field projection in MIR place");
}

#[test]
fn index_expr_creates_index_projection() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let slice_ty = ctx_mut.mk_ty(TyKind::Slice(i32_ty));
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let mut b = ThirBuilder::new(i32_ty, interner.clone());
    let mut stmts = Vec::new();
    b.add_let_binding("arr", slice_ty, None, &mut stmts);
    b.add_let_binding("i", i32_ty, None, &mut stmts);

    let index_expr = b.expr(
        ExprKind::Index {
            base: Box::new(b.var_ref_expr("arr", slice_ty)),
            index: Box::new(b.var_ref_expr("i", i32_ty)),
        },
        i32_ty,
    );
    stmts.push(thir::Stmt::Expr { expr: index_expr });

    let body = b.into_body(stmts, vec![]);
    let result = lower_body(&mock, &body);

    let found_index_proj = result.body.basic_blocks.iter().any(|bb| {
        bb.statements.iter().any(|stmt| {
            if let glyim_mir::StatementKind::Assign(_, rvalue) = &stmt.kind
                && let glyim_mir::Rvalue::Use(operand) = rvalue
                && let glyim_mir::Operand::Copy(place) | glyim_mir::Operand::Move(place) = operand
            {
                return place
                    .projection
                    .iter()
                    .any(|elem| matches!(elem, ProjectionElem::Index(_)));
            }
            false
        })
    });
    assert!(found_index_proj, "expected Index projection in MIR place");
}
