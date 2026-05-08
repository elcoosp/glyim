use crate::ty::{Ty, TyKind, TyArena};
use crate::unify::UnificationTable;
use crate::diagnostics::TypeError;
use glyim_diag::Span;
use glyim_interner::Interner;

#[test]
fn snapshot_infinite_type_error() {
    let mut arena = TyArena::new();
    let mut table = UnificationTable::new();
    let mut interner = Interner::new();

    let t0 = table.new_var(&mut arena, Span::new(10, 20));
    let vec_sym = interner.intern("Vec");
    let vec_t0 = arena.alloc(TyKind::App(vec_sym, vec![t0]));

    let mut errors = Vec::new();
    let _ = table.unify(&mut arena, t0, vec_t0, Span::new(10, 20), &mut |e| errors.push(e));

    insta::assert_debug_snapshot!(errors);
}

#[test]
fn snapshot_mismatch_with_diff() {
    let mut arena = TyArena::new();
    let mut table = UnificationTable::with_interner(Interner::new());
    let mut interner = Interner::new();

    let vec_sym = interner.intern("Vec");
    let result_sym = interner.intern("Result");
    let int_ty = arena.alloc(TyKind::Int);
    let bool_ty = arena.alloc(TyKind::Bool);
    let result_ib = arena.alloc(TyKind::App(result_sym, vec![int_ty, bool_ty]));
    let result_bb = arena.alloc(TyKind::App(result_sym, vec![bool_ty, bool_ty]));
    let vec_ib = arena.alloc(TyKind::App(vec_sym, vec![result_ib]));
    let vec_bb = arena.alloc(TyKind::App(vec_sym, vec![result_bb]));

    let mut errors = Vec::new();
    let _ = table.unify(&mut arena, vec_ib, vec_bb, Span::new(0, 30), &mut |e| errors.push(e));

    insta::assert_debug_snapshot!(errors);
}

#[test]
fn snapshot_option_autofix() {
    let mut arena = TyArena::new();
    let mut table = UnificationTable::with_interner(Interner::new());
    let mut interner = Interner::new();

    let opt_sym = interner.intern("Option");
    let int_ty = arena.alloc(TyKind::Int);
    let option_int = arena.alloc(TyKind::App(opt_sym, vec![int_ty]));

    let mut errors = Vec::new();
    let _ = table.unify(&mut arena, option_int, int_ty, Span::new(0, 10), &mut |e| errors.push(e));

    insta::assert_debug_snapshot!(errors);
}
