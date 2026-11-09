use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::str::FromStr;

use crate::annotations::directives::{self, FixtureAnnotations};

#[derive(Clone, Debug)]
/// `TestConfig`.
pub struct TestConfig {
    /// `Struct`.
    pub mode: TestMode,
    /// `Struct`.
    pub revisions: Vec<String>,
    /// `Struct`.
    pub revision_compile_flags: HashMap<String, Vec<String>>,
    /// `Struct`.
    pub compile_flags: Vec<String>,
    /// `Struct`.
    pub error_patterns: Vec<String>,
    /// `Struct`.
    pub needs_llvm: bool,
    /// `Struct`.
    pub min_version: Option<String>,
    /// `Struct`.
    pub ignore: bool,
    /// `Struct`.
    pub only_target: Option<String>,
    /// `Struct`.
    pub aux_files: Vec<PathBuf>,
    /// `Struct`.
    pub timeout_secs: u64,
    /// `Struct`.
    pub check_stdout: Option<String>,
    /// `Struct`.
    pub check_stderr: Option<String>,
    /// `Struct`.
    pub expected_exit_code: Option<i32>,
    /// §3.5.30 item 4: warning-presence assertions for `compile-pass`
    /// fixtures. Each pattern must match at least one warning diagnostic
    /// (`expect-warning:`) or must match zero warnings
    /// (`forbid-warning:`). Locks compiler-warning behaviour into CI
    /// (run-pass `check-stderr` only sees *program* stderr).
    pub expect_warnings: Vec<String>,
    /// Patterns that must match zero warnings (`forbid-warning:`).
    pub forbid_warnings: Vec<String>,
    /// `M1.4` readiness-ledger tags: registry feature IDs the fixture
    /// exercises (`//@ feature:`). Empty = not tagged (rejected by the
    /// collector gate).
    pub features: Vec<String>,
    /// `M1.4` readiness-ledger tags: declared backends (`//@ backends:`).
    /// Empty = not declared (rejected by the collector gate).
    pub backends: Vec<String>,
    /// M1.6 strict-xfail witness tag (`//@ known-gap: GAP-NNNN`, plan
    /// §3.3). `None` = not a witness fixture. Only meaningful together
    /// with [`TestMode::CompileFail`] — the collector rejects the
    /// combination with any other mode
    /// ([`crate::error::TestDiscoveryError::WitnessRequiresCompileFail`]).
    pub known_gap: Option<String>,
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            mode: TestMode::CompilePass,
            revisions: Vec::new(),
            revision_compile_flags: HashMap::new(),
            compile_flags: Vec::new(),
            error_patterns: Vec::new(),
            needs_llvm: false,
            min_version: None,
            ignore: false,
            only_target: None,
            aux_files: Vec::new(),
            timeout_secs: 60,
            check_stdout: None,
            check_stderr: None,
            expected_exit_code: None,
            expect_warnings: Vec::new(),
            forbid_warnings: Vec::new(),
            features: Vec::new(),
            backends: Vec::new(),
            known_gap: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// `TestMode`.
pub enum TestMode {
    /// `Variant`.
    CompilePass,
    /// `Variant`.
    CompileFail,
    /// `Variant`.
    Ui,
    /// `Variant`.
    RunPass,
    /// `Variant`.
    RunFail,
}

impl FromStr for TestMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "compile-pass" => Ok(Self::CompilePass),
            "compile-fail" => Ok(Self::CompileFail),
            "ui" => Ok(Self::Ui),
            "run-pass" => Ok(Self::RunPass),
            "run-fail" => Ok(Self::RunFail),
            other => Err(format!(
                "unknown test-mode: {other:?}. Expected: compile-pass, compile-fail, ui, run-pass, run-fail"
            )),
        }
    }
}

impl TestMode {
    /// `from_str_exact`.
    pub fn from_str_exact(s: &str) -> Result<Self, String> {
        s.parse()
    }
    /// `dir_name`.
    #[must_use]
    pub fn dir_name(self) -> &'static str {
        match self {
            Self::CompilePass => "compile-pass",
            Self::CompileFail => "compile-fail",
            Self::Ui => "ui",
            Self::RunPass => "run-pass",
            Self::RunFail => "run-fail",
        }
    }
}

/// `ParsedConfig`.
pub struct ParsedConfig {
    /// `Struct`.
    pub config: TestConfig,
    /// `Struct`.
    pub has_explicit_mode: bool,
    /// `M1.4`: whether the fixture carried at least one `//@` directive.
    pub has_annotations: bool,
    /// The parsed `//@` header (empty struct when none present).
    pub annotations: FixtureAnnotations,
}

/// `parse_test_config`.
///
/// Merges two spellings of the fixture header:
/// - the canonical `//@` grammar (M1.4, parsed by
///   `annotations::directives::parse_directives`), and
/// - the deprecated legacy `// key: value` headers (parsed inline below).
///
/// A legacy key and its canonical `//@` equivalent must not both appear in
/// one fixture: ambiguity is drift, and drift is rejected at parse time
/// rather than resolved by a silent precedence rule.
pub fn parse_test_config(source: &str) -> Result<ParsedConfig, String> {
    let mut config = TestConfig::default();
    let mut has_explicit_mode = false;
    // Legacy keys seen, for canonical-vs-legacy conflict detection.
    let mut legacy_seen: HashSet<&'static str> = HashSet::new();

    for line in source.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("//") {
            if trimmed.is_empty() {
                continue;
            }
            break;
        }
        let content = trimmed[2..].trim();

        if let Some(rest) = content.strip_prefix('[')
            && let Some(bracket_end) = rest.find(']')
        {
            let rev = &rest[..bracket_end];
            let directive = rest[bracket_end + 1..].trim();
            if let Some(value) = directive.strip_prefix("compile-flags:") {
                legacy_seen.insert("[rev] compile-flags");
                config
                    .revision_compile_flags
                    .entry(rev.to_string())
                    .or_default()
                    .extend(
                        shell_words::split(value.trim())
                            .map_err(|e| format!("invalid compile flags: {e}"))?,
                    );
            }
            continue;
        }

        if let Some(value) = content.strip_prefix("test-mode:") {
            legacy_seen.insert("test-mode");
            config.mode = value.parse::<TestMode>()?;
            has_explicit_mode = true;
        } else if let Some(value) = content.strip_prefix("revisions:") {
            legacy_seen.insert("revisions");
            config.revisions = value.split_whitespace().map(String::from).collect();
        } else if let Some(value) = content.strip_prefix("compile-flags:") {
            legacy_seen.insert("compile-flags");
            config.compile_flags.extend(
                shell_words::split(value.trim())
                    .map_err(|e| format!("invalid compile flags: {e}"))?,
            );
        } else if let Some(value) = content.strip_prefix("error-pattern:") {
            legacy_seen.insert("error-pattern");
            config.error_patterns.push(value.trim().to_string());
        } else if content == "needs-llvm" {
            legacy_seen.insert("needs-llvm");
            config.needs_llvm = true;
        } else if let Some(value) = content.strip_prefix("min-version:") {
            legacy_seen.insert("min-version");
            config.min_version = Some(value.trim().to_string());
        } else if content == "ignore" {
            legacy_seen.insert("ignore");
            config.ignore = true;
        } else if let Some(value) = content.strip_prefix("only-target:") {
            legacy_seen.insert("only-target");
            config.only_target = Some(value.trim().to_string());
        } else if let Some(value) = content.strip_prefix("aux-file:") {
            legacy_seen.insert("aux-file");
            config.aux_files.push(PathBuf::from(value.trim()));
        } else if let Some(value) = content.strip_prefix("timeout:") {
            legacy_seen.insert("timeout");
            config.timeout_secs = value.trim().parse().unwrap_or(60);
        } else if let Some(value) = content.strip_prefix("check-stdout:") {
            legacy_seen.insert("check-stdout");
            config.check_stdout = Some(value.trim().to_string());
        } else if let Some(value) = content.strip_prefix("check-stderr:") {
            legacy_seen.insert("check-stderr");
            config.check_stderr = Some(value.trim().to_string());
        } else if let Some(value) = content.strip_prefix("exit-code:") {
            legacy_seen.insert("exit-code");
            config.expected_exit_code = Some(value.trim().parse().unwrap_or(1));
        } else if let Some(value) = content.strip_prefix("expect-warning:") {
            legacy_seen.insert("expect-warning");
            config.expect_warnings.push(value.trim().to_string());
        } else if let Some(value) = content.strip_prefix("forbid-warning:") {
            legacy_seen.insert("forbid-warning");
            config.forbid_warnings.push(value.trim().to_string());
        }
    }

    // M1.4: parse the canonical `//@` header and merge it, rejecting
    // legacy/canonical double spellings of the same key.
    let annotations = directives::parse_directives(source)
        .map_err(|(line, msg)| format!("`//@` annotation error at line {line}: {msg}"))?;

    let conflicts: Vec<&str> = [
        (
            legacy_seen.contains("test-mode") && annotations.mode.is_some(),
            "test-mode / mode",
        ),
        (
            legacy_seen.contains("exit-code") && annotations.expect_exit.is_some(),
            "exit-code / expect-exit",
        ),
        (
            legacy_seen.contains("check-stdout") && annotations.expect_stdout.is_some(),
            "check-stdout / expect-stdout",
        ),
        (
            legacy_seen.contains("check-stderr") && annotations.expect_stderr.is_some(),
            "check-stderr / expect-stderr",
        ),
        (
            legacy_seen.contains("error-pattern") && !annotations.expect_errors.is_empty(),
            "error-pattern / expect-error",
        ),
        (
            legacy_seen.contains("expect-warning") && !annotations.expect_warnings.is_empty(),
            "expect-warning (legacy + //@)",
        ),
        (
            legacy_seen.contains("forbid-warning") && !annotations.forbid_warnings.is_empty(),
            "forbid-warning (legacy + //@)",
        ),
        (
            legacy_seen.contains("needs-llvm") && annotations.needs_llvm,
            "needs-llvm (legacy + //@)",
        ),
        (
            legacy_seen.contains("ignore") && annotations.ignore,
            "ignore (legacy + //@)",
        ),
        (
            legacy_seen.contains("only-target") && annotations.only_target.is_some(),
            "only-target (legacy + //@)",
        ),
        (
            legacy_seen.contains("min-version") && annotations.min_version.is_some(),
            "min-version (legacy + //@)",
        ),
        (
            legacy_seen.contains("timeout") && annotations.timeout_secs.is_some(),
            "timeout (legacy + //@)",
        ),
        (
            legacy_seen.contains("aux-file") && !annotations.aux_files.is_empty(),
            "aux-file (legacy + //@)",
        ),
        (
            legacy_seen.contains("compile-flags") && !annotations.compile_flags.is_empty(),
            "compile-flags (legacy + //@)",
        ),
        (
            legacy_seen.contains("revisions") && !annotations.revisions.is_empty(),
            "revisions (legacy + //@)",
        ),
        (
            legacy_seen.contains("[rev] compile-flags")
                && !annotations.revision_compile_flags.is_empty(),
            "[rev] compile-flags (legacy + //@)",
        ),
    ]
    .into_iter()
    .filter_map(|(conflicts, pair)| conflicts.then_some(pair))
    .collect();

    if !conflicts.is_empty() {
        return Err(format!(
            "fixture mixes legacy `//` headers and canonical `//@` directives for: {}. \
             Use the `//@` form only (see annotations::directives)",
            conflicts.join(", ")
        ));
    }

    if let Some(mode) = annotations.mode {
        config.mode = mode;
        has_explicit_mode = true;
    }
    if let Some(exit) = annotations.expect_exit {
        config.expected_exit_code = Some(exit);
    }
    if annotations.expect_stdout.is_some() {
        config.check_stdout.clone_from(&annotations.expect_stdout);
    }
    if annotations.expect_stderr.is_some() {
        config.check_stderr.clone_from(&annotations.expect_stderr);
    }
    if !annotations.expect_errors.is_empty() {
        config.error_patterns.clone_from(&annotations.expect_errors);
    }
    if !annotations.expect_warnings.is_empty() {
        config
            .expect_warnings
            .clone_from(&annotations.expect_warnings);
    }
    if !annotations.forbid_warnings.is_empty() {
        config
            .forbid_warnings
            .clone_from(&annotations.forbid_warnings);
    }
    if annotations.needs_llvm {
        config.needs_llvm = true;
    }
    if annotations.ignore {
        config.ignore = true;
    }
    if annotations.only_target.is_some() {
        config.only_target.clone_from(&annotations.only_target);
    }
    if annotations.min_version.is_some() {
        config.min_version.clone_from(&annotations.min_version);
    }
    if let Some(secs) = annotations.timeout_secs {
        config.timeout_secs = secs;
    }
    if !annotations.aux_files.is_empty() {
        config.aux_files.clone_from(&annotations.aux_files);
    }
    if !annotations.compile_flags.is_empty() {
        config.compile_flags.clone_from(&annotations.compile_flags);
    }
    if !annotations.revisions.is_empty() {
        config.revisions.clone_from(&annotations.revisions);
    }
    if !annotations.revision_compile_flags.is_empty() {
        config
            .revision_compile_flags
            .clone_from(&annotations.revision_compile_flags);
    }
    config.features.clone_from(&annotations.features);
    config
        .backends
        .clone_from(&annotations.backends.clone().unwrap_or_default());
    config.known_gap.clone_from(&annotations.known_gap);

    Ok(ParsedConfig {
        config,
        has_explicit_mode,
        has_annotations: annotations.present,
        annotations,
    })
}
