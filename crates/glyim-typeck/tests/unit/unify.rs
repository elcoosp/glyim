use glyim_typeck::unify::unify;
use glyim_hir::types::HirType;
use glyim_interner::Interner;

fn sym(interner: &mut Interner, s: &str) -> glyim_interner::Symbol {
    interner.intern(s)
}

#[test]
fn unify_int_with_type_param() {
    let mut i = Interner::new();
    let t = sym(&mut i, "T");
    let result = unify(&HirType::Int, &HirType::Named(t), &[t]);
    assert!(result.is_ok());
    let sub = result.unwrap();
    assert_eq!(sub[&t], HirType::Int);
}

#[test]
fn unify_int_with_bool_fails() {
    let result = unify(&HirType::Int, &HirType::Bool, &[]);
    assert!(result.is_err());
}

#[test]
fn unify_generic_same_base() {
    let mut i = Interner::new();
    let t = sym(&mut i, "T");
    let vec_sym = sym(&mut i, "Vec");
    let result = unify(
        &HirType::Generic(vec_sym, vec![HirType::Int]),
        &HirType::Generic(vec_sym, vec![HirType::Named(t)]),
        &[t],
    );
    assert!(result.is_ok());
}

#[test]
fn unify_matching_named_types() {
    let mut i = Interner::new();
    let s1 = sym(&mut i, "Point");
    let s2 = sym(&mut i, "Point");
    let result = unify(&HirType::Named(s1), &HirType::Named(s2), &[]);
    assert!(result.is_ok());
}

#[test]
fn unify_different_named_types_fails() {
    let mut i = Interner::new();
    let s1 = sym(&mut i, "Point");
    let s2 = sym(&mut i, "Color");
    let result = unify(&HirType::Named(s1), &HirType::Named(s2), &[]);
    assert!(result.is_err());
}

#[test]
fn unify_rawptr_recursive() {
    let mut i = Interner::new();
    let t = sym(&mut i, "T");
    let result = unify(
        &HirType::RawPtr(Box::new(HirType::Int)),
        &HirType::RawPtr(Box::new(HirType::Named(t))),
        &[t],
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap()[&t], HirType::Int);
}

#[test]
fn unify_option_recursive() {
    let mut i = Interner::new();
    let t = sym(&mut i, "T");
    let result = unify(
        &HirType::Option(Box::new(HirType::Int)),
        &HirType::Option(Box::new(HirType::Named(t))),
        &[t],
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap()[&t], HirType::Int);
}

#[test]
fn unify_result_recursive() {
    let mut i = Interner::new();
    let t = sym(&mut i, "T");
    let e = sym(&mut i, "E");
    let result = unify(
        &HirType::Result(
            Box::new(HirType::Int),
            Box::new(HirType::Str),
        ),
        &HirType::Result(
            Box::new(HirType::Named(t)),
            Box::new(HirType::Named(e)),
        ),
        &[t, e],
    );
    assert!(result.is_ok());
    let sub = result.unwrap();
    assert_eq!(sub[&t], HirType::Int);
    assert_eq!(sub[&e], HirType::Str);
}

#[test]
fn unify_tuple_recursive() {
    let mut i = Interner::new();
    let t = sym(&mut i, "T");
    let result = unify(
        &HirType::Tuple(vec![HirType::Int, HirType::Bool]),
        &HirType::Tuple(vec![HirType::Named(t), HirType::Bool]),
        &[t],
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap()[&t], HirType::Int);
}

#[test]
fn unify_conflict_detected() {
    let mut i = Interner::new();
    let t = sym(&mut i, "T");
    let result = unify(&HirType::Bool, &HirType::Named(t), &[t]);
    assert!(result.is_ok());
    let sub = result.unwrap();
    assert_eq!(sub[&t], HirType::Bool);
}
