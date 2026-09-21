//! S15-T01: layout_of computes correct size/align for structs

use crate::*;
use glyim_core::arena::IndexVec;
use glyim_core::primitives::*;
use glyim_test::with_fresh_ty_ctx;

/// Helper: build a struct AdtDef with the given field types, register it, and return the Ty.
fn make_struct_ty(
    ctx: &mut glyim_type::TyCtxMut,
    adt_id: glyim_core::AdtId,
    field_tys: Vec<glyim_type::Ty>,
) -> glyim_type::Ty {
    let mut fields: IndexVec<glyim_type::FieldIdx, glyim_type::FieldDef> = IndexVec::new();
    for (i, ty) in field_tys.into_iter().enumerate() {
        fields.push(glyim_type::FieldDef {
            name: ctx.resolver().intern(&format!("f{}", i)),
            ty,
        });
    }
    let def = glyim_type::AdtDef {
        kind: glyim_type::AdtKind::Struct,
        fields,
        variants: vec![glyim_type::VariantDef {
            name: ctx.resolver().intern("S"),
            style: glyim_type::adt_def::VariantStyle::Unit,
            fields: IndexVec::new(),
        }],
        generic_params: vec![],
    };
    ctx.register_adt(adt_id, def);
    let substs = ctx.intern_substitution(vec![]);
    ctx.mk_adt(adt_id, substs)
}

#[test]
fn s15_t01_struct_two_i32_fields() {
    let (ctx, struct_ty) = with_fresh_ty_ctx(|c| {
        let i32_ty = c.mk_ty(glyim_type::TyKind::Int(IntTy::I32));
        make_struct_ty(c, glyim_core::AdtId::from_raw(100), vec![i32_ty, i32_ty])
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let layout = computer
        .layout_of(struct_ty)
        .expect("struct layout should succeed");
    assert_eq!(layout.size, Size::bytes(8), "size mismatch");
    assert_eq!(layout.align, Align::from_bytes(4), "align mismatch");
    assert!(!layout.is_unsized, "struct should be sized");

    match &layout.fields {
        FieldsShape::Arbitrary { offsets } => {
            assert_eq!(offsets.len(), 2);
            assert_eq!(offsets[glyim_type::FieldIdx::from_raw(0)], Size::ZERO);
            assert_eq!(offsets[glyim_type::FieldIdx::from_raw(1)], Size::bytes(4));
        }
        _ => panic!("expected Arbitrary fields for struct"),
    }
    assert!(matches!(
        &layout.variants,
        VariantsShape::Single { index: 0 }
    ));
}

#[test]
fn s15_t01_struct_with_alignment_gap() {
    let (ctx, struct_ty) = with_fresh_ty_ctx(|c| {
        let u8_ty = c.mk_ty(glyim_type::TyKind::Uint(UintTy::U8));
        let i32_ty = c.mk_ty(glyim_type::TyKind::Int(IntTy::I32));
        make_struct_ty(c, glyim_core::AdtId::from_raw(101), vec![u8_ty, i32_ty])
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let layout = computer
        .layout_of(struct_ty)
        .expect("struct layout should succeed");
    assert_eq!(layout.size, Size::bytes(8), "size should be 8 with padding");
    assert_eq!(layout.align, Align::from_bytes(4), "align should be 4");
}

#[test]
fn s15_t01_struct_empty() {
    let (ctx, struct_ty) =
        with_fresh_ty_ctx(|c| make_struct_ty(c, glyim_core::AdtId::from_raw(102), vec![]));
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let layout = computer
        .layout_of(struct_ty)
        .expect("empty struct layout should succeed");
    assert_eq!(layout.size, Size::ZERO, "empty struct size should be 0");
    assert_eq!(layout.align, Align::ONE, "empty struct align should be 1");
}

#[test]
fn s15_t01_struct_single_bool() {
    let (ctx, struct_ty) = with_fresh_ty_ctx(|c| {
        make_struct_ty(c, glyim_core::AdtId::from_raw(103), vec![c.bool_ty()])
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let layout = computer
        .layout_of(struct_ty)
        .expect("struct layout should succeed");
    assert_eq!(layout.size, Size::bytes(1));
    assert_eq!(layout.align, Align::ONE);
}

#[test]
fn s15_t01_struct_three_fields_mixed() {
    let (ctx, struct_ty) = with_fresh_ty_ctx(|c| {
        let bool_ty = c.bool_ty();
        let i32_ty = c.mk_ty(glyim_type::TyKind::Int(IntTy::I32));
        let u8_ty = c.mk_ty(glyim_type::TyKind::Uint(UintTy::U8));
        make_struct_ty(
            c,
            glyim_core::AdtId::from_raw(104),
            vec![bool_ty, i32_ty, u8_ty],
        )
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let layout = computer
        .layout_of(struct_ty)
        .expect("struct layout should succeed");
    assert_eq!(
        layout.size,
        Size::bytes(12),
        "size should be 12 with padding"
    );
    assert_eq!(layout.align, Align::from_bytes(4), "align should be 4");
}

#[test]
fn s15_t01_struct_with_pointer() {
    let (ctx, struct_ty) = with_fresh_ty_ctx(|c| {
        let ref_ty = c.mk_ref(glyim_type::Region::Erased, c.bool_ty(), Mutability::Not);
        let i32_ty = c.mk_ty(glyim_type::TyKind::Int(IntTy::I32));
        make_struct_ty(c, glyim_core::AdtId::from_raw(105), vec![ref_ty, i32_ty])
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let layout = computer
        .layout_of(struct_ty)
        .expect("struct layout should succeed");
    assert_eq!(layout.size, Size::bytes(16), "size should be 16");
    assert_eq!(layout.align, Align::from_bytes(8), "align should be 8");
}
