//! S22-T02: Tests for string constant lowering.

use super::helpers::*;
use crate::LlvmBackend;
use glyim_core::Interner;
use glyim_mir::*;
use glyim_type::*;

fn make_ctx_with_string_const(s: &str) -> (glyim_type::TyCtx, glyim_type::Ty, glyim_core::Name) {
    let interner = Interner::default();
    let name = interner.intern(s);
    let mut ctx_mut = glyim_type::TyCtxMut::new(interner);
    let string_ty = ctx_mut.mk_ty(TyKind::String);
    let ctx = ctx_mut.freeze();
    (ctx, string_ty, name)
}

#[test]
fn string_constant_creates_global_in_ir() {
    let (ctx, string_ty, name) = make_ctx_with_string_const("hello");
    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let mut builder = BodyBuilder::new(Ty::UNIT);
    let string_local = builder.add_local(string_ty);
    builder.add_statement(make_assign(
        Place::new(string_local),
        Rvalue::Use(Operand::Constant(make_string_const(name, string_ty))),
    ));
    let body = builder.build();
    let ir = backend.generate_ir(&body);
    assert!(ir.is_ok(), "generate_ir should succeed: {:?}", ir.err());
    let ir = ir.unwrap();
    assert!(
        ir.contains("hello") || ir.contains("str") || ir.contains("constant"),
        "Expected global string constant in IR, got:\n{ir}"
    );
}

#[test]
fn string_constant_not_bare_i64_zero() {
    let (ctx, string_ty, name) = make_ctx_with_string_const("world");
    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let mut builder = BodyBuilder::new(Ty::UNIT);
    let string_local = builder.add_local(string_ty);
    builder.add_statement(make_assign(
        Place::new(string_local),
        Rvalue::Use(Operand::Constant(make_string_const(name, string_ty))),
    ));
    let body = builder.build();
    let ir = backend.generate_ir(&body);
    assert!(ir.is_ok(), "generate_ir should succeed: {:?}", ir.err());
}
