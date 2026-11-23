//! M3.3 CT-5 error-code registry — acceptance tests (plan §6 M3.3).
//!
//! The plan's machine-checkable `DoD` for M3.3's registry half is
//! (docs/roadmaps/glyim-master-plan.md §6, M3.3 row, `DoD` column):
//!
//! "all existing codes migrated into `errors/*.toml`; ad-hoc
//! `ErrorCode{..}` construction forbidden by lint"
//!
//! These tests pin that contract: every code the compiler emits is listed
//! in `errors/codes.toml`, construction goes through the registry-checked
//! [`ErrorCode::from_registered`](crate::ErrorCode::from_registered) (which
//! panics for an unregistered code), and [`ErrorCode::explain`] /
//! [`explain_code`](crate::error_registry::explain_code) return the
//! registered explanation — the `--explain <code>` surface.

use crate::error_registry::{explain_code, registry_entries};
use crate::{ErrorCategory, ErrorCode};

/// The exact set of `(category, number)` codes constructed across the
/// compiler (grepped from every `ErrorCode::from_registered` / constructor
/// site). The registry MUST list every one: a code used in a diagnostic but
/// absent from `errors/codes.toml` is an unregistered ad-hoc code, which the
/// `DoD` forbids. If a new code is introduced without registering it here,
/// [`ErrorCode::from_registered`] panics and this test names the offender.
const EMITTED_CODES: &[(ErrorCategory, u16)] = &[
    (ErrorCategory::Lex, 1),
    (ErrorCategory::Parse, 1),
    (ErrorCategory::Type, 0),
    (ErrorCategory::Type, 1),
    (ErrorCategory::Type, 2),
    (ErrorCategory::Type, 50),
    (ErrorCategory::Type, 51),
    (ErrorCategory::Type, 60),
    (ErrorCategory::Type, 61),
    (ErrorCategory::Borrow, 1),
    (ErrorCategory::Comptime, 1),
    (ErrorCategory::Internal, 0),
];

#[test]
fn registry_lists_every_emitted_code() {
    let registry = registry_entries();
    assert!(!registry.is_empty(), "registry must not be empty");
    for (category, number) in EMITTED_CODES {
        let found = registry
            .iter()
            .any(|e| e.code.category == *category && e.code.number == *number);
        assert!(
            found,
            "emitted code {category:?}:{number:04} is not registered in errors/codes.toml — \
             add it before constructing a diagnostic with it (M3.3 DoD)",
        );
    }
}

#[test]
fn from_registered_round_trips_each_emitted_code() {
    for (category, number) in EMITTED_CODES {
        let code = ErrorCode::from_registered(*category, *number);
        assert_eq!(code.category, *category);
        assert_eq!(code.number, *number);
    }
}

#[test]
fn from_registered_panics_for_unregistered_code() {
    // A code that is never emitted and not in the registry. Constructing it
    // MUST panic: this is the "ad-hoc ErrorCode{..} construction forbidden"
    // enforcement (realized at runtime for internal callers and at compile
    // time for external crates via `pub(crate)` fields).
    let result = std::panic::catch_unwind(|| ErrorCode::from_registered(ErrorCategory::Type, 9999));
    assert!(
        result.is_err(),
        "from_registered must panic for an unregistered code (M3.3 DoD: ad-hoc construction forbidden)",
    );
}

#[test]
fn explain_returns_nonempty_for_each_emitted_code() {
    for (category, number) in EMITTED_CODES {
        let code = ErrorCode::from_registered(*category, *number);
        let explanation = code.explain();
        assert!(
            !explanation.is_empty(),
            "code {category:?}:{number:04} has an empty explanation in errors/codes.toml — \
             --explain needs non-empty text",
        );
    }
}

#[test]
fn explain_code_parses_the_display_form() {
    // `ErrorCode::Display` spells Type:60 as `T0060`. The `--explain T0060`
    // surface parses that string back to the registered explanation.
    let via_struct = ErrorCode::from_registered(ErrorCategory::Type, 60).explain();
    let via_str = explain_code("T0060").expect("T0060 must be registered");
    assert_eq!(via_struct, via_str);
    assert!(
        via_str.contains("await"),
        "T0060 explanation should describe the async lowering gap"
    );
}

#[test]
fn explain_code_rejects_unknown_strings() {
    assert!(
        explain_code("ZZZZ").is_none(),
        "unknown code string must return None, not panic"
    );
    assert!(explain_code("not-a-code").is_none());
}

#[test]
fn registry_entries_are_sorted_and_unique_by_code() {
    // Determinism (plan §3.5 / M3.9): the registry iterates in a stable
    // order and never lists a (category, number) twice, so `--explain` and
    // the level renderer never depend on hash order.
    let registry = registry_entries();
    for window in registry.windows(2) {
        let a = &window[0].code;
        let b = &window[1].code;
        assert!(
            (a.category as u8, a.number) < (b.category as u8, b.number),
            "registry entries must be strictly sorted by (category, number)",
        );
    }
}
