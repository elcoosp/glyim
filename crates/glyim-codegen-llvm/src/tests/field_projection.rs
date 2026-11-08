//! S22-T03: Tests for field projection on structs/tuples.

use super::helpers::*;
use crate::LlvmBackend;
use glyim_core::primitives::*;
use glyim_mir::*;
use glyim_test::with_fresh_ty_ctx;
use glyim_type::*;

#[test]
fn field_projection_on_tuple_generates_gep() {
    let (ctx, (tuple_ty, i32_ty)) = with_fresh_ty_ctx(|c| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let bool_ty = c.bool_ty();
        let subst = c.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(bool_ty)]);
        let tuple_ty = c.mk_tuple(subst);
        (tuple_ty, i32_ty)
    });
    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let mut builder = BodyBuilder::new(Ty::UNIT);
    let tuple_local = builder.add_local(tuple_ty);
    let field_local = builder.add_local(i32_ty);
    builder.add_statement(make_assign(
        Place {
            local: field_local,
            projection: Box::new([ProjectionElem::Field(FieldIdx::from_raw(0))]),
        },
        Rvalue::Use(Operand::Copy(Place::new(tuple_local))),
    ));
    let body = builder.build();
    let ir = backend.generate_ir(&body);
    assert!(ir.is_ok(), "generate_ir should succeed: {:?}", ir.err());
    let ir = ir.unwrap();
    assert!(
        ir.contains("getelementptr") || ir.contains("gep"),
        "Expected GEP instruction for field projection in IR:\n{ir}"
    );
}

#[test]
fn field_projection_on_tuple_second_field() {
    let (ctx, (tuple_ty, bool_ty)) = with_fresh_ty_ctx(|c| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let bool_ty = c.bool_ty();
        let subst = c.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(bool_ty)]);
        let tuple_ty = c.mk_tuple(subst);
        (tuple_ty, bool_ty)
    });
    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let mut builder = BodyBuilder::new(Ty::UNIT);
    let tuple_local = builder.add_local(tuple_ty);
    let field_local = builder.add_local(bool_ty);
    builder.add_statement(make_assign(
        Place {
            local: field_local,
            projection: Box::new([ProjectionElem::Field(FieldIdx::from_raw(1))]),
        },
        Rvalue::Use(Operand::Copy(Place::new(tuple_local))),
    ));
    let body = builder.build();
    let ir = backend.generate_ir(&body);
    assert!(ir.is_ok(), "generate_ir should succeed: {:?}", ir.err());
}

#[test]
fn field_projection_on_nested_tuple() {
    let (ctx, (outer_ty, inner_ty, i32_ty)) = with_fresh_ty_ctx(|c| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let bool_ty = c.bool_ty();
        let inner_subst =
            c.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(bool_ty)]);
        let inner_ty = c.mk_tuple(inner_subst);
        let outer_subst =
            c.intern_substitution(vec![GenericArg::Ty(inner_ty), GenericArg::Ty(i32_ty)]);
        let outer_ty = c.mk_tuple(outer_subst);
        (outer_ty, inner_ty, i32_ty)
    });
    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let mut builder = BodyBuilder::new(Ty::UNIT);
    let outer_local = builder.add_local(outer_ty);
    let inner_local = builder.add_local(inner_ty);
    let field_local = builder.add_local(i32_ty);
    builder.add_statement(make_assign(
        Place {
            local: inner_local,
            projection: Box::new([ProjectionElem::Field(FieldIdx::from_raw(0))]),
        },
        Rvalue::Use(Operand::Copy(Place::new(outer_local))),
    ));
    builder.add_statement(make_assign(
        Place {
            local: field_local,
            projection: Box::new([ProjectionElem::Field(FieldIdx::from_raw(0))]),
        },
        Rvalue::Use(Operand::Copy(Place::new(inner_local))),
    ));
    let body = builder.build();
    let ir = backend.generate_ir(&body);
    assert!(ir.is_ok(), "generate_ir should succeed: {:?}", ir.err());
}

#[test]
fn deref_on_ref_type_loads_pointer() {
    let (ctx, (ref_ty, i32_ty)) = with_fresh_ty_ctx(|c| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let ref_ty = c.mk_ref(Region::Erased, i32_ty, Mutability::Not);
        (ref_ty, i32_ty)
    });
    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let mut builder = BodyBuilder::new(Ty::UNIT);
    let ref_local = builder.add_local(ref_ty);
    let val_local = builder.add_local(i32_ty);
    // val_local = *ref_local (Deref goes on the source place, not destination)
    builder.add_statement(make_assign(
        Place::new(val_local),
        Rvalue::Use(Operand::Copy(Place {
            local: ref_local,
            projection: Box::new([ProjectionElem::Deref]),
        })),
    ));
    let body = builder.build();
    let ir = backend.generate_ir(&body);
    assert!(ir.is_ok(), "generate_ir should succeed: {:?}", ir.err());
    let ir = ir.unwrap();
    assert!(
        ir.contains("load"),
        "Deref on reference should produce a load, got:\n{ir}"
    );
}
