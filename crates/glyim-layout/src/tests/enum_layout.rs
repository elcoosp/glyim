//! S15-T02: layout_of computes niche encoding for enums

use crate::*;
use glyim_core::arena::IndexVec;
use glyim_core::primitives::*;
use glyim_test::with_fresh_ty_ctx;

/// Helper: build an enum AdtDef with given variants, register it.
fn make_enum_ty(
    ctx: &mut glyim_type::TyCtxMut,
    adt_id: glyim_core::AdtId,
    variants: Vec<Vec<glyim_type::Ty>>,
) -> glyim_type::Ty {
    let variant_defs: Vec<glyim_type::VariantDef> = variants
        .into_iter()
        .enumerate()
        .map(|(vi, field_tys)| {
            let mut fields: IndexVec<glyim_type::FieldIdx, glyim_type::FieldDef> = IndexVec::new();
            for (fi, ty) in field_tys.into_iter().enumerate() {
                fields.push(glyim_type::FieldDef {
                    name: ctx.resolver().intern(&format!("v{}_f{}", vi, fi)),
                    ty,
                });
            }
            glyim_type::VariantDef {
                name: ctx.resolver().intern(&format!("V{}", vi)),
                style: glyim_type::adt_def::VariantStyle::Unit,
                fields,
            }
        })
        .collect();

    let mut all_field_tys: Vec<glyim_type::Ty> = Vec::new();
    for v in &variant_defs {
        for f in v.fields.iter() {
            all_field_tys.push(f.ty);
        }
    }
    let mut top_fields: IndexVec<glyim_type::FieldIdx, glyim_type::FieldDef> = IndexVec::new();
    for (i, ty) in all_field_tys.into_iter().enumerate() {
        top_fields.push(glyim_type::FieldDef {
            name: ctx.resolver().intern(&format!("f{}", i)),
            ty,
        });
    }

    let def = glyim_type::AdtDef {
        kind: glyim_type::AdtKind::Enum,
        fields: top_fields,
        variants: variant_defs,
        generic_params: vec![],
    };
    ctx.register_adt(adt_id, def);
    let substs = ctx.intern_substitution(vec![]);
    ctx.mk_adt(adt_id, substs)
}

#[test]
fn s15_t02_enum_simple_two_variants_no_data() {
    let (ctx, enum_ty) = with_fresh_ty_ctx(|c| {
        make_enum_ty(c, glyim_core::AdtId::from_raw(200), vec![vec![], vec![]])
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let layout = computer
        .layout_of(enum_ty)
        .expect("enum layout should succeed");

    assert!(!layout.is_unsized);
    match &layout.variants {
        VariantsShape::Multiple {
            tag_encoding,
            variants,
            ..
        } => {
            assert_eq!(variants.len(), 2);
            let _ = tag_encoding;
        }
        VariantsShape::Single { .. } => panic!("enum should have Multiple variants"),
    }
    assert!(
        layout.size.0 >= 1,
        "enum should have at least 1 byte for discriminant"
    );
}

#[test]
fn s15_t02_enum_with_data_variants() {
    let (ctx, enum_ty) = with_fresh_ty_ctx(|c| {
        let i32_ty = c.mk_ty(glyim_type::TyKind::Int(IntTy::I32));
        let bool_ty = c.bool_ty();
        make_enum_ty(
            c,
            glyim_core::AdtId::from_raw(201),
            vec![vec![i32_ty], vec![bool_ty]],
        )
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let layout = computer
        .layout_of(enum_ty)
        .expect("enum layout should succeed");

    match &layout.variants {
        VariantsShape::Multiple {
            tag,
            tag_field,
            tag_encoding,
            variants,
            ..
        } => {
            let tag_kind = ctx.ty_kind(*tag);
            assert!(
                matches!(
                    tag_kind,
                    glyim_type::TyKind::Int(_)
                        | glyim_type::TyKind::Uint(_)
                        | glyim_type::TyKind::Bool
                ),
                "tag should be an integer or bool type, got {:?}",
                tag_kind
            );
            assert_eq!(*tag_field, 0);
            let _ = tag_encoding;
            assert_eq!(variants.len(), 2);
        }
        VariantsShape::Single { .. } => panic!("enum should have Multiple variants"),
    }
    assert!(
        layout.size.0 >= 4,
        "enum with i32 variant should be at least 4 bytes"
    );
}

#[test]
fn s15_t02_enum_niche_option_bool() {
    let (ctx, enum_ty) = with_fresh_ty_ctx(|c| {
        let bool_ty = c.bool_ty();
        make_enum_ty(
            c,
            glyim_core::AdtId::from_raw(202),
            vec![vec![bool_ty], vec![]],
        )
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let layout = computer
        .layout_of(enum_ty)
        .expect("enum layout should succeed");

    match &layout.variants {
        VariantsShape::Multiple { tag_encoding, .. } => match tag_encoding {
            TagEncoding::Niche {
                untagged_variant,
                niche_variants,
                niche_start,
            } => {
                assert_eq!(*untagged_variant, 0);
                assert_eq!(niche_variants.clone(), 1..=1);
                assert_eq!(*niche_start, 2);
            }
            TagEncoding::Direct => {}
        },
        VariantsShape::Single { .. } => panic!("enum should have Multiple variants"),
    }
    assert!(layout.size.0 <= 2, "Option<bool> should be at most 2 bytes");
}

#[test]
fn s15_t02_enum_three_variants_no_data() {
    let (ctx, enum_ty) = with_fresh_ty_ctx(|c| {
        make_enum_ty(
            c,
            glyim_core::AdtId::from_raw(203),
            vec![vec![], vec![], vec![]],
        )
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let layout = computer
        .layout_of(enum_ty)
        .expect("enum layout should succeed");

    assert!(layout.size.0 >= 1);
    match &layout.variants {
        VariantsShape::Multiple { variants, .. } => {
            assert_eq!(variants.len(), 3);
        }
        VariantsShape::Single { .. } => panic!("should have Multiple variants"),
    }
}

#[test]
fn s15_t02_enum_large_discriminant() {
    let (ctx, enum_ty) = with_fresh_ty_ctx(|c| {
        let variants: Vec<Vec<glyim_type::Ty>> = (0..256).map(|_| vec![]).collect();
        make_enum_ty(c, glyim_core::AdtId::from_raw(204), variants)
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let layout = computer
        .layout_of(enum_ty)
        .expect("enum layout should succeed");
    assert!(layout.size.0 >= 1);
}

#[test]
fn s15_t02_enum_257_variants_needs_u16() {
    let (ctx, enum_ty) = with_fresh_ty_ctx(|c| {
        let variants: Vec<Vec<glyim_type::Ty>> = (0..257).map(|_| vec![]).collect();
        make_enum_ty(c, glyim_core::AdtId::from_raw(205), variants)
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let layout = computer
        .layout_of(enum_ty)
        .expect("enum layout should succeed");
    assert!(
        layout.size.0 >= 2,
        "257-variant enum needs at least 2 bytes for u16 tag"
    );
}

#[test]
fn s15_t02_enum_nested_option_niche() {
    // §7.2: a nested enum must reuse the inner enum's *free* discriminant
    // values as its own niche, so Option<Option<bool>> collapses to 1 byte.
    let (ctx, outer_ty) = with_fresh_ty_ctx(|c| {
        let bool_ty = c.bool_ty();
        // Option<bool> = enum { None, Some(bool) }
        let option_bool = make_enum_ty(
            c,
            glyim_core::AdtId::from_raw(206),
            vec![vec![bool_ty], vec![]],
        );
        // Option<Option<bool>> = enum { None, Some(Option<bool>) }
        make_enum_ty(
            c,
            glyim_core::AdtId::from_raw(207),
            vec![vec![option_bool], vec![]],
        )
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let layout = computer
        .layout_of(outer_ty)
        .expect("nested enum layout should succeed");

    assert_eq!(
        layout.size.0, 1,
        "Option<Option<bool>> must collapse to a single byte via recursive niche detection"
    );
    match &layout.variants {
        VariantsShape::Multiple { tag_encoding, .. } => match tag_encoding {
            TagEncoding::Niche { .. } => {}
            TagEncoding::Direct => panic!("nested Option should be niche-encoded"),
        },
        VariantsShape::Single { .. } => panic!("should have Multiple variants"),
    }
}

#[test]
fn s15_t02_enum_direct_tag_recursive_niche() {
    // A Direct-tag enum (no field niche) still exposes a free discriminant
    // tail an enclosing enum can reuse.
    let (ctx, outer_ty) = with_fresh_ty_ctx(|c| {
        // Inner: 3-variant fieldless enum -> Direct U8 tag, uses values 0,1,2,
        // leaving [3,255) free.
        let inner = make_enum_ty(
            c,
            glyim_core::AdtId::from_raw(208),
            vec![vec![], vec![], vec![]],
        );
        // Outer: Option<inner>
        make_enum_ty(
            c,
            glyim_core::AdtId::from_raw(209),
            vec![vec![inner], vec![]],
        )
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let layout = computer
        .layout_of(outer_ty)
        .expect("nested enum layout should succeed");
    // inner is a Direct-tag 3-variant enum; the recursive niche must keep the
    // outer enum to a single byte.
    assert_eq!(
        layout.size.0, 1,
        "Option<3-variant-enum> must collapse to a single byte via recursive niche detection"
    );
}
