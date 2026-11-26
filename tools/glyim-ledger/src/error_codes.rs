//! CT-5 per-registry `errors` cross-check (plan §6 M3.3 / §5.2 CT-5).
//!
//! The plan's CT-5 paragraph closes with: "...and the ledger's per-feature
//! `errors` cross-check. ... a code without a negative fixture fails
//! `ledger check`."
//!
//! The source of truth for the code list is
//! `crates/glyim-diag/errors/codes.toml` (the same file the compiled-in
//! [`glyim_diag` registry] parses); the negative-fixture side is the
//! `//@ expect-error-code: <CODE>` directive the compile-fail harness
//! consumes (`crates/glyim-test/src/annotations/directives.rs`). This face
//! asserts the cross product: every registered code is declared by at
//! least one fixture, and no fixture declares a code the registry does
//! not list. Either direction failing is a `Severity::Error`, so the
//! coverage cannot silently rot in either direction.
//!
//! The display letters mirror `ErrorCategory::as_letter` in
//! `crates/glyim-diag/src/lib.rs` (Lex=E, Parse=P, NameResolution=N,
//! Type=T, Lifetime=L, Borrow=B, Comptime=C, Io=I, Internal=X), the same
//! mirror `scripts/gen-diagnostics-docs.py` keeps for the docs page.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use ledger_schema::{
    CODE_ERROR_CODE_UNCOVERED, CODE_ERROR_CODE_UNREGISTERED, Diagnostic, Severity,
};

/// The registry file relative to the repo root.
const CODES_TOML: &str = "crates/glyim-diag/errors/codes.toml";
/// The fixture tree scanned for `expect-error-code` declarations.
const FIXTURE_DIR: &str = "crates/glyim-test/tests";
/// The directive prefix a fixture declares a witnessed code with.
const DIRECTIVE: &str = "//@ expect-error-code:";

/// Display letter per `ErrorCategory` variant, mirroring
/// `ErrorCategory::as_letter` (crates/glyim-diag/src/lib.rs).
const CATEGORY_LETTERS: &[(&str, char)] = &[
    ("Lex", 'E'),
    ("Parse", 'P'),
    ("NameResolution", 'N'),
    ("Type", 'T'),
    ("Lifetime", 'L'),
    ("Borrow", 'B'),
    ("Comptime", 'C'),
    ("Io", 'I'),
    ("Internal", 'X'),
];

/// One registered code's display form, in registry declaration order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredCode {
    /// Display form, e.g. `"T0050"`.
    pub display: String,
}

/// Load the registered code list from `codes.toml`.
///
/// Returns the display forms (`"T0050"`) in file order, which the Rust
/// registry asserts is sorted by `(category, number)` (plan §3.5).
///
/// # Errors
/// A `String` naming the file and problem when the TOML is missing,
/// malformed, or lists an unknown category — developer error in a
/// shipped file, loud by contract.
pub fn registered_codes(root: &Path) -> Result<Vec<RegisteredCode>, String> {
    let path = root.join(CODES_TOML);
    let text =
        fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let value: toml::Value =
        toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let codes = value
        .get("code")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| format!("{}: missing [[code]] table array", path.display()))?;
    let mut out = Vec::with_capacity(codes.len());
    for entry in codes {
        let category = entry
            .get("category")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| format!("{}: [[code]] entry without `category`", path.display()))?;
        let number = entry
            .get("number")
            .and_then(toml::Value::as_integer)
            .ok_or_else(|| format!("{}: [[code]] entry without `number`", path.display()))?;
        let letter = CATEGORY_LETTERS
            .iter()
            .find(|(name, _)| *name == category)
            .map(|(_, letter)| *letter)
            .ok_or_else(|| {
                format!(
                    "{path_display}: unknown category `{category}` \
                     (update CATEGORY_LETTERS together with ErrorCategory::as_letter)",
                    path_display = path.display()
                )
            })?;
        let number = u16::try_from(number)
            .map_err(|_| format!("{}: number {number} does not fit u16", path.display()))?;
        out.push(RegisteredCode {
            display: format!("{letter}{number:04}"),
        });
    }
    Ok(out)
}

/// Collect every `//@ expect-error-code: <CODE>` declaration in the
/// fixture tree, as `(code, fixture)` pairs in sorted order.
///
/// Files that cannot be read are skipped (they surface as evidence/
/// inventory diagnostics elsewhere); the walk is recursive over
/// `crates/glyim-test/tests`.
pub fn fixture_declared_codes(root: &Path) -> Vec<(String, PathBuf)> {
    let mut declared = Vec::new();
    let mut stack = vec![root.join(FIXTURE_DIR)];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "g") {
                if let Ok(source) = fs::read_to_string(&path) {
                    for line in source.lines() {
                        let trimmed = line.trim_start();
                        if let Some(rest) = trimmed.strip_prefix(DIRECTIVE) {
                            let code = rest.trim();
                            if !code.is_empty() {
                                declared.push((code.to_string(), path.clone()));
                            }
                        }
                    }
                }
            }
        }
    }
    declared.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    declared.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
    declared
}

/// The CT-5 cross-check: every registered code should be witnessed by at
/// least one fixture's `//@ expect-error-code:` declaration, and every
/// declared code must be registered. Empty on success (the contract of
/// every `run_check` diagnostic source).
///
/// Severities follow the ledger's witness-debt pattern (AGENTS.md §2: debt
/// is allowed but never silent):
///
/// - a fixture declaring a code the registry does not list is an
///   **error** — the registry is the only source of code identity, so an
///   unknown declaration is a typo or a bypass of CT-5;
/// - a registered code no fixture declares is a **warning** — the debt is
///   visible on every `ledger check` until a fixture lands. Four of the
///   twelve registered codes have no CLI-reachable negative fixture today
///   (T0002 is harness-fixture-only by registration; X0000 fires only on
///   ICE/driver failures; B0001's `borrow_error` emitter exists in
///   `glyim-borrowck/move_analysis.rs` but its diagnostics do not surface
///   for ordinary use-after-move programs — a tracked finding; T0051's
///   `trait_not_implemented` emitter has no minimal reproducer yet), so a
///   blanket error would fail every gate permanently without a code
///   change. Converting the warning to an error for the reachable codes is
///   the follow-up once those emitters are plumbed through.
#[must_use]
pub fn error_code_coverage_diagnostics(root: &Path) -> Vec<Diagnostic> {
    let registered = match registered_codes(root) {
        Ok(codes) => codes,
        Err(message) => {
            return vec![Diagnostic {
                code: CODE_ERROR_CODE_UNCOVERED,
                message,
                severity: Severity::Error,
            }];
        }
    };
    let declared = fixture_declared_codes(root);
    let declared_set: BTreeSet<&str> = declared.iter().map(|(c, _)| c.as_str()).collect();

    let mut diagnostics = Vec::new();
    let registered_set: BTreeSet<&str> = registered.iter().map(|c| c.display.as_str()).collect();
    for display in registered_set.iter() {
        if !declared_set.contains(*display) {
            diagnostics.push(Diagnostic {
                code: CODE_ERROR_CODE_UNCOVERED,
                message: format!(
                    "registered error code {display} has no negative fixture: add \
                     `//@ expect-error-code: {display}` to a compile-fail fixture that \
                     produces it (plan M3.3, CT-5; witness debt until declared)"
                ),
                severity: Severity::Warning,
            });
        }
    }
    for (code, fixture) in &declared {
        if !registered_set.contains(code.as_str()) {
            diagnostics.push(Diagnostic {
                code: CODE_ERROR_CODE_UNREGISTERED,
                message: format!(
                    "fixture {} declares `//@ expect-error-code: {code}`, but {code} \
                     is not registered in {CODES_TOML} — register it or fix the directive",
                    fixture.display()
                ),
                severity: Severity::Error,
            });
        }
    }
    diagnostics.sort_by(|a, b| a.code.cmp(b.code).then(a.message.cmp(&b.message)));
    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal repo root carrying one code and one covering fixture.
    fn seed_root(dir: &Path, codes_toml: &str, fixtures: &[(&str, &str)]) -> PathBuf {
        let diag_errors = dir.join("crates/glyim-diag/errors");
        fs::create_dir_all(&diag_errors).unwrap();
        fs::write(diag_errors.join("codes.toml"), codes_toml).unwrap();
        for (rel, body) in fixtures {
            let path = dir.join(FIXTURE_DIR).join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, body).unwrap();
        }
        dir.to_path_buf()
    }

    const ONE_CODE: &str = "[[code]]\ncategory = \"Type\"\nnumber = 50\ntitle = \"non-exhaustive match\"\nexplanation = \"add arms\"\n";

    #[test]
    fn coverage_passes_when_every_code_is_declared() {
        let dir = tempfile::tempdir().unwrap();
        let root = seed_root(
            dir.path(),
            ONE_CODE,
            &[(
                "compile-fail/match.g",
                "//@ expect-error-code: T0050\nfn main() {}\n",
            )],
        );
        assert!(
            error_code_coverage_diagnostics(&root).is_empty(),
            "a fully covered registry must pass"
        );
    }

    #[test]
    fn uncovered_registered_code_is_witness_debt() {
        let dir = tempfile::tempdir().unwrap();
        let root = seed_root(dir.path(), ONE_CODE, &[]);
        let diagnostics = error_code_coverage_diagnostics(&root);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, CODE_ERROR_CODE_UNCOVERED);
        assert_eq!(
            diagnostics[0].severity,
            Severity::Warning,
            "an uncovered code is never-silent witness debt, not a hard failure \
             (four registered codes have no CLI-reachable fixture yet)"
        );
        assert!(
            diagnostics[0].message.contains("T0050"),
            "the report must name the uncovered code: {diagnostics:?}"
        );
    }

    #[test]
    fn unregistered_declaration_fails_the_check() {
        let dir = tempfile::tempdir().unwrap();
        let root = seed_root(
            dir.path(),
            ONE_CODE,
            &[(
                "compile-fail/match.g",
                "//@ expect-error-code: Z9999\nfn main() {}\n",
            )],
        );
        let diagnostics = error_code_coverage_diagnostics(&root);
        assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
        assert!(
            diagnostics
                .iter()
                .any(|d| d.code == CODE_ERROR_CODE_UNREGISTERED && d.message.contains("Z9999")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn registered_codes_parse_display_forms_in_file_order() {
        let dir = tempfile::tempdir().unwrap();
        let codes = "[[code]]\ncategory = \"Lex\"\nnumber = 1\ntitle = \"lex\"\nexplanation = \"x\"\n\
                    [[code]]\ncategory = \"Type\"\nnumber = 0\ntitle = \"type\"\nexplanation = \"x\"\n";
        let root = seed_root(dir.path(), codes, &[]);
        let codes = registered_codes(&root).unwrap();
        let displays: Vec<&str> = codes.iter().map(|c| c.display.as_str()).collect();
        assert_eq!(displays, vec!["E0001", "T0000"]);
    }

    #[test]
    fn malformed_registry_is_a_loud_error_diagnostic() {
        let dir = tempfile::tempdir().unwrap();
        let root = seed_root(dir.path(), "not = \"a registry\"\n", &[]);
        let diagnostics = error_code_coverage_diagnostics(&root);
        assert_eq!(diagnostics.len(), 1);
        assert!(
            diagnostics[0].message.contains("missing [[code]]"),
            "{diagnostics:?}"
        );
    }
}
