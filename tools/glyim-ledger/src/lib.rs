//! CLI library for the Readiness Ledger tool (plan M1.1).
//!
//! Two subcommands:
//!
//! - `glyim-ledger check [ROOT]` — load `ROOT/ledger`, run the schema and
//!   semantic validators, print every diagnostic sorted, exit 1 on any
//!   error. This is the CI registry gate.
//! - `glyim-ledger schema [OUT]` — write the JSON-Schema export document
//!   for both entry types to `OUT` (pretty-printed), or to stdout.
//!
//! Argument handling mirrors `glyim-metrics`: hand-rolled, no `clap`, so
//! the tool's own supply chain stays dependency-light.

use std::fs;
use std::path::{Path, PathBuf};

use ledger_schema::{Diagnostic, Severity, load_registry, registry_schema};

/// Parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `check [ROOT]` — registry gate.
    Check {
        /// Project directory containing the `ledger/` registry (`.` by
        /// default).
        root: PathBuf,
    },
    /// `schema [OUT]` — JSON-Schema export.
    Schema {
        /// File to write the pretty-printed document to; `None` writes to
        /// stdout.
        out: Option<PathBuf>,
    },
}

/// Usage text returned with argument errors.
#[must_use]
pub fn usage() -> String {
    "usage: glyim-ledger check [ROOT]\n   or: glyim-ledger schema [OUT]\n\
     \x20 ROOT defaults to the current directory; with no subcommand `check`\n\
     \x20 is assumed. `schema` writes to OUT or stdout."
        .into()
}

/// Parse CLI arguments (excluding argv[0]).
///
/// # Errors
///
/// Returns a human-readable message (including the usage text) for
/// unknown subcommands, unknown flags, or extra positional arguments.
pub fn parse_args<I>(args: I) -> Result<Command, String>
where
    I: Iterator<Item = String>,
{
    let items: Vec<String> = args.collect();
    match items.first().map(String::as_str) {
        None => Ok(Command::Check {
            root: PathBuf::from("."),
        }),
        Some("check" | "schema") => {
            let rest = &items[1..];
            if rest.len() > 1 {
                return Err(format!(
                    "unexpected extra argument {}\n{}",
                    rest[1],
                    usage()
                ));
            }
            if items[0] == "check" {
                let root = rest
                    .first()
                    .map_or_else(|| PathBuf::from("."), PathBuf::from);
                Ok(Command::Check { root })
            } else {
                let out = rest.first().map(|p| PathBuf::from(p));
                Ok(Command::Schema { out })
            }
        }
        Some("-h" | "--help") => Err(usage()),
        Some(flag) if flag.starts_with('-') => Err(format!("unknown flag {flag}\n{}", usage())),
        Some(subcommand) => Err(format!("unknown subcommand {subcommand}\n{}", usage())),
    }
}

/// Result of a full `ledger check` run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckOutcome {
    /// All diagnostics (schema + semantic), sorted and de-duplicated.
    pub diagnostics: Vec<Diagnostic>,
    /// Feature entries loaded.
    pub features: usize,
    /// Gap entries loaded.
    pub gaps: usize,
    /// Registry files walked (including files that failed to parse).
    pub files: usize,
}

impl CheckOutcome {
    /// Number of error-severity diagnostics.
    #[must_use]
    pub fn errors(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    }

    /// Number of warning-severity diagnostics.
    #[must_use]
    pub fn warnings(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count()
    }

    /// True when no error-severity diagnostic was produced.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.errors() == 0
    }
}

/// Load + validate the registry under `root` (the directory that contains
/// `ledger/`).
#[must_use]
pub fn run_check(root: &Path) -> CheckOutcome {
    let loaded = load_registry(&root.join("ledger"));
    let mut diagnostics = loaded.diagnostics;
    diagnostics.extend(loaded.registry.validate());
    diagnostics.sort_by(|a, b| a.code.cmp(b.code).then(a.message.cmp(&b.message)));
    diagnostics.dedup();
    CheckOutcome {
        diagnostics,
        features: loaded.registry.feature_count(),
        gaps: loaded.registry.gap_count(),
        files: loaded.registry.file_count(),
    }
}

/// Render the full stdout report for a check run.
#[must_use]
pub fn format_outcome(outcome: &CheckOutcome) -> String {
    let mut lines: Vec<String> = outcome
        .diagnostics
        .iter()
        .map(ToString::to_string)
        .collect();
    if outcome.is_ok() {
        lines.push(format!(
            "ledger check OK: {} features, {} gaps, {} files, 0 errors, {} warning(s)",
            outcome.features,
            outcome.gaps,
            outcome.files,
            outcome.warnings()
        ));
    } else {
        lines.push(format!(
            "ledger check FAILED: {} error(s), {} warning(s)",
            outcome.errors(),
            outcome.warnings()
        ));
        lines.push(format!(
            "registry: {} features, {} gaps, {} files",
            outcome.features, outcome.gaps, outcome.files
        ));
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// Write the combined JSON-Schema export to `out` (pretty-printed).
///
/// # Errors
///
/// Returns a message when the file cannot be written.
pub fn write_schema(out: &Path) -> Result<String, String> {
    let document = registry_schema();
    let text = serde_json::to_string_pretty(&document)
        .map_err(|err| format!("cannot serialize ledger schema: {err}"))?
        + "\n";
    fs::write(out, text).map_err(|err| format!("cannot write {}: {err}", out.display()))?;
    Ok(format!("wrote ledger JSON schema to {}", out.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledger_schema::CODE_DUP_FEATURE;

    fn args(items: &[&str]) -> impl Iterator<Item = String> {
        items.iter().map(|s| (*s).to_string())
    }

    // -- argument parsing ----------------------------------------------------

    #[test]
    fn empty_args_mean_check_with_default_root() {
        let cmd = parse_args(args(&[])).expect("parse");
        assert_eq!(
            cmd,
            Command::Check {
                root: PathBuf::from(".")
            }
        );
    }

    #[test]
    fn check_takes_optional_root() {
        let cmd = parse_args(args(&["check", "/tmp/reg"])).expect("parse");
        assert_eq!(
            cmd,
            Command::Check {
                root: PathBuf::from("/tmp/reg")
            }
        );
    }

    #[test]
    fn schema_defaults_to_stdout() {
        let cmd = parse_args(args(&["schema"])).expect("parse");
        assert_eq!(cmd, Command::Schema { out: None });
    }

    #[test]
    fn schema_takes_optional_output_path() {
        let cmd = parse_args(args(&["schema", "out.json"])).expect("parse");
        assert_eq!(
            cmd,
            Command::Schema {
                out: Some(PathBuf::from("out.json"))
            }
        );
    }

    #[test]
    fn unknown_subcommand_is_rejected_with_usage() {
        let err = parse_args(args(&["frobnicate"])).expect_err("reject");
        assert!(err.contains("usage:"), "error must include usage: {err}");
    }

    #[test]
    fn unknown_flag_is_rejected() {
        let err = parse_args(args(&["--wat"])).expect_err("reject");
        assert!(
            err.contains("unknown flag"),
            "message names the flag: {err}"
        );
    }

    #[test]
    fn help_flag_returns_usage() {
        let err = parse_args(args(&["-h"])).expect_err("help exits via usage");
        assert!(err.contains("usage:"));
    }

    #[test]
    fn extra_positional_is_rejected() {
        let err = parse_args(args(&["check", "a", "b"])).expect_err("reject");
        assert!(err.contains("unexpected extra argument"), "{err}");
    }

    // -- check run -----------------------------------------------------------

    fn write_file(root: &Path, rel: &str, contents: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, contents).expect("write");
    }

    fn feature_toml(id: &str) -> String {
        format!(
            "[[feature]]\n\
             id = \"{id}\"\n\
             title = \"t {id}\"\n\
             area = \"lang.test\"\n\
             tier = \"core\"\n\
             spec = \"docs/specs/v0.1.0.md\"\n\
             target_level = \"L5\"\n\
             backends = [\"gam\", \"llvm\"]\n\
             owner_lane = \"M\"\n"
        )
    }

    fn gap_toml(id: &str, affects: &str) -> String {
        format!(
            "[[gap]]\n\
             id = \"{id}\"\n\
             class = \"a\"\n\
             title = \"g {id}\"\n\
             affects = [\"{affects}\"]\n\
             caps = \"L2\"\n\
             opened_in = \"v0.3.0\"\n\
             status = \"unwitnessed\"\n"
        )
    }

    #[test]
    fn check_passes_on_a_valid_registry_with_witness_debt() {
        let dir = tempfile::tempdir().expect("tmp");
        let ledger = dir.path().join("ledger");
        write_file(&ledger, "features/lang/a.toml", &feature_toml("LANG-A"));
        write_file(&ledger, "gaps/x.toml", &gap_toml("GAP-1", "LANG-A"));
        let outcome = run_check(dir.path());
        assert!(
            outcome.is_ok(),
            "valid registry must pass: {:?}",
            outcome.diagnostics
        );
        assert_eq!(outcome.features, 1);
        assert_eq!(outcome.gaps, 1);
        assert_eq!(outcome.files, 2);
        assert_eq!(outcome.warnings(), 1, "one witness-debt warning");
    }

    #[test]
    fn check_fails_on_duplicate_feature_ids() {
        let dir = tempfile::tempdir().expect("tmp");
        let ledger = dir.path().join("ledger");
        write_file(&ledger, "features/lang/a.toml", &feature_toml("LANG-DUP"));
        write_file(&ledger, "features/lang/b.toml", &feature_toml("LANG-DUP"));
        let outcome = run_check(dir.path());
        assert_eq!(outcome.errors(), 1);
        assert_eq!(outcome.diagnostics[0].code, CODE_DUP_FEATURE);
    }

    #[test]
    fn check_fails_loudly_without_a_ledger_directory() {
        let dir = tempfile::tempdir().expect("tmp");
        let outcome = run_check(dir.path());
        assert!(!outcome.is_ok(), "missing registry must fail");
        assert_eq!(outcome.files, 0);
    }

    // -- rendering -----------------------------------------------------------

    #[test]
    fn format_outcome_renders_ok_summary() {
        let outcome = CheckOutcome {
            diagnostics: vec![],
            features: 6,
            gaps: 2,
            files: 8,
        };
        let text = format_outcome(&outcome);
        assert_eq!(
            text,
            "ledger check OK: 6 features, 2 gaps, 8 files, 0 errors, 0 warning(s)\n"
        );
    }

    #[test]
    fn format_outcome_lists_diagnostics_then_failure_summary() {
        let outcome = CheckOutcome {
            diagnostics: vec![Diagnostic {
                code: CODE_DUP_FEATURE,
                message: "duplicate feature id LANG-DUP".into(),
                severity: Severity::Error,
            }],
            features: 2,
            gaps: 0,
            files: 2,
        };
        let text = format_outcome(&outcome);
        assert!(text.contains("error[LEDGER-DUP-FEATURE]: duplicate feature id LANG-DUP"));
        assert!(text.contains("ledger check FAILED: 1 error(s), 0 warning(s)"));
        assert!(text.contains("2 features, 0 gaps, 2 files"));
    }

    // -- schema export -------------------------------------------------------

    #[test]
    fn write_schema_pretty_prints_to_a_file() {
        let dir = tempfile::tempdir().expect("tmp");
        let out = dir.path().join("ledger-schema.json");
        let summary = write_schema(&out).expect("write");
        assert!(summary.contains(&out.display().to_string()), "{summary}");
        let raw = std::fs::read_to_string(&out).expect("read back");
        let value: serde_json::Value = serde_json::from_str(&raw).expect("valid JSON");
        assert!(value.get("feature").is_some());
        assert!(value.get("gap").is_some());
        assert!(raw.contains("\n  \""), "pretty-printed, not single-line");
    }

    #[test]
    fn write_schema_reports_write_failures() {
        let dir = tempfile::tempdir().expect("tmp");
        let out = dir.path().join("no-such-dir").join("schema.json");
        assert!(write_schema(&out).is_err(), "missing parent dir must error");
    }
}
