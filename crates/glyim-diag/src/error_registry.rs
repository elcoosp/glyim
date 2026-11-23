//! CT-5 error-code registry (plan §6 M3.3 / §5.2).
//!
//! The source of truth for every diagnostic code glyim can emit is the
//! TOML file [`codes`](../errors/codes.toml), compiled into the binary with
//! [`include_str!`] and parsed once into a sorted, deduplicated table. A
//! code is constructable only through
//! [`ErrorCode::from_registered`](crate::ErrorCode::from_registered), which
//! looks it up here; the `DoD` "ad-hoc `ErrorCode{..}` construction
//! forbidden" is realized by `ErrorCode` being `#[non_exhaustive]`
//! (compile-time enforcement: external crates cannot use a struct literal)
//! and by `from_registered` panicking for any `(category, number)` not
//! listed here (runtime enforcement for the one internal chokepoint).
//!
//! [`ErrorCode::explain`](crate::ErrorCode::explain) and
//! [`explain_code`] return each entry's `explanation` — the `--explain <code>`
//! surface.
//!
//! Determinism (plan §3.5): the table is parsed once into a `Vec` and
//! iterated in insertion order, and [`registry_entries`] asserts that order
//! is sorted by `(category, number)` so consumers never depend on a hash
//! map's iteration order.

use crate::{ErrorCategory, ErrorCode};
use std::sync::OnceLock;

/// One registered error-code entry.
///
/// `code` is the typed `(category, number)` pair; `title` is the short
/// human label; `explanation` is the long-form `--explain` text. Both
/// strings live for the program's lifetime because the owning [`Vec`] is
/// held in a [`OnceLock`], so [`ErrorCode::explain`] can return `&'static
/// str` without leaking.
#[derive(Debug, Clone)]
pub struct ErrorCodeEntry {
    /// The typed code this entry describes.
    pub code: ErrorCode,
    /// Short human label, e.g. `"non-exhaustive match"`.
    pub title: String,
    /// Long-form explanation printed by `--explain <code>`.
    pub explanation: String,
}

/// The compiled-in registry text (`crates/glyim-diag/errors/codes.toml`).
const CODES_TOML: &str = include_str!("../errors/codes.toml");

static REGISTRY: OnceLock<Vec<ErrorCodeEntry>> = OnceLock::new();

/// The parsed registry, built once from [`CODES_TOML`] and reused for every
/// lookup. The `Vec` is held in a [`OnceLock`], so the returned slice and the
/// `&str`s borrowed from it are `&'static`.
fn registry() -> &'static [ErrorCodeEntry] {
    REGISTRY.get_or_init(|| parse_registry(CODES_TOML))
}

/// Parse the registry TOML into a sorted `Vec<ErrorCodeEntry>`.
///
/// Panics on a malformed `codes.toml` (a developer error in a file shipped
/// with the compiler, caught the first time any diagnostic is constructed in
/// CI). Each panic message names the offending field so the fix is obvious.
fn parse_registry(text: &str) -> Vec<ErrorCodeEntry> {
    let root: toml::Value = toml::from_str(text).expect("errors/codes.toml must parse");
    let codes = root
        .get("code")
        .and_then(toml::Value::as_array)
        .expect("errors/codes.toml must contain a [[code]] table array");
    let mut entries: Vec<ErrorCodeEntry> = codes
        .iter()
        .map(|entry| {
            let category = entry
                .get("category")
                .and_then(toml::Value::as_str)
                .expect("each [[code]] entry needs a string `category`");
            let number = entry
                .get("number")
                .and_then(toml::Value::as_integer)
                .expect("each [[code]] entry needs an integer `number`");
            let title = entry
                .get("title")
                .and_then(toml::Value::as_str)
                .expect("each [[code]] entry needs a string `title`");
            let explanation = entry
                .get("explanation")
                .and_then(toml::Value::as_str)
                .expect("each [[code]] entry needs a string `explanation`");
            let category = ErrorCategory::from_str(category)
                .unwrap_or_else(|| panic!("errors/codes.toml: unknown category `{category}`"));
            let number = u16::try_from(number)
                .unwrap_or_else(|_| panic!("errors/codes.toml: number {number} out of u16 range"));
            ErrorCodeEntry {
                code: ErrorCode { category, number },
                title: title.to_owned(),
                explanation: explanation.to_owned(),
            }
        })
        .collect();
    // Determinism (plan §3.5): keep the table sorted by (category, number).
    entries.sort_by(|a, b| {
        (a.code.category as u8)
            .cmp(&(b.code.category as u8))
            .then(a.code.number.cmp(&b.code.number))
    });
    let mut deduped: Vec<ErrorCodeEntry> = Vec::with_capacity(entries.len());
    for entry in entries {
        if let Some(prev) = deduped.last() {
            assert!(
                !((prev.code.category == entry.code.category)
                    && prev.code.number == entry.code.number),
                "errors/codes.toml: duplicate code {}{:04}",
                prev.code.category.as_letter(),
                prev.code.number,
            );
        }
        deduped.push(entry);
    }
    deduped
}

/// Look up a registered entry by `(category, number)`.
///
/// Returns `None` for an unregistered code — the value
/// [`ErrorCode::from_registered`](crate::ErrorCode::from_registered) uses to
/// decide whether to panic, and [`ErrorCode::explain`](crate::ErrorCode::explain)
/// reads to fetch the explanation.
pub(crate) fn lookup(category: ErrorCategory, number: u16) -> Option<&'static ErrorCodeEntry> {
    registry()
        .iter()
        .find(|e| e.code.category == category && e.code.number == number)
}

/// Construct-check: `Ok(())` if the code is registered, `Err(())` if not.
///
/// Separated from
/// [`ErrorCode::from_registered`](crate::ErrorCode::from_registered) so the
/// panic message is formatted once, at the call site, with the offending
/// code.
pub(crate) fn is_registered(category: ErrorCategory, number: u16) -> bool {
    lookup(category, number).is_some()
}

/// Every registered entry, in sorted `(category, number)` order.
///
/// Exposed for the M3.3 completeness test (every emitted code must appear)
/// and for future renderers that list all codes.
#[must_use]
pub fn registry_entries() -> &'static [ErrorCodeEntry] {
    registry()
}

/// The `--explain <code>` surface: parse a display-form code string
/// (`"T0060"`, `"E0001"`, …) and return its registered explanation, or
/// `None` if the string is not a registered code.
///
/// The display form is [`ErrorCode`](crate::ErrorCode)'s `Display` spelling:
/// one category letter (`E`/`P`/`N`/`T`/`L`/`B`/`C`/`I`/`X`) followed by a
/// zero-padded four-digit number. Unknown letters, non-numeric tails, and
/// registered-looking strings whose `(category, number)` is not in the
/// registry all return `None` rather than panicking — `--explain` is a
/// user-facing CLI surface and must reject typos gracefully.
#[must_use]
pub fn explain_code(code_str: &str) -> Option<&'static str> {
    let mut chars = code_str.chars();
    let letter = chars.next()?;
    let rest = chars.as_str();
    let category = ErrorCategory::from_letter(letter)?;
    let trimmed = rest.trim_start_matches('0');
    let n = trimmed.parse::<u16>().ok()?;
    lookup(category, n).map(|e| e.explanation.as_str())
}
