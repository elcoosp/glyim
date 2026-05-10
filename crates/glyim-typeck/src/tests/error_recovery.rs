use crate::ty::{Ty, TyKind, TyArena};
use crate::unify::UnificationTable;
use crate::diagnostics::span_to_src;
use glyim_diag::Span;

#[test]
fn error_recovery_no_cascade() {
    let mut arena = TyArena::new();
    let mut table = UnificationTable::new();

    let t_int = arena.alloc(TyKind::Int);
    let t_str = arena.alloc(TyKind::Str);

    // Unify Int and Str -> should fail, emitting error
    let mut errors = Vec::new();
    let result = table.unify(
        &mut arena, t_int, t_str, Span::new(0, 1),
        &mut |e| errors.push(e),
    );
    assert!(result.is_err());
    assert!(!errors.is_empty());

    // After the failure, Error type should unify with anything
    let t_error = arena.alloc(TyKind::Error);
    let t_int2 = arena.alloc(TyKind::Int);
    let mut errors2 = Vec::new();
    let result2 = table.unify(
        &mut arena, t_error, t_int2, Span::new(5, 6),
        &mut |e| errors2.push(e),
    );
    assert!(result2.is_ok());
    assert!(errors2.is_empty()); // no cascading errors
}
