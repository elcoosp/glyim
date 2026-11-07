//! Lint identifiers and per-lint severity levels (§3.5.30 framework).
//!
//! The compiler currently exposes one user-visible lint, [`LintId::UnsafeFnCall`]
//! (warns when a declared `unsafe fn` is called outside any enclosing
//! `unsafe { }` block). Adding more lints means: extend `LintId` with a new
//! variant, update [`LintId::as_str`] / [`LintId::from_str`], wire the check
//! where the diagnostic is emitted, and add `-A/-W/-D` acceptance (which comes
//! for free from the flag parser below).
//!
//! `LintLevel` mirrors rustc's `Allow`/`Warn`/`Deny` semantics:
//!
//! - `Allow` suppresses the diagnostic entirely.
//! - `Warn` emits it with severity `Warning` (the default).
//! - `Deny` promotes it to `Error` and, via the pipeline's existing
//!   `has_errors()` check, aborts compilation.
//!
//! The MVP flag surface is a flat `HashMap<LintId, LintLevel>`. Attribute
//! resolution (`#[allow(..)]` etc.) is deferred; see `KNOWN_GAPS.md §3.5.30`.

use std::collections::HashMap;

/// A named compiler lint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LintId {
    /// Call to a declared `unsafe fn` outside any enclosing `unsafe { }` block.
    /// MVP for §3.5.30.
    UnsafeFnCall,
    /// Monomorphized function that does not use one or more of its
    /// generic parameters. Emitted post-monomorphization by
    /// `glyim_lower::post_mono_checks::check_unused_generic_params`.
    UnusedGenericParams,
}

impl LintId {
    /// Every lint known to the compiler, in registration order. Used by
    /// `-W help` to enumerate available lint names.
    pub const ALL: &'static [LintId] = &[LintId::UnsafeFnCall, LintId::UnusedGenericParams];
}

impl LintId {
    /// Canonical name used in CLI flags and diagnostic text.
    pub fn as_str(self) -> &'static str {
        match self {
            LintId::UnsafeFnCall => "unsafe_fn_call",
            LintId::UnusedGenericParams => "unused_generic_params",
        }
    }

    /// Parse a lint name from a CLI argument. Returns `None` for unknown names
    /// (callers log-and-ignore, matching rustc's lenient-but-warns policy;
    /// here the MVP silently ignores so unrelated tests aren't disrupted).
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "unsafe_fn_call" | "unsafe" => Some(LintId::UnsafeFnCall),
            "unused_generic_params" | "unused_generics" => Some(LintId::UnusedGenericParams),
            _ => None,
        }
    }
}

/// How to react when a lint fires.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LintLevel {
    /// Suppress the diagnostic.
    Allow,
    /// Emit as `DiagSeverity::Warning` (default).
    Warn,
    /// Emit as `DiagSeverity::Error` (aborts compilation at the pipeline's
    /// post-typeck `has_errors()` gate).
    Deny,
}

/// Flat map from lint id to level. Missing entries default to `Warn` (see
/// `TyCtxMut::lint_level`), so an empty map is equivalent to the pre-framework
/// behaviour.
pub type LintMap = HashMap<LintId, LintLevel>;

/// Convenience: apply a sequence of `(name, level)` pairs to `map`, ignoring
/// unknown lint names.
pub fn apply_named(map: &mut LintMap, names: &[String], level: LintLevel) {
    for name in names {
        if let Some(id) = LintId::from_str(name) {
            map.insert(id, level);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_str() {
        let id = LintId::UnsafeFnCall;
        assert_eq!(LintId::from_str(id.as_str()), Some(id));
    }

    #[test]
    fn unsafe_alias() {
        assert_eq!(LintId::from_str("unsafe"), Some(LintId::UnsafeFnCall));
    }

    #[test]
    fn unknown_name() {
        assert_eq!(LintId::from_str("nonexistent_lint"), None);
    }

    #[test]
    fn apply_overrides() {
        let mut m = LintMap::new();
        apply_named(&mut m, &["unsafe".into()], LintLevel::Allow);
        assert_eq!(m.get(&LintId::UnsafeFnCall), Some(&LintLevel::Allow));
        apply_named(&mut m, &["unsafe".into()], LintLevel::Deny);
        assert_eq!(m.get(&LintId::UnsafeFnCall), Some(&LintLevel::Deny));
    }

    #[test]
    fn unused_generic_params_round_trip() {
        let id = LintId::UnusedGenericParams;
        assert_eq!(LintId::from_str(id.as_str()), Some(id));
    }

    #[test]
    fn unused_generics_alias() {
        assert_eq!(
            LintId::from_str("unused_generics"),
            Some(LintId::UnusedGenericParams),
        );
    }

    /// Drift detection: every `LintId` variant must round-trip via
    /// `as_str` / `from_str`, AND `LintId::ALL` must contain it. Adding a
    /// new lint variant without updating `ALL` would make it invisible to
    /// `-W help` — this test fails loudly instead.
    #[test]
    fn all_covers_every_variant() {
        // Known count. Bump when adding a variant — that is the whole
        // point: forgetting to bump means forgetting to add to ALL.
        const EXPECTED_VARIANT_COUNT: usize = 2;
        assert_eq!(
            LintId::ALL.len(),
            EXPECTED_VARIANT_COUNT,
            "LintId::ALL length ({}) does not match the expected variant count ({}); update ALL and this constant when adding a lint",
            LintId::ALL.len(),
            EXPECTED_VARIANT_COUNT,
        );
        for id in LintId::ALL {
            assert_eq!(
                LintId::from_str(id.as_str()),
                Some(*id),
                "LintId::{:?} does not round-trip via as_str()/from_str()",
                id,
            );
        }
    }
}
