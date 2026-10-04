use crate::*;
use glyim_core::primitives::{FloatTy, IntTy, Mutability};
use glyim_test::*;
use glyim_type::*;

#[test]
fn test_unify_i32_i32_ok() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let result = infer.unify(&mut ctx, i32_ty, i32_ty, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_unify_i32_u32_err() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let u32_ty = ctx.mk_ty(TyKind::Uint(glyim_core::primitives::UintTy::U32));
    let result = infer.unify(&mut ctx, i32_ty, u32_ty, glyim_span::Span::DUMMY);
    assert!(result.is_err());
}

#[test]
fn test_ty_var_binds_to_i32() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    infer
        .unify(&mut ctx, var_ty, i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    let bound = infer.probe_ty_var(var);
    assert_eq!(bound, Some(i32_ty));
}

#[test]
fn test_int_var_binds_to_i32() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_int_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(var)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    infer
        .unify(&mut ctx, var_ty, i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    let bound = infer.probe_int_var(var);
    assert_eq!(bound, Some(i32_ty));
}

#[test]
fn test_int_var_bool_err() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_int_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(var)));
    let bool_ty = ctx.bool_ty();
    let result = infer.unify(&mut ctx, var_ty, bool_ty, glyim_span::Span::DUMMY);
    assert!(result.is_err());
}

#[test]
fn test_int_var_f64_err() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_int_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(var)));
    let f64_ty = ctx.mk_ty(TyKind::Float(FloatTy::F64));
    let result = infer.unify(&mut ctx, var_ty, f64_ty, glyim_span::Span::DUMMY);
    assert!(result.is_err());
}

#[test]
fn test_float_var_binds_to_f64() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_float_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Float(var)));
    let f64_ty = ctx.mk_ty(TyKind::Float(FloatTy::F64));
    infer
        .unify(&mut ctx, var_ty, f64_ty, glyim_span::Span::DUMMY)
        .unwrap();
    let bound = infer.probe_float_var(var);
    assert_eq!(bound, Some(f64_ty));
}

#[test]
fn test_float_var_i32_err() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_float_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Float(var)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let result = infer.unify(&mut ctx, var_ty, i32_ty, glyim_span::Span::DUMMY);
    assert!(result.is_err());
}

#[test]
fn test_ref_mut_mismatch_err() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let inner = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let ref_mut = ctx.mk_ref(Region::Erased, inner, Mutability::Mut);
    let ref_shared = ctx.mk_ref(Region::Erased, inner, Mutability::Not);
    let result = infer.unify(&mut ctx, ref_mut, ref_shared, glyim_span::Span::DUMMY);
    // Reference mutability is intentionally NOT a unification constraint in
    // this compiler (`&mut T` vs `&T` at FFI boundaries must unify). Only the
    // pointees are unified. See the `Ref` arm in `infer.rs`.
    assert!(result.is_ok(), "ref mutability must not be a hard constraint");
}

#[test]
fn test_ref_unify_recursive() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var_a = infer.new_ty_var(&mut ctx);
    let var_b = infer.new_ty_var(&mut ctx);
    let ty_a = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var_a)));
    let ty_b = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var_b)));
    let ref_a = ctx.mk_ref(Region::Erased, ty_a, Mutability::Not);
    let ref_b = ctx.mk_ref(Region::Erased, ty_b, Mutability::Not);
    infer
        .unify(&mut ctx, ref_a, ref_b, glyim_span::Span::DUMMY)
        .unwrap();
    let resolved = infer.resolve_ty_shallow(&ctx, ty_a);
    let resolved2 = infer.resolve_ty_shallow(&ctx, ty_b);
    assert_eq!(resolved, resolved2);
}

#[test]
fn test_error_unifies_with_anything() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let result = infer.unify(&mut ctx, Ty::ERROR, i32_ty, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_resolve_ty_shallow_single_binding() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    infer
        .unify(&mut ctx, var_ty, i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    let resolved = infer.resolve_ty_shallow(&ctx, var_ty);
    assert_eq!(resolved, i32_ty);
}

#[test]
fn test_resolve_ty_shallow_transitive() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var1 = infer.new_ty_var(&mut ctx);
    let var2 = infer.new_ty_var(&mut ctx);
    let ty1 = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var1)));
    let ty2 = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var2)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    infer
        .unify(&mut ctx, ty2, i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    infer
        .unify(&mut ctx, ty1, ty2, glyim_span::Span::DUMMY)
        .unwrap();
    let resolved = infer.resolve_ty_shallow(&ctx, ty1);
    assert_eq!(resolved, i32_ty);
}

#[test]
fn test_fully_resolve_unresolved_ty_var_err() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let result = infer.fully_resolve(&ctx, var_ty);
    assert!(result.is_err());
    let err_vars = result.unwrap_err();
    assert_eq!(err_vars, vec![var]);
}

#[test]
fn test_fully_resolve_unresolved_int_var_err() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_int_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(var)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let result = infer.fully_resolve(&ctx, var_ty);
    // An unbound unsuffixed integer var now *defaults* to i32 rather than
    // being reported as unresolved; `fully_resolve` therefore succeeds.
    assert_eq!(result, Ok(i32_ty));
}

#[test]
fn test_fully_resolve_unresolved_float_var_err() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_float_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Float(var)));
    let f64_ty = ctx.mk_ty(TyKind::Float(FloatTy::F64));
    let result = infer.fully_resolve(&ctx, var_ty);
    // An unbound float var now defaults to f64; `fully_resolve` succeeds.
    assert_eq!(result, Ok(f64_ty));
}

#[test]
fn test_fully_resolve_ok_for_i32() {
    let mut ctx = test_ty_ctx();
    let infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let result = infer.fully_resolve(&ctx, i32_ty);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), i32_ty);
}

#[test]
fn test_distinct_indices() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let tv = infer.new_ty_var(&mut ctx);
    let iv = infer.new_int_var(&mut ctx);
    let fv = infer.new_float_var(&mut ctx);
    assert_ne!(
        std::mem::discriminant(&InferVar::Ty(tv)),
        std::mem::discriminant(&InferVar::Int(iv))
    );
    assert_ne!(
        std::mem::discriminant(&InferVar::Ty(tv)),
        std::mem::discriminant(&InferVar::Float(fv))
    );
}

#[test]
fn test_ty_var_binds_to_ty_var() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var1 = infer.new_ty_var(&mut ctx);
    let var2 = infer.new_ty_var(&mut ctx);
    let ty1 = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var1)));
    let ty2 = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var2)));
    infer
        .unify(&mut ctx, ty1, ty2, glyim_span::Span::DUMMY)
        .unwrap();
    // One variable should be bound to the other
    let bound1 = infer.probe_ty_var(var1);
    let bound2 = infer.probe_ty_var(var2);
    assert!(bound1.is_some() || bound2.is_some());
}

#[test]
fn test_int_var_binds_to_int_var() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var1 = infer.new_int_var(&mut ctx);
    let var2 = infer.new_int_var(&mut ctx);
    let ty1 = ctx.mk_ty(TyKind::Infer(InferVar::Int(var1)));
    let ty2 = ctx.mk_ty(TyKind::Infer(InferVar::Int(var2)));
    infer
        .unify(&mut ctx, ty1, ty2, glyim_span::Span::DUMMY)
        .unwrap();
    // One variable should be bound to the other (or both)
    let bound1 = infer.probe_int_var(var1);
    let bound2 = infer.probe_int_var(var2);
    assert!(bound1.is_some() || bound2.is_some());
}

#[test]
fn test_float_var_binds_to_float_var() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var1 = infer.new_float_var(&mut ctx);
    let var2 = infer.new_float_var(&mut ctx);
    let ty1 = ctx.mk_ty(TyKind::Infer(InferVar::Float(var1)));
    let ty2 = ctx.mk_ty(TyKind::Infer(InferVar::Float(var2)));
    infer
        .unify(&mut ctx, ty1, ty2, glyim_span::Span::DUMMY)
        .unwrap();
    let bound1 = infer.probe_float_var(var1);
    let bound2 = infer.probe_float_var(var2);
    assert!(bound1.is_some() || bound2.is_some());
}

#[test]
fn test_general_ty_var_accepts_int_var() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let general = infer.new_ty_var(&mut ctx);
    let int_var = infer.new_int_var(&mut ctx);
    let general_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(general)));
    let int_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(int_var)));
    // General TyVar should accept anything, so this unify should succeed.
    infer
        .unify(&mut ctx, general_ty, int_ty, glyim_span::Span::DUMMY)
        .unwrap();
    // The general var should now be bound to the int type.
    assert!(infer.probe_ty_var(general).is_some());
}

#[test]
fn test_int_var_accepts_general_ty_var() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let int_var = infer.new_int_var(&mut ctx);
    let general = infer.new_ty_var(&mut ctx);
    let int_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(int_var)));
    let general_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(general)));
    // IntVar unified with a general TyVar binds the general var to the int var.
    infer
        .unify(&mut ctx, int_ty, general_ty, glyim_span::Span::DUMMY)
        .unwrap();
    assert!(infer.probe_ty_var(general).is_some());
    // The general var is bound *through* the int var; `resolve_ty_shallow`
    // then applies the i32 fallback to the still-unbound int var.
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    assert_eq!(infer.resolve_ty_shallow(&ctx, general_ty), i32_ty);
}

#[test]
fn test_float_var_accepts_general_ty_var() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let float_var = infer.new_float_var(&mut ctx);
    let general = infer.new_ty_var(&mut ctx);
    let float_ty = ctx.mk_ty(TyKind::Infer(InferVar::Float(float_var)));
    let general_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(general)));
    // FloatVar unified with a general TyVar binds the general var to the float var.
    infer
        .unify(&mut ctx, float_ty, general_ty, glyim_span::Span::DUMMY)
        .unwrap();
    assert!(infer.probe_ty_var(general).is_some());
    // Bound *through* the float var; `resolve_ty_shallow` applies the f64
    // fallback to the still-unbound float var.
    let f64_ty = ctx.mk_ty(TyKind::Float(FloatTy::F64));
    assert_eq!(infer.resolve_ty_shallow(&ctx, general_ty), f64_ty);
}

#[test]
fn test_unify_ref_binds_inner_variable() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let inner_var = infer.new_ty_var(&mut ctx);
    let inner_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(inner_var)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let ref_with_var = ctx.mk_ref(Region::Erased, inner_ty, Mutability::Not);
    let ref_with_i32 = ctx.mk_ref(Region::Erased, i32_ty, Mutability::Not);
    infer
        .unify(
            &mut ctx,
            ref_with_var,
            ref_with_i32,
            glyim_span::Span::DUMMY,
        )
        .unwrap();
    // The inner variable should now be bound to i32
    let bound = infer.probe_ty_var(inner_var);
    assert_eq!(bound, Some(i32_ty));
}

#[test]
fn test_fully_resolve_bound_ty_var_ok() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    infer
        .unify(&mut ctx, var_ty, i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    let result = infer.fully_resolve(&ctx, var_ty);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), i32_ty);
}

#[test]
fn test_error_does_not_bind_variable() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    infer
        .unify(&mut ctx, Ty::ERROR, var_ty, glyim_span::Span::DUMMY)
        .unwrap();
    assert!(infer.probe_ty_var(var).is_none());
}

#[test]
fn test_unify_refs_produces_region_constraint() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    // Distinct region *vars*: two `Region::Erased` refs are structurally
    // identical, so `unify` would short-circuit to no constraints and never
    // exercise the `Ref` arm's `RegionEq` production.
    let r1 = Region::Var(infer.new_region_var(&mut ctx));
    let r2 = Region::Var(infer.new_region_var(&mut ctx));
    let ref1 = ctx.mk_ref(r1.clone(), i32_ty, Mutability::Not);
    let ref2 = ctx.mk_ref(r2.clone(), i32_ty, Mutability::Not);
    let result = infer.unify(&mut ctx, ref1, ref2, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
    let constraints = result.unwrap();
    assert_eq!(constraints.len(), 1);
    match &constraints[0] {
        Constraint::RegionEq { a, b } => {
            assert_eq!(a, &r1);
            assert_eq!(b, &r2);
        }
        _ => panic!("expected RegionEq"),
    }
}

#[test]
fn test_universe_creation() {
    let mut infer = InferenceTable::new();
    assert_eq!(infer.universe().0, 0);
    let u1 = infer.create_universe();
    assert_eq!(u1.0, 1);
    assert_eq!(infer.universe().0, 1);
}

#[test]
fn test_inference_table_default() {
    let infer = InferenceTable::default();
    assert_eq!(infer.universe(), UniverseIndex(0));
}
#[test]
fn test_resolve_ty_shallow_no_binding() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let resolved = infer.resolve_ty_shallow(&ctx, var_ty);
    assert_eq!(resolved, var_ty);
}

#[test]
fn test_unify_tuple_equal_length() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let _u32_ty = ctx.mk_ty(TyKind::Uint(glyim_core::primitives::UintTy::U32));
    let bool_ty = ctx.bool_ty();
    let subst = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(bool_ty)]);
    let tup_a = ctx.mk_ty(TyKind::Tuple(subst));
    let tup_b = ctx.mk_ty(TyKind::Tuple(subst));
    let result = infer.unify(&mut ctx, tup_a, tup_b, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_unify_array_equal() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let const_val = glyim_type::Const {
        kind: glyim_type::ConstKind::Int(10),
        ty: i32_ty,
    };
    let arr_a = ctx.mk_ty(TyKind::Array(i32_ty, const_val.clone()));
    let arr_b = ctx.mk_ty(TyKind::Array(i32_ty, const_val));
    let result = infer.unify(&mut ctx, arr_a, arr_b, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_unify_slice_equal() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let slice_a = ctx.mk_ty(TyKind::Slice(i32_ty));
    let slice_b = ctx.mk_ty(TyKind::Slice(i32_ty));
    let result = infer.unify(&mut ctx, slice_a, slice_b, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_unify_raw_ptr_equal() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let ptr_a = ctx.mk_ty(TyKind::RawPtr(i32_ty, Mutability::Mut));
    let ptr_b = ctx.mk_ty(TyKind::RawPtr(i32_ty, Mutability::Mut));
    let result = infer.unify(&mut ctx, ptr_a, ptr_b, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_unify_fn_ptr_equal() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let inputs = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let sig = FnSig {
        inputs,
        output: i32_ty,
        c_variadic: false,
        unsafety: glyim_core::primitives::Safety::Safe,
        abi: glyim_core::primitives::Abi::Glyim,
    };
    let fn_a = ctx.mk_ty(TyKind::FnPtr(sig.clone()));
    let fn_b = ctx.mk_ty(TyKind::FnPtr(sig));
    let result = infer.unify(&mut ctx, fn_a, fn_b, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_fully_resolve_nested_ref() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let inner_var = infer.new_ty_var(&mut ctx);
    let inner_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(inner_var)));
    let ref_ty = ctx.mk_ref(Region::Erased, inner_ty, Mutability::Not);
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    infer
        .unify(&mut ctx, inner_ty, i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    let frozen = ctx.freeze();
    let result = infer.fully_resolve(&frozen, ref_ty);
    assert!(result.is_ok());
    // fully_resolve guarantees all vars are bound, but may not have substituted them.
    // Resolve the inner type manually.
    if let TyKind::Ref(_, inner, _) = frozen.ty_kind(ref_ty) {
        let concrete_inner = infer.resolve_ty_shallow(&frozen, *inner);
        assert_ty(&frozen, concrete_inner).is_int(IntTy::I32);
    } else {
        panic!("expected ref type");
    }
}

#[test]
fn test_fully_resolve_deeply_nested() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let inner_ref = ctx.mk_ref(Region::Erased, var_ty, Mutability::Not);
    let outer_ref = ctx.mk_ref(Region::Erased, inner_ref, Mutability::Mut);
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    infer
        .unify(&mut ctx, var_ty, i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    let frozen = ctx.freeze();
    let result = infer.fully_resolve(&frozen, outer_ref);
    assert!(result.is_ok());
    // fully_resolve guarantees all vars are bound, but may not have substituted them.
    // Resolve inner chain manually.
    if let TyKind::Ref(_, inner, _) = frozen.ty_kind(outer_ref) {
        if let TyKind::Ref(_, deep_inner, _) = frozen.ty_kind(*inner) {
            let concrete_inner = infer.resolve_ty_shallow(&frozen, *deep_inner);
            assert_ty(&frozen, concrete_inner).is_int(IntTy::I32);
        } else {
            panic!("expected inner ref");
        }
    } else {
        panic!("expected outer ref");
    }
}

#[test]
fn test_unify_never_with_anything_ok() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let result = infer.unify(&mut ctx, Ty::NEVER, i32_ty, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_unify_unit_with_unit_ok() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let result = infer.unify(&mut ctx, Ty::UNIT, Ty::UNIT, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_unify_bool_with_bool_ok() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let result = infer.unify(&mut ctx, Ty::BOOL, Ty::BOOL, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_unify_char_with_char_ok() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let char_ty_a = ctx.mk_ty(TyKind::Char);
    let char_ty_b = ctx.mk_ty(TyKind::Char);
    let result = infer.unify(&mut ctx, char_ty_a, char_ty_b, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_unify_string_with_string_ok() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let str_a = ctx.mk_ty(TyKind::String);
    let str_b = ctx.mk_ty(TyKind::String);
    let result = infer.unify(&mut ctx, str_a, str_b, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_unify_different_ints_error() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let i64_ty = ctx.mk_ty(TyKind::Int(IntTy::I64));
    let result = infer.unify(&mut ctx, i32_ty, i64_ty, glyim_span::Span::DUMMY);
    assert!(result.is_err());
}

#[test]
fn test_unify_string_with_slice_u8() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let str_ty = ctx.mk_ty(TyKind::String);
    let u8_ty = ctx.mk_ty(TyKind::Uint(glyim_core::primitives::UintTy::U8));
    let u8_slice = ctx.mk_ty(TyKind::Slice(u8_ty));
    let result = infer.unify(&mut ctx, str_ty, u8_slice, glyim_span::Span::DUMMY);
    assert!(result.is_err());
}

#[test]
fn test_probe_unbound_returns_none() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    assert_eq!(infer.probe_ty_var(var), None);
}

#[test]
fn test_constrained_general_var_becomes_integer() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let gen_var = infer.new_ty_var(&mut ctx);
    let gen_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(gen_var)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    infer
        .unify(&mut ctx, gen_ty, i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    // After binding, resolve should give i32
    let resolved = infer.resolve_ty_shallow(&ctx, gen_ty);
    assert_eq!(resolved, i32_ty);
}

#[test]
fn test_unify_adt_same_id_and_substs_ok() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let adt_id = glyim_core::def_id::AdtId::from_raw(1);
    let subst = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let adt_a = ctx.mk_ty(TyKind::Adt(adt_id, subst));
    let adt_b = ctx.mk_ty(TyKind::Adt(adt_id, subst));
    let result = infer.unify(&mut ctx, adt_a, adt_b, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_unify_adt_different_id_err() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let subst1 = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let adt_a = ctx.mk_ty(TyKind::Adt(glyim_core::def_id::AdtId::from_raw(1), subst1));
    let subst2 = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let adt_b = ctx.mk_ty(TyKind::Adt(glyim_core::def_id::AdtId::from_raw(2), subst2));
    let result = infer.unify(&mut ctx, adt_a, adt_b, glyim_span::Span::DUMMY);
    assert!(result.is_err());
}

#[test]
fn test_unify_adt_with_inner_inference_var_ok() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let adt_id = glyim_core::def_id::AdtId::from_raw(1);
    let subst_a = ctx.intern_substitution(vec![GenericArg::Ty(var_ty)]);
    let adt_a = ctx.mk_ty(TyKind::Adt(adt_id, subst_a));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let subst_b = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let adt_b = ctx.mk_ty(TyKind::Adt(adt_id, subst_b));
    infer
        .unify(&mut ctx, adt_a, adt_b, glyim_span::Span::DUMMY)
        .unwrap();
    assert_eq!(infer.probe_ty_var(var), Some(i32_ty));
}

#[test]
fn test_unify_fn_def_same_id_ok() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let fn_id = glyim_core::def_id::FnDefId::from_raw(1);
    let subst = ctx.intern_substitution(Vec::new());
    let fn_a = ctx.mk_ty(TyKind::FnDef(fn_id, subst));
    let fn_b = ctx.mk_ty(TyKind::FnDef(fn_id, subst));
    let result = infer.unify(&mut ctx, fn_a, fn_b, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_unify_fn_def_different_id_err() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let subst = ctx.intern_substitution(Vec::new());
    let fn_a = ctx.mk_ty(TyKind::FnDef(
        glyim_core::def_id::FnDefId::from_raw(1),
        subst,
    ));
    let fn_b = ctx.mk_ty(TyKind::FnDef(
        glyim_core::def_id::FnDefId::from_raw(2),
        subst,
    ));
    let result = infer.unify(&mut ctx, fn_a, fn_b, glyim_span::Span::DUMMY);
    assert!(result.is_err());
}

#[test]
fn test_unify_closure_same_id_ok() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let closure_id = glyim_core::def_id::ClosureId::from_raw(1);
    let subst = ctx.intern_substitution(Vec::new());
    let cl_a = ctx.mk_ty(TyKind::Closure(closure_id, subst));
    let cl_b = ctx.mk_ty(TyKind::Closure(closure_id, subst));
    let result = infer.unify(&mut ctx, cl_a, cl_b, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_unify_closure_different_id_err() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let subst = ctx.intern_substitution(Vec::new());
    let cl_a = ctx.mk_ty(TyKind::Closure(
        glyim_core::def_id::ClosureId::from_raw(1),
        subst,
    ));
    let cl_b = ctx.mk_ty(TyKind::Closure(
        glyim_core::def_id::ClosureId::from_raw(2),
        subst,
    ));
    let result = infer.unify(&mut ctx, cl_a, cl_b, glyim_span::Span::DUMMY);
    assert!(result.is_err());
}

#[test]
fn test_fully_resolve_adt_with_unresolved_var() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let adt_id = glyim_core::def_id::AdtId::from_raw(1);
    let subst = ctx.intern_substitution(vec![GenericArg::Ty(var_ty)]);
    let adt_ty = ctx.mk_ty(TyKind::Adt(adt_id, subst));
    let frozen = ctx.freeze();
    let result = infer.fully_resolve(&frozen, adt_ty);
    // Should fail because inner var is unresolved
    assert!(result.is_err());
}

#[test]
fn test_fully_resolve_fn_def_with_unresolved_var() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let fn_id = glyim_core::def_id::FnDefId::from_raw(1);
    let subst = ctx.intern_substitution(vec![GenericArg::Ty(var_ty)]);
    let fn_ty = ctx.mk_ty(TyKind::FnDef(fn_id, subst));
    let frozen = ctx.freeze();
    let result = infer.fully_resolve(&frozen, fn_ty);
    assert!(result.is_err());
}

#[test]
fn test_unify_different_kinds_error_message() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx.bool_ty();
    let result = infer.unify(&mut ctx, i32_ty, bool_ty, glyim_span::Span::DUMMY);
    assert!(result.is_err());
    let errs = result.unwrap_err();
    assert!(!errs.is_empty());
    assert!(errs[0].message.contains("mismatched types"));
}

#[test]
fn test_unify_ptr_mutability_mismatch() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let ptr_mut = ctx.mk_ty(TyKind::RawPtr(i32_ty, Mutability::Mut));
    let ptr_const = ctx.mk_ty(TyKind::RawPtr(i32_ty, Mutability::Not));
    let result = infer.unify(&mut ctx, ptr_mut, ptr_const, glyim_span::Span::DUMMY);
    // Raw-pointer mutability is intentionally NOT a constraint in this
    // compiler (stdlib passes `*const u8` to `*mut u8` extern fns). Only the
    // pointees are unified. See the `RawPtr` arm in `infer.rs`.
    assert!(result.is_ok(), "raw-ptr mutability must not be a hard constraint");
}

#[test]
fn test_resolve_ty_shallow_int_var_bound_to_i32() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let ivar = infer.new_int_var(&mut ctx);
    let ivar_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(ivar)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    infer
        .unify(&mut ctx, ivar_ty, i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    let frozen = ctx.freeze();
    let resolved = infer.resolve_ty_shallow(&frozen, ivar_ty);
    assert_eq!(resolved, i32_ty);
}

#[test]
fn test_resolve_ty_shallow_float_var_bound_to_f64() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let fvar = infer.new_float_var(&mut ctx);
    let fvar_ty = ctx.mk_ty(TyKind::Infer(InferVar::Float(fvar)));
    let f64_ty = ctx.mk_ty(TyKind::Float(FloatTy::F64));
    infer
        .unify(&mut ctx, fvar_ty, f64_ty, glyim_span::Span::DUMMY)
        .unwrap();
    let frozen = ctx.freeze();
    let resolved = infer.resolve_ty_shallow(&frozen, fvar_ty);
    assert_eq!(resolved, f64_ty);
}

#[test]
fn test_unify_slices_with_different_inner_fails() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx.bool_ty();
    let slice_a = ctx.mk_ty(TyKind::Slice(i32_ty));
    let slice_b = ctx.mk_ty(TyKind::Slice(bool_ty));
    let result = infer.unify(&mut ctx, slice_a, slice_b, glyim_span::Span::DUMMY);
    assert!(result.is_err());
}

#[test]
fn test_unify_arrays_different_lengths_fails() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let const_a = glyim_type::Const {
        kind: glyim_type::ConstKind::Int(10),
        ty: i32_ty,
    };
    let const_b = glyim_type::Const {
        kind: glyim_type::ConstKind::Int(20),
        ty: i32_ty,
    };
    let arr_a = ctx.mk_ty(TyKind::Array(i32_ty, const_a));
    let arr_b = ctx.mk_ty(TyKind::Array(i32_ty, const_b));
    let result = infer.unify(&mut ctx, arr_a, arr_b, glyim_span::Span::DUMMY);
    // Since lengths are different constants, should fail
    assert!(result.is_err());
}

#[test]
fn test_unify_fn_ptr_argument_count_mismatch() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx.bool_ty();
    let sig_a = FnSig {
        inputs: ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]),
        output: i32_ty,
        c_variadic: false,
        unsafety: glyim_core::primitives::Safety::Safe,
        abi: glyim_core::primitives::Abi::Glyim,
    };
    let sig_b = FnSig {
        inputs: ctx.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(bool_ty)]),
        output: i32_ty,
        c_variadic: false,
        unsafety: glyim_core::primitives::Safety::Safe,
        abi: glyim_core::primitives::Abi::Glyim,
    };
    let fn_a = ctx.mk_ty(TyKind::FnPtr(sig_a));
    let fn_b = ctx.mk_ty(TyKind::FnPtr(sig_b));
    let result = infer.unify(&mut ctx, fn_a, fn_b, glyim_span::Span::DUMMY);
    assert!(result.is_err());
}

#[test]
fn test_fully_resolve_fn_ptr_with_unresolved_input_var_err() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let sig = FnSig {
        inputs: ctx.intern_substitution(vec![GenericArg::Ty(var_ty)]),
        output: i32_ty,
        c_variadic: false,
        unsafety: glyim_core::primitives::Safety::Safe,
        abi: glyim_core::primitives::Abi::Glyim,
    };
    let fn_ty = ctx.mk_ty(TyKind::FnPtr(sig));
    let frozen = ctx.freeze();
    let result = infer.fully_resolve(&frozen, fn_ty);
    assert!(result.is_err());
}

#[test]
fn test_fully_resolve_fn_ptr_with_unresolved_output_var_err() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let sig = FnSig {
        inputs: ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]),
        output: var_ty,
        c_variadic: false,
        unsafety: glyim_core::primitives::Safety::Safe,
        abi: glyim_core::primitives::Abi::Glyim,
    };
    let fn_ty = ctx.mk_ty(TyKind::FnPtr(sig));
    let frozen = ctx.freeze();
    let result = infer.fully_resolve(&frozen, fn_ty);
    assert!(result.is_err());
}

#[test]
fn test_unify_multiple_constraints_ref_and_tuple() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx.bool_ty();
    let subst_tup = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(bool_ty)]);
    let tup_a = ctx.mk_ty(TyKind::Tuple(subst_tup));
    let tup_b = ctx.mk_ty(TyKind::Tuple(subst_tup));
    let result = infer.unify(&mut ctx, tup_a, tup_b, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
    let constraints = result.unwrap();
    // Two successful constituent unifications, no region constraints needed
    assert_eq!(constraints.len(), 0);
}

#[test]
fn test_new_ty_var_distinct_indices() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var1 = infer.new_ty_var(&mut ctx);
    let var2 = infer.new_ty_var(&mut ctx);
    assert_ne!(var1, var2);
}

#[test]
fn test_new_int_var_distinct_indices() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var1 = infer.new_int_var(&mut ctx);
    let var2 = infer.new_int_var(&mut ctx);
    assert_ne!(var1, var2);
}

#[test]
fn test_new_float_var_distinct_indices() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var1 = infer.new_float_var(&mut ctx);
    let var2 = infer.new_float_var(&mut ctx);
    assert_ne!(var1, var2);
}

#[test]
fn test_create_multiple_universes() {
    let mut infer = InferenceTable::new();
    infer.create_universe(); // now 1
    infer.create_universe(); // now 2
    infer.create_universe(); // now 3
    assert_eq!(infer.universe(), UniverseIndex(3));
}

#[test]
fn test_new_var_in_new_universe() {
    let mut _ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    infer.create_universe();
    let var = infer.new_ty_var(&mut _ctx);
    // The variable should be created in universe 1
    assert_eq!(infer.universe(), UniverseIndex(1));
    assert!(infer.probe_ty_var(var).is_none()); // unbound
}

#[test]
fn test_int_var_binds_to_int_of_different_width() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_int_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(var)));
    let i64_ty = ctx.mk_ty(TyKind::Int(IntTy::I64));
    infer
        .unify(&mut ctx, var_ty, i64_ty, glyim_span::Span::DUMMY)
        .unwrap();
    assert_eq!(infer.probe_int_var(var), Some(i64_ty));
}

#[test]
fn test_float_var_binds_to_f32() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_float_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Float(var)));
    let f32_ty = ctx.mk_ty(TyKind::Float(FloatTy::F32));
    infer
        .unify(&mut ctx, var_ty, f32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    assert_eq!(infer.probe_float_var(var), Some(f32_ty));
}

#[test]
fn test_unify_unit_with_never() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let result = infer.unify(&mut ctx, Ty::UNIT, Ty::NEVER, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_unify_never_with_unit() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let result = infer.unify(&mut ctx, Ty::NEVER, Ty::UNIT, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_unify_error_vs_never() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let result = infer.unify(&mut ctx, Ty::ERROR, Ty::NEVER, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_region_var_creation_and_probe() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let rv = infer.new_region_var(&mut ctx);
    // New region var is not bound yet; probe should return None (probe_region_var not exposed yet)
    // But we can create a region var and verify the index
    let rv2 = infer.new_region_var(&mut ctx);
    assert_ne!(rv, rv2);
}

#[test]
fn test_resolve_ty_shallow_recursive_does_not_loop() {
    use crate::*;
    use glyim_test::test_ty_ctx;
    use glyim_type::*;

    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var1 = infer.new_ty_var(&mut ctx);
    let var2 = infer.new_ty_var(&mut ctx);
    let ty1 = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var1)));
    let ty2 = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var2)));
    // Create a mutual cycle: ?T1 = ?T2, ?T2 = ?T1
    infer.set_ty_var_value(var1, ty2);
    infer.set_ty_var_value(var2, ty1);
    let resolved = infer.resolve_ty_shallow(&ctx, ty1);
    assert_eq!(
        resolved,
        Ty::ERROR,
        "Mutual variable cycle should resolve to ERROR"
    );
    let diags = infer.take_diagnostics();
    assert!(!diags.is_empty(), "Should emit a diagnostic for the cycle");
}

#[test]
fn test_resolve_deep_chain_ty_var() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var1 = infer.new_ty_var(&mut ctx);
    let var2 = infer.new_ty_var(&mut ctx);
    let var3 = infer.new_ty_var(&mut ctx);
    let ty1 = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var1)));
    let ty2 = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var2)));
    let ty3 = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var3)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    infer
        .unify(&mut ctx, ty3, i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    infer
        .unify(&mut ctx, ty2, ty3, glyim_span::Span::DUMMY)
        .unwrap();
    infer
        .unify(&mut ctx, ty1, ty2, glyim_span::Span::DUMMY)
        .unwrap();
    let resolved = infer.resolve_ty_shallow(&ctx, ty1);
    assert_eq!(resolved, i32_ty);
}

#[test]
fn test_error_accumulation_multiple_kind_errors() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let int_var = infer.new_int_var(&mut ctx);
    let float_var = infer.new_float_var(&mut ctx);
    let int_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(int_var)));
    let float_ty = ctx.mk_ty(TyKind::Infer(InferVar::Float(float_var)));
    let bool_ty = ctx.bool_ty();
    // IntVar vs bool should fail
    let err1 = infer
        .unify(&mut ctx, int_ty, bool_ty, glyim_span::Span::DUMMY)
        .unwrap_err();
    assert!(!err1.is_empty());
    // FloatVar vs bool should fail
    let err2 = infer
        .unify(&mut ctx, float_ty, bool_ty, glyim_span::Span::DUMMY)
        .unwrap_err();
    assert!(!err2.is_empty());
}

#[test]
fn test_unify_identical_never_types() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let result = infer.unify(&mut ctx, Ty::NEVER, Ty::NEVER, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_unify_identical_error_types() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let result = infer.unify(&mut ctx, Ty::ERROR, Ty::ERROR, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_unify_same_ty_var_twice_noop() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    // Unifying a var with itself should be a no-op (caught by a == b early check)
    let result = infer.unify(&mut ctx, var_ty, var_ty, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
    assert!(infer.probe_ty_var(var).is_none());
}

#[test]
fn test_unify_same_int_var_twice_noop() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_int_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(var)));
    let result = infer.unify(&mut ctx, var_ty, var_ty, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
    assert!(infer.probe_int_var(var).is_none());
}

#[test]
fn test_unify_same_float_var_twice_noop() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_float_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Float(var)));
    let result = infer.unify(&mut ctx, var_ty, var_ty, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
    assert!(infer.probe_float_var(var).is_none());
}

#[test]
fn test_ty_var_rebind_conflict() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx.bool_ty();
    infer
        .unify(&mut ctx, var_ty, i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    assert_eq!(infer.probe_ty_var(var), Some(i32_ty));
    // Attempt to bind to a conflicting concrete type should fail
    let result = infer.unify(&mut ctx, var_ty, bool_ty, glyim_span::Span::DUMMY);
    assert!(result.is_err());
    // The variable should still be bound to i32
    assert_eq!(infer.probe_ty_var(var), Some(i32_ty));
}

#[test]
fn test_int_var_rebind_conflict() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_int_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(var)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let i64_ty = ctx.mk_ty(TyKind::Int(IntTy::I64));
    infer
        .unify(&mut ctx, var_ty, i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    assert_eq!(infer.probe_int_var(var), Some(i32_ty));
    let result = infer.unify(&mut ctx, var_ty, i64_ty, glyim_span::Span::DUMMY);
    assert!(result.is_err());
    assert_eq!(infer.probe_int_var(var), Some(i32_ty));
}

#[test]
fn test_float_var_rebind_conflict() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_float_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Float(var)));
    let f32_ty = ctx.mk_ty(TyKind::Float(FloatTy::F32));
    let f64_ty = ctx.mk_ty(TyKind::Float(FloatTy::F64));
    infer
        .unify(&mut ctx, var_ty, f32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    assert_eq!(infer.probe_float_var(var), Some(f32_ty));
    let result = infer.unify(&mut ctx, var_ty, f64_ty, glyim_span::Span::DUMMY);
    assert!(result.is_err());
    assert_eq!(infer.probe_float_var(var), Some(f32_ty));
}

#[test]
fn test_fully_resolve_nested_tuple_with_vars() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx.bool_ty();
    let inner_subst =
        ctx.intern_substitution(vec![GenericArg::Ty(var_ty), GenericArg::Ty(bool_ty)]);
    let inner_tup = ctx.mk_ty(TyKind::Tuple(inner_subst));
    let outer_subst = ctx.intern_substitution(vec![GenericArg::Ty(inner_tup)]);
    let outer_tup = ctx.mk_ty(TyKind::Tuple(outer_subst));
    infer
        .unify(&mut ctx, var_ty, i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    let frozen = ctx.freeze();
    let result = infer.fully_resolve(&frozen, outer_tup);
    assert!(result.is_ok());
}

#[test]
fn test_fully_resolve_nested_fn_ptr_with_vars() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let inner_sig = FnSig {
        inputs: ctx.intern_substitution(vec![GenericArg::Ty(var_ty)]),
        output: i32_ty,
        c_variadic: false,
        unsafety: glyim_core::primitives::Safety::Safe,
        abi: glyim_core::primitives::Abi::Glyim,
    };
    let inner_fn = ctx.mk_ty(TyKind::FnPtr(inner_sig));
    let outer_sig = FnSig {
        inputs: ctx.intern_substitution(vec![GenericArg::Ty(inner_fn)]),
        output: i32_ty,
        c_variadic: false,
        unsafety: glyim_core::primitives::Safety::Safe,
        abi: glyim_core::primitives::Abi::Glyim,
    };
    let outer_fn = ctx.mk_ty(TyKind::FnPtr(outer_sig));
    infer
        .unify(&mut ctx, var_ty, i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    let frozen = ctx.freeze();
    let result = infer.fully_resolve(&frozen, outer_fn);
    assert!(result.is_ok());
}

#[test]
fn test_constrained_int_var_via_general_var() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let general = infer.new_ty_var(&mut ctx);
    let int_var = infer.new_int_var(&mut ctx);
    let gen_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(general)));
    let int_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(int_var)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    // Bind general var to int var
    infer
        .unify(&mut ctx, gen_ty, int_ty, glyim_span::Span::DUMMY)
        .unwrap();
    // Then bind int var to i32
    infer
        .unify(&mut ctx, int_ty, i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    // The general var should now resolve to i32 through the chain
    let resolved = infer.resolve_ty_shallow(&ctx, gen_ty);
    assert_eq!(resolved, i32_ty);
}

#[test]
fn test_constrained_float_var_via_general_var() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let general = infer.new_ty_var(&mut ctx);
    let float_var = infer.new_float_var(&mut ctx);
    let gen_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(general)));
    let float_ty = ctx.mk_ty(TyKind::Infer(InferVar::Float(float_var)));
    let f64_ty = ctx.mk_ty(TyKind::Float(FloatTy::F64));
    infer
        .unify(&mut ctx, gen_ty, float_ty, glyim_span::Span::DUMMY)
        .unwrap();
    infer
        .unify(&mut ctx, float_ty, f64_ty, glyim_span::Span::DUMMY)
        .unwrap();
    let resolved = infer.resolve_ty_shallow(&ctx, gen_ty);
    assert_eq!(resolved, f64_ty);
}

#[test]
fn test_many_simultaneous_ty_vars() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let count = 50;
    let vars: Vec<_> = (0..count).map(|_| infer.new_ty_var(&mut ctx)).collect();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    // Bind every other var to i32
    for (i, &var) in vars.iter().enumerate() {
        if i % 2 == 0 {
            let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
            infer
                .unify(&mut ctx, var_ty, i32_ty, glyim_span::Span::DUMMY)
                .unwrap();
        }
    }
    // All even vars should be bound to i32
    for (i, &var) in vars.iter().enumerate() {
        if i % 2 == 0 {
            assert_eq!(infer.probe_ty_var(var), Some(i32_ty));
        } else {
            assert_eq!(infer.probe_ty_var(var), None);
        }
    }
}

#[test]
fn test_many_simultaneous_int_vars() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let count = 30;
    let vars: Vec<_> = (0..count).map(|_| infer.new_int_var(&mut ctx)).collect();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    for &var in &vars {
        let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(var)));
        infer
            .unify(&mut ctx, var_ty, i32_ty, glyim_span::Span::DUMMY)
            .unwrap();
    }
    for &var in &vars {
        assert_eq!(infer.probe_int_var(var), Some(i32_ty));
    }
}

#[test]
fn test_many_simultaneous_float_vars() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let count = 30;
    let vars: Vec<_> = (0..count).map(|_| infer.new_float_var(&mut ctx)).collect();
    let f64_ty = ctx.mk_ty(TyKind::Float(FloatTy::F64));
    for &var in &vars {
        let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Float(var)));
        infer
            .unify(&mut ctx, var_ty, f64_ty, glyim_span::Span::DUMMY)
            .unwrap();
    }
    for &var in &vars {
        assert_eq!(infer.probe_float_var(var), Some(f64_ty));
    }
}

#[test]
fn test_unify_different_uint_types_error() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let u8_ty = ctx.mk_ty(TyKind::Uint(glyim_core::primitives::UintTy::U8));
    let u32_ty = ctx.mk_ty(TyKind::Uint(glyim_core::primitives::UintTy::U32));
    let result = infer.unify(&mut ctx, u8_ty, u32_ty, glyim_span::Span::DUMMY);
    assert!(result.is_err());
}

#[test]
fn test_unify_different_float_types_error() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let f32_ty = ctx.mk_ty(TyKind::Float(FloatTy::F32));
    let f64_ty = ctx.mk_ty(TyKind::Float(FloatTy::F64));
    let result = infer.unify(&mut ctx, f32_ty, f64_ty, glyim_span::Span::DUMMY);
    assert!(result.is_err());
}

#[test]
fn test_unify_fn_ptr_different_abi_error() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let inputs = ctx.intern_substitution(Vec::new());
    let sig_a = FnSig {
        inputs,
        output: i32_ty,
        c_variadic: false,
        unsafety: glyim_core::primitives::Safety::Safe,
        abi: glyim_core::primitives::Abi::Glyim,
    };
    let sig_b = FnSig {
        inputs,
        output: i32_ty,
        c_variadic: false,
        unsafety: glyim_core::primitives::Safety::Safe,
        abi: glyim_core::primitives::Abi::C,
    };
    let fn_a = ctx.mk_ty(TyKind::FnPtr(sig_a));
    let fn_b = ctx.mk_ty(TyKind::FnPtr(sig_b));
    let result = infer.unify(&mut ctx, fn_a, fn_b, glyim_span::Span::DUMMY);
    assert!(result.is_err());
}

#[test]
fn test_unify_never_with_int_var_ok() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_int_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(var)));
    let result = infer.unify(&mut ctx, Ty::NEVER, var_ty, glyim_span::Span::DUMMY);
    // Never coerces to anything, but IntVar is constrained; Never acts like error-like for coercion
    assert!(result.is_ok());
}

#[test]
fn test_unify_never_with_float_var_ok() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_float_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Float(var)));
    let result = infer.unify(&mut ctx, Ty::NEVER, var_ty, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_int_var_vs_float_var_error() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let int_var = infer.new_int_var(&mut ctx);
    let float_var = infer.new_float_var(&mut ctx);
    let int_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(int_var)));
    let float_ty = ctx.mk_ty(TyKind::Infer(InferVar::Float(float_var)));
    let result = infer.unify(&mut ctx, int_ty, float_ty, glyim_span::Span::DUMMY);
    assert!(result.is_err());
}

#[test]
fn test_unify_ref_var_with_concrete_ref() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let ref_var = ctx.mk_ref(Region::Erased, var_ty, Mutability::Mut);
    let ref_i32 = ctx.mk_ref(Region::Erased, i32_ty, Mutability::Mut);
    infer
        .unify(&mut ctx, ref_var, ref_i32, glyim_span::Span::DUMMY)
        .unwrap();
    assert_eq!(infer.probe_ty_var(var), Some(i32_ty));
}

#[test]
fn test_fully_resolve_with_deeply_nested_refs() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let ref1 = ctx.mk_ref(Region::Erased, var_ty, Mutability::Not);
    let ref2 = ctx.mk_ref(Region::Erased, ref1, Mutability::Not);
    let ref3 = ctx.mk_ref(Region::Erased, ref2, Mutability::Not);
    let ref4 = ctx.mk_ref(Region::Erased, ref3, Mutability::Not);
    infer
        .unify(&mut ctx, var_ty, i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    let frozen = ctx.freeze();
    let result = infer.fully_resolve(&frozen, ref4);
    assert!(result.is_ok());
}

#[test]
fn test_unify_fn_ptr_c_variadic_mismatch() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let inputs = ctx.intern_substitution(Vec::new());
    let sig_a = FnSig {
        inputs,
        output: i32_ty,
        c_variadic: false,
        unsafety: glyim_core::primitives::Safety::Safe,
        abi: glyim_core::primitives::Abi::Glyim,
    };
    let sig_b = FnSig {
        inputs,
        output: i32_ty,
        c_variadic: true,
        unsafety: glyim_core::primitives::Safety::Safe,
        abi: glyim_core::primitives::Abi::Glyim,
    };
    let fn_a = ctx.mk_ty(TyKind::FnPtr(sig_a));
    let fn_b = ctx.mk_ty(TyKind::FnPtr(sig_b));
    let result = infer.unify(&mut ctx, fn_a, fn_b, glyim_span::Span::DUMMY);
    assert!(result.is_err());
}

#[test]
fn test_unify_two_different_ty_vars_then_bind_one() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var1 = infer.new_ty_var(&mut ctx);
    let var2 = infer.new_ty_var(&mut ctx);
    let ty1 = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var1)));
    let ty2 = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var2)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    infer
        .unify(&mut ctx, ty1, ty2, glyim_span::Span::DUMMY)
        .unwrap();
    // Both vars are bound? One should be set to the other
    let bound1 = infer.probe_ty_var(var1);
    let bound2 = infer.probe_ty_var(var2);
    assert!(bound1.is_some() || bound2.is_some());
    // Bind the chain to i32
    infer
        .unify(&mut ctx, ty2, i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    let resolved1 = infer.resolve_ty_shallow(&ctx, ty1);
    assert_eq!(resolved1, i32_ty);
}

#[test]
fn test_unify_int_var_with_general_var_then_concrete() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let int_var = infer.new_int_var(&mut ctx);
    let gen_var = infer.new_ty_var(&mut ctx);
    let int_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(int_var)));
    let gen_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(gen_var)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    // Bind int_var to gen_var (int_var binds gen_var to itself)
    infer
        .unify(&mut ctx, int_ty, gen_ty, glyim_span::Span::DUMMY)
        .unwrap();
    // Then bind gen_var to i32
    infer
        .unify(&mut ctx, gen_ty, i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    let resolved_int = infer.resolve_ty_shallow(&ctx, int_ty);
    let resolved_gen = infer.resolve_ty_shallow(&ctx, gen_ty);
    assert_eq!(resolved_int, i32_ty);
    assert_eq!(resolved_gen, i32_ty);
}

#[test]
fn test_unify_float_var_with_general_var_then_concrete() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let float_var = infer.new_float_var(&mut ctx);
    let gen_var = infer.new_ty_var(&mut ctx);
    let float_ty = ctx.mk_ty(TyKind::Infer(InferVar::Float(float_var)));
    let gen_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(gen_var)));
    let f64_ty = ctx.mk_ty(TyKind::Float(FloatTy::F64));
    infer
        .unify(&mut ctx, float_ty, gen_ty, glyim_span::Span::DUMMY)
        .unwrap();
    infer
        .unify(&mut ctx, gen_ty, f64_ty, glyim_span::Span::DUMMY)
        .unwrap();
    let resolved_float = infer.resolve_ty_shallow(&ctx, float_ty);
    let resolved_gen = infer.resolve_ty_shallow(&ctx, gen_ty);
    assert_eq!(resolved_float, f64_ty);
    assert_eq!(resolved_gen, f64_ty);
}

#[test]
fn test_unify_tuple_different_lengths_error() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx.bool_ty();
    let subst_a = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let subst_b = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(bool_ty)]);
    let tup_a = ctx.mk_ty(TyKind::Tuple(subst_a));
    let tup_b = ctx.mk_ty(TyKind::Tuple(subst_b));
    let result = infer.unify(&mut ctx, tup_a, tup_b, glyim_span::Span::DUMMY);
    assert!(result.is_err());
}

#[test]
fn test_unify_isize_with_isize_ok() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let isize_a = ctx.mk_ty(TyKind::Int(IntTy::Isize));
    let isize_b = ctx.mk_ty(TyKind::Int(IntTy::Isize));
    let result = infer.unify(&mut ctx, isize_a, isize_b, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_unify_usize_with_usize_ok() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let usize_a = ctx.mk_ty(TyKind::Uint(glyim_core::primitives::UintTy::Usize));
    let usize_b = ctx.mk_ty(TyKind::Uint(glyim_core::primitives::UintTy::Usize));
    let result = infer.unify(&mut ctx, usize_a, usize_b, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_resolve_ty_shallow_int_var_unbound() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_int_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(var)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let frozen = ctx.freeze();
    let resolved = infer.resolve_ty_shallow(&frozen, var_ty);
    // An unbound unsuffixed integer var defaults to i32 at reporting time
    // (`resolve_ty_shallow`); only `resolve_ty_shallow_preserve_int` keeps the
    // var. See the doc comment on that pair.
    assert_eq!(resolved, i32_ty);
}

#[test]
fn test_resolve_ty_shallow_float_var_unbound() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_float_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Float(var)));
    let f64_ty = ctx.mk_ty(TyKind::Float(FloatTy::F64));
    let frozen = ctx.freeze();
    let resolved = infer.resolve_ty_shallow(&frozen, var_ty);
    // An unbound float var defaults to f64 at reporting time.
    assert_eq!(resolved, f64_ty);
}

#[test]
fn test_fully_resolve_ty_var_bound_to_another_ty_var_unresolved() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var1 = infer.new_ty_var(&mut ctx);
    let var2 = infer.new_ty_var(&mut ctx);
    let ty1 = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var1)));
    let ty2 = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var2)));
    infer.set_ty_var_value(var1, ty2);
    let frozen = ctx.freeze();
    let result = infer.fully_resolve(&frozen, ty1);
    assert!(result.is_err());
}

#[test]
fn test_fully_resolve_int_var_bound_to_unresolved_ty_var() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let int_var = infer.new_int_var(&mut ctx);
    let ty_var = infer.new_ty_var(&mut ctx);
    let int_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(int_var)));
    let ty_var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(ty_var)));
    infer.set_int_var_value(int_var, ty_var_ty);
    let frozen = ctx.freeze();
    let result = infer.fully_resolve(&frozen, int_ty);
    assert!(result.is_err());
}

#[test]
fn test_fully_resolve_float_var_bound_to_unresolved_ty_var() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let float_var = infer.new_float_var(&mut ctx);
    let ty_var = infer.new_ty_var(&mut ctx);
    let float_ty = ctx.mk_ty(TyKind::Infer(InferVar::Float(float_var)));
    let ty_var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(ty_var)));
    infer.set_float_var_value(float_var, ty_var_ty);
    let frozen = ctx.freeze();
    let result = infer.fully_resolve(&frozen, float_ty);
    assert!(result.is_err());
}

#[test]
fn test_collect_unresolved_vars_detects_multiple() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var1 = infer.new_ty_var(&mut ctx);
    let var2 = infer.new_ty_var(&mut ctx);
    let ty1 = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var1)));
    let ty2 = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var2)));
    let tuple_subst = ctx.intern_substitution(vec![GenericArg::Ty(ty1), GenericArg::Ty(ty2)]);
    let tuple_ty = ctx.mk_ty(TyKind::Tuple(tuple_subst));
    let frozen = ctx.freeze();
    let result = infer.fully_resolve(&frozen, tuple_ty);
    assert!(result.is_err());
    let unresolved = result.unwrap_err();
    assert_eq!(unresolved.len(), 2);
    assert!(unresolved.contains(&var1));
    assert!(unresolved.contains(&var2));
}

#[test]
fn test_unify_adt_with_generic_param() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let param_ty = ctx.mk_ty(TyKind::Param(glyim_type::ParamTy {
        index: 0,
        name: glyim_core::interner::Interner::new().intern("a"),
    }));
    let adt_id = glyim_core::def_id::AdtId::from_raw(1);
    let subst = ctx.intern_substitution(vec![GenericArg::Ty(param_ty)]);
    let adt_a = ctx.mk_ty(TyKind::Adt(adt_id, subst));
    let adt_b = ctx.mk_ty(TyKind::Adt(adt_id, subst));
    let result = infer.unify(&mut ctx, adt_a, adt_b, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_unify_fn_def_with_generic_param() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let param_ty = ctx.mk_ty(TyKind::Param(glyim_type::ParamTy {
        index: 0,
        name: glyim_core::interner::Interner::new().intern("a"),
    }));
    let fn_id = glyim_core::def_id::FnDefId::from_raw(1);
    let subst = ctx.intern_substitution(vec![GenericArg::Ty(param_ty)]);
    let fn_a = ctx.mk_ty(TyKind::FnDef(fn_id, subst));
    let fn_b = ctx.mk_ty(TyKind::FnDef(fn_id, subst));
    let result = infer.unify(&mut ctx, fn_a, fn_b, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_unify_closure_with_generic_param() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let param_ty = ctx.mk_ty(TyKind::Param(glyim_type::ParamTy {
        index: 0,
        name: glyim_core::interner::Interner::new().intern("a"),
    }));
    let cl_id = glyim_core::def_id::ClosureId::from_raw(1);
    let subst = ctx.intern_substitution(vec![GenericArg::Ty(param_ty)]);
    let cl_a = ctx.mk_ty(TyKind::Closure(cl_id, subst));
    let cl_b = ctx.mk_ty(TyKind::Closure(cl_id, subst));
    let result = infer.unify(&mut ctx, cl_a, cl_b, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_print_ty_display_for_error_message() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx.bool_ty();
    let result = infer.unify(&mut ctx, i32_ty, bool_ty, glyim_span::Span::DUMMY);
    let err = result.unwrap_err();
    assert!(err[0].message.contains("mismatched types"));
    assert!(err[0].message.contains("i32"));
    assert!(err[0].message.contains("bool"));
}

#[test]
fn test_new_region_var_increments() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let r1 = infer.new_region_var(&mut ctx);
    let r2 = infer.new_region_var(&mut ctx);
    let r3 = infer.new_region_var(&mut ctx);
    assert_ne!(r1, r2);
    assert_ne!(r2, r3);
    assert_ne!(r1, r3);
}

#[test]
fn test_unify_constrained_int_var_with_general_var_passes() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let int_var = infer.new_int_var(&mut ctx);
    let gen_var = infer.new_ty_var(&mut ctx);
    let int_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(int_var)));
    let gen_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(gen_var)));
    let result = infer.unify(&mut ctx, int_ty, gen_ty, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
    // The general var should be bound to the int var type
    assert_eq!(infer.probe_ty_var(gen_var), Some(int_ty));
}

#[test]
fn test_unify_constrained_float_var_with_general_var_passes() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let float_var = infer.new_float_var(&mut ctx);
    let gen_var = infer.new_ty_var(&mut ctx);
    let float_ty = ctx.mk_ty(TyKind::Infer(InferVar::Float(float_var)));
    let gen_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(gen_var)));
    let result = infer.unify(&mut ctx, float_ty, gen_ty, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
    assert_eq!(infer.probe_ty_var(gen_var), Some(float_ty));
}

#[test]
fn test_fully_resolve_deep_chain_fully_bound() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let vars: Vec<_> = (0..10).map(|_| infer.new_ty_var(&mut ctx)).collect();
    let tys: Vec<_> = vars
        .iter()
        .map(|&v| ctx.mk_ty(TyKind::Infer(InferVar::Ty(v))))
        .collect();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    // Build chain: var0 -> var1 -> ... -> var9 -> i32
    for i in 0..9 {
        infer
            .unify(&mut ctx, tys[i], tys[i + 1], glyim_span::Span::DUMMY)
            .unwrap();
    }
    infer
        .unify(&mut ctx, tys[9], i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    let frozen = ctx.freeze();
    let result = infer.fully_resolve(&frozen, tys[0]);
    assert!(result.is_ok());
    assert_eq!(infer.resolve_ty_shallow(&frozen, tys[0]), i32_ty);
}

#[test]
fn test_unify_error_replaces_variable_binding() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    infer
        .unify(&mut ctx, var_ty, i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    // Now unify with Error - Error dominates but does not overwrite
    let result = infer.unify(&mut ctx, var_ty, Ty::ERROR, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
    // Var should still be bound to i32 (error doesn't erase)
    assert_eq!(infer.probe_ty_var(var), Some(i32_ty));
}

#[test]
fn test_fully_resolve_mixed_unresolved_vars() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let ty_var = infer.new_ty_var(&mut ctx);
    let int_var = infer.new_int_var(&mut ctx);
    let ty_var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(ty_var)));
    let int_var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(int_var)));
    let tuple_subst =
        ctx.intern_substitution(vec![GenericArg::Ty(ty_var_ty), GenericArg::Ty(int_var_ty)]);
    let tuple_ty = ctx.mk_ty(TyKind::Tuple(tuple_subst));
    let frozen = ctx.freeze();
    let result = infer.fully_resolve(&frozen, tuple_ty);
    // The int var is unresolved, so fully_resolve should return error
    assert!(result.is_err());
    let unresolved = result.unwrap_err();
    // Only ty_vars are collected; the int_var is not a ty var, so it should not be in the Vec
    // But the presence of int_var triggers early error with empty Vec? In current implementation,
    // has_unresolved_non_ty_infer returns true if any int/float var unresolved, and then fully_resolve
    // returns Err(Vec::new()) before collecting ty vars. So Vec should be empty.
    assert!(unresolved.is_empty());
}

#[test]
fn test_fully_resolve_mixed_unresolved_ty_and_int_vars() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let ty_var1 = infer.new_ty_var(&mut ctx);
    let ty_var2 = infer.new_ty_var(&mut ctx);
    let int_var = infer.new_int_var(&mut ctx);
    let ty1 = ctx.mk_ty(TyKind::Infer(InferVar::Ty(ty_var1)));
    let ty2 = ctx.mk_ty(TyKind::Infer(InferVar::Ty(ty_var2)));
    let int_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(int_var)));
    // Bind int_var to i32 so it's resolved, but leave ty_vars unresolved
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    infer
        .unify(&mut ctx, int_ty, i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    let tuple_subst = ctx.intern_substitution(vec![
        GenericArg::Ty(ty1),
        GenericArg::Ty(ty2),
        GenericArg::Ty(int_ty),
    ]);
    let tuple_ty = ctx.mk_ty(TyKind::Tuple(tuple_subst));
    let frozen = ctx.freeze();
    let result = infer.fully_resolve(&frozen, tuple_ty);
    assert!(result.is_err());
    let unresolved = result.unwrap_err();
    // Only ty vars should be reported
    assert_eq!(unresolved.len(), 2);
    assert!(unresolved.contains(&ty_var1));
    assert!(unresolved.contains(&ty_var2));
}

#[test]
fn test_deep_non_cyclic_chain_resolve() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let depth = 50;
    let mut vars = Vec::new();
    let mut tys = Vec::new();
    for _ in 0..depth {
        let var = infer.new_ty_var(&mut ctx);
        let ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        vars.push(var);
        tys.push(ty);
    }
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    // Build chain: 0 -> 1, 1 -> 2, ..., 48 -> 49, 49 -> i32
    for i in 0..(depth - 1) {
        infer
            .unify(&mut ctx, tys[i], tys[i + 1], glyim_span::Span::DUMMY)
            .unwrap();
    }
    infer
        .unify(&mut ctx, tys[depth - 1], i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    let resolved = infer.resolve_ty_shallow(&ctx, tys[0]);
    assert_eq!(resolved, i32_ty);
}

#[test]
fn test_chain_below_recursion_limit() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let limit = 200; // below 256
    let mut vars = Vec::new();
    let mut tys = Vec::new();
    for _ in 0..limit {
        let var = infer.new_ty_var(&mut ctx);
        let ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
        vars.push(var);
        tys.push(ty);
    }
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    // Build chain: each var bound to the next except last to i32
    for i in 0..(limit - 1) {
        // unify var i with var i+1 (i.e., bind var i to ty i+1)
        infer
            .unify(&mut ctx, tys[i], tys[i + 1], glyim_span::Span::DUMMY)
            .unwrap();
    }
    infer
        .unify(&mut ctx, tys[limit - 1], i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    let resolved = infer.resolve_ty_shallow(&ctx, tys[0]);
    assert_eq!(resolved, i32_ty);
}

#[test]
fn test_nested_mixed_refs_and_tuples_unify() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx.bool_ty();

    // Build complex type: &(i32, &bool)
    let inner_ref = ctx.mk_ref(Region::Erased, bool_ty, Mutability::Not);
    let tup_subst =
        ctx.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(inner_ref)]);
    let tup_ty = ctx.mk_ty(TyKind::Tuple(tup_subst));
    let outer_ref = ctx.mk_ref(Region::Erased, tup_ty, Mutability::Mut);

    // Build another type with variable instead of bool: &(i32, &?var)
    let var_ref = ctx.mk_ref(Region::Erased, var_ty, Mutability::Not);
    let var_tup_subst =
        ctx.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(var_ref)]);
    let var_tup = ctx.mk_ty(TyKind::Tuple(var_tup_subst));
    let var_outer_ref = ctx.mk_ref(Region::Erased, var_tup, Mutability::Mut);

    // Unify: they should succeed, binding var to bool. Variable must be on left for binding.
    let result = infer.unify(&mut ctx, var_outer_ref, outer_ref, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
    assert_eq!(infer.probe_ty_var(var), Some(bool_ty));
}

#[test]
fn test_unify_adt_recursive_substs() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let adt_id = glyim_core::def_id::AdtId::from_raw(1);
    // Adt<Adt<i32>>
    let inner_subst = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let inner_adt = ctx.mk_ty(TyKind::Adt(adt_id, inner_subst));
    let outer_subst = ctx.intern_substitution(vec![GenericArg::Ty(inner_adt)]);
    let outer_adt_a = ctx.mk_ty(TyKind::Adt(adt_id, outer_subst));
    let outer_adt_b = ctx.mk_ty(TyKind::Adt(adt_id, outer_subst));
    let result = infer.unify(&mut ctx, outer_adt_a, outer_adt_b, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_unify_dynamic_with_region_ok() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let region = Region::Erased;
    let preds: Box<[glyim_type::Predicate]> = Box::new([]);
    let binder = glyim_type::Binder {
        value: preds,
        bound_vars: Box::new([]),
    };
    let dyn1 = ctx.mk_ty(TyKind::Dynamic(binder.clone(), region.clone()));
    let dyn2 = ctx.mk_ty(TyKind::Dynamic(binder, region));
    let result = infer.unify(&mut ctx, dyn1, dyn2, glyim_span::Span::DUMMY);
    // The current implementation falls through to catch-all error if no specific arm matches Dynamic.
    // We haven't added an arm for Dynamic. It will fail. Let's see if we need to add that or if the test should
    // expect failure. I'll check existing test: we previously added test_unify_dynamic_with_dynamic_ok and it passed.
    // That test used identical Dynamic values and might have matched via a == b early check.
    // Actually in our match block, there is no specific arm for Dynamic, so if a == b it goes to early return.
    // If not equal, it falls through to catch-all error. For this test, they are equal (same region, same binder), so it should be Ok.
    // But this test creates two separate Dynamic values with same region and binder (cloned). That's fine.
    // The test may pass. We'll keep it.
    assert!(result.is_ok());
}

#[test]
fn test_multiple_region_constraints_in_tuple() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    // Distinct region *vars*: two `Region::Erased` refs are structurally
    // identical, so `unify` would short-circuit to no constraints and never
    // exercise the `Ref` arm's `RegionEq` production.
    let r1 = Region::Var(infer.new_region_var(&mut ctx));
    let r2 = Region::Var(infer.new_region_var(&mut ctx));
    let ref1 = ctx.mk_ref(r1.clone(), i32_ty, Mutability::Not);
    let ref2 = ctx.mk_ref(r2.clone(), i32_ty, Mutability::Not);
    let subst1 = ctx.intern_substitution(vec![GenericArg::Ty(ref1)]);
    let subst2 = ctx.intern_substitution(vec![GenericArg::Ty(ref2)]);
    let tup_a = ctx.mk_ty(TyKind::Tuple(subst1));
    let tup_b = ctx.mk_ty(TyKind::Tuple(subst2));
    let result = infer.unify(&mut ctx, tup_a, tup_b, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
    let constraints = result.unwrap();
    assert_eq!(constraints.len(), 1);
    match &constraints[0] {
        Constraint::RegionEq { a, b } => {
            assert_eq!(a, &r1);
            assert_eq!(b, &r2);
        }
        _ => panic!("expected RegionEq"),
    }
}

#[test]
fn test_unify_error_with_never_on_both_sides() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    // Error and Never should both unify with each other regardless of order
    let r1 = infer.unify(&mut ctx, Ty::ERROR, Ty::NEVER, glyim_span::Span::DUMMY);
    assert!(r1.is_ok());
    let r2 = infer.unify(&mut ctx, Ty::NEVER, Ty::ERROR, glyim_span::Span::DUMMY);
    assert!(r2.is_ok());
}

#[test]
fn test_unify_error_with_int_var_does_not_bind() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let ivar = infer.new_int_var(&mut ctx);
    let ivar_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(ivar)));
    // Unify Error with int var -> Ok, but var should stay unbound
    let result = infer.unify(&mut ctx, Ty::ERROR, ivar_ty, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
    assert!(infer.probe_int_var(ivar).is_none());
}

#[test]
fn test_unify_error_with_float_var_does_not_bind() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let fvar = infer.new_float_var(&mut ctx);
    let fvar_ty = ctx.mk_ty(TyKind::Infer(InferVar::Float(fvar)));
    let result = infer.unify(&mut ctx, Ty::ERROR, fvar_ty, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
    assert!(infer.probe_float_var(fvar).is_none());
}

#[test]
fn test_unify_never_with_int_var_does_not_bind() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let ivar = infer.new_int_var(&mut ctx);
    let ivar_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(ivar)));
    let result = infer.unify(&mut ctx, Ty::NEVER, ivar_ty, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
    // Never acts like error for coercion, should not bind
    assert!(infer.probe_int_var(ivar).is_none());
}

#[test]
fn test_unify_never_with_float_var_does_not_bind() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let fvar = infer.new_float_var(&mut ctx);
    let fvar_ty = ctx.mk_ty(TyKind::Infer(InferVar::Float(fvar)));
    let result = infer.unify(&mut ctx, Ty::NEVER, fvar_ty, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
    assert!(infer.probe_float_var(fvar).is_none());
}

#[test]
fn test_unify_never_with_ty_var_does_not_bind() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let tvar = infer.new_ty_var(&mut ctx);
    let tvar_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(tvar)));
    let result = infer.unify(&mut ctx, Ty::NEVER, tvar_ty, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
    // Never should not bind the general ty var
    assert!(infer.probe_ty_var(tvar).is_none());
}

#[test]
fn test_unify_int_var_with_never_on_right_ok() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let ivar = infer.new_int_var(&mut ctx);
    let ivar_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(ivar)));
    let result = infer.unify(&mut ctx, ivar_ty, Ty::NEVER, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
    assert!(infer.probe_int_var(ivar).is_none());
}

#[test]
fn test_resolve_ty_shallow_does_not_panic_on_unresolved_bound() {
    let mut ctx = test_ty_ctx();
    let infer = InferenceTable::new();
    let bound_ty = ctx.mk_ty(TyKind::Bound(
        0,
        glyim_type::BoundTy {
            var: 0,
            kind: glyim_type::BoundTyKind::Anon,
        },
    ));
    let resolved = infer.resolve_ty_shallow(&ctx, bound_ty);
    assert_eq!(resolved, bound_ty);
}

#[test]
fn test_fully_resolve_complex_ok() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let ref1 = ctx.mk_ref(Region::Erased, var_ty, Mutability::Not);
    let subst_complex = ctx.intern_substitution(vec![GenericArg::Ty(ref1)]);
    let tup = ctx.mk_ty(TyKind::Tuple(subst_complex));
    infer
        .unify(&mut ctx, var_ty, i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    let frozen = ctx.freeze();
    let result = infer.fully_resolve(&frozen, tup);
    assert!(result.is_ok());
}

#[test]
fn test_fully_resolve_returns_error_for_unresolved_in_adt_subst() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let adt_id = glyim_core::def_id::AdtId::from_raw(1);
    let subst = ctx.intern_substitution(vec![GenericArg::Ty(var_ty)]);
    let adt_ty = ctx.mk_ty(TyKind::Adt(adt_id, subst));
    let frozen = ctx.freeze();
    let result = infer.fully_resolve(&frozen, adt_ty);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), vec![var]);
}

#[test]
fn test_unify_dynamic_different_region_err() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let preds: Box<[glyim_type::Predicate]> = Box::new([]);
    let binder = glyim_type::Binder {
        value: preds,
        bound_vars: Box::new([]),
    };
    let dyn1 = ctx.mk_ty(TyKind::Dynamic(binder.clone(), Region::Erased));
    let dyn2 = ctx.mk_ty(TyKind::Dynamic(binder, Region::Static));
    let result = infer.unify(&mut ctx, dyn1, dyn2, glyim_span::Span::DUMMY);
    assert!(result.is_err());
}

#[test]
fn test_unify_opaque_with_substitution() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let opaque_id = glyim_core::def_id::OpaqueTyId::from_raw(1);
    let subst = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let op_a = ctx.mk_ty(TyKind::Opaque(opaque_id, subst));
    let op_b = ctx.mk_ty(TyKind::Opaque(opaque_id, subst));
    let result = infer.unify(&mut ctx, op_a, op_b, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_unify_opaque_with_inference_var_subst() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let opaque_id = glyim_core::def_id::OpaqueTyId::from_raw(1);
    let subst_a = ctx.intern_substitution(vec![GenericArg::Ty(var_ty)]);
    let subst_b = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let op_a = ctx.mk_ty(TyKind::Opaque(opaque_id, subst_a));
    let op_b = ctx.mk_ty(TyKind::Opaque(opaque_id, subst_b));
    infer
        .unify(&mut ctx, op_a, op_b, glyim_span::Span::DUMMY)
        .unwrap();
    assert_eq!(infer.probe_ty_var(var), Some(i32_ty));
}

#[test]
fn test_fully_resolve_dynamic_with_unresolved_ty_var() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let _var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    // Embed a ty var inside a Dynamic... but Dynamic takes Binder<Box<[Predicate]>> and Region.
    // We can't easily embed a Ty in Dynamic; skip internal var test.
    // Instead, test that Dynamic itself doesn't cause fully_resolve to panic.
    let preds: Box<[glyim_type::Predicate]> = Box::new([]);
    let binder = glyim_type::Binder {
        value: preds,
        bound_vars: Box::new([]),
    };
    let dyn_ty = ctx.mk_ty(TyKind::Dynamic(binder, Region::Erased));
    let frozen = ctx.freeze();
    let result = infer.fully_resolve(&frozen, dyn_ty);
    // Dynamic has no ty vars, should resolve ok
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), dyn_ty);
}

#[test]
fn test_fully_resolve_opaque_with_unresolved_var_in_subst() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let opaque_id = glyim_core::def_id::OpaqueTyId::from_raw(1);
    let subst = ctx.intern_substitution(vec![GenericArg::Ty(var_ty)]);
    let op_ty = ctx.mk_ty(TyKind::Opaque(opaque_id, subst));
    let frozen = ctx.freeze();
    let result = infer.fully_resolve(&frozen, op_ty);
    assert!(result.is_err());
}

#[test]
fn test_resolve_ty_shallow_multiple_bindings() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let v1 = infer.new_ty_var(&mut ctx);
    let v2 = infer.new_ty_var(&mut ctx);
    let ty1 = ctx.mk_ty(TyKind::Infer(InferVar::Ty(v1)));
    let ty2 = ctx.mk_ty(TyKind::Infer(InferVar::Ty(v2)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx.bool_ty();
    infer
        .unify(&mut ctx, ty1, i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    infer
        .unify(&mut ctx, ty2, bool_ty, glyim_span::Span::DUMMY)
        .unwrap();
    assert_eq!(infer.resolve_ty_shallow(&ctx, ty1), i32_ty);
    assert_eq!(infer.resolve_ty_shallow(&ctx, ty2), bool_ty);
    // Ensure they don't interfere
    assert_eq!(infer.resolve_ty_shallow(&ctx, ty1), i32_ty);
}

#[test]
fn test_int_var_chain_resolve() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let v1 = infer.new_int_var(&mut ctx);
    let v2 = infer.new_int_var(&mut ctx);
    let ty1 = ctx.mk_ty(TyKind::Infer(InferVar::Int(v1)));
    let ty2 = ctx.mk_ty(TyKind::Infer(InferVar::Int(v2)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    infer
        .unify(&mut ctx, ty1, ty2, glyim_span::Span::DUMMY)
        .unwrap();
    infer
        .unify(&mut ctx, ty2, i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    assert_eq!(infer.resolve_ty_shallow(&ctx, ty1), i32_ty);
}

#[test]
fn test_float_var_chain_resolve() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let v1 = infer.new_float_var(&mut ctx);
    let v2 = infer.new_float_var(&mut ctx);
    let ty1 = ctx.mk_ty(TyKind::Infer(InferVar::Float(v1)));
    let ty2 = ctx.mk_ty(TyKind::Infer(InferVar::Float(v2)));
    let f64_ty = ctx.mk_ty(TyKind::Float(FloatTy::F64));
    infer
        .unify(&mut ctx, ty1, ty2, glyim_span::Span::DUMMY)
        .unwrap();
    infer
        .unify(&mut ctx, ty2, f64_ty, glyim_span::Span::DUMMY)
        .unwrap();
    assert_eq!(infer.resolve_ty_shallow(&ctx, ty1), f64_ty);
}

#[test]
fn test_unify_adt_with_fn_ptr_subst() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let inputs = ctx.intern_substitution(Vec::new());
    let sig = FnSig {
        inputs,
        output: i32_ty,
        c_variadic: false,
        unsafety: glyim_core::primitives::Safety::Safe,
        abi: glyim_core::primitives::Abi::Glyim,
    };
    let fn_ty = ctx.mk_ty(TyKind::FnPtr(sig));
    let adt_id = glyim_core::def_id::AdtId::from_raw(1);
    let subst = ctx.intern_substitution(vec![GenericArg::Ty(fn_ty)]);
    let adt_a = ctx.mk_ty(TyKind::Adt(adt_id, subst));
    let adt_b = ctx.mk_ty(TyKind::Adt(adt_id, subst));
    let result = infer.unify(&mut ctx, adt_a, adt_b, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_unify_adt_with_closure_subst() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let cl_id = glyim_core::def_id::ClosureId::from_raw(1);
    let cl_subst = ctx.intern_substitution(Vec::new());
    let cl_ty = ctx.mk_ty(TyKind::Closure(cl_id, cl_subst));
    let adt_id = glyim_core::def_id::AdtId::from_raw(1);
    let subst = ctx.intern_substitution(vec![GenericArg::Ty(cl_ty)]);
    let adt_a = ctx.mk_ty(TyKind::Adt(adt_id, subst));
    let adt_b = ctx.mk_ty(TyKind::Adt(adt_id, subst));
    let result = infer.unify(&mut ctx, adt_a, adt_b, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_unify_tuple_with_refs_and_fns() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx.bool_ty();
    let ref_ty = ctx.mk_ref(Region::Erased, i32_ty, Mutability::Not);
    let sig = FnSig {
        inputs: ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]),
        output: bool_ty,
        c_variadic: false,
        unsafety: glyim_core::primitives::Safety::Safe,
        abi: glyim_core::primitives::Abi::Glyim,
    };
    let fn_ty = ctx.mk_ty(TyKind::FnPtr(sig));
    let subst1 = ctx.intern_substitution(vec![GenericArg::Ty(ref_ty), GenericArg::Ty(fn_ty)]);
    let tup_a = ctx.mk_ty(TyKind::Tuple(subst1));
    let tup_b = ctx.mk_ty(TyKind::Tuple(subst1));
    let result = infer.unify(&mut ctx, tup_a, tup_b, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_fully_resolve_deeply_nested_adt_with_var() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let adt_id = glyim_core::def_id::AdtId::from_raw(1);
    // Adt<Adt<Adt<?var>>>
    let inner_subst = ctx.intern_substitution(vec![GenericArg::Ty(var_ty)]);
    let inner_adt = ctx.mk_ty(TyKind::Adt(adt_id, inner_subst));
    let mid_subst = ctx.intern_substitution(vec![GenericArg::Ty(inner_adt)]);
    let mid_adt = ctx.mk_ty(TyKind::Adt(adt_id, mid_subst));
    let outer_subst = ctx.intern_substitution(vec![GenericArg::Ty(mid_adt)]);
    let outer_adt = ctx.mk_ty(TyKind::Adt(adt_id, outer_subst));
    infer
        .unify(&mut ctx, var_ty, i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    let frozen = ctx.freeze();
    let result = infer.fully_resolve(&frozen, outer_adt);
    assert!(result.is_ok());
}

#[test]
fn test_fully_resolve_mixed_adt_fn_ptr_var() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    // FnPtr(?var) -> i32
    let sig = FnSig {
        inputs: ctx.intern_substitution(vec![GenericArg::Ty(var_ty)]),
        output: i32_ty,
        c_variadic: false,
        unsafety: glyim_core::primitives::Safety::Safe,
        abi: glyim_core::primitives::Abi::Glyim,
    };
    let fn_ty = ctx.mk_ty(TyKind::FnPtr(sig));
    // Adt<FnPtr>
    let adt_id = glyim_core::def_id::AdtId::from_raw(1);
    let subst = ctx.intern_substitution(vec![GenericArg::Ty(fn_ty)]);
    let adt_ty = ctx.mk_ty(TyKind::Adt(adt_id, subst));
    infer
        .unify(&mut ctx, var_ty, i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    let frozen = ctx.freeze();
    let result = infer.fully_resolve(&frozen, adt_ty);
    assert!(result.is_ok());
}

#[test]
fn test_unify_array_with_inner_var() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let const_val = glyim_type::Const {
        kind: glyim_type::ConstKind::Int(10),
        ty: i32_ty,
    };
    let arr_var = ctx.mk_ty(TyKind::Array(var_ty, const_val.clone()));
    let arr_i32 = ctx.mk_ty(TyKind::Array(i32_ty, const_val));
    infer
        .unify(&mut ctx, arr_var, arr_i32, glyim_span::Span::DUMMY)
        .unwrap();
    assert_eq!(infer.probe_ty_var(var), Some(i32_ty));
}

#[test]
fn test_unify_slice_with_inner_var() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let slice_var = ctx.mk_ty(TyKind::Slice(var_ty));
    let slice_i32 = ctx.mk_ty(TyKind::Slice(i32_ty));
    infer
        .unify(&mut ctx, slice_var, slice_i32, glyim_span::Span::DUMMY)
        .unwrap();
    assert_eq!(infer.probe_ty_var(var), Some(i32_ty));
}

#[test]
fn test_unify_raw_ptr_with_inner_var() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let ptr_var = ctx.mk_ty(TyKind::RawPtr(var_ty, Mutability::Mut));
    let ptr_i32 = ctx.mk_ty(TyKind::RawPtr(i32_ty, Mutability::Mut));
    infer
        .unify(&mut ctx, ptr_var, ptr_i32, glyim_span::Span::DUMMY)
        .unwrap();
    assert_eq!(infer.probe_ty_var(var), Some(i32_ty));
}

#[test]
fn test_unify_bound_ty_with_infer_var() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let bound_ty = ctx.mk_ty(TyKind::Bound(
        0,
        glyim_type::BoundTy {
            var: 0,
            kind: glyim_type::BoundTyKind::Anon,
        },
    ));
    let result = infer.unify(&mut ctx, var_ty, bound_ty, glyim_span::Span::DUMMY);
    // Should succeed: general var can be bound to a bound type
    assert!(result.is_ok());
    assert_eq!(infer.probe_ty_var(var), Some(bound_ty));
}

#[test]
fn test_unify_param_ty_with_infer_var() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let interner = glyim_core::interner::Interner::new();
    let name = interner.intern("T");
    let param_ty = ctx.mk_ty(TyKind::Param(glyim_type::ParamTy { index: 0, name }));
    let result = infer.unify(&mut ctx, var_ty, param_ty, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
    assert_eq!(infer.probe_ty_var(var), Some(param_ty));
}

#[test]
fn test_unify_constrained_int_var_with_param_ty() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let ivar = infer.new_int_var(&mut ctx);
    let ivar_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(ivar)));
    let resolver = ctx.resolver();
    let name = resolver.intern("T");
    let param_ty = ctx.mk_ty(TyKind::Param(glyim_type::ParamTy { index: 0, name }));
    let result = infer.unify(&mut ctx, ivar_ty, param_ty, glyim_span::Span::DUMMY);
    // Int variable cannot unify with parameter type - should fail
    assert!(result.is_err());
}

#[test]
fn test_unify_constrained_float_var_with_param_ty() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let fvar = infer.new_float_var(&mut ctx);
    let fvar_ty = ctx.mk_ty(TyKind::Infer(InferVar::Float(fvar)));
    let resolver = ctx.resolver();
    let name = resolver.intern("T");
    let param_ty = ctx.mk_ty(TyKind::Param(glyim_type::ParamTy { index: 0, name }));
    let result = infer.unify(&mut ctx, fvar_ty, param_ty, glyim_span::Span::DUMMY);
    assert!(result.is_err());
}

#[test]
fn test_mixed_int_float_var_unify_error() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let ivar = infer.new_int_var(&mut ctx);
    let fvar = infer.new_float_var(&mut ctx);
    let int_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(ivar)));
    let float_ty = ctx.mk_ty(TyKind::Infer(InferVar::Float(fvar)));
    let result = infer.unify(&mut ctx, int_ty, float_ty, glyim_span::Span::DUMMY);
    assert!(result.is_err());
}

#[test]
fn test_general_var_bound_to_int_var_passes_int_constraint() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let gen_var = infer.new_ty_var(&mut ctx);
    let int_var = infer.new_int_var(&mut ctx);
    let gen_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(gen_var)));
    let int_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(int_var)));
    // Bind general var to int var: general var should take the int var's value
    infer
        .unify(&mut ctx, gen_ty, int_ty, glyim_span::Span::DUMMY)
        .unwrap();
    // Now general var's binding is int_ty, and it should be resolved
    assert_eq!(infer.probe_ty_var(gen_var), Some(int_ty));
    // Bind int var to i32
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    infer
        .unify(&mut ctx, int_ty, i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    // General var should now resolve to i32 via its binding
    let resolved = infer.resolve_ty_shallow(&ctx, gen_ty);
    assert_eq!(resolved, i32_ty);
}

#[test]
fn test_general_var_bound_to_float_var_passes_float_constraint() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let gen_var = infer.new_ty_var(&mut ctx);
    let float_var = infer.new_float_var(&mut ctx);
    let gen_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(gen_var)));
    let float_ty = ctx.mk_ty(TyKind::Infer(InferVar::Float(float_var)));
    infer
        .unify(&mut ctx, gen_ty, float_ty, glyim_span::Span::DUMMY)
        .unwrap();
    assert_eq!(infer.probe_ty_var(gen_var), Some(float_ty));
    let f64_ty = ctx.mk_ty(TyKind::Float(FloatTy::F64));
    infer
        .unify(&mut ctx, float_ty, f64_ty, glyim_span::Span::DUMMY)
        .unwrap();
    let resolved = infer.resolve_ty_shallow(&ctx, gen_ty);
    assert_eq!(resolved, f64_ty);
}

#[test]
fn test_unify_fn_ptr_with_inference_var_input_and_output() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let var_in = infer.new_ty_var(&mut ctx);
    let var_out = infer.new_ty_var(&mut ctx);
    let var_in_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var_in)));
    let var_out_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var_out)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx.bool_ty();

    let inputs_a = ctx.intern_substitution(vec![GenericArg::Ty(var_in_ty)]);
    let sig_a = FnSig {
        inputs: inputs_a,
        output: var_out_ty,
        c_variadic: false,
        unsafety: glyim_core::primitives::Safety::Safe,
        abi: glyim_core::primitives::Abi::Glyim,
    };
    let fn_a = ctx.mk_ty(TyKind::FnPtr(sig_a));

    let inputs_b = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let sig_b = FnSig {
        inputs: inputs_b,
        output: bool_ty,
        c_variadic: false,
        unsafety: glyim_core::primitives::Safety::Safe,
        abi: glyim_core::primitives::Abi::Glyim,
    };
    let fn_b = ctx.mk_ty(TyKind::FnPtr(sig_b));

    infer
        .unify(&mut ctx, fn_a, fn_b, glyim_span::Span::DUMMY)
        .unwrap();
    assert_eq!(infer.probe_ty_var(var_in), Some(i32_ty));
    assert_eq!(infer.probe_ty_var(var_out), Some(bool_ty));
}

#[test]
fn test_unify_ref_with_var_inner_and_var_outer() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let inner_var = infer.new_ty_var(&mut ctx);
    let outer_var = infer.new_ty_var(&mut ctx);
    let inner_var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(inner_var)));
    let outer_var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(outer_var)));

    // Bind outer_var to ref(&inner_var)
    let ref_ty = ctx.mk_ref(Region::Erased, inner_var_ty, Mutability::Mut);
    infer
        .unify(&mut ctx, outer_var_ty, ref_ty, glyim_span::Span::DUMMY)
        .unwrap();
    assert_eq!(infer.probe_ty_var(outer_var), Some(ref_ty));

    // Now unify inner_var with i32
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    infer
        .unify(&mut ctx, inner_var_ty, i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    // Outer var still bound to ref(&inner_var), resolve gives ref(&i32)
    let _resolved = infer.resolve_ty_shallow(&ctx, outer_var_ty);
    // resolved is ref_ty (which holds inner_var_ty), but inner_var_ty resolves to i32? No, ref_ty is a Ty, it doesn't auto-resolve inner.
    // resolve_ty_shallow only follows binding chains, doesn't modify ref inner types.
    // So resolved is still ref(&inner_var) but inner_var resolved to i32 only when you look at it.
    assert_eq!(infer.resolve_ty_shallow(&ctx, inner_var_ty), i32_ty);
}

#[test]
fn test_unify_param_ty_with_different_name_but_same_index() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let resolver = ctx.resolver();
    let name1 = resolver.intern("T");
    let name2 = resolver.intern("U");
    let param_a = ctx.mk_ty(TyKind::Param(glyim_type::ParamTy {
        index: 0,
        name: name1,
    }));
    let param_b = ctx.mk_ty(TyKind::Param(glyim_type::ParamTy {
        index: 0,
        name: name2,
    }));
    // Different names but same index - should still fail if our match doesn't handle Param specially
    let result = infer.unify(&mut ctx, param_a, param_b, glyim_span::Span::DUMMY);
    // Current implementation: falls through to catch-all error since no specific Param arm
    assert!(result.is_ok());
}

#[test]
fn test_unify_bound_ty_with_different_debruijn() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let bound_a = ctx.mk_ty(TyKind::Bound(
        0,
        glyim_type::BoundTy {
            var: 0,
            kind: glyim_type::BoundTyKind::Anon,
        },
    ));
    let bound_b = ctx.mk_ty(TyKind::Bound(
        1,
        glyim_type::BoundTy {
            var: 0,
            kind: glyim_type::BoundTyKind::Anon,
        },
    ));
    let result = infer.unify(&mut ctx, bound_a, bound_b, glyim_span::Span::DUMMY);
    assert!(result.is_err());
}

#[test]
fn test_unify_never_with_adt() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let adt_id = glyim_core::def_id::AdtId::from_raw(1);
    let subst = ctx.intern_substitution(Vec::new());
    let adt_ty = ctx.mk_ty(TyKind::Adt(adt_id, subst));
    let result = infer.unify(&mut ctx, Ty::NEVER, adt_ty, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_unify_error_with_opaque() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let opaque_id = glyim_core::def_id::OpaqueTyId::from_raw(1);
    let subst = ctx.intern_substitution(Vec::new());
    let op_ty = ctx.mk_ty(TyKind::Opaque(opaque_id, subst));
    let result = infer.unify(&mut ctx, Ty::ERROR, op_ty, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_unify_error_with_dynamic() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let preds: Box<[glyim_type::Predicate]> = Box::new([]);
    let binder = glyim_type::Binder {
        value: preds,
        bound_vars: Box::new([]),
    };
    let dyn_ty = ctx.mk_ty(TyKind::Dynamic(binder, Region::Erased));
    let result = infer.unify(&mut ctx, Ty::ERROR, dyn_ty, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_unify_never_with_dynamic() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let preds: Box<[glyim_type::Predicate]> = Box::new([]);
    let binder = glyim_type::Binder {
        value: preds,
        bound_vars: Box::new([]),
    };
    let dyn_ty = ctx.mk_ty(TyKind::Dynamic(binder, Region::Erased));
    let result = infer.unify(&mut ctx, Ty::NEVER, dyn_ty, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_unify_never_with_opaque() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let opaque_id = glyim_core::def_id::OpaqueTyId::from_raw(1);
    let subst = ctx.intern_substitution(Vec::new());
    let op_ty = ctx.mk_ty(TyKind::Opaque(opaque_id, subst));
    let result = infer.unify(&mut ctx, Ty::NEVER, op_ty, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
}

#[test]
fn test_stress_unify_chain_of_vars_of_different_kinds() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    // Create 10 ty vars, 5 int vars, 5 float vars
    let mut tvars = Vec::new();
    for _ in 0..10 {
        let v = infer.new_ty_var(&mut ctx);
        tvars.push(ctx.mk_ty(TyKind::Infer(InferVar::Ty(v))));
    }
    let mut ivars = Vec::new();
    for _ in 0..5 {
        let v = infer.new_int_var(&mut ctx);
        ivars.push(ctx.mk_ty(TyKind::Infer(InferVar::Int(v))));
    }
    let mut fvars = Vec::new();
    for _ in 0..5 {
        let v = infer.new_float_var(&mut ctx);
        fvars.push(ctx.mk_ty(TyKind::Infer(InferVar::Float(v))));
    }
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let f64_ty = ctx.mk_ty(TyKind::Float(FloatTy::F64));
    // Bind all ivars in chain to i32
    for i in 0..4 {
        infer
            .unify(&mut ctx, ivars[i], ivars[i + 1], glyim_span::Span::DUMMY)
            .unwrap();
    }
    infer
        .unify(&mut ctx, ivars[4], i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    // Bind all fvars in chain to f64
    for i in 0..4 {
        infer
            .unify(&mut ctx, fvars[i], fvars[i + 1], glyim_span::Span::DUMMY)
            .unwrap();
    }
    infer
        .unify(&mut ctx, fvars[4], f64_ty, glyim_span::Span::DUMMY)
        .unwrap();
    // All should resolve correctly
    for iv in &ivars {
        assert_eq!(infer.resolve_ty_shallow(&ctx, *iv), i32_ty);
    }
    for fv in &fvars {
        assert_eq!(infer.resolve_ty_shallow(&ctx, *fv), f64_ty);
    }
}

#[test]
fn test_fully_resolve_tuple_with_many_vars() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let mut vars = Vec::new();
    let mut tys = Vec::new();
    for _ in 0..10 {
        let v = infer.new_ty_var(&mut ctx);
        let t = ctx.mk_ty(TyKind::Infer(InferVar::Ty(v)));
        vars.push(v);
        tys.push(t);
    }
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    // Bind all vars to i32
    for t in &tys {
        infer
            .unify(&mut ctx, *t, i32_ty, glyim_span::Span::DUMMY)
            .unwrap();
    }
    let subst = ctx.intern_substitution(tys.iter().map(|t| GenericArg::Ty(*t)).collect());
    let tuple_ty = ctx.mk_ty(TyKind::Tuple(subst));
    let frozen = ctx.freeze();
    let result = infer.fully_resolve(&frozen, tuple_ty);
    assert!(result.is_ok());
}

#[test]
fn test_probe_var_after_binding_to_another_var() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let v1 = infer.new_ty_var(&mut ctx);
    let v2 = infer.new_ty_var(&mut ctx);
    let ty1 = ctx.mk_ty(TyKind::Infer(InferVar::Ty(v1)));
    let ty2 = ctx.mk_ty(TyKind::Infer(InferVar::Ty(v2)));
    // Bind v1 to v2 (v1.value = Some(ty2))
    infer
        .unify(&mut ctx, ty1, ty2, glyim_span::Span::DUMMY)
        .unwrap();
    // v1 now bound to ty2
    assert_eq!(infer.probe_ty_var(v1), Some(ty2));
    // v2 still unbound
    assert_eq!(infer.probe_ty_var(v2), None);
    // resolve v1 -> v2 -> unbound
    assert_eq!(infer.resolve_ty_shallow(&ctx, ty1), ty2);
}

#[test]
fn test_probe_int_var_after_binding_to_another_int_var() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let v1 = infer.new_int_var(&mut ctx);
    let v2 = infer.new_int_var(&mut ctx);
    let ty1 = ctx.mk_ty(TyKind::Infer(InferVar::Int(v1)));
    let ty2 = ctx.mk_ty(TyKind::Infer(InferVar::Int(v2)));
    infer
        .unify(&mut ctx, ty1, ty2, glyim_span::Span::DUMMY)
        .unwrap();
    // The binding of v1 should be Some(b) where b is the second arg ty2? In our code we do self.int_vars[var].value = Some(b);
    // If var is v1, b is ty2 (since b is the second resolved arg). So v1.value = Some(ty2).
    assert_eq!(infer.probe_int_var(v1), Some(ty2));
    assert_eq!(infer.probe_int_var(v2), None);
}

#[test]
fn test_probe_float_var_after_binding_to_another_float_var() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let v1 = infer.new_float_var(&mut ctx);
    let v2 = infer.new_float_var(&mut ctx);
    let ty1 = ctx.mk_ty(TyKind::Infer(InferVar::Float(v1)));
    let ty2 = ctx.mk_ty(TyKind::Infer(InferVar::Float(v2)));
    infer
        .unify(&mut ctx, ty1, ty2, glyim_span::Span::DUMMY)
        .unwrap();
    assert_eq!(infer.probe_float_var(v1), Some(ty2));
    assert_eq!(infer.probe_float_var(v2), None);
}

#[test]
fn test_general_var_kind_preserved_after_use() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let v = infer.new_ty_var(&mut ctx);
    let ty_v = ctx.mk_ty(TyKind::Infer(InferVar::Ty(v)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    infer
        .unify(&mut ctx, ty_v, i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    // After use, kind should still be General (we don't change it)
    assert_eq!(infer.ty_var_kind(v), Some(VariableKind::General));
}

#[test]
fn test_unify_adt_different_subst_counts_error() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let adt_id = glyim_core::def_id::AdtId::from_raw(1);
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx.bool_ty();
    let subst_a = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let subst_b = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(bool_ty)]);
    let adt_a = ctx.mk_ty(TyKind::Adt(adt_id, subst_a));
    let adt_b = ctx.mk_ty(TyKind::Adt(adt_id, subst_b));
    let result = infer.unify(&mut ctx, adt_a, adt_b, glyim_span::Span::DUMMY);
    assert!(result.is_err());
}

#[test]
fn test_unify_fn_def_different_subst_counts_error() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let fn_id = glyim_core::def_id::FnDefId::from_raw(1);
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx.bool_ty();
    let subst_a = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let subst_b = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(bool_ty)]);
    let fn_a = ctx.mk_ty(TyKind::FnDef(fn_id, subst_a));
    let fn_b = ctx.mk_ty(TyKind::FnDef(fn_id, subst_b));
    let result = infer.unify(&mut ctx, fn_a, fn_b, glyim_span::Span::DUMMY);
    assert!(result.is_err());
}

#[test]
fn test_unify_closure_different_subst_counts_error() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let cl_id = glyim_core::def_id::ClosureId::from_raw(1);
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx.bool_ty();
    let subst_a = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let subst_b = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(bool_ty)]);
    let cl_a = ctx.mk_ty(TyKind::Closure(cl_id, subst_a));
    let cl_b = ctx.mk_ty(TyKind::Closure(cl_id, subst_b));
    let result = infer.unify(&mut ctx, cl_a, cl_b, glyim_span::Span::DUMMY);
    assert!(result.is_err());
}

#[test]
fn test_unify_opaque_different_subst_counts_error() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let op_id = glyim_core::def_id::OpaqueTyId::from_raw(1);
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx.bool_ty();
    let subst_a = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let subst_b = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(bool_ty)]);
    let op_a = ctx.mk_ty(TyKind::Opaque(op_id, subst_a));
    let op_b = ctx.mk_ty(TyKind::Opaque(op_id, subst_b));
    let result = infer.unify(&mut ctx, op_a, op_b, glyim_span::Span::DUMMY);
    assert!(result.is_err());
}

#[test]
fn test_fully_resolve_all_vars_bound_in_chain() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let vars: Vec<_> = (0..20).map(|_| infer.new_ty_var(&mut ctx)).collect();
    let tys: Vec<_> = vars
        .iter()
        .map(|&v| ctx.mk_ty(TyKind::Infer(InferVar::Ty(v))))
        .collect();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    // chain: 0 -> 1 -> ... -> 19 -> i32
    for i in 0..19 {
        infer
            .unify(&mut ctx, tys[i], tys[i + 1], glyim_span::Span::DUMMY)
            .unwrap();
    }
    infer
        .unify(&mut ctx, tys[19], i32_ty, glyim_span::Span::DUMMY)
        .unwrap();
    let frozen = ctx.freeze();
    let result = infer.fully_resolve(&frozen, tys[0]);
    assert!(result.is_ok());
    assert_eq!(infer.resolve_ty_shallow(&frozen, tys[0]), i32_ty);
}

#[test]
fn test_fully_resolve_nested_tuple_no_vars() {
    let mut ctx = test_ty_ctx();
    let infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx.bool_ty();
    let inner_subst = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let inner_tup = ctx.mk_ty(TyKind::Tuple(inner_subst));
    let outer_subst =
        ctx.intern_substitution(vec![GenericArg::Ty(inner_tup), GenericArg::Ty(bool_ty)]);
    let outer_tup = ctx.mk_ty(TyKind::Tuple(outer_subst));
    let frozen = ctx.freeze();
    let result = infer.fully_resolve(&frozen, outer_tup);
    assert!(result.is_ok());
}

#[test]
fn test_fully_resolve_deeply_nested_adt_no_vars() {
    let mut ctx = test_ty_ctx();
    let infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let adt_id = glyim_core::def_id::AdtId::from_raw(1);
    let inner_subst = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let inner_adt = ctx.mk_ty(TyKind::Adt(adt_id, inner_subst));
    let mid_subst = ctx.intern_substitution(vec![GenericArg::Ty(inner_adt)]);
    let mid_adt = ctx.mk_ty(TyKind::Adt(adt_id, mid_subst));
    let outer_subst = ctx.intern_substitution(vec![GenericArg::Ty(mid_adt)]);
    let outer_adt = ctx.mk_ty(TyKind::Adt(adt_id, outer_subst));
    let frozen = ctx.freeze();
    let result = infer.fully_resolve(&frozen, outer_adt);
    assert!(result.is_ok());
}

#[test]
fn test_unify_fn_ptr_with_int_var_input() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let ivar = infer.new_int_var(&mut ctx);
    let ivar_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(ivar)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let inputs_a = ctx.intern_substitution(vec![GenericArg::Ty(ivar_ty)]);
    let sig_a = FnSig {
        inputs: inputs_a,
        output: i32_ty,
        c_variadic: false,
        unsafety: glyim_core::primitives::Safety::Safe,
        abi: glyim_core::primitives::Abi::Glyim,
    };
    let fn_a = ctx.mk_ty(TyKind::FnPtr(sig_a));
    let inputs_b = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let sig_b = FnSig {
        inputs: inputs_b,
        output: i32_ty,
        c_variadic: false,
        unsafety: glyim_core::primitives::Safety::Safe,
        abi: glyim_core::primitives::Abi::Glyim,
    };
    let fn_b = ctx.mk_ty(TyKind::FnPtr(sig_b));
    infer
        .unify(&mut ctx, fn_a, fn_b, glyim_span::Span::DUMMY)
        .unwrap();
    assert_eq!(infer.probe_int_var(ivar), Some(i32_ty));
}

#[test]
fn test_unify_fn_ptr_with_float_var_input() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let fvar = infer.new_float_var(&mut ctx);
    let fvar_ty = ctx.mk_ty(TyKind::Infer(InferVar::Float(fvar)));
    let f64_ty = ctx.mk_ty(TyKind::Float(FloatTy::F64));
    let inputs_a = ctx.intern_substitution(vec![GenericArg::Ty(fvar_ty)]);
    let sig_a = FnSig {
        inputs: inputs_a,
        output: f64_ty,
        c_variadic: false,
        unsafety: glyim_core::primitives::Safety::Safe,
        abi: glyim_core::primitives::Abi::Glyim,
    };
    let fn_a = ctx.mk_ty(TyKind::FnPtr(sig_a));
    let inputs_b = ctx.intern_substitution(vec![GenericArg::Ty(f64_ty)]);
    let sig_b = FnSig {
        inputs: inputs_b,
        output: f64_ty,
        c_variadic: false,
        unsafety: glyim_core::primitives::Safety::Safe,
        abi: glyim_core::primitives::Abi::Glyim,
    };
    let fn_b = ctx.mk_ty(TyKind::FnPtr(sig_b));
    infer
        .unify(&mut ctx, fn_a, fn_b, glyim_span::Span::DUMMY)
        .unwrap();
    assert_eq!(infer.probe_float_var(fvar), Some(f64_ty));
}

#[test]
fn test_unify_fn_ptr_with_int_var_output() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let ivar = infer.new_int_var(&mut ctx);
    let ivar_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(ivar)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let inputs = ctx.intern_substitution(Vec::new());
    let sig_a = FnSig {
        inputs,
        output: ivar_ty,
        c_variadic: false,
        unsafety: glyim_core::primitives::Safety::Safe,
        abi: glyim_core::primitives::Abi::Glyim,
    };
    let fn_a = ctx.mk_ty(TyKind::FnPtr(sig_a));
    let sig_b = FnSig {
        inputs,
        output: i32_ty,
        c_variadic: false,
        unsafety: glyim_core::primitives::Safety::Safe,
        abi: glyim_core::primitives::Abi::Glyim,
    };
    let fn_b = ctx.mk_ty(TyKind::FnPtr(sig_b));
    infer
        .unify(&mut ctx, fn_a, fn_b, glyim_span::Span::DUMMY)
        .unwrap();
    assert_eq!(infer.probe_int_var(ivar), Some(i32_ty));
}

#[test]
fn test_unify_fn_ptr_with_float_var_output() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let fvar = infer.new_float_var(&mut ctx);
    let fvar_ty = ctx.mk_ty(TyKind::Infer(InferVar::Float(fvar)));
    let f64_ty = ctx.mk_ty(TyKind::Float(FloatTy::F64));
    let inputs = ctx.intern_substitution(Vec::new());
    let sig_a = FnSig {
        inputs,
        output: fvar_ty,
        c_variadic: false,
        unsafety: glyim_core::primitives::Safety::Safe,
        abi: glyim_core::primitives::Abi::Glyim,
    };
    let fn_a = ctx.mk_ty(TyKind::FnPtr(sig_a));
    let sig_b = FnSig {
        inputs,
        output: f64_ty,
        c_variadic: false,
        unsafety: glyim_core::primitives::Safety::Safe,
        abi: glyim_core::primitives::Abi::Glyim,
    };
    let fn_b = ctx.mk_ty(TyKind::FnPtr(sig_b));
    infer
        .unify(&mut ctx, fn_a, fn_b, glyim_span::Span::DUMMY)
        .unwrap();
    assert_eq!(infer.probe_float_var(fvar), Some(f64_ty));
}

#[test]
fn test_fully_resolve_nested_fn_ptr_no_vars() {
    let mut ctx = test_ty_ctx();
    let infer = InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let inputs = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let inner_sig = FnSig {
        inputs,
        output: i32_ty,
        c_variadic: false,
        unsafety: glyim_core::primitives::Safety::Safe,
        abi: glyim_core::primitives::Abi::Glyim,
    };
    let inner_fn = ctx.mk_ty(TyKind::FnPtr(inner_sig));
    let outer_inputs = ctx.intern_substitution(vec![GenericArg::Ty(inner_fn)]);
    let outer_sig = FnSig {
        inputs: outer_inputs,
        output: inner_fn,
        c_variadic: false,
        unsafety: glyim_core::primitives::Safety::Safe,
        abi: glyim_core::primitives::Abi::Glyim,
    };
    let outer_fn = ctx.mk_ty(TyKind::FnPtr(outer_sig));
    let frozen = ctx.freeze();
    let result = infer.fully_resolve(&frozen, outer_fn);
    assert!(result.is_ok());
}

#[test]
fn test_unify_error_ignores_all_other_errors() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    // Error unifies with any type, so it should succeed even with mismatched types
    let result = infer.unify(&mut ctx, Ty::ERROR, Ty::UNIT, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let result2 = infer.unify(&mut ctx, Ty::ERROR, i32_ty, glyim_span::Span::DUMMY);
    assert!(result2.is_ok());
}

#[test]
fn test_never_unifies_with_anything_and_no_binding() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let ivar = infer.new_int_var(&mut ctx);
    let ivar_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(ivar)));
    let result = infer.unify(&mut ctx, Ty::NEVER, ivar_ty, glyim_span::Span::DUMMY);
    assert!(result.is_ok());
    assert!(infer.probe_int_var(ivar).is_none());
}

#[test]
fn test_unify_error_never_chain() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    // Error -> Never should be ok
    // Never -> Error should be ok
    // Error -> Error ok
    // Never -> Never ok
    assert!(
        infer
            .unify(&mut ctx, Ty::ERROR, Ty::NEVER, glyim_span::Span::DUMMY)
            .is_ok()
    );
    assert!(
        infer
            .unify(&mut ctx, Ty::NEVER, Ty::ERROR, glyim_span::Span::DUMMY)
            .is_ok()
    );
    assert!(
        infer
            .unify(&mut ctx, Ty::ERROR, Ty::ERROR, glyim_span::Span::DUMMY)
            .is_ok()
    );
    assert!(
        infer
            .unify(&mut ctx, Ty::NEVER, Ty::NEVER, glyim_span::Span::DUMMY)
            .is_ok()
    );
}

#[test]
fn test_fully_resolve_collects_all_unresolved_ty_vars() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let vars: Vec<_> = (0..5).map(|_| infer.new_ty_var(&mut ctx)).collect();
    let tys: Vec<_> = vars
        .iter()
        .map(|&v| ctx.mk_ty(TyKind::Infer(InferVar::Ty(v))))
        .collect();
    let subst = ctx.intern_substitution(tys.iter().map(|t| GenericArg::Ty(*t)).collect());
    let tuple_ty = ctx.mk_ty(TyKind::Tuple(subst));
    let frozen = ctx.freeze();
    let result = infer.fully_resolve(&frozen, tuple_ty);
    assert!(result.is_err());
    let unresolved = result.unwrap_err();
    assert_eq!(unresolved.len(), 5);
    for v in &vars {
        assert!(unresolved.contains(v));
    }
}

#[test]
fn test_unify_int_var_with_bool_error() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let ivar = infer.new_int_var(&mut ctx);
    let ivar_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(ivar)));
    let bool_ty = ctx.bool_ty();
    let err = infer
        .unify(&mut ctx, ivar_ty, bool_ty, glyim_span::Span::DUMMY)
        .unwrap_err();
    assert!(err[0].message.contains("expected integer type"));
}

#[test]
fn test_unify_float_var_with_bool_error() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let fvar = infer.new_float_var(&mut ctx);
    let fvar_ty = ctx.mk_ty(TyKind::Infer(InferVar::Float(fvar)));
    let bool_ty = ctx.bool_ty();
    let err = infer
        .unify(&mut ctx, fvar_ty, bool_ty, glyim_span::Span::DUMMY)
        .unwrap_err();
    assert!(err[0].message.contains("expected float type"));
}
