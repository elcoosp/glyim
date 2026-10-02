use glyim_core::primitives::{IntTy, UintTy};
use glyim_test::test_ty_ctx;
use glyim_type::*;

#[test]
fn test_coerce_array_to_slice() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let usize_ty = ctx_mut.mk_ty(TyKind::Uint(UintTy::Usize));
    let len_const = Const {
        kind: ConstKind::Int(3),
        ty: usize_ty,
    };
    let array_ty = ctx_mut.mk_ty(TyKind::Array(i32_ty, len_const));
    let slice_ty = ctx_mut.mk_ty(TyKind::Slice(i32_ty));
    let ctx = ctx_mut.freeze();
    assert!(crate::fulfill::can_coerce(&ctx, array_ty, slice_ty));
    assert!(!crate::fulfill::can_coerce(&ctx, slice_ty, array_ty));
}

#[test]
fn test_coerce_identity() {
    let ctx_mut = test_ty_ctx();
    let bool_ty = ctx_mut.bool_ty();
    let ctx = ctx_mut.freeze();
    assert!(crate::fulfill::can_coerce(&ctx, bool_ty, bool_ty));
}

#[test]
fn test_coerce_ref_subtyping() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let ref_i32 = ctx_mut.mk_ref(
        Region::Erased,
        i32_ty,
        glyim_core::primitives::Mutability::Not,
    );
    let ref_i32_again = ctx_mut.mk_ref(
        Region::Erased,
        i32_ty,
        glyim_core::primitives::Mutability::Not,
    );
    let ctx = ctx_mut.freeze();
    assert!(crate::fulfill::can_coerce(&ctx, ref_i32, ref_i32_again));
}

#[test]
fn test_coerce_fn_item_to_fn_ptr() {
    use glyim_core::def_id::FnDefId;
    use glyim_core::primitives::{Abi, Safety};

    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let sig = FnSig {
        inputs: ctx_mut.intern_substitution(vec![GenericArg::Ty(i32_ty)]),
        output: i32_ty,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let fn_def_id = FnDefId::from_raw(900);
    ctx_mut.register_fn_sig(fn_def_id, sig.clone());

    let fn_subst = ctx_mut.intern_substitution(vec![]);
    let fn_item_ty = ctx_mut.mk_ty(TyKind::FnDef(fn_def_id, fn_subst));
    let fn_ptr_ty = ctx_mut.mk_ty(TyKind::FnPtr(sig.clone()));
    let ctx = ctx_mut.freeze();

    // A function item coerces to a fn pointer with a matching signature.
    assert!(
        crate::fulfill::can_coerce(&ctx, fn_item_ty, fn_ptr_ty),
        "fn item should coerce to fn pointer with matching signature"
    );
}

#[test]
fn test_coerce_non_capturing_closure_to_fn_ptr() {
    use glyim_core::def_id::ClosureId;
    use glyim_core::primitives::{Abi, Safety};

    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));

    // Non-capturing closure: signature `fn(i32) -> i32`, no capture environment.
    let closure_adt = ctx_mut.register_closure(vec![]);
    let closure_id = ClosureId::from_raw(closure_adt.to_raw());
    let closure_sig = FnSig {
        inputs: ctx_mut.intern_substitution(vec![GenericArg::Ty(i32_ty)]),
        output: i32_ty,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    ctx_mut.register_closure_sig(closure_id, closure_sig.clone());
    let closure_substs = ctx_mut.intern_substitution(vec![]);

    // A matching fn pointer.
    let fn_ptr_ty = ctx_mut.mk_ty(TyKind::FnPtr(closure_sig.clone()));

    // A mismatched fn pointer (different param type) must NOT coerce.
    let bool_ty = ctx_mut.bool_ty();
    let mismatched_sig = FnSig {
        inputs: ctx_mut.intern_substitution(vec![GenericArg::Ty(bool_ty)]),
        output: i32_ty,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let mismatched_fn_ptr = ctx_mut.mk_ty(TyKind::FnPtr(mismatched_sig));

    let closure_ty = ctx_mut.mk_ty(TyKind::Closure(closure_id, closure_substs));

    let ctx = ctx_mut.freeze();

    assert!(
        crate::fulfill::can_coerce(&ctx, closure_ty, fn_ptr_ty),
        "non-capturing closure should coerce to fn pointer with matching signature"
    );
    assert!(
        !crate::fulfill::can_coerce(&ctx, closure_ty, mismatched_fn_ptr),
        "closure must NOT coerce to fn pointer with mismatched signature"
    );
}

#[test]
fn test_coerce_capturing_closure_rejects_fn_ptr() {
    use glyim_core::def_id::ClosureId;
    use glyim_core::primitives::{Abi, Safety};

    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));

    // Capturing closure: one captured i32 in its environment.
    let capture_name = ctx_mut.resolver().intern("capture_0");
    let closure_adt = ctx_mut.register_closure(vec![(capture_name, i32_ty)]);
    let closure_id = ClosureId::from_raw(closure_adt.to_raw());
    let closure_sig = FnSig {
        inputs: ctx_mut.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(i32_ty)]),
        output: i32_ty,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    ctx_mut.register_closure_sig(closure_id, closure_sig.clone());
    let closure_substs = ctx_mut.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let closure_ty = ctx_mut.mk_ty(TyKind::Closure(closure_id, closure_substs));

    let fn_ptr_substs = ctx_mut.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let fn_ptr_sig = FnSig {
        inputs: fn_ptr_substs,
        output: i32_ty,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let fn_ptr_ty = ctx_mut.mk_ty(TyKind::FnPtr(fn_ptr_sig));

    let ctx = ctx_mut.freeze();
    assert!(
        !crate::fulfill::can_coerce(&ctx, closure_ty, fn_ptr_ty),
        "capturing closure must NOT coerce to bare fn pointer"
    );
}

#[test]
fn test_coerce_fn_item_to_fn_ptr_rejects_mismatched_sig() {
    use glyim_core::def_id::FnDefId;
    use glyim_core::primitives::{Abi, Safety};

    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let u64_ty = ctx_mut.mk_ty(TyKind::Uint(UintTy::U64));
    let def_sig = FnSig {
        inputs: ctx_mut.intern_substitution(vec![GenericArg::Ty(i32_ty)]),
        output: i32_ty,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let fn_def_id = FnDefId::from_raw(901);
    ctx_mut.register_fn_sig(fn_def_id, def_sig);

    let fn_subst = ctx_mut.intern_substitution(vec![]);
    let fn_item_ty = ctx_mut.mk_ty(TyKind::FnDef(fn_def_id, fn_subst));
    // Target fn pointer has a different return type.
    let mismatched_sig = FnSig {
        inputs: ctx_mut.intern_substitution(vec![GenericArg::Ty(i32_ty)]),
        output: u64_ty,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let fn_ptr_ty = ctx_mut.mk_ty(TyKind::FnPtr(mismatched_sig));
    let ctx = ctx_mut.freeze();

    assert!(
        !crate::fulfill::can_coerce(&ctx, fn_item_ty, fn_ptr_ty),
        "fn item must NOT coerce to fn pointer with mismatched signature"
    );
}

/// `can_coerce` in `solver` (the variant driven by `Predicate::Coerce`) must
/// also support fn-item → fn-pointer coercion, consistent with `fulfill`.
#[test]
fn test_solver_coerce_fn_item_to_fn_ptr() {
    use glyim_core::def_id::FnDefId;
    use glyim_core::primitives::{Abi, Safety};

    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let sig = FnSig {
        inputs: ctx_mut.intern_substitution(vec![GenericArg::Ty(i32_ty)]),
        output: i32_ty,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let fn_def_id = FnDefId::from_raw(902);
    ctx_mut.register_fn_sig(fn_def_id, sig.clone());

    let fn_subst = ctx_mut.intern_substitution(vec![]);
    let fn_item_ty = ctx_mut.mk_ty(TyKind::FnDef(fn_def_id, fn_subst));
    let fn_ptr_ty = ctx_mut.mk_ty(TyKind::FnPtr(sig.clone()));
    let ctx = ctx_mut.freeze();

    assert!(
        crate::solver::can_coerce(&ctx, fn_item_ty, fn_ptr_ty),
        "solver can_coerce: fn item should coerce to fn pointer with matching signature"
    );
}

#[test]
fn test_solver_coerce_fn_item_to_fn_ptr_rejects_mismatch() {
    use glyim_core::def_id::FnDefId;
    use glyim_core::primitives::{Abi, Safety};

    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let u64_ty = ctx_mut.mk_ty(TyKind::Uint(UintTy::U64));
    let def_sig = FnSig {
        inputs: ctx_mut.intern_substitution(vec![GenericArg::Ty(i32_ty)]),
        output: i32_ty,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let fn_def_id = FnDefId::from_raw(903);
    ctx_mut.register_fn_sig(fn_def_id, def_sig);

    let fn_subst = ctx_mut.intern_substitution(vec![]);
    let fn_item_ty = ctx_mut.mk_ty(TyKind::FnDef(fn_def_id, fn_subst));
    let mismatched_sig = FnSig {
        inputs: ctx_mut.intern_substitution(vec![GenericArg::Ty(i32_ty)]),
        output: u64_ty,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let fn_ptr_ty = ctx_mut.mk_ty(TyKind::FnPtr(mismatched_sig));
    let ctx = ctx_mut.freeze();

    assert!(
        !crate::solver::can_coerce(&ctx, fn_item_ty, fn_ptr_ty),
        "solver can_coerce: fn item must NOT coerce to fn pointer with mismatched signature"
    );
}

/// SOLVE-7 regression: `can_coerce` had a precedence bug
/// (`(mut_a == mut_b) || (…) && can_coerce(inner)`) that skipped the pointee
/// check whenever the mutabilities matched. `&i32 -> &str` must NOT coerce.
#[test]
fn test_coerce_ref_pointee_must_match() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let str_ty = ctx_mut.mk_ty(TyKind::String);
    let ref_i32 = ctx_mut.mk_ref(Region::Erased, i32_ty, glyim_core::primitives::Mutability::Not);
    let ref_str = ctx_mut.mk_ref(Region::Erased, str_ty, glyim_core::primitives::Mutability::Not);
    let ctx = ctx_mut.freeze();
    assert!(
        !crate::fulfill::can_coerce(&ctx, ref_i32, ref_str),
        "&i32 must not coerce to &str (SOLVE-7)"
    );
    assert!(
        !crate::solver::can_coerce(&ctx, ref_i32, ref_str),
        "&i32 must not coerce to &str (SOLVE-7, solver.rs copy)"
    );
}

/// Sanity: `&i32 -> &i32` still coerces (identity), and `&mut i32 -> &i32`
/// (mutability weakening) still coerces.
#[test]
fn test_coerce_ref_same_pointee_still_works() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let ref_i32 = ctx_mut.mk_ref(Region::Erased, i32_ty, glyim_core::primitives::Mutability::Not);
    let refmut_i32 = ctx_mut.mk_ref(Region::Erased, i32_ty, glyim_core::primitives::Mutability::Mut);
    let ctx = ctx_mut.freeze();
    assert!(crate::fulfill::can_coerce(&ctx, ref_i32, ref_i32));
    assert!(crate::fulfill::can_coerce(&ctx, refmut_i32, ref_i32));
}
