//! Comprehensive tests for `TypeDisplay` and `compute_flags` behavior.
use super::helpers::with_fresh_ty_ctx;
use crate::display::PrintTy;
use crate::*;
use glyim_core::def_id::{AdtId, ClosureId, FnDefId, OpaqueTyId, TraitDefId};
use glyim_core::primitives::{Abi, FloatTy, IntTy, Mutability, Safety, UintTy};

// --- Display rendering tests ---

#[test]
fn display_all_scalar_types() {
    let (ctx, results) = with_fresh_ty_ctx(|ctx| {
        vec![
            ("bool", ctx.bool_ty()),
            ("!", ctx.never_ty()),
            ("()", ctx.unit_ty()),
            ("<error>", ctx.error_ty()),
            ("i8", ctx.mk_ty(TyKind::Int(IntTy::I8))),
            ("i16", ctx.mk_ty(TyKind::Int(IntTy::I16))),
            ("i32", ctx.mk_ty(TyKind::Int(IntTy::I32))),
            ("i64", ctx.mk_ty(TyKind::Int(IntTy::I64))),
            ("isize", ctx.mk_ty(TyKind::Int(IntTy::Isize))),
            ("u8", ctx.mk_ty(TyKind::Uint(UintTy::U8))),
            ("u16", ctx.mk_ty(TyKind::Uint(UintTy::U16))),
            ("u32", ctx.mk_ty(TyKind::Uint(UintTy::U32))),
            ("u64", ctx.mk_ty(TyKind::Uint(UintTy::U64))),
            ("usize", ctx.mk_ty(TyKind::Uint(UintTy::Usize))),
            ("f32", ctx.mk_ty(TyKind::Float(FloatTy::F32))),
            ("f64", ctx.mk_ty(TyKind::Float(FloatTy::F64))),
            ("char", ctx.mk_ty(TyKind::Char)),
            ("str", ctx.mk_ty(TyKind::String)),
        ]
    });
    for (expected, ty) in results {
        let printed = PrintTy::new(ty, &ctx).to_string();
        assert_eq!(printed, expected, "Mismatch for {expected}");
    }
}

#[test]
fn display_ref_and_raw_ptr() {
    let (ctx, results) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
        let ref_shared = ctx.mk_ref(Region::Erased, i32_ty, Mutability::Not);
        let ref_mut = ctx.mk_ref(Region::Erased, i32_ty, Mutability::Mut);
        let raw_const = ctx.mk_ty(TyKind::RawPtr(i32_ty, Mutability::Not));
        let raw_mut = ctx.mk_ty(TyKind::RawPtr(i32_ty, Mutability::Mut));
        vec![
            ("&i32", ref_shared),
            ("&mut i32", ref_mut),
            ("*const i32", raw_const),
            ("*mut i32", raw_mut),
        ]
    });
    for (expected, ty) in results {
        assert_eq!(PrintTy::new(ty, &ctx).to_string(), expected);
    }
}

#[test]
fn display_slice_and_array() {
    let (ctx, results) = with_fresh_ty_ctx(|ctx| {
        let u8_ty = ctx.mk_ty(TyKind::Uint(UintTy::U8));
        let slice = ctx.mk_ty(TyKind::Slice(u8_ty));
        let usize_ty = ctx.mk_ty(TyKind::Uint(UintTy::Usize));
        let arr = ctx.mk_ty(TyKind::Array(
            u8_ty,
            Const {
                kind: ConstKind::Uint(10),
                ty: usize_ty,
            },
        ));
        vec![("[u8]", slice), ("[u8; _]", arr)]
    });
    for (expected, ty) in results {
        assert_eq!(PrintTy::new(ty, &ctx).to_string(), expected);
    }
}

#[test]
fn display_tuple() {
    let (ctx, results) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
        let bool_ty = ctx.bool_ty();
        let unit_ty = ctx.unit_ty();
        let pair_subst =
            ctx.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(bool_ty)]);
        let triple_subst = ctx.intern_substitution(vec![
            GenericArg::Ty(i32_ty),
            GenericArg::Ty(bool_ty),
            GenericArg::Ty(unit_ty),
        ]);
        let empty_subst = ctx.intern_substitution(vec![]);
        vec![
            ("()", ctx.mk_ty(TyKind::Tuple(empty_subst))),
            ("(i32, bool)", ctx.mk_ty(TyKind::Tuple(pair_subst))),
            ("(i32, bool, ())", ctx.mk_ty(TyKind::Tuple(triple_subst))),
        ]
    });
    for (expected, ty) in results {
        assert_eq!(PrintTy::new(ty, &ctx).to_string(), expected);
    }
}

#[test]
fn display_fn_ptr_variants() {
    let (ctx, results) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
        let bool_ty = ctx.bool_ty();
        let empty_inputs = ctx.intern_substitution(vec![]);
        let one_input = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
        let two_inputs =
            ctx.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(bool_ty)]);
        let tys: Vec<(&str, Ty)> = vec![
            (
                "fn() -> bool",
                ctx.mk_ty(TyKind::FnPtr(FnSig {
                    inputs: empty_inputs,
                    output: bool_ty,
                    c_variadic: false,
                    unsafety: Safety::Safe,
                    abi: Abi::Glyim,
                })),
            ),
            (
                "unsafe fn(i32) -> bool",
                ctx.mk_ty(TyKind::FnPtr(FnSig {
                    inputs: one_input,
                    output: bool_ty,
                    c_variadic: false,
                    unsafety: Safety::Unsafe,
                    abi: Abi::Glyim,
                })),
            ),
            (
                "extern \"C\" fn(i32) -> i32",
                ctx.mk_ty(TyKind::FnPtr(FnSig {
                    inputs: one_input,
                    output: i32_ty,
                    c_variadic: false,
                    unsafety: Safety::Safe,
                    abi: Abi::C,
                })),
            ),
            (
                "fn(i32, bool) -> bool",
                ctx.mk_ty(TyKind::FnPtr(FnSig {
                    inputs: two_inputs,
                    output: bool_ty,
                    c_variadic: false,
                    unsafety: Safety::Safe,
                    abi: Abi::Glyim,
                })),
            ),
            (
                "fn(i32, ...) -> bool",
                ctx.mk_ty(TyKind::FnPtr(FnSig {
                    inputs: one_input,
                    output: bool_ty,
                    c_variadic: true,
                    unsafety: Safety::Safe,
                    abi: Abi::Glyim,
                })),
            ),
        ];
        tys
    });
    for (expected, ty) in results {
        assert_eq!(PrintTy::new(ty, &ctx).to_string(), expected);
    }
}

#[test]
fn display_adt_and_defs() {
    let (ctx, results) = with_fresh_ty_ctx(|ctx| {
        let adt_id = AdtId::from_raw(5);
        let fn_def_id = FnDefId::from_raw(10);
        let closure_id = ClosureId::from_raw(15);
        let opaque_id = OpaqueTyId::from_raw(20);
        let empty_subst = ctx.intern_substitution(vec![]);
        let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
        let subst = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
        vec![
            ("Adt5", ctx.mk_ty(TyKind::Adt(adt_id, empty_subst))),
            ("Adt5<i32>", ctx.mk_ty(TyKind::Adt(adt_id, subst))),
            ("FnDef10", ctx.mk_ty(TyKind::FnDef(fn_def_id, empty_subst))),
            ("FnDef10<...>", ctx.mk_ty(TyKind::FnDef(fn_def_id, subst))),
            (
                "Closure15",
                ctx.mk_ty(TyKind::Closure(closure_id, empty_subst)),
            ),
            (
                "Closure15<...>",
                ctx.mk_ty(TyKind::Closure(closure_id, subst)),
            ),
            (
                "Opaque20",
                ctx.mk_ty(TyKind::Opaque(opaque_id, empty_subst)),
            ),
            ("Opaque20<...>", ctx.mk_ty(TyKind::Opaque(opaque_id, subst))),
        ]
    });
    for (expected, ty) in results {
        assert_eq!(PrintTy::new(ty, &ctx).to_string(), expected);
    }
}

#[test]
fn display_dynamic() {
    let (ctx, ty) = with_fresh_ty_ctx(|ctx| {
        let trait_def_id = TraitDefId::from_raw(1);
        let pred = Predicate::Trait(TraitPredicate {
            trait_ref: TraitRef {
                def_id: trait_def_id,
                substs: ctx.intern_substitution(vec![]),
            },
            polarity: ImplPolarity::Positive,
        });
        let binder = Binder::bind(Box::new([pred]) as Box<[Predicate]>, Box::new([]));
        ctx.mk_ty(TyKind::Dynamic(binder, Region::Static))
    });
    let printed = PrintTy::new(ty, &ctx).to_string();
    assert!(printed.starts_with("dyn "));
    assert!(printed.ends_with(" + 'static"));
}

#[test]
fn display_param_and_bound() {
    let (ctx, results) = with_fresh_ty_ctx(|ctx| {
        let t_name = ctx.resolver().intern("T");
        let param = ctx.mk_ty(TyKind::Param(ParamTy {
            index: 0,
            name: t_name,
        }));
        let bound_anon = ctx.mk_ty(TyKind::Bound(
            1,
            BoundTy {
                var: 1,
                kind: BoundTyKind::Anon,
            },
        ));
        let bound_param = ctx.mk_ty(TyKind::Bound(
            2,
            BoundTy {
                var: 2,
                kind: BoundTyKind::Param(t_name),
            },
        ));
        vec![("T", param), ("?1", bound_anon), ("T", bound_param)]
    });
    for (expected, ty) in results {
        assert_eq!(PrintTy::new(ty, &ctx).to_string(), expected);
    }
}

#[test]
fn display_infer_types() {
    let (ctx, results) = with_fresh_ty_ctx(|ctx| {
        let ty_var = ctx.mk_ty(TyKind::Infer(InferVar::Ty(TyVar::from_raw(0))));
        let int_var = ctx.mk_ty(TyKind::Infer(InferVar::Int(IntVar::from_raw(1))));
        let float_var = ctx.mk_ty(TyKind::Infer(InferVar::Float(FloatVar::from_raw(2))));
        vec![("?ty0", ty_var), ("?int1", int_var), ("?float2", float_var)]
    });
    for (expected, ty) in results {
        assert_eq!(PrintTy::new(ty, &ctx).to_string(), expected);
    }
}

#[test]
fn display_depth_limit_enforced() {
    let (ctx, ty) = with_fresh_ty_ctx(|ctx| {
        let inner = ctx.mk_ty(TyKind::Int(IntTy::I32));
        // Create a deeply nested type: & & & ... i32 (15 levels)
        let mut current = inner;
        for _ in 0..15 {
            current = ctx.mk_ref(Region::Erased, current, Mutability::Not);
        }
        current
    });
    let printed = PrintTy::new(ty, &ctx).to_string();
    // Display goes 11 levels deep (depth 0..10 show "&", depth 11 shows "…")
    // because MAX_DISPLAY_DEPTH is 10; the 11th level triggers truncation.
    assert!(
        printed.ends_with("…"),
        "Expected truncation '…', got: {printed}"
    );
    assert!(
        !printed.starts_with("…"),
        "Truncation should be at the innermost level"
    );
    // Verify depth limiting kicked in: should be shorter than full 15 "&i32"
    assert!(printed.len() < 20, "Truncated output too long: {printed}");
}

// --- Flags correctness tests ---

#[test]
fn depth_overflow_does_not_set_has_error() {
    // compute_flags uses precomputed flags (constant-time lookup) and does NOT
    // recurse, so HAS_DEPTH_OVERFLOW is only triggered if compute_flags itself
    // is called with depth > 64. During normal type allocation, depth is always 0.
    // Deeply nested types can be allocated without triggering depth overflow.
    let (ctx, ty) = with_fresh_ty_ctx(|ctx| {
        let inner = ctx.bool_ty();
        let mut current = inner;
        for _ in 0..70 {
            current = ctx.mk_ref(Region::Erased, current, Mutability::Not);
        }
        current
    });
    let flags = ctx.ty_flags(ty);
    // HAS_DEPTH_OVERFLOW is NOT set during allocation (flags are precomputed)
    assert!(!flags.contains(TypeFlags::HAS_DEPTH_OVERFLOW));
    assert!(!flags.contains(TypeFlags::HAS_ERROR));
    assert!(!ctx.ty_is_error(ty));
    assert!(!ctx.ty_has_depth_overflow(ty));
}

#[test]
fn ty_is_error_only_checks_has_error() {
    let (ctx, error_ty) = with_fresh_ty_ctx(|ctx| ctx.error_ty());
    assert!(ctx.ty_is_error(error_ty));
    let (ctx, bool_ty) = with_fresh_ty_ctx(|ctx| ctx.bool_ty());
    assert!(!ctx.ty_is_error(bool_ty));
    // Deeply nested types do NOT have depth overflow from allocation
    // (compute_flags uses precomputed flags, no recursion).
    let (ctx, deep_ty) = with_fresh_ty_ctx(|ctx| {
        let inner = ctx.bool_ty();
        let mut current = inner;
        for _ in 0..70 {
            current = ctx.mk_ref(Region::Erased, current, Mutability::Not);
        }
        current
    });
    assert!(!ctx.ty_is_error(deep_ty));
    // Not an error, and no depth overflow from allocation
    assert!(!ctx.ty_has_depth_overflow(deep_ty));
}

#[test]
fn flags_propagate_through_wrappers() {
    let (ctx, results) = with_fresh_ty_ctx(|ctx| {
        let infer_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(TyVar::from_raw(0))));
        let ref_ty = ctx.mk_ref(Region::Erased, infer_ty, Mutability::Not);
        let slice_ty = ctx.mk_ty(TyKind::Slice(infer_ty));
        let usize_ty = ctx.mk_ty(TyKind::Uint(UintTy::Usize));
        let arr_ty = ctx.mk_ty(TyKind::Array(
            infer_ty,
            Const {
                kind: ConstKind::Uint(5),
                ty: usize_ty,
            },
        ));
        let raw_ty = ctx.mk_ty(TyKind::RawPtr(infer_ty, Mutability::Not));
        vec![
            ("ref", ref_ty),
            ("slice", slice_ty),
            ("array", arr_ty),
            ("raw_ptr", raw_ty),
        ]
    });
    for (name, ty) in results {
        let flags = ctx.ty_flags(ty);
        assert!(
            flags.contains(TypeFlags::HAS_TY_INFER),
            "{name} should have HAS_TY_INFER"
        );
    }
}

#[test]
fn flags_propagate_through_substitution_types() {
    let (ctx, results) = with_fresh_ty_ctx(|ctx| {
        let infer_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(TyVar::from_raw(0))));
        let subst = ctx.intern_substitution(vec![GenericArg::Ty(infer_ty)]);
        let adt_id = AdtId::from_raw(1);
        let fn_def_id = FnDefId::from_raw(1);
        let tuple_ty = ctx.mk_ty(TyKind::Tuple(subst));
        let adt_ty = ctx.mk_ty(TyKind::Adt(adt_id, subst));
        let fn_def_ty = ctx.mk_ty(TyKind::FnDef(fn_def_id, subst));
        let fn_sig = FnSig {
            inputs: subst,
            output: ctx.bool_ty(),
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::Glyim,
        };
        let fn_ptr_ty = ctx.mk_ty(TyKind::FnPtr(fn_sig));
        vec![
            ("tuple", tuple_ty),
            ("adt", adt_ty),
            ("fn_def", fn_def_ty),
            ("fn_ptr", fn_ptr_ty),
        ]
    });
    for (name, ty) in results {
        let flags = ctx.ty_flags(ty);
        assert!(
            flags.contains(TypeFlags::HAS_TY_INFER),
            "{name} should propagate HAS_TY_INFER"
        );
    }
}

#[test]
fn has_error_flag_independent_of_depth_overflow() {
    let (ctx, error_ty) = with_fresh_ty_ctx(|ctx| ctx.error_ty());
    assert!(ctx.ty_flags(error_ty).contains(TypeFlags::HAS_ERROR));
    assert!(
        !ctx.ty_flags(error_ty)
            .contains(TypeFlags::HAS_DEPTH_OVERFLOW)
    );
}

#[test]
fn display_lifetime_and_const_in_substitution() {
    let (ctx, ty) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
        let usize_ty = ctx.mk_ty(TyKind::Uint(UintTy::Usize));
        let subst = ctx.intern_substitution(vec![
            GenericArg::Ty(i32_ty),
            GenericArg::Lifetime(Region::Static),
            GenericArg::Const(Const {
                kind: ConstKind::Uint(42),
                ty: usize_ty,
            }),
        ]);
        ctx.mk_ty(TyKind::Tuple(subst))
    });
    let printed = PrintTy::new(ty, &ctx).to_string();
    // Tuple with ty, lifetime, const args: "(i32, '_, {const})"
    assert_eq!(printed, "(i32, '_, {const})");
}
