use crate::unify::ErrorGuaranteed;
use crate::ty::{TyKind, TyArena};
use crate::unify::UnificationTable;
use glyim_diag::Span;

#[test]
fn error_guaranteed_is_send_sync() {
    fn assert_bounds<T: Send + Sync>() {}
    assert_bounds::<ErrorGuaranteed>();
}

#[test]
fn unification_table_new_var() {
    let mut arena = TyArena::new();
    let mut table = UnificationTable::new();
    let ty = table.new_var(&mut arena, Span::new(0, 1));
    assert!(matches!(arena.get(ty), TyKind::Infer));
}

#[test]
fn unification_table_new_var_creates_distinct_vars() {
    let mut arena = TyArena::new();
    let mut table = UnificationTable::new();
    let t0 = table.new_var(&mut arena, Span::new(0, 1));
    let t1 = table.new_var(&mut arena, Span::new(2, 3));
    assert_ne!(t0, t1);
}

#[test]
fn unification_table_find_self() {
    let mut arena = TyArena::new();
    let mut table = UnificationTable::new();
    let t0 = table.new_var(&mut arena, Span::new(0, 1));
    let root = table.find(&arena, t0);
    assert_eq!(root, t0);
}
