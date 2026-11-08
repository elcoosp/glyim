use crate::lower::lower_body;
use crate::tests::mock_lower_ctx::TestLowerCtx;
use crate::tests::thir_builder::ThirBuilder;
use glyim_mir::MirConstKind;
use glyim_test::test_ty_ctx;
use glyim_type::*;
use glyim_typeck::thir::{self, ExprKind, Literal};

/// S20-T04: String literal creates valid `MirConst::String`
#[test]
fn string_literal_lowers_to_mir_const_string() {
    let mut ctx_mut = test_ty_ctx();
    let string_ty = ctx_mut.mk_ty(TyKind::String);
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let b = ThirBuilder::new(string_ty, interner.clone());
    let hello_name = interner.intern("hello");
    let string_lit = b.expr(ExprKind::Literal(Literal::String(hello_name)), string_ty);
    let body = b.into_body(vec![thir::Stmt::Expr { expr: string_lit }], vec![]);
    let result = lower_body(&mock, &body);

    let found_string = result.body.basic_blocks.iter().any(|bb| {
        bb.statements.iter().any(|stmt| {
            if let glyim_mir::StatementKind::Assign(_, rvalue) = &stmt.kind
                && let glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(c)) = rvalue
            {
                return matches!(&c.kind, MirConstKind::String(n) if *n == hello_name);
            }
            false
        })
    });
    assert!(found_string, "expected MirConstKind::String in MIR");
}

/// S20-T04 (cont): Char literal creates valid `MirConst::Int`
#[test]
fn char_literal_lowers_to_int_const() {
    let mut ctx_mut = test_ty_ctx();
    let char_ty = ctx_mut.mk_ty(TyKind::Char);
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let b = ThirBuilder::new(char_ty, interner.clone());
    let char_lit = b.expr(ExprKind::Literal(Literal::Char('A')), char_ty);
    let body = b.into_body(vec![thir::Stmt::Expr { expr: char_lit }], vec![]);
    let result = lower_body(&mock, &body);

    let found_char = result.body.basic_blocks.iter().any(|bb| {
        bb.statements.iter().any(|stmt| {
            if let glyim_mir::StatementKind::Assign(_, rvalue) = &stmt.kind
                && let glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(c)) = rvalue
            {
                return matches!(&c.kind, MirConstKind::Int(v) if *v == 'A' as i128);
            }
            false
        })
    });
    assert!(
        found_char,
        "expected MirConstKind::Int(65) for char 'A' in MIR"
    );
}
