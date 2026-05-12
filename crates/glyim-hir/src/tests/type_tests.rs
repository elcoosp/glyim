use crate::types::{HirType, SubstitutionError, TypeVar, substitute_type, substitute_type_safe};
use glyim_interner::Interner;
use std::collections::HashMap;

#[test]
fn test_type_has_infer_detection() {
    assert!(HirType::Infer(TypeVar::from_raw_unchecked(0)).has_infer());
    assert!(!HirType::Int.has_infer());
    assert!(!HirType::Named(Interner::new().intern("Foo")).has_infer());
}

#[test]
fn test_has_param_detection() {
    let mut interner = Interner::new();
    let t = interner.intern("T");
    assert!(HirType::Param(t).has_param());
    assert!(!HirType::Int.has_param());
    assert!(!HirType::Named(interner.intern("Vec")).has_param());
}

#[test]
fn test_has_infer_nested() {
    let nested = HirType::Generic(
        Interner::new().intern("Vec"),
        vec![HirType::Infer(TypeVar::from_raw_unchecked(0))],
    );
    assert!(nested.has_infer());
}

#[test]
fn test_substitute_type_safe_basic() {
    let mut interner = Interner::new();
    let t = interner.intern("T");
    let sub = HashMap::from([(t, HirType::Int)]);

    let result = substitute_type_safe(&HirType::Param(t), &sub);
    assert_eq!(result, Ok(HirType::Int));
}

#[test]
fn test_substitute_type_depth_limit() {
    let mut interner = Interner::new();
    let t = interner.intern("T");
    // Create deeply nested type
    let mut ty = HirType::Param(t);
    for _ in 0..300 {
        ty = HirType::RawPtr(Box::new(ty));
    }
    let sub = HashMap::from([(t, HirType::Int)]);
    let result = substitute_type_safe(&ty, &sub);
    assert!(matches!(result, Err(SubstitutionError::DepthExceeded)));
}

#[test]
fn test_typevar_from_raw_unchecked() {
    let tv = TypeVar::from_raw_unchecked(42);
    assert_eq!(tv.raw_index(), 42);
}

#[test]
fn test_hir_type_is_send_sync() {
    fn assert_bounds<T: Send + Sync>() {}
    assert_bounds::<HirType>();
    assert_bounds::<TypeVar>();
}
