use crate::ty::{Ty, TyKind, TyArena};
use crate::unify::UnificationTable;
use crate::freeze::resolve_ty;
use glyim_hir::types::HirType;
use glyim_interner::Interner;
use glyim_diag::Span;

fn resolve_simple(ty: Ty, arena: &TyArena) -> HirType {
    let table = UnificationTable::new();
    resolve_ty(arena, &table, ty)
}

#[test]
fn freeze_int() {
    let mut arena = TyArena::new();
    let ty = arena.alloc(TyKind::Int);
    let hir = resolve_simple(ty, &arena);
    assert!(matches!(hir, HirType::Int));
}

#[test]
fn freeze_bool() {
    let mut arena = TyArena::new();
    let ty = arena.alloc(TyKind::Bool);
    let hir = resolve_simple(ty, &arena);
    assert!(matches!(hir, HirType::Bool));
}

#[test]
fn freeze_named() {
    let mut arena = TyArena::new();
    let mut interner = Interner::new();
    let sym = interner.intern("MyStruct");
    let ty = arena.alloc(TyKind::Named(sym));
    let hir = resolve_simple(ty, &arena);
    // Since we now store Symbol directly, we need to match on the Symbol.
    // We can intern the string again to get a Symbol for comparison.
    let expected_sym = interner.intern("MyStruct");
    if let HirType::Named(name) = hir {
        assert_eq!(name, expected_sym);
    } else {
        panic!("Expected Named");
    }
}

#[test]
fn freeze_generic() {
    let mut arena = TyArena::new();
    let mut interner = Interner::new();
    let vec_sym = interner.intern("Vec");
    let inner = arena.alloc(TyKind::Int);
    let ty = arena.alloc(TyKind::App(vec_sym, vec![inner]));
    let hir = resolve_simple(ty, &arena);
    if let HirType::Generic(name, args) = hir {
        assert_eq!(name, vec_sym);
        assert_eq!(args.len(), 1);
        assert!(matches!(args[0], HirType::Int));
    } else {
        panic!("Expected Generic");
    }
}

#[test]
fn freeze_infer_returns_error() {
    let mut arena = TyArena::new();
    let ty = arena.fresh_infer(Span::new(0, 1));
    let hir = resolve_simple(ty, &arena);
    assert!(matches!(hir, HirType::Error));
}

#[test]
fn freeze_error_returns_error() {
    let mut arena = TyArena::new();
    let ty = arena.alloc(TyKind::Error);
    let hir = resolve_simple(ty, &arena);
    assert!(matches!(hir, HirType::Error));
}

#[test]
fn freeze_unified_infer_resolves() {
    let mut arena = TyArena::new();
    let mut table = UnificationTable::new();
    let infer_ty = table.new_var(&mut arena, Span::new(0, 1));
    let int_ty = arena.alloc(TyKind::Int);
    table.unify(&mut arena, infer_ty, int_ty, Span::new(0, 1), &mut |_| {}).unwrap();
    let hir = resolve_ty(&arena, &table, infer_ty);
    assert!(matches!(hir, HirType::Int));
}
