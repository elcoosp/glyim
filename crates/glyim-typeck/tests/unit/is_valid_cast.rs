use glyim_typeck::typeck::resolver::is_valid_cast;
use glyim_hir::types::HirType;

#[test]
fn valid_primitive_casts() {
    assert!(is_valid_cast(&HirType::Int, &HirType::Float));
    assert!(is_valid_cast(&HirType::Float, &HirType::Int));
    assert!(is_valid_cast(&HirType::Int, &HirType::Int));
}

#[test]
fn valid_ptr_to_any_cast() {
    assert!(is_valid_cast(
        &HirType::RawPtr(Box::new(HirType::Int)),
        &HirType::Int
    ));
    assert!(is_valid_cast(
        &HirType::RawPtr(Box::new(HirType::Float)),
        &HirType::Float
    ));
}

#[test]
fn valid_named_to_int() {
    let mut interner = glyim_interner::Interner::new();
    let sym = interner.intern("MyStruct");
    assert!(is_valid_cast(&HirType::Named(sym), &HirType::Int));
}

#[test]
fn invalid_cast_int_to_struct() {
    let mut interner = glyim_interner::Interner::new();
    let sym = interner.intern("Point");
    assert!(!is_valid_cast(&HirType::Int, &HirType::Named(sym)));
}

#[test]
fn generic_cast_matches_same_base() {
    let mut interner = glyim_interner::Interner::new();
    let vec_sym = interner.intern("Vec");
    let t_sym = interner.intern("T");
    assert!(is_valid_cast(
        &HirType::Generic(vec_sym, vec![HirType::Int]),
        &HirType::Generic(vec_sym, vec![HirType::Named(t_sym)])
    ));
}

#[test]
fn invalid_cast_int_to_str() {
    assert!(!is_valid_cast(&HirType::Int, &HirType::Str));
}

#[test]
fn invalid_cast_bool_to_int() {
    assert!(!is_valid_cast(&HirType::Bool, &HirType::Int));
}

#[test]
fn valid_cast_any_to_named() {
    let mut interner = glyim_interner::Interner::new();
    let sym = interner.intern("SomeType");
    assert!(is_valid_cast(&HirType::Int, &HirType::Named(sym)));
}

#[test]
fn valid_cast_named_to_generic_same_base() {
    let mut interner = glyim_interner::Interner::new();
    let vec_sym = interner.intern("Vec");
    assert!(is_valid_cast(
        &HirType::Named(vec_sym),
        &HirType::Generic(vec_sym, vec![HirType::Int])
    ));
}
