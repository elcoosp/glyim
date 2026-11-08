use crate::lower::lower_body;
use crate::tests::mock_lower_ctx::TestLowerCtx;
use crate::tests::thir_builder::ThirBuilder;
use glyim_core::def_id::FnDefId;
use glyim_core::primitives::IntTy;
use glyim_mir::MirConstKind;
use glyim_test::test_ty_ctx;
use glyim_type::*;
use glyim_typeck::thir::{self, ExprKind};

/// S20-T05: `FnRef` lowers to `Operand::Constant(MirConst::Fn)`
#[test]
fn fn_ref_lowers_to_constant_fn() {
    let mut ctx_mut = test_ty_ctx();
    let _i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let fn_def_id = FnDefId::from_raw(7);
    let empty_subst = ctx_mut.intern_substitution(vec![]);
    let fn_ty = ctx_mut.mk_ty(TyKind::FnDef(fn_def_id, empty_subst));
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let b = ThirBuilder::new(fn_ty, interner.clone());
    let fn_ref_expr = b.expr(ExprKind::FnRef(fn_def_id), fn_ty);
    let body = b.into_body(vec![thir::Stmt::Expr { expr: fn_ref_expr }], vec![]);
    let result = lower_body(&mock, &body);

    let found_fn_const = result.body.basic_blocks.iter().any(|bb| {
        bb.statements.iter().any(|stmt| {
            if let glyim_mir::StatementKind::Assign(_, rvalue) = &stmt.kind
                && let glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(c)) = rvalue
            {
                return matches!(&c.kind, MirConstKind::Fn(id, _substs) if id.to_raw() == 7);
            }
            false
        })
    });
    assert!(found_fn_const, "expected MirConstKind::Fn(7, _) in MIR");
}
