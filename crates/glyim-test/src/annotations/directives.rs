//! `//@` fixture annotation grammar (plan M1.4).
//!
//! Every `.g` fixture carries a machine-readable annotation header using
//! `//@ key: value` directives, parsed here. The grammar is the canonical
//! spelling of the pre-existing `// key: value` header directives (which
//! remain accepted as a deprecated alias) plus the readiness-ledger tags the
//! master plan adds: `feature` (registry IDs), `mode`, `backends`, and the
//! `expect-*` outcome family.
//!
//! Rules:
//! - Directives are recognized only in the *leading comment block* (the same
//!   convention `parse_test_config` uses), so a `//@` mention inside test
//!   code is never misread as a header.
//! - Unknown keys are hard errors: a typo must not silently become a
//!   no-op tag (the ledger consumes these tags downstream).
//! - Within one fixture, a legacy `// key:` line and its canonical `//@`
//!   equivalent must not both appear — ambiguity is drift, and drift is
//!   rejected at parse time.

use std::collections::HashMap;
use std::path::PathBuf;

use super::super::harness::config::TestMode;

/// Backends a fixture may declare. The set mirrors
/// `ledger_schema::Backend` (the registry's serialization names); the two
/// must not drift apart — `ledger check` re-validates IDs against the
/// registry, and the corpus test pins the spelling here.
pub const KNOWN_BACKENDS: &[&str] = &["gam", "llvm", "cranelift", "bytecode"];

/// The parsed `//@` annotation header of a fixture.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FixtureAnnotations {
    /// Registry feature IDs this fixture exercises (`//@ feature:`),
    /// in declaration order, duplicates rejected.
    pub features: Vec<String>,
    /// Declared backends (`//@ backends:`), validated against
    /// [`KNOWN_BACKENDS`]. `None` = not declared.
    pub backends: Option<Vec<String>>,
    /// Explicit mode (`//@ mode:`). Conflicts with a legacy
    /// `// test-mode:` line are rejected by the merge in
    /// `parse_test_config`.
    pub mode: Option<TestMode>,
    /// Expected process exit code (`//@ expect-exit:`).
    pub expect_exit: Option<i32>,
    /// Expected stdout content (`//@ expect-stdout:`).
    pub expect_stdout: Option<String>,
    /// Expected stderr content (`//@ expect-stderr:`).
    pub expect_stderr: Option<String>,
    /// Diagnostic substring expectations (`//@ expect-error:`), repeatable.
    pub expect_errors: Vec<String>,
    /// Warning-presence patterns (`//@ expect-warning:`), repeatable.
    pub expect_warnings: Vec<String>,
    /// Warning-absence patterns (`//@ forbid-warning:`), repeatable.
    pub forbid_warnings: Vec<String>,
    /// `//@ needs-llvm` flag.
    pub needs_llvm: bool,
    /// `//@ ignore` flag.
    pub ignore: bool,
    /// `//@ only-target:` target triple.
    pub only_target: Option<String>,
    /// `//@ min-version:` compiler version floor.
    pub min_version: Option<String>,
    /// `//@ timeout:` seconds.
    pub timeout_secs: Option<u64>,
    /// `//@ aux-file:` companion files, repeatable.
    pub aux_files: Vec<PathBuf>,
    /// `//@ compile-flags:` extra compiler flags.
    pub compile_flags: Vec<String>,
    /// `//@ revisions:` named revision set.
    pub revisions: Vec<String>,
    /// `//@ [rev] compile-flags:` per-revision flags.
    pub revision_compile_flags: HashMap<String, Vec<String>>,
    /// True when at least one `//@` directive was present. This is the
    /// `M1.4` `DoD` bit: fixtures without any annotation are rejected by the
    /// collector.
    pub present: bool,
}

impl FixtureAnnotations {
    /// Outcome expectation declared for run-pass / run-fail fixtures:
    /// an explicit exit code or a stdout/stderr pattern.
    #[must_use]
    pub fn has_outcome_expectation(&self) -> bool {
        self.expect_exit.is_some() || self.expect_stdout.is_some() || self.expect_stderr.is_some()
    }
}

/// Parse the `//@` annotation header of a fixture source.
///
/// Only the leading comment block is inspected; parsing never reads past
/// the first non-comment, non-blank line. Unknown keys, malformed values,
/// and duplicate singular keys are errors carrying the 1-based source line
/// number.
///
/// # Errors
/// Returns the offending line number and a human-readable message when a
/// directive is malformed or uses an unknown key.
pub fn parse_directives(source: &str) -> Result<FixtureAnnotations, (usize, String)> {
    let mut out = FixtureAnnotations::default();

    for (idx, line) in source.lines().enumerate() {
        let lineno = idx + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !trimmed.starts_with("//") {
            break; // end of the leading comment block
        }

        let body = match trimmed.strip_prefix("//@") {
            Some(rest) => rest,
            None => continue, // legacy `//` comment or prose: not ours
        };

        out.present = true;
        parse_directive_body(body.trim(), lineno, &mut out)?;
    }

    Ok(out)
}

fn err(lineno: usize, msg: impl Into<String>) -> Result<(), (usize, String)> {
    Err((lineno, msg.into()))
}

fn parse_directive_body(
    body: &str,
    lineno: usize,
    out: &mut FixtureAnnotations,
) -> Result<(), (usize, String)> {
    // Revision-scoped form: `//@ [rev] compile-flags: ...`
    if let Some(rest) = body.strip_prefix('[')
        && let Some(end) = rest.find(']')
    {
        let rev = rest[..end].trim();
        let directive = rest[end + 1..].trim();
        if rev.is_empty() {
            return err(lineno, "empty revision name in `[rev]` directive");
        }
        let Some(value) = directive.strip_prefix("compile-flags:") else {
            return err(
                lineno,
                format!("revision-scoped directive must be `compile-flags:`, got {directive:?}"),
            );
        };
        let flags = split_flags(value)?;
        out.revision_compile_flags
            .entry(rev.to_string())
            .or_default()
            .extend(flags);
        return Ok(());
    }

    let Some((key, value)) = body.split_once(':') else {
        // Flag keys are spelled without a colon.
        let key = body.trim();
        match key {
            "needs-llvm" => {
                out.needs_llvm = true;
                return Ok(());
            }
            "ignore" => {
                out.ignore = true;
                return Ok(());
            }
            "" => return err(lineno, "empty `//@` directive"),
            other => {
                return err(
                    lineno,
                    format!(
                        "unknown directive `{other}` (expected `key: value`; flags are `needs-llvm`, `ignore`)"
                    ),
                );
            }
        }
    };

    let key = key.trim();
    let value = value.trim();
    if value.is_empty() {
        return err(lineno, format!("directive `{key}:` requires a value"));
    }

    match key {
        "feature" => {
            for id in value.split(',') {
                let id = id.trim();
                if id.is_empty() {
                    return err(lineno, "empty feature ID in list");
                }
                if out.features.iter().any(|f| f == id) {
                    return err(lineno, format!("duplicate feature ID `{id}`"));
                }
                if !is_valid_feature_id(id) {
                    return err(
                        lineno,
                        format!(
                            "malformed feature ID `{id}` (expected SCREAMING-KEBAB, e.g. LANG-TY-ARRAY)"
                        ),
                    );
                }
                out.features.push(id.to_string());
            }
            Ok(())
        }
        "mode" => {
            if out.mode.is_some() {
                return err(lineno, "duplicate `mode:` directive");
            }
            let mode: TestMode = value.parse().map_err(|e: String| (lineno, e))?;
            out.mode = Some(mode);
            Ok(())
        }
        "backends" => {
            if out.backends.is_some() {
                return err(lineno, "duplicate `backends:` directive");
            }
            let mut parsed = Vec::new();
            for b in value.split(',') {
                let b = b.trim();
                if !KNOWN_BACKENDS.contains(&b) {
                    return err(
                        lineno,
                        format!(
                            "unknown backend `{b}` (allowed: {})",
                            KNOWN_BACKENDS.join(", ")
                        ),
                    );
                }
                if parsed.contains(&b.to_string()) {
                    return err(lineno, format!("duplicate backend `{b}`"));
                }
                parsed.push(b.to_string());
            }
            out.backends = Some(parsed);
            Ok(())
        }
        "expect-exit" => {
            if out.expect_exit.is_some() {
                return err(lineno, "duplicate `expect-exit:` directive");
            }
            let code: i32 = value
                .parse()
                .map_err(|e| (lineno, format!("invalid exit code {value:?}: {e}")))?;
            out.expect_exit = Some(code);
            Ok(())
        }
        "expect-stdout" => {
            if out.expect_stdout.is_some() {
                return err(lineno, "duplicate `expect-stdout:` directive");
            }
            out.expect_stdout = Some(value.to_string());
            Ok(())
        }
        "expect-stderr" => {
            if out.expect_stderr.is_some() {
                return err(lineno, "duplicate `expect-stderr:` directive");
            }
            out.expect_stderr = Some(value.to_string());
            Ok(())
        }
        "expect-error" => {
            out.expect_errors.push(value.to_string());
            Ok(())
        }
        "expect-warning" => {
            out.expect_warnings.push(value.to_string());
            Ok(())
        }
        "forbid-warning" => {
            out.forbid_warnings.push(value.to_string());
            Ok(())
        }
        "only-target" => {
            if out.only_target.is_some() {
                return err(lineno, "duplicate `only-target:` directive");
            }
            out.only_target = Some(value.to_string());
            Ok(())
        }
        "min-version" => {
            if out.min_version.is_some() {
                return err(lineno, "duplicate `min-version:` directive");
            }
            out.min_version = Some(value.to_string());
            Ok(())
        }
        "timeout" => {
            if out.timeout_secs.is_some() {
                return err(lineno, "duplicate `timeout:` directive");
            }
            let secs: u64 = value
                .parse()
                .map_err(|e| (lineno, format!("invalid timeout {value:?}: {e}")))?;
            out.timeout_secs = Some(secs);
            Ok(())
        }
        "aux-file" => {
            out.aux_files.push(PathBuf::from(value));
            Ok(())
        }
        "compile-flags" => {
            let flags = split_flags(value)?;
            out.compile_flags.extend(flags);
            Ok(())
        }
        "revisions" => {
            out.revisions
                .extend(value.split_whitespace().map(String::from));
            Ok(())
        }
        other => err(
            lineno,
            format!(
                "unknown `//@` directive key `{other}` (typo? see M1.4 grammar in annotations::directives)"
            ),
        ),
    }
}

fn split_flags(value: &str) -> Result<Vec<String>, (usize, String)> {
    shell_words::split(value).map_err(|e| (0usize, format!("invalid compile flags: {e}")))
}

/// Registry-style ID check: `SCREAMING-KEBAB` — uppercase alphanumerics and
/// hyphen-separated groups. The ledger owns the *semantic* validation
/// (known area, ID/area agreement); this only pins the shape so a
/// lower-case slug or dotted path fails loudly here.
#[must_use]
pub fn is_valid_feature_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-')
        && !id.starts_with('-')
        && !id.ends_with('-')
        && !id.contains("--")
}

#[cfg(test)]
mod tests;
