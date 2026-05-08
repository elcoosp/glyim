use crate::ty::{Ty, TyKind, TyArena};
use crate::rep::{Rep, RepMeta, build_rep_struct, build_rep_enum};
use crate::rep::optics::{generate_lenses, generate_prisms};
use crate::reflect::generate_type_meta;
use glyim_interner::Interner;

#[test]
fn rep_unit() {
    assert!(matches!(Rep::Unit, Rep::Unit));
}

#[test]
fn build_rep_struct_simple() {
    let mut arena = TyArena::new();
    let mut interner = Interner::new();
    let name_sym = interner.intern("name");
    let age_sym = interner.intern("age");
    let user_sym = interner.intern("User");
    let str_ty = arena.alloc(TyKind::Str);
    let int_ty = arena.alloc(TyKind::Int);
    let rep = build_rep_struct(user_sym, &[(name_sym, str_ty), (age_sym, int_ty)], vec![]);
    assert!(matches!(rep, Rep::Meta(_, _)));
}

#[test]
fn build_rep_enum_simple() {
    let mut arena = TyArena::new();
    let mut interner = Interner::new();
    let none_sym = interner.intern("None");
    let some_sym = interner.intern("Some");
    let opt_sym = interner.intern("Option");
    let int_ty = arena.alloc(TyKind::Int);
    let u0 = interner.intern("_0");
    let rep = build_rep_enum(opt_sym, &[(none_sym, vec![]), (some_sym, vec![(u0, int_ty)])], vec![]);
    assert!(matches!(rep, Rep::Meta(_, _)));
}

#[test]
fn generate_lenses_for_struct() {
    let mut arena = TyArena::new();
    let mut interner = Interner::new();
    let name_sym = interner.intern("name");
    let age_sym = interner.intern("age");
    let user_sym = interner.intern("User");
    let str_ty = arena.alloc(TyKind::Str);
    let int_ty = arena.alloc(TyKind::Int);
    let rep = build_rep_struct(user_sym, &[(name_sym, str_ty), (age_sym, int_ty)], vec![]);
    let lenses = generate_lenses(&rep);
    assert_eq!(lenses.len(), 2);
}

#[test]
fn generate_prisms_for_enum() {
    let mut arena = TyArena::new();
    let mut interner = Interner::new();
    let none_sym = interner.intern("None");
    let some_sym = interner.intern("Some");
    let opt_sym = interner.intern("Option");
    let int_ty = arena.alloc(TyKind::Int);
    let u0 = interner.intern("_0");
    let rep = build_rep_enum(opt_sym, &[(none_sym, vec![]), (some_sym, vec![(u0, int_ty)])], vec![]);
    let prisms = generate_prisms(&rep);
    assert!(prisms.len() >= 1);
}

#[test]
fn generate_type_meta_basic() {
    let meta = generate_type_meta(1, "User", &["name".into(), "age".into()], &[2, 3]);
    assert_eq!(meta.type_id, 1);
    assert_eq!(meta.field_count, 2);
}
