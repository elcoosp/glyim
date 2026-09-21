use crate::lower::lower_body;
use crate::tests::mock_lower_ctx::TestLowerCtx;
use crate::tests::thir_builder::ThirBuilder;
use glyim_core::def_id::AdtId;
use glyim_core::primitives::IntTy;
use glyim_mir::ProjectionElem;
use glyim_test::test_ty_ctx;
use glyim_type::*;
use glyim_typeck::thir::{self, ExprKind};

/// S20-T01: Field access lowers to correct FieldIdx projection
#[test]
fn field_access_uses_resolved_field_idx() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let adt_id = AdtId::from_raw(42);
    let subst = ctx_mut.intern_substitution(vec![]);
    let struct_ty = ctx_mut.mk_adt(adt_id, subst);
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();

    let mut mock = TestLowerCtx::new(&ctx);
    // Register field "y" at index 1 for adt_id=42, variant=0
    let field_y = interner.intern("y");
    mock.add_field_index(adt_id, 0, field_y, FieldIdx::from_raw(1));

    let mut b = ThirBuilder::new(i32_ty, interner.clone());
    let mut stmts = Vec::new();
    b.add_let_binding("s", struct_ty, None, &mut stmts);

    let field_expr = b.expr(
        ExprKind::Field {
            receiver: Box::new(b.var_ref_expr("s", struct_ty)),
            field: field_y,
            ty: i32_ty,
        },
        i32_ty,
    );
    stmts.push(thir::Stmt::Expr { expr: field_expr });

    let body = b.into_body(stmts, vec![]);
    let result = lower_body(&mock, &body);

    // The MIR body should have a place with a Field(FieldIdx(1)) projection
    let found_field_proj = result.body.basic_blocks.iter().any(|bb| {
        bb.statements.iter().any(|stmt| {
            if let glyim_mir::StatementKind::Assign(_, rvalue) = &stmt.kind
                && let glyim_mir::Rvalue::Use(operand) = rvalue
                && let glyim_mir::Operand::Copy(place) = operand
            {
                return place
                    .projection
                    .iter()
                    .any(|elem| matches!(elem, ProjectionElem::Field(idx) if idx.to_raw() == 1));
            }
            false
        })
    });
    assert!(
        found_field_proj,
        "expected Field(FieldIdx(1)) projection in MIR"
    );
}

#[test]
fn field_access_with_no_resolution_emits_error_const() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let adt_id = AdtId::from_raw(99);
    let subst = ctx_mut.intern_substitution(vec![]);
    let struct_ty = ctx_mut.mk_adt(adt_id, subst);
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();

    // Mock with no field resolution for this ADT
    let mock = TestLowerCtx::new(&ctx);

    let mut b = ThirBuilder::new(i32_ty, interner.clone());
    let mut stmts = Vec::new();
    b.add_let_binding("s", struct_ty, None, &mut stmts);

    let field_z = interner.intern("z");
    let field_expr = b.expr(
        ExprKind::Field {
            receiver: Box::new(b.var_ref_expr("s", struct_ty)),
            field: field_z,
            ty: i32_ty,
        },
        i32_ty,
    );
    stmts.push(thir::Stmt::Expr { expr: field_expr });

    let body = b.into_body(stmts, vec![]);
    let result = lower_body(&mock, &body);

    // Should produce an Error constant since field resolution fails
    let found_error = result.body.basic_blocks.iter().any(|bb| {
        bb.statements.iter().any(|stmt| {
            if let glyim_mir::StatementKind::Assign(_, rvalue) = &stmt.kind
                && let glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(c)) = rvalue
            {
                return matches!(c.kind, glyim_mir::MirConstKind::Error);
            }
            false
        })
    });
    assert!(
        found_error,
        "expected Error constant when field resolution fails"
    );
}

#[test]
fn non_copy_field_access_lowers_as_move_with_drop_flag() {
    // Phase 4 (GLYIM_DESTUB_PLAN): reading a non-Copy struct field as an
    // rvalue is a *partial move* — it must lower to `Operand::Move` (not
    // `Operand::Copy`) and register a drop-flag that is cleared at the move
    // site (so the parent's scope-exit Drop is guarded by
    // `elaborate_scope_drops`).
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let parent_adt = AdtId::from_raw(42);
    let inner_adt = AdtId::from_raw(43);
    let subst = ctx_mut.intern_substitution(vec![]);
    let struct_ty = ctx_mut.mk_adt(parent_adt, subst);
    let inner_ty = ctx_mut.mk_adt(inner_adt, subst); // ADT => non-Copy
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();

    let mut mock = TestLowerCtx::new(&ctx);
    let field_f = interner.intern("f");
    mock.add_field_index(parent_adt, 0, field_f, FieldIdx::from_raw(0));

    let mut b = ThirBuilder::new(i32_ty, interner.clone());
    let mut stmts = Vec::new();
    b.add_let_binding("s", struct_ty, None, &mut stmts);

    let field_expr = b.expr(
        ExprKind::Field {
            receiver: Box::new(b.var_ref_expr("s", struct_ty)),
            field: field_f,
            ty: inner_ty, // non-Copy field type
        },
        inner_ty,
    );
    stmts.push(thir::Stmt::Expr { expr: field_expr });

    let body = b.into_body(stmts, vec![]);
    let result = lower_body(&mock, &body);

    // (a) the field read must be a Move, not a Copy.
    let found_move = result.body.basic_blocks.iter().any(|bb| {
        bb.statements.iter().any(|stmt| {
            if let glyim_mir::StatementKind::Assign(
                _,
                glyim_mir::Rvalue::Use(glyim_mir::Operand::Move(place)),
            ) = &stmt.kind
            {
                return place
                    .projection
                    .iter()
                    .any(|elem| matches!(elem, ProjectionElem::Field(idx) if idx.to_raw() == 0));
            }
            false
        })
    });
    assert!(
        found_move,
        "non-Copy field access must lower to Operand::Move"
    );

    // (b) a drop-flag clear (Assign(_, Bool(false))) must be emitted at the
    //     move site, proving register_partial_move ran.
    let found_flag_clear = result.body.basic_blocks.iter().any(|bb| {
        bb.statements.iter().any(|stmt| {
            if let glyim_mir::StatementKind::Assign(
                _,
                glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(c)),
            ) = &stmt.kind
            {
                return matches!(c.kind, glyim_mir::MirConstKind::Bool(false));
            }
            false
        })
    });
    assert!(
        found_flag_clear,
        "partial move must clear a drop-flag (Assign(_, Bool(false)))"
    );
}

#[test]
fn copy_field_access_lowers_as_copy_without_drop_flag() {
    // Contrast: a Copy field (i32) read as an rvalue must remain
    // `Operand::Copy` and must NOT register/clear a drop-flag.
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let parent_adt = AdtId::from_raw(42);
    let subst = ctx_mut.intern_substitution(vec![]);
    let struct_ty = ctx_mut.mk_adt(parent_adt, subst);
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();

    let mut mock = TestLowerCtx::new(&ctx);
    let field_f = interner.intern("f");
    mock.add_field_index(parent_adt, 0, field_f, FieldIdx::from_raw(0));

    let mut b = ThirBuilder::new(i32_ty, interner.clone());
    let mut stmts = Vec::new();
    b.add_let_binding("s", struct_ty, None, &mut stmts);

    let field_expr = b.expr(
        ExprKind::Field {
            receiver: Box::new(b.var_ref_expr("s", struct_ty)),
            field: field_f,
            ty: i32_ty, // Copy field type
        },
        i32_ty,
    );
    stmts.push(thir::Stmt::Expr { expr: field_expr });

    let body = b.into_body(stmts, vec![]);
    let result = lower_body(&mock, &body);

    let found_copy = result.body.basic_blocks.iter().any(|bb| {
        bb.statements.iter().any(|stmt| {
            if let glyim_mir::StatementKind::Assign(
                _,
                glyim_mir::Rvalue::Use(glyim_mir::Operand::Copy(place)),
            ) = &stmt.kind
            {
                return place
                    .projection
                    .iter()
                    .any(|elem| matches!(elem, ProjectionElem::Field(idx) if idx.to_raw() == 0));
            }
            false
        })
    });
    assert!(found_copy, "Copy field access must lower to Operand::Copy");

    let found_flag_clear = result.body.basic_blocks.iter().any(|bb| {
        bb.statements.iter().any(|stmt| {
            if let glyim_mir::StatementKind::Assign(
                _,
                glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(c)),
            ) = &stmt.kind
            {
                return matches!(c.kind, glyim_mir::MirConstKind::Bool(false));
            }
            false
        })
    });
    assert!(
        !found_flag_clear,
        "Copy field access must NOT clear a drop-flag"
    );
}
