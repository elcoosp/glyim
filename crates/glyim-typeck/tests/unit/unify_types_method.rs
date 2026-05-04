use glyim_typeck::TypeChecker;
use glyim_hir::types::HirType;
use glyim_interner::Interner;
use std::collections::HashMap;

#[test]
fn unify_types_nested_generics() {
    let mut tc = TypeChecker::new(Interner::new());
    let t = tc.interner.intern("T");
    let u = tc.interner.intern("U");
    let type_params = vec![t, u];

    let mut sub = HashMap::new();
    tc.unify_types(
        &HirType::Generic(
            tc.interner.intern("Pair"),
            vec![HirType::Int, HirType::Bool],
        ),
        &HirType::Generic(
            tc.interner.intern("Pair"),
            vec![HirType::Named(t), HirType::Named(u)],
        ),
        &type_params,
        &mut sub,
    );
    assert_eq!(sub[&t], HirType::Int);
    assert_eq!(sub[&u], HirType::Bool);
}

#[test]
fn unify_types_rawptr_recursive() {
    let mut tc = TypeChecker::new(Interner::new());
    let t = tc.interner.intern("T");
    let type_params = vec![t];

    let mut sub = HashMap::new();
    tc.unify_types(
        &HirType::RawPtr(Box::new(HirType::Int)),
        &HirType::RawPtr(Box::new(HirType::Named(t))),
        &type_params,
        &mut sub,
    );
    assert_eq!(sub[&t], HirType::Int);
}

#[test]
fn unify_types_tuple_recursive() {
    let mut tc = TypeChecker::new(Interner::new());
    let t = tc.interner.intern("T");
    let u = tc.interner.intern("U");
    let type_params = vec![t, u];

    let mut sub = HashMap::new();
    tc.unify_types(
        &HirType::Tuple(vec![HirType::Int, HirType::Bool]),
        &HirType::Tuple(vec![HirType::Named(t), HirType::Named(u)]),
        &type_params,
        &mut sub,
    );
    assert_eq!(sub[&t], HirType::Int);
    assert_eq!(sub[&u], HirType::Bool);
}

#[test]
fn unify_types_named_concrete_to_generic() {
    let mut tc = TypeChecker::new(Interner::new());
    let t = tc.interner.intern("T");
    let type_params = vec![t];

    let mut sub = HashMap::new();
    tc.unify_types(
        &HirType::Named(tc.interner.intern("Vec")),
        &HirType::Named(t),
        &type_params,
        &mut sub,
    );
    assert_eq!(
        sub[&t],
        HirType::Named(tc.interner.intern("Vec"))
    );
}
#[test]
fn unify_types_rawptr_success() {
    let mut tc = TypeChecker::new(Interner::new());
    let t = tc.interner.intern("T");
    let type_params = vec![t];
    let mut sub = HashMap::new();
    tc.unify_types(
        &HirType::RawPtr(Box::new(HirType::Int)),
        &HirType::RawPtr(Box::new(HirType::Named(t))),
        &type_params,
        &mut sub,
    );
    assert_eq!(sub[&t], HirType::Int);
}

#[test]
fn unify_types_option_success() {
    let mut tc = TypeChecker::new(Interner::new());
    let t = tc.interner.intern("T");
    let type_params = vec![t];
    let mut sub = HashMap::new();
    tc.unify_types(
        &HirType::Option(Box::new(HirType::Int)),
        &HirType::Option(Box::new(HirType::Named(t))),
        &type_params,
        &mut sub,
    );
    assert_eq!(sub[&t], HirType::Int);
}

#[test]
fn unify_types_result_success() {
    let mut tc = TypeChecker::new(Interner::new());
    let t = tc.interner.intern("T");
    let e = tc.interner.intern("E");
    let type_params = vec![t, e];
    let mut sub = HashMap::new();
    tc.unify_types(
        &HirType::Result(
            Box::new(HirType::Int),
            Box::new(HirType::Str),
        ),
        &HirType::Result(
            Box::new(HirType::Named(t)),
            Box::new(HirType::Named(e)),
        ),
        &type_params,
        &mut sub,
    );
    assert_eq!(sub[&t], HirType::Int);
    assert_eq!(sub[&e], HirType::Str);
}
#[test]
fn unify_types_rawptr_success() {
    let mut tc = TypeChecker::new(Interner::new());
    let t = tc.interner.intern("T");
    let type_params = vec![t];
    let mut sub = HashMap::new();
    tc.unify_types(
        &HirType::RawPtr(Box::new(HirType::Int)),
        &HirType::RawPtr(Box::new(HirType::Named(t))),
        &type_params,
        &mut sub,
    );
    assert_eq!(sub[&t], HirType::Int);
}

#[test]
fn unify_types_option_success() {
    let mut tc = TypeChecker::new(Interner::new());
    let t = tc.interner.intern("T");
    let type_params = vec![t];
    let mut sub = HashMap::new();
    tc.unify_types(
        &HirType::Option(Box::new(HirType::Int)),
        &HirType::Option(Box::new(HirType::Named(t))),
        &type_params,
        &mut sub,
    );
    assert_eq!(sub[&t], HirType::Int);
}

#[test]
fn unify_types_result_success() {
    let mut tc = TypeChecker::new(Interner::new());
    let t = tc.interner.intern("T");
    let e = tc.interner.intern("E");
    let type_params = vec![t, e];
    let mut sub = HashMap::new();
    tc.unify_types(
        &HirType::Result(
            Box::new(HirType::Int),
            Box::new(HirType::Str),
        ),
        &HirType::Result(
            Box::new(HirType::Named(t)),
            Box::new(HirType::Named(e)),
        ),
        &type_params,
        &mut sub,
    );
    assert_eq!(sub[&t], HirType::Int);
    assert_eq!(sub[&e], HirType::Str);
}
#[test]
fn unify_types_rawptr_success() {
    let mut tc = TypeChecker::new(Interner::new());
    let t = tc.interner.intern("T");
    let type_params = vec![t];
    let mut sub = HashMap::new();
    tc.unify_types(
        &HirType::RawPtr(Box::new(HirType::Int)),
        &HirType::RawPtr(Box::new(HirType::Named(t))),
        &type_params,
        &mut sub,
    );
    assert_eq!(sub[&t], HirType::Int);
}

#[test]
fn unify_types_option_success() {
    let mut tc = TypeChecker::new(Interner::new());
    let t = tc.interner.intern("T");
    let type_params = vec![t];
    let mut sub = HashMap::new();
    tc.unify_types(
        &HirType::Option(Box::new(HirType::Int)),
        &HirType::Option(Box::new(HirType::Named(t))),
        &type_params,
        &mut sub,
    );
    assert_eq!(sub[&t], HirType::Int);
}

#[test]
fn unify_types_result_success() {
    let mut tc = TypeChecker::new(Interner::new());
    let t = tc.interner.intern("T");
    let e = tc.interner.intern("E");
    let type_params = vec![t, e];
    let mut sub = HashMap::new();
    tc.unify_types(
        &HirType::Result(
            Box::new(HirType::Int),
            Box::new(HirType::Str),
        ),
        &HirType::Result(
            Box::new(HirType::Named(t)),
            Box::new(HirType::Named(e)),
        ),
        &type_params,
        &mut sub,
    );
    assert_eq!(sub[&t], HirType::Int);
    assert_eq!(sub[&e], HirType::Str);
}
