//! `ledger verify` — the M1.13 determinism self-check (plan §6 M1.13,
//! first slice).
//!
//! Plan M1.13: "Ledger self-validation: run `compute` twice and on two
//! machines → identical bytes." This slice implements the *twice* half as
//! a command: [`verify_determinism`] runs every pure ledger pipeline the
//! tool exposes — compute (text, JSON, snapshot document), status
//! (Markdown), and the M1.12 release gates (text, JSON) — **twice**, from
//! two independent load passes over the same root, and compares the
//! rendered bytes. Any drift names the check that moved; the command
//! exits non-zero when a check is not byte-identical, which is what makes
//! it a *gate* rather than a report (plan C1: status is computed, never
//! typed — and re-computing it must be stable).
//!
//! The two-machine half is deliberately absent here: byte equality across
//! machines needs a committed artifact to compare against (CI job +
//! golden), which is a later M1.13 slice. The in-process double run pins
//! the property those CI jobs will reuse.
//!
//! # Determinism contract
//!
//! The checker itself is deterministic: checks run in a fixed order, each
//! check compares exactly two renders of the same pure function over the
//! same inputs, and the outcome carries no timing, no environment, and no
//! randomness — running `verify` twice prints identical bytes (pinned by
//! the repeat-render test).
//!
//! # Seams
//!
//! The checker reuses the executors verbatim ([`crate::run_compute`],
//! [`crate::run_status`], [`crate::run_gates`]) — it adds no new IO
//! boundary, so a future evidence source automatically participates in
//! the self-check. A drift is reported as the failing check's name, which
//! doubles as the pointer to the module whose renderer moved.

use std::fmt::Write as _;
use std::path::Path;

use ledger_schema::{Diagnostic, Severity};
use serde::Serialize;

use crate::compute::{ComputeOutcome, format_compute_json, format_compute_report, run_compute};
use crate::diff::ComputeSnapshot;
use crate::gates::{format_gates_json, format_gates_text, run_gates};
use crate::status::{format_status_markdown, run_status};

/// Schema tag of the verify output JSON contract.
pub const VERIFY_SCHEMA: &str = "glyim.ledger-verify/1";

/// The fixed, ordered list of self-check names (the JSON contract's
/// `checks[].name` spellings). The order is part of the contract: CI logs
/// diff line-by-line across runs.
pub const CHECK_NAMES: [&str; 6] = [
    "compute-text",
    "compute-json",
    "compute-snapshot",
    "status-markdown",
    "gates-text",
    "gates-json",
];

/// Result of one named double-run comparison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeterminismCheck {
    /// The check name (one of [`CHECK_NAMES`]).
    pub name: String,
    /// True when both passes rendered identical bytes.
    pub identical: bool,
    /// Byte length of the rendered artifact (0 when the pass failed to
    /// produce one because of load diagnostics — the diagnostics are
    /// reported separately and fail the command anyway).
    pub bytes: usize,
}

/// Result of a full `ledger verify` run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyOutcome {
    /// One check per pipeline, in [`CHECK_NAMES`] order.
    pub checks: Vec<DeterminismCheck>,
    /// Load/validation diagnostics from the underlying passes, sorted and
    /// de-duplicated (they fail the command like `check` does).
    pub diagnostics: Vec<Diagnostic>,
}

impl VerifyOutcome {
    /// Number of checks that rendered non-identical bytes.
    #[must_use]
    pub fn drifted(&self) -> usize {
        self.checks.iter().filter(|check| !check.identical).count()
    }

    /// True when no error-severity diagnostic was produced and every
    /// check is byte-identical. This is the M1.13 gate condition.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.diagnostics
            .iter()
            .all(|d| d.severity != Severity::Error)
            && self.drifted() == 0
    }
}

// -- engine -----------------------------------------------------------------

/// Renders the `glyim.ledger-snapshot/1` document bytes for one compute
/// outcome (the same pretty serialization `FileSnapshotStore::save`
/// writes, compared in memory instead of through a file).
fn snapshot_bytes(outcome: &ComputeOutcome) -> String {
    let snapshot = ComputeSnapshot::from_outcome(outcome);
    let mut text = serde_json::to_string_pretty(&snapshot)
        .unwrap_or_else(|err| format!("{{\"error\":\"{err}\"}}"));
    text.push('\n');
    text
}

/// Runs every ledger pipeline twice over `root` — two independent load
/// passes each — and compares the rendered bytes per check.
///
/// The two passes share no state: every executor reloads the registry,
/// policy, evidence, gate declaration, and gaps from disk, so the
/// comparison covers exactly the property M1.13 pins ("run compute twice
/// → identical bytes") rather than a cached re-render.
#[must_use]
pub fn verify_determinism(root: &Path, evidence: Option<&Path>) -> VerifyOutcome {
    let span = tracing::info_span!("verify", root = %root.display());
    let _enter = span.enter();

    let mut diagnostics = Vec::new();

    // Pass A and pass B are structurally identical; writing them as two
    // blocks keeps the two *independent* load passes visible (no shared
    // intermediate), which is the property under test.
    let compute_a = run_compute(root, evidence);
    let status_a = run_status(root, evidence);
    let gates_a = run_gates(root, evidence);

    let compute_b = run_compute(root, evidence);
    let status_b = run_status(root, evidence);
    let gates_b = run_gates(root, evidence);

    diagnostics.extend(compute_b.diagnostics.iter().cloned());
    diagnostics.extend(gates_b.diagnostics.iter().cloned());
    diagnostics.sort_by(|a, b| a.code.cmp(b.code).then(a.message.cmp(&b.message)));
    diagnostics.dedup();

    let pairs = [
        (
            CHECK_NAMES[0],
            format_compute_report(&compute_a),
            format_compute_report(&compute_b),
        ),
        (
            CHECK_NAMES[1],
            format_compute_json(&compute_a),
            format_compute_json(&compute_b),
        ),
        (
            CHECK_NAMES[2],
            snapshot_bytes(&compute_a),
            snapshot_bytes(&compute_b),
        ),
        (
            CHECK_NAMES[3],
            format_status_markdown(&status_a.compute),
            format_status_markdown(&status_b.compute),
        ),
        (
            CHECK_NAMES[4],
            format_gates_text(&gates_a),
            format_gates_text(&gates_b),
        ),
        (
            CHECK_NAMES[5],
            format_gates_json(&gates_a),
            format_gates_json(&gates_b),
        ),
    ];

    let checks: Vec<DeterminismCheck> = pairs
        .into_iter()
        .map(|(name, a, b)| DeterminismCheck {
            name: name.to_string(),
            identical: a == b,
            bytes: b.len(),
        })
        .collect();

    tracing::debug!(
        checks = checks.len(),
        drifted = checks.iter().filter(|c| !c.identical).count(),
        "determinism self-check complete"
    );

    VerifyOutcome {
        checks,
        diagnostics,
    }
}

// -- renderers ---------------------------------------------------------------

/// JSON document rendered by [`format_verify_json`].
#[derive(Serialize)]
struct VerifyJson<'a> {
    schema: &'static str,
    checks: usize,
    drifted: usize,
    ok: bool,
    entries: &'a [DeterminismCheck],
}

/// Renders the `glyim.ledger-verify/1` JSON contract (pretty-printed).
#[must_use]
pub fn format_verify_json(outcome: &VerifyOutcome) -> String {
    let document = VerifyJson {
        schema: VERIFY_SCHEMA,
        checks: outcome.checks.len(),
        drifted: outcome.drifted(),
        ok: outcome.is_ok(),
        entries: &outcome.checks,
    };
    let mut text = serde_json::to_string_pretty(&document).unwrap_or_else(|err| {
        // Field sets are fixed and values are plain strings/numbers, so
        // serialization is infallible by construction; this arm keeps the
        // renderer total without introducing a Result into the format_*
        // family contract (same shape as `format_gates_json`).
        format!("{{\"schema\":\"{VERIFY_SCHEMA}\",\"error\":\"{err}\"}}")
    });
    text.push('\n');
    text
}

/// Renders the text self-check report: a verdict line, then one line per
/// check (`name identical bytes`), then any diagnostics.
#[must_use]
pub fn format_verify_text(outcome: &VerifyOutcome) -> String {
    let mut text = String::new();
    let _ = writeln!(
        text,
        "verify: {} check(s), {} drifted, {} diagnostic(s) — {}",
        outcome.checks.len(),
        outcome.drifted(),
        outcome.diagnostics.len(),
        if outcome.is_ok() {
            "byte-identical across double runs"
        } else {
            "DETERMINISM DRIFT OR DIAGNOSTICS"
        },
    );
    for check in &outcome.checks {
        let _ = writeln!(
            text,
            "  {:<16} {:<9} {} bytes",
            check.name,
            if check.identical {
                "identical"
            } else {
                "DRIFTED"
            },
            check.bytes,
        );
    }
    for diagnostic in &outcome.diagnostics {
        let _ = writeln!(text, "{diagnostic}");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledger_schema::Severity;

    fn outcome(checks: Vec<DeterminismCheck>) -> VerifyOutcome {
        VerifyOutcome {
            checks,
            diagnostics: Vec::new(),
        }
    }

    fn check(name: &str, identical: bool) -> DeterminismCheck {
        DeterminismCheck {
            name: name.to_string(),
            identical,
            bytes: 42,
        }
    }

    #[test]
    fn check_names_are_the_six_pipelines_in_contract_order() {
        assert_eq!(
            CHECK_NAMES,
            [
                "compute-text",
                "compute-json",
                "compute-snapshot",
                "status-markdown",
                "gates-text",
                "gates-json",
            ]
        );
    }

    #[test]
    fn drifted_counts_non_identical_checks() {
        let parsed = outcome(vec![
            check("compute-text", true),
            check("gates-json", false),
        ]);
        assert_eq!(parsed.drifted(), 1);
        assert!(!parsed.is_ok(), "drift must fail the gate");
    }

    #[test]
    fn an_error_diagnostic_fails_even_when_bytes_match() {
        let mut parsed = outcome(vec![check("compute-text", true)]);
        parsed.diagnostics.push(Diagnostic {
            code: "LEDGER-TEST-ERR",
            message: "boom".to_string(),
            severity: Severity::Error,
        });
        assert!(!parsed.is_ok());
    }

    #[test]
    fn a_warning_diagnostic_does_not_fail_identical_bytes() {
        let mut parsed = outcome(vec![check("compute-text", true)]);
        parsed.diagnostics.push(Diagnostic {
            code: "LEDGER-WITNESS-DEBT",
            message: "debt".to_string(),
            severity: Severity::Warning,
        });
        assert!(parsed.is_ok());
    }

    #[test]
    fn text_report_renders_verdict_checks_and_diagnostics() {
        let parsed = outcome(vec![
            check("compute-text", true),
            check("gates-json", false),
        ]);
        let text = format_verify_text(&parsed);
        assert!(text.contains("verify: 2 check(s), 1 drifted"), "{text}");
        let compute_line = text
            .lines()
            .find(|l| l.contains("compute-text"))
            .expect("line");
        assert!(compute_line.contains("identical"), "{compute_line}");
        let drift_line = text
            .lines()
            .find(|l| l.contains("gates-json"))
            .expect("line");
        assert!(drift_line.contains("DRIFTED"), "{drift_line}");
        assert!(text.contains("DETERMINISM DRIFT"), "{text}");
    }

    #[test]
    fn json_contract_carries_schema_counts_and_entries() {
        let parsed = outcome(vec![check("compute-json", true)]);
        let json = format_verify_json(&parsed);
        let value: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(value["schema"], VERIFY_SCHEMA);
        assert_eq!(value["checks"], 1);
        assert_eq!(value["drifted"], 0);
        assert_eq!(value["ok"], true);
        assert_eq!(value["entries"][0]["name"], "compute-json");
        assert_eq!(value["entries"][0]["identical"], true);
        assert_eq!(value["entries"][0]["bytes"], 42);
    }

    #[test]
    fn renderers_are_byte_identical_on_repeat_runs() {
        let parsed = outcome(vec![check("compute-text", true)]);
        assert_eq!(format_verify_text(&parsed), format_verify_text(&parsed));
        assert_eq!(format_verify_json(&parsed), format_verify_json(&parsed));
    }

    #[test]
    fn verify_passes_on_a_valid_synthetic_registry() {
        // The full-pipeline property test: a valid registry (feature +
        // witnessed gap + policy + spec + gates declaration) must verify
        // byte-identical across the two passes, warnings included.
        let dir = tempfile::tempdir().expect("tmp");
        let ledger = dir.path().join("ledger");
        std::fs::create_dir_all(ledger.join("features")).expect("mkdir");
        std::fs::create_dir_all(ledger.join("gaps")).expect("mkdir");
        std::fs::create_dir_all(dir.path().join("docs/specs")).expect("mkdir specs");
        std::fs::write(dir.path().join("docs/specs/v0.1.0.md"), "# spec\n").expect("spec");
        std::fs::write(
            dir.path().join("ledger/policy.toml"),
            "schema = \"glyim.ledger-policy/1\"\n",
        )
        .expect("policy");
        std::fs::write(
            ledger.join("features/a.toml"),
            "[[feature]]\n\
             id = \"LANG-V\"\n\
             title = \"v\"\n\
             area = \"lang.control-flow\"\n\
             tier = \"core\"\n\
             spec = \"docs/specs/v0.1.0.md\"\n\
             target_level = \"L5\"\n\
             backends = [\"gam\"]\n\
             owner_lane = \"M\"\n",
        )
        .expect("feature");
        std::fs::write(
            ledger.join("gaps/a.toml"),
            "[[gap]]\n\
             id = \"GAP-0001\"\n\
             class = \"a\"\n\
             title = \"g\"\n\
             affects = [\"LANG-V\"]\n\
             caps = \"L2\"\n\
             opened_in = \"v0.3.0\"\n\
             status = \"witnessed\"\n\
             witness = \"w.g\"\n",
        )
        .expect("gap");
        // The witness must exist on disk (a missing fixture is an
        // LEDGER-WITNESS-MISSING error) and carry the strict-xfail marker.
        std::fs::write(
            dir.path().join("w.g"),
            "//@ known-gap: GAP-0001\nfn main() -> i32 { 0 }\n",
        )
        .expect("witness");
        std::fs::write(
            ledger.join("gates.toml"),
            "schema = \"glyim.ledger-gates-registry/1\"\n\n\
             [[gate]]\n\
             id = \"RG-10\"\n\
             title = \"Gap closure\"\n\
             criterion = \"0 open class-b gaps\"\n\
             evidence = \"gaps\"\n",
        )
        .expect("gates");

        let parsed = verify_determinism(dir.path(), None);
        assert!(
            parsed.is_ok(),
            "valid registry must verify: checks={:?} diagnostics={:?}",
            parsed.checks,
            parsed.diagnostics
        );
        assert_eq!(parsed.checks.len(), CHECK_NAMES.len());
        for check in &parsed.checks {
            assert!(check.identical, "check {} drifted", check.name);
            assert!(check.bytes > 0, "check {} rendered no bytes", check.name);
        }
    }

    #[test]
    fn verify_reports_but_survives_a_missing_registry() {
        // No ledger under the temp root: every pass produces load
        // diagnostics (which fail the gate) yet the checks still compare
        // — both passes render the same diagnostics-driven bytes.
        let dir = tempfile::tempdir().expect("tmp");
        let parsed = verify_determinism(dir.path(), None);
        assert!(!parsed.is_ok(), "missing registry must fail the gate");
        assert_eq!(parsed.checks.len(), CHECK_NAMES.len());
    }
}
