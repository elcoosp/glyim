use crate::diagnostics::zippering::zip_diff;
use crate::diagnostics::biabduction::bi_abductive_synthesis;
use crate::diagnostics::{TypeError, AutoFix};
use crate::ty::{Ty, TyKind, TyArena};
use crate::unify::UnificationTable;
use glyim_interner::Interner;
use glyim_diag::Span;

// Zippering tests
#[test]
fn zip_diff_identical() {
    let mut arena = TyArena::new();
    let t0 = arena.alloc(TyKind::Int);
    let t1 = arena.alloc(TyKind::Int);
    let result = zip_diff(&arena, t0, t1, "root".to_string());
    assert!(result.is_none());
}

#[test]
fn zip_diff_different_primitives() {
    let mut arena = TyArena::new();
    let t0 = arena.alloc(TyKind::Int);
    let t1 = arena.alloc(TyKind::Bool);
    let result = zip_diff(&arena, t0, t1, "root".to_string());
    assert!(result.is_some());
}

#[test]
fn zip_diff_nested_generic() {
    let mut arena = TyArena::new();
    let mut interner = Interner::new();
    let vec_sym = interner.intern("Vec");
    let result_sym = interner.intern("Result");
    let int_ty = arena.alloc(TyKind::Int);
    let bool_ty = arena.alloc(TyKind::Bool);
    let result_ib = arena.alloc(TyKind::App(result_sym, vec![int_ty, bool_ty]));
    let result_bb = arena.alloc(TyKind::App(result_sym, vec![bool_ty, bool_ty]));
    let vec_ib = arena.alloc(TyKind::App(vec_sym, vec![result_ib]));
    let vec_bb = arena.alloc(TyKind::App(vec_sym, vec![result_bb]));
    let result = zip_diff(&arena, vec_ib, vec_bb, "root".to_string());
    assert!(result.is_some());
    assert!(result.unwrap().contains("T0.T0"));
}

// Biabduction tests
#[test]
fn biabduction_wrap_option() {
    let mut arena = TyArena::new();
    let mut interner = Interner::new();
    let opt_sym = interner.intern("Option");
    let int_ty = arena.alloc(TyKind::Int);
    let option_int = arena.alloc(TyKind::App(opt_sym, vec![int_ty]));
    let result = bi_abductive_synthesis(&arena, &interner, option_int, int_ty);
    assert!(result.is_some());
    assert!(matches!(result.unwrap(), AutoFix::WrapWithOptions(_)));
}

#[test]
fn biabduction_wrap_ok() {
    let mut arena = TyArena::new();
    let mut interner = Interner::new();
    let result_sym = interner.intern("Result");
    let int_ty = arena.alloc(TyKind::Int);
    let err_ty = arena.alloc(TyKind::Named(interner.intern("Error")));
    let result_int = arena.alloc(TyKind::App(result_sym, vec![int_ty, err_ty]));
    let result = bi_abductive_synthesis(&arena, &interner, result_int, int_ty);
    assert!(result.is_some());
    assert!(matches!(result.unwrap(), AutoFix::WrapWithOk(_)));
}

// Unify with diagnostics
#[test]
fn unify_mismatch_includes_diff_path() {
    let mut arena = TyArena::new();
    let mut interner = Interner::new();
    let mut table = UnificationTable::with_interner(interner.clone());
    let vec_sym = interner.intern("Vec");
    let int_ty = arena.alloc(TyKind::Int);
    let bool_ty = arena.alloc(TyKind::Bool);
    let vec_int = arena.alloc(TyKind::App(vec_sym, vec![int_ty]));
    let vec_bool = arena.alloc(TyKind::App(vec_sym, vec![bool_ty]));
    let mut errors = Vec::new();
    let _ = table.unify(&mut arena, vec_int, vec_bool, Span::new(0, 10), &mut |e| errors.push(e));
    assert!(!errors.is_empty());
    if let TypeError::MismatchedTypes { diff_path, .. } = &errors[0] {
        assert!(diff_path.is_some());
    }
}

#[test]
fn unify_mismatch_option_includes_autofix() {
    let mut arena = TyArena::new();
    let mut interner = Interner::new();
    let mut table = UnificationTable::with_interner(interner.clone());
    let opt_sym = interner.intern("Option");
    let int_ty = arena.alloc(TyKind::Int);
    let option_int = arena.alloc(TyKind::App(opt_sym, vec![int_ty]));
    let mut errors = Vec::new();
    let _ = table.unify(&mut arena, option_int, int_ty, Span::new(0, 5), &mut |e| errors.push(e));
    assert!(!errors.is_empty());
    if let TypeError::MismatchedTypes { autofix, .. } = &errors[0] {
        assert!(autofix.is_some());
    }
}
