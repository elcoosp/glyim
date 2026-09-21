//! Object safety checks for traits used as `dyn Trait`.
//!
//! A trait is object-safe if:
//! 1. It does not require `Self: Sized` (explicitly or implicitly).
//! 2. All methods have receivers that can be dispatched (i.e., take `self` by reference
//!    or by value where `Self: Sized` is not required).
//! 3. No method has generic type parameters.
//! 4. All associated types are constrained (mentioned) by at least one method signature,
//!    so they can be resolved through the trait object's vtable.
//! 5. All supertraits are themselves object-safe (computed by the caller, since this
//!    module deliberately avoids depending on trait-resolution machinery).

use glyim_core::Name;
use glyim_core::def_id::TraitDefId;
use glyim_span::Span;

/// Reasons a trait is not object-safe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectSafetyViolation {
    /// The trait requires `Self: Sized` (either directly or via a bound).
    SelfSized,
    /// A method has a generic type parameter, which can't be monomorphized through a vtable.
    /// Struct.
    GenericMethod {
        /// method field.
        method: Name,
        /// span field.
        span: Span,
    },
    /// A method does not take `self` (no receiver) — static methods cannot be dispatched.
    /// Struct.
    StaticMethod {
        /// method field.
        method: Name,
        /// span field.
        span: Span,
    },
    /// A method takes `self` by value on a trait that does not have `Self: Sized`.
    /// Struct.
    ByValueSelf {
        /// method field.
        method: Name,
        /// span field.
        span: Span,
    },
    /// An associated function is not callable through a trait object.
    /// Struct.
    AssociatedFunction {
        /// name field.
        name: Name,
        /// span field.
        span: Span,
    },
    /// The trait has an associated type that is not constrained (mentioned in any method
    /// signature), so it cannot be inferred from the trait object's vtable.
    /// Struct.
    UnconstrainedAssociatedType {
        /// name field.
        name: Name,
        /// span field.
        span: Span,
    },
    /// A supertrait of this trait is itself not object-safe.
    /// Struct.
    SupertraitNotObjectSafe {
        /// trait_id field.
        trait_id: TraitDefId,
        /// span field.
        span: Span,
    },
}

/// HIR-level representation of a method signature for object safety checking.
/// This avoids depending on glyim-hir from glyim-type.
#[derive(Debug, Clone)]
pub struct MethodSignature {
    /// Name of the method
    pub name: Name,
    /// Span for error reporting
    pub span: Span,
    /// Whether the method takes `self` by value (`self`), reference (`&self`), or has no self.
    pub self_kind: MethodSelfKind,
    /// Whether the method has generic type parameters (excluding lifetime params).
    pub has_generic_params: bool,
    /// Whether the method returns `Self` (which would make it non-object-safe if by-value).
    pub returns_self: bool,
}

/// How a method takes the `self` parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodSelfKind {
    /// `self` by value: the method takes ownership.
    ByValue,
    /// `&self` or `&mut self`: the method takes a reference.
    ByReference,
    /// No `self` parameter: a static method or associated function.
    None,
}

/// Information about a trait's associated type, needed for object-safety checking.
#[derive(Debug, Clone)]
pub struct AssociatedTypeInfo {
    /// Name of the associated type.
    pub name: Name,
    /// Span for error reporting.
    pub span: Span,
    /// Whether the associated type is mentioned (constrained) in **every** method
    /// signature of the trait. If not, it cannot be inferred from the trait object's
    /// vtable and the trait is not object-safe.
    pub is_constrained_in_all_methods: bool,
}

/// Pre-resolved object-safety result for a supertrait, computed by the caller
/// (`glyim-typeck`, which performs the recursive walk over `TraitContext`).
/// `glyim-type` intentionally does not own trait-resolution recursion.
#[derive(Debug, Clone)]
pub struct SupertraitSafety {
    /// The supertrait's def id.
    pub trait_id: TraitDefId,
    /// Whether that supertrait is itself object-safe.
    pub is_safe: bool,
    /// Span for error reporting (the supertrait bound's span in the trait def).
    pub span: Span,
}

/// Full input for an object-safety check of a single trait.
#[derive(Debug, Clone, Default)]
pub struct TraitObjectSafetyInput<'a> {
    /// Whether the trait (or one of its bounds) requires `Self: Sized`.
    pub requires_self_sized: bool,
    /// The trait's method signatures.
    pub methods: &'a [MethodSignature],
    /// The trait's associated types.
    pub associated_types: &'a [AssociatedTypeInfo],
    /// Pre-resolved object-safety of each supertrait.
    pub supertrait_safety: &'a [SupertraitSafety],
}

/// Checks whether a trait is object-safe given its full description.
///
/// Returns a list of violations. An empty list means the trait is object-safe.
pub fn check_object_safety(input: &TraitObjectSafetyInput<'_>) -> Vec<ObjectSafetyViolation> {
    let mut violations = Vec::new();

    if input.requires_self_sized {
        violations.push(ObjectSafetyViolation::SelfSized);
    }

    for method in input.methods {
        // Generic methods can't be put in a vtable
        if method.has_generic_params {
            violations.push(ObjectSafetyViolation::GenericMethod {
                method: method.name,
                span: method.span,
            });
        }

        match method.self_kind {
            MethodSelfKind::ByValue => {
                // Taking self by value is only allowed if the trait requires Self: Sized,
                // but we already flagged that. If not, it's a separate violation.
                if !input.requires_self_sized {
                    violations.push(ObjectSafetyViolation::ByValueSelf {
                        method: method.name,
                        span: method.span,
                    });
                }
            }
            MethodSelfKind::None => {
                // Static methods / associated functions without self cannot be dispatched.
                // However, they can still exist on an object-safe trait; they just can't be
                // called through the trait object. Glyim might allow this with a warning,
                // but for now we treat it as a violation.
                violations.push(ObjectSafetyViolation::StaticMethod {
                    method: method.name,
                    span: method.span,
                });
            }
            MethodSelfKind::ByReference => {
                // Fine: &self or &mut self
            }
        }
    }

    // Associated types must be constrained (mentioned) by the methods, otherwise they
    // cannot be resolved through the trait object's vtable.
    for at in input.associated_types {
        if !at.is_constrained_in_all_methods {
            violations.push(ObjectSafetyViolation::UnconstrainedAssociatedType {
                name: at.name,
                span: at.span,
            });
        }
    }

    // Every supertrait must itself be object-safe.
    for st in input.supertrait_safety {
        if !st.is_safe {
            violations.push(ObjectSafetyViolation::SupertraitNotObjectSafe {
                trait_id: st.trait_id,
                span: st.span,
            });
        }
    }

    violations
}
