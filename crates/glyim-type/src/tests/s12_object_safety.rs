//! S12-T04: Object safety checks reject self-types correctly.

use crate::object_safety::*;
use glyim_core::def_id::TraitDefId;
use glyim_core::interner::Interner;
use glyim_span::{ByteIdx, FileId, Span, SyntaxContext};

fn test_span() -> Span {
    Span::range(
        FileId::BOGUS,
        ByteIdx::ZERO,
        ByteIdx::ZERO,
        SyntaxContext::ROOT,
    )
}

fn named(s: &str) -> glyim_core::interner::Name {
    Interner::default().intern(s)
}

/// Helper that builds a `TraitObjectSafetyInput` from just the parts the older
/// tests care about, leaving associated types and supertraits empty.
fn check(requires_self_sized: bool, methods: &[MethodSignature]) -> Vec<ObjectSafetyViolation> {
    check_object_safety(&TraitObjectSafetyInput {
        requires_self_sized,
        methods,
        associated_types: &[],
        supertrait_safety: &[],
    })
}

fn method(name: &str, self_kind: MethodSelfKind, has_generic_params: bool) -> MethodSignature {
    MethodSignature {
        name: named(name),
        span: test_span(),
        self_kind,
        has_generic_params,
        returns_self: false,
    }
}

// ---- Self: Sized trait is not object-safe ----

#[test]
fn self_sized_trait_is_not_object_safe() {
    let violations = check(true, &[]);
    assert_eq!(violations.len(), 1);
    assert!(matches!(violations[0], ObjectSafetyViolation::SelfSized));
}

// ---- Trait with only &self methods is object-safe ----

#[test]
fn trait_with_ref_self_only_is_object_safe() {
    let methods = vec![method("method", MethodSelfKind::ByReference, false)];
    let violations = check(false, &methods);
    assert!(
        violations.is_empty(),
        "trait with only &self methods should be object-safe"
    );
}

// ---- Trait with generic method is not object-safe ----

#[test]
fn generic_method_is_not_object_safe() {
    let methods = vec![method("generic_fn", MethodSelfKind::ByReference, true)];
    let violations = check(false, &methods);
    assert!(
        violations
            .iter()
            .any(|v| matches!(v, ObjectSafetyViolation::GenericMethod { .. }))
    );
}

// ---- Trait with static method (no self) is not object-safe ----

#[test]
fn static_method_is_not_object_safe() {
    let methods = vec![method("new", MethodSelfKind::None, false)];
    let violations = check(false, &methods);
    assert!(
        violations
            .iter()
            .any(|v| matches!(v, ObjectSafetyViolation::StaticMethod { .. }))
    );
}

// ---- Trait with self by value (without Self: Sized) is not object-safe ----

#[test]
fn by_value_self_without_sized_is_not_object_safe() {
    let methods = vec![method("into_inner", MethodSelfKind::ByValue, false)];
    let violations = check(false, &methods);
    assert!(
        violations
            .iter()
            .any(|v| matches!(v, ObjectSafetyViolation::ByValueSelf { .. }))
    );
}

// ---- Self: Sized with by-value self is only SelfSized violation ----

#[test]
fn self_sized_with_by_value_self_only_flags_self_sized() {
    let methods = vec![method("into_inner", MethodSelfKind::ByValue, false)];
    let violations = check(true, &methods);
    // Should only have SelfSized, not ByValueSelf (since Self: Sized already covers it)
    assert!(
        violations
            .iter()
            .any(|v| matches!(v, ObjectSafetyViolation::SelfSized))
    );
    assert!(
        !violations
            .iter()
            .any(|v| matches!(v, ObjectSafetyViolation::ByValueSelf { .. }))
    );
}

// ---- Multiple violations ----

#[test]
fn multiple_violations_reported() {
    let methods = vec![
        MethodSignature {
            name: named("generic_fn"),
            span: test_span(),
            self_kind: MethodSelfKind::ByReference,
            has_generic_params: true,
            returns_self: false,
        },
        MethodSignature {
            name: named("static_fn"),
            span: test_span(),
            self_kind: MethodSelfKind::None,
            has_generic_params: false,
            returns_self: false,
        },
    ];
    let violations = check(false, &methods);
    assert!(violations.len() >= 2, "should have at least 2 violations");
}

// ---- Empty trait is object-safe ----

#[test]
fn empty_trait_is_object_safe() {
    let violations = check(false, &[]);
    assert!(violations.is_empty());
}

// ---- Mix of safe and unsafe methods ----

#[test]
fn mixed_methods_reports_only_violations() {
    let methods = vec![
        MethodSignature {
            name: named("safe_method"),
            span: test_span(),
            self_kind: MethodSelfKind::ByReference,
            has_generic_params: false,
            returns_self: false,
        },
        MethodSignature {
            name: named("unsafe_generic"),
            span: test_span(),
            self_kind: MethodSelfKind::ByReference,
            has_generic_params: true,
            returns_self: false,
        },
        MethodSignature {
            name: named("another_safe"),
            span: test_span(),
            self_kind: MethodSelfKind::ByReference,
            has_generic_params: false,
            returns_self: false,
        },
    ];
    let violations = check(false, &methods);
    assert_eq!(
        violations.len(),
        1,
        "only the generic method should be a violation"
    );
    assert!(
        matches!(&violations[0], ObjectSafetyViolation::GenericMethod { method, .. } if *method == named("unsafe_generic"))
    );
}

// ---- Tier 2.3: unconstrained associated type makes the trait not object-safe ----

#[test]
fn unconstrained_associated_type_is_not_object_safe() {
    let methods = vec![method("f", MethodSelfKind::ByReference, false)];
    let associated_types = vec![AssociatedTypeInfo {
        name: named("Bar"),
        span: test_span(),
        is_constrained_in_all_methods: false,
    }];
    let violations = check_object_safety(&TraitObjectSafetyInput {
        requires_self_sized: false,
        methods: &methods,
        associated_types: &associated_types,
        supertrait_safety: &[],
    });
    assert!(
        violations
            .iter()
            .any(|v| matches!(v, ObjectSafetyViolation::UnconstrainedAssociatedType { .. })),
        "trait with an unconstrained associated type must be flagged not object-safe"
    );
}

#[test]
fn constrained_associated_type_is_object_safe() {
    let methods = vec![method("f", MethodSelfKind::ByReference, false)];
    let associated_types = vec![AssociatedTypeInfo {
        name: named("Bar"),
        span: test_span(),
        is_constrained_in_all_methods: true,
    }];
    let violations = check_object_safety(&TraitObjectSafetyInput {
        requires_self_sized: false,
        methods: &methods,
        associated_types: &associated_types,
        supertrait_safety: &[],
    });
    assert!(
        !violations
            .iter()
            .any(|v| matches!(v, ObjectSafetyViolation::UnconstrainedAssociatedType { .. })),
        "trait whose associated type is constrained must not be flagged for that reason"
    );
}

// ---- Tier 2.3: an unsafe supertrait makes the trait not object-safe ----

#[test]
fn unsafe_supertrait_is_not_object_safe() {
    let methods = vec![method("f", MethodSelfKind::ByReference, false)];
    let supertrait_safety = vec![SupertraitSafety {
        trait_id: TraitDefId::from_usize(7),
        is_safe: false,
        span: test_span(),
    }];
    let violations = check_object_safety(&TraitObjectSafetyInput {
        requires_self_sized: false,
        methods: &methods,
        associated_types: &[],
        supertrait_safety: &supertrait_safety,
    });
    assert!(
        violations
            .iter()
            .any(|v| matches!(v, ObjectSafetyViolation::SupertraitNotObjectSafe { .. })),
        "trait with a non-object-safe supertrait must be flagged not object-safe"
    );
}

#[test]
fn safe_supertrait_is_object_safe() {
    let methods = vec![method("f", MethodSelfKind::ByReference, false)];
    let supertrait_safety = vec![SupertraitSafety {
        trait_id: TraitDefId::from_usize(7),
        is_safe: true,
        span: test_span(),
    }];
    let violations = check_object_safety(&TraitObjectSafetyInput {
        requires_self_sized: false,
        methods: &methods,
        associated_types: &[],
        supertrait_safety: &supertrait_safety,
    });
    assert!(
        !violations
            .iter()
            .any(|v| matches!(v, ObjectSafetyViolation::SupertraitNotObjectSafe { .. })),
        "trait with an object-safe supertrait must not be flagged for that reason"
    );
}
