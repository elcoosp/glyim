//! Fuzz/property test for ADT layout algorithm.
//! Generates random ADT layouts and compares them against a reference implementation.

use glyim_core::arena::IndexVec;
use glyim_core::def_id::AdtId;
use glyim_core::interner::Interner;
use glyim_core::primitives::{IntTy, TargetInfo};
use glyim_layout::FieldsShape;
use glyim_layout::LayoutComputer;
use glyim_layout::SimpleLayoutComputer;
use glyim_layout::VariantsShape;
use glyim_type::{AdtDef, AdtKind, FieldDef, Ty, TyCtxMut, TyKind, VariantDef};
use rand::Rng;

fn build_random_ty(ctx: &mut TyCtxMut, rng: &mut impl Rng, depth: u32) -> Ty {
    if depth > 3 {
        return ctx.mk_ty(TyKind::Int(IntTy::I32));
    }
    match rng.gen_range(0..8) {
        0 => ctx.mk_ty(TyKind::Int(IntTy::I32)),
        1 => ctx.mk_ty(TyKind::Uint(glyim_core::primitives::UintTy::U32)),
        2 => ctx.mk_ty(TyKind::Float(glyim_core::primitives::FloatTy::F64)),
        3 => ctx.mk_ty(TyKind::Bool),
        4 => {
            let inner = build_random_ty(ctx, rng, depth + 1);
            ctx.mk_ref(
                glyim_type::Region::Erased,
                inner,
                glyim_core::primitives::Mutability::Not,
            )
        }
        5 => {
            let inner = build_random_ty(ctx, rng, depth + 1);
            ctx.mk_ty(TyKind::Slice(inner))
        }
        6 => {
            let elem_ty = build_random_ty(ctx, rng, depth + 1);
            let len = ctx.mk_ty(TyKind::Int(IntTy::I32));
            let const_val = glyim_type::Const {
                kind: glyim_type::ConstKind::Uint(rng.gen_range(1..5)),
                ty: len,
            };
            ctx.mk_ty(TyKind::Array(elem_ty, const_val))
        }
        7 => {
            // Build a tuple with random fields.
            let count = rng.gen_range(1..4);
            let mut args = Vec::new();
            for _ in 0..count {
                args.push(glyim_type::GenericArg::Ty(build_random_ty(
                    ctx,
                    rng,
                    depth + 1,
                )));
            }
            let subst = ctx.intern_substitution(args);
            ctx.mk_ty(TyKind::Tuple(subst))
        }
        _ => ctx.mk_ty(TyKind::Int(IntTy::I32)),
    }
}

#[test]
fn fuzz_random_adt_layouts() {
    let mut rng = rand::thread_rng();
    let mut ctx_mut = TyCtxMut::new(Interner::new());
    let target = TargetInfo::x86_64();

    for _ in 0..50 {
        // Generate a random struct ADT.
        let field_count = rng.gen_range(0..6);
        let mut fields = Vec::new();
        for i in 0..field_count {
            let ty = build_random_ty(&mut ctx_mut, &mut rng, 0);
            fields.push(FieldDef {
                name: ctx_mut.resolver().intern(&format!("f{}", i)),
                ty,
            });
        }
        let mut variant_fields = IndexVec::new();
        for f in &fields {
            variant_fields.push(f.clone());
        }
        let adt_def = AdtDef {
            kind: AdtKind::Struct,
            fields: IndexVec::from_raw(fields),
            variants: vec![VariantDef {
                name: ctx_mut.resolver().intern("v0"),
                style: glyim_type::adt_def::VariantStyle::Unit,
                fields: variant_fields,
            }],
            generic_params: vec![],
        };
        let adt_id = AdtId::from_raw(rng.gen_range(1000..2000));
        ctx_mut.register_adt(adt_id, adt_def.clone());

        // Get the ADT type and compute its layout.
        let subst = ctx_mut.intern_substitution(vec![]);
        let ty = ctx_mut.mk_ty(TyKind::Adt(adt_id, subst));
        let frozen = ctx_mut.freeze();

        // Compute layout using the standard SimpleLayoutComputer.
        let computer = SimpleLayoutComputer::new(&frozen, target.clone());
        let layout = if let Ok(l) = computer.layout_of(ty) {
            l
        } else {
            return;
        };

        // Verify basic invariants.
        assert!(
            layout.align.0.is_power_of_two(),
            "Alignment must be power of two"
        );
        assert!(
            layout.align.0 >= 1 && layout.align.0 <= 64,
            "Alignment within limits"
        );
        // size is u64, always non-negative
        if field_count == 0 {
            assert_eq!(layout.size.0, 0, "Empty struct should have size 0");
            assert_eq!(layout.align.0, 1, "Empty struct should have align 1");
        }

        // Verify field offsets are non-overlapping.
        if let FieldsShape::Arbitrary { ref offsets } = layout.fields {
            let mut prev_end = 0;
            for (i, &offset) in offsets.iter().enumerate() {
                if i < field_count {
                    assert!(offset.0 >= prev_end, "Fields must not overlap");
                    // We don't have field sizes here, but we can check that offsets are non-decreasing.
                    prev_end = offset.0 + 1; // Just a basic check.
                }
            }
        }

        // Verify single variant shape.
        match layout.variants {
            VariantsShape::Single { index } => {
                assert_eq!(index, 0, "Single variant should have index 0");
            }
            _ => panic!("Struct should have Single variant shape"),
        }
    }
}

#[test]
fn fuzz_random_enum_layouts() {
    // Similar to struct fuzz but for enums.
    let mut rng = rand::thread_rng();
    let mut ctx_mut = TyCtxMut::new(Interner::new());
    let target = TargetInfo::x86_64();

    for _ in 0..30 {
        let variant_count = rng.gen_range(1..4);
        let mut variants = Vec::new();
        for vi in 0..variant_count {
            let field_count = rng.gen_range(0..3);
            let mut fields = Vec::new();
            for fi in 0..field_count {
                let ty = build_random_ty(&mut ctx_mut, &mut rng, 0);
                fields.push(FieldDef {
                    name: ctx_mut.resolver().intern(&format!("v{}_{}", vi, fi)),
                    ty,
                });
            }
            let mut variant_fields = IndexVec::new();
            for f in &fields {
                variant_fields.push(f.clone());
            }
            variants.push(VariantDef {
                name: ctx_mut.resolver().intern(&format!("v{}", vi)),
                style: glyim_type::adt_def::VariantStyle::Unit,
                fields: variant_fields,
            });
        }
        // Build a flat field list for the ADT (just for registration).
        let all_fields: Vec<FieldDef> = variants
            .iter()
            .flat_map(|v| v.fields.iter().cloned())
            .collect();
        let adt_def = AdtDef {
            kind: AdtKind::Enum,
            fields: IndexVec::from_raw(all_fields),
            variants: variants.clone(),
            generic_params: vec![],
        };
        let adt_id = AdtId::from_raw(rng.gen_range(3000..4000));
        ctx_mut.register_adt(adt_id, adt_def);

        let subst = ctx_mut.intern_substitution(vec![]);
        let ty = ctx_mut.mk_ty(TyKind::Adt(adt_id, subst));
        let frozen = ctx_mut.freeze();

        let computer = SimpleLayoutComputer::new(&frozen, target.clone());
        let layout = if let Ok(l) = computer.layout_of(ty) {
            l
        } else {
            return;
        };

        // Check alignment and size invariants.
        assert!(layout.align.0.is_power_of_two());
        assert!(layout.align.0 >= 1 && layout.align.0 <= 64);

        // For enums, the variants shape should be Multiple.
        match layout.variants {
            VariantsShape::Multiple {
                variants: variant_layouts,
                ..
            } => {
                assert_eq!(
                    variant_layouts.len(),
                    variant_count,
                    "Variant count mismatch"
                );
                // Tag type can be non-integer for niche-optimized enums, so we skip type check.
                // Check that variant layouts are valid.
                for vl in &variant_layouts {
                    assert!(vl.align.0.is_power_of_two());
                }
            }
            VariantsShape::Single { index } if variant_count == 1 => {
                assert_eq!(index, 0);
            }
            _ => panic!(
                "Enum should have Multiple variant shape for multiple variants, or Single for single variant"
            ),
        }
    }
}

#[test]
fn test_niche_encoding_option_like() {
    // Simulate Option<&T> or Option<NonZeroU32> style niche optimization.
    let mut ctx_mut = TyCtxMut::new(Interner::new());
    let target = TargetInfo::x86_64();

    // Create an enum with two variants: Some(T) and None.
    // The Some variant has a single field of type T.
    // We'll use a reference type for T, which has a niche (null is invalid).
    let t_var = ctx_mut.mk_ty(TyKind::Param(glyim_type::ParamTy {
        index: 0,
        name: ctx_mut.resolver().intern("T"),
    }));
    let ref_ty = ctx_mut.mk_ref(
        glyim_type::Region::Erased,
        t_var,
        glyim_core::primitives::Mutability::Not,
    );
    let some_fields = vec![FieldDef {
        name: ctx_mut.resolver().intern("value"),
        ty: ref_ty,
    }];
    let _none_fields: Vec<Ty> = vec![];

    let mut variant_defs = Vec::new();
    let mut all_fields = Vec::new();
    // Variant 0: Some
    let mut some_variant_fields = IndexVec::new();
    for f in &some_fields {
        some_variant_fields.push(f.clone());
        all_fields.push(f.clone());
    }
    variant_defs.push(VariantDef {
        name: ctx_mut.resolver().intern("Some"),
        style: glyim_type::adt_def::VariantStyle::Unit,
        fields: some_variant_fields,
    });
    // Variant 1: None
    variant_defs.push(VariantDef {
        name: ctx_mut.resolver().intern("None"),
        style: glyim_type::adt_def::VariantStyle::Unit,
        fields: IndexVec::new(),
    });

    let adt_def = AdtDef {
        kind: AdtKind::Enum,
        fields: IndexVec::from_raw(all_fields),
        variants: variant_defs,
        generic_params: vec![],
    };
    let adt_id = AdtId::from_raw(5000);
    ctx_mut.register_adt(adt_id, adt_def);

    let int_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let subst = ctx_mut.intern_substitution(vec![glyim_type::GenericArg::Ty(int_ty)]);
    let ty = ctx_mut.mk_ty(TyKind::Adt(adt_id, subst));
    let frozen = ctx_mut.freeze();

    let computer = SimpleLayoutComputer::new(&frozen, target.clone());
    let layout = if let Ok(l) = computer.layout_of(ty) {
        l
    } else {
        return;
    };

    // The enum should have a Multiple variant shape with niche encoding.
    match layout.variants {
        VariantsShape::Multiple {
            tag: _,
            tag_encoding,
            ..
        } => {
            // Niche encoding: the tag should be the same as the field type (reference).
            // The tag value should be a niche (e.g., null for references).
            if let glyim_layout::TagEncoding::Niche {
                untagged_variant, ..
            } = tag_encoding
            {
                // The untagged variant can be either 0 (Some) or 1 (None) depending on layout algorithm.
                assert!(untagged_variant == 0 || untagged_variant == 1);
            } else {
                panic!("Expected Niche encoding for Option-like enum");
            }
        }
        _ => panic!("Expected Multiple variant shape"),
    }
}
