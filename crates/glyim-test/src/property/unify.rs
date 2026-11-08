use glyim_solve::InferenceTable;
use glyim_span::Span;
use glyim_type::{Ty, TyCtxMut};

/// `test_unify_var_with_concrete`.
pub fn test_unify_var_with_concrete(
    ctx: &mut TyCtxMut,
    infer: &mut InferenceTable,
    var_ty: Ty,
    concrete: Ty,
) {
    let result = infer.unify(ctx, var_ty, concrete, Span::DUMMY);
    assert!(
        result.is_ok(),
        "unification failed: {var_ty:?} with {concrete:?}"
    );
}

/// `test_unify_different_types_fails`.
pub fn test_unify_different_types_fails(
    ctx: &mut TyCtxMut,
    infer: &mut InferenceTable,
    a: Ty,
    b: Ty,
) {
    let result = infer.unify(ctx, a, b, Span::DUMMY);
    assert!(
        result.is_err(),
        "unification should fail for different types: {a:?} vs {b:?}"
    );
}

/// `test_unify_same_type_succeeds`.
pub fn test_unify_same_type_succeeds(ctx: &mut TyCtxMut, infer: &mut InferenceTable, ty: Ty) {
    let result = infer.unify(ctx, ty, ty, Span::DUMMY);
    assert!(
        result.is_ok(),
        "unification of same type should succeed: {ty:?}"
    );
}
