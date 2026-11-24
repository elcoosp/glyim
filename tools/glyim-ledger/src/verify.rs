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
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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

/// One rendered face of a single load pass: the check name (one of
/// [`CHECK_NAMES`]) and the exact rendered bytes.
type RenderedCheck = (&'static str, String);

/// Renders every ledger pipeline once over `root` — one independent load
/// pass — and returns the checks in [`CHECK_NAMES`] order.
///
/// Shared by the double-run self-check ([`verify_determinism`]) and the
/// golden faces ([`write_verify_golden`]/[`verify_against_golden_path`]):
/// the same renderer feeds both, so the golden can never pin bytes the
/// self-check would not compare.
fn render_pass(root: &Path, evidence: Option<&Path>) -> Vec<RenderedCheck> {
    let compute = run_compute(root, evidence);
    let status = run_status(root, evidence);
    let gates = run_gates(root, evidence);
    vec![
        (CHECK_NAMES[0], format_compute_report(&compute)),
        (CHECK_NAMES[1], format_compute_json(&compute)),
        (CHECK_NAMES[2], snapshot_bytes(&compute)),
        (CHECK_NAMES[3], format_status_markdown(&status.compute)),
        (CHECK_NAMES[4], format_gates_text(&gates)),
        (CHECK_NAMES[5], format_gates_json(&gates)),
    ]
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

    let pass_a = render_pass(root, evidence);
    let pass_b = render_pass(root, evidence);

    // Diagnostics come from the second pass (the executors reload
    // everything; a second pass reports the same stream as the first).
    let compute_b = run_compute(root, evidence);
    let gates_b = run_gates(root, evidence);
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    diagnostics.extend(compute_b.diagnostics.iter().cloned());
    diagnostics.extend(gates_b.diagnostics.iter().cloned());
    diagnostics.sort_by(|a, b| a.code.cmp(b.code).then(a.message.cmp(&b.message)));
    diagnostics.dedup();

    let checks: Vec<DeterminismCheck> = CHECK_NAMES
        .iter()
        .zip(pass_a.iter())
        .map(|(name, (rendered_name, bytes))| {
            debug_assert_eq!(name, rendered_name);
            let twin = pass_b
                .iter()
                .find(|(other_name, _)| other_name == rendered_name);
            DeterminismCheck {
                name: (*name).to_string(),
                identical: twin.is_some_and(|(_, other)| other == bytes),
                bytes: bytes.len(),
            }
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

// -- reproducibility golden (M1.13 two-machine half) -------------------------

/// Schema tag of the committed golden artifact (`ledger/verify-golden.json`).
pub const VERIFY_GOLDEN_SCHEMA: &str = "glyim.ledger-verify-golden/1";

/// Digest record for one rendered check inside a [`VerifyGolden`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoldenCheck {
    /// The check name (one of [`CHECK_NAMES`]).
    pub name: String,
    /// Rendered byte length.
    pub bytes: usize,
    /// SHA-256 of the rendered bytes (hex). Digests are the transportable
    /// form of the byte-equality property: two machines compare digests,
    /// and equal SHA-256 over equal byte lengths is the byte equality the
    /// plan pins (the same fixture digest discipline the evidence store
    /// uses).
    pub sha256: String,
}

/// The committed reproducibility artifact: per-check digests of every
/// ledger render, pinned in-tree and re-verified in CI (the plan M1.13
/// "two machines" half — the in-process double run pins determinism
/// within a machine, the golden pins it across machines and commits).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyGolden {
    /// Schema tag; must equal [`VERIFY_GOLDEN_SCHEMA`].
    pub schema: String,
    /// One digest per rendered check, in [`CHECK_NAMES`] order.
    pub checks: Vec<GoldenCheck>,
}

/// Result of comparing today's renders against a committed [`VerifyGolden`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoldenOutcome {
    /// The golden artifact path the renders were compared against.
    pub path: String,
    /// Per-check drift verdicts, in [`CHECK_NAMES`] order (only checks
    /// that appear in both the golden and the render are compared).
    pub drifts: Vec<GoldenDrift>,
    /// Load/validation diagnostics (a missing or malformed golden, an
    /// errored render pass) — errors fail the command like `verify` does.
    pub diagnostics: Vec<Diagnostic>,
}

impl GoldenOutcome {
    /// True when every check matched the golden and no error diagnostic
    /// was produced. This is the M1.13 reproducibility-gate condition.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.diagnostics
            .iter()
            .all(|d| d.severity != Severity::Error)
            && self.drifts.is_empty()
    }
}

/// One drifted check: the golden's digest vs today's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GoldenDrift {
    /// The check name that drifted.
    pub name: String,
    /// Byte length pinned in the golden.
    pub golden_bytes: usize,
    /// SHA-256 pinned in the golden.
    pub golden_sha256: String,
    /// Byte length rendered today.
    pub actual_bytes: usize,
    /// SHA-256 rendered today.
    pub actual_sha256: String,
}

/// SHA-256 of `bytes` as lowercase hex.
fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Renders every check once over `root` and digests them into a
/// [`VerifyGolden`] — the exact artifact `ledger verify --write-golden
/// PATH` commits.
#[must_use]
pub fn render_verify_golden(root: &Path, evidence: Option<&Path>) -> VerifyGolden {
    let checks = render_pass(root, evidence)
        .into_iter()
        .map(|(name, bytes)| GoldenCheck {
            name: name.to_string(),
            bytes: bytes.len(),
            sha256: sha256_hex(bytes.as_bytes()),
        })
        .collect();
    VerifyGolden {
        schema: VERIFY_GOLDEN_SCHEMA.to_string(),
        checks,
    }
}

/// Renders the golden and writes it to `path` (pretty JSON + trailing
/// newline), returning the golden that was written.
///
/// # Errors
///
/// Serialization or file IO failures become error-severity diagnostics on
/// the returned outcome (the exit-code contract stays in `main.rs`).
pub fn write_verify_golden(root: &Path, evidence: Option<&Path>, path: &Path) -> GoldenOutcome {
    let mut diagnostics = Vec::new();
    let golden = render_verify_golden(root, evidence);
    let text = match serde_json::to_string_pretty(&golden) {
        Ok(mut text) => {
            text.push('\n');
            text
        }
        Err(err) => {
            diagnostics.push(Diagnostic {
                code: "E-LEDGER-GOLDEN-WRITE",
                message: format!(
                    "cannot serialize the verify golden for {}: {err}",
                    path.display()
                ),
                severity: Severity::Error,
            });
            return GoldenOutcome {
                path: path.display().to_string(),
                drifts: Vec::new(),
                diagnostics,
            };
        }
    };
    if let Err(err) = std::fs::write(path, &text) {
        diagnostics.push(Diagnostic {
            code: "E-LEDGER-GOLDEN-WRITE",
            message: format!("cannot write the verify golden {}: {err}", path.display()),
            severity: Severity::Error,
        });
    }
    GoldenOutcome {
        path: path.display().to_string(),
        drifts: Vec::new(),
        diagnostics,
    }
}

/// Loads a committed golden from `path` and compares today's renders
/// against it, per check, in [`CHECK_NAMES`] order.
///
/// Every failure mode — unreadable file, unparsable JSON, foreign schema
/// tag, a golden missing a check the renderer produces — is an
/// error-severity diagnostic (a golden that cannot be trusted must fail
/// the gate, never silently pass).
#[must_use]
pub fn verify_against_golden_path(
    root: &Path,
    evidence: Option<&Path>,
    path: &Path,
) -> GoldenOutcome {
    let mut diagnostics = Vec::new();
    let path_string = path.display().to_string();
    let loaded = std::fs::read_to_string(path).map_err(|err| Diagnostic {
        code: "E-LEDGER-GOLDEN-LOAD",
        message: format!("cannot read the verify golden {path_string}: {err}"),
        severity: Severity::Error,
    });
    let golden = match loaded {
        Ok(text) => match serde_json::from_str::<VerifyGolden>(&text) {
            Ok(golden) if golden.schema == VERIFY_GOLDEN_SCHEMA => golden,
            Ok(golden) => {
                diagnostics.push(Diagnostic {
                    code: "E-LEDGER-GOLDEN-SCHEMA",
                    message: format!(
                        "verify golden {path_string} carries schema {:?}, expected {VERIFY_GOLDEN_SCHEMA:?}",
                        golden.schema
                    ),
                    severity: Severity::Error,
                });
                return GoldenOutcome {
                    path: path_string,
                    drifts: Vec::new(),
                    diagnostics,
                };
            }
            Err(err) => {
                diagnostics.push(Diagnostic {
                    code: "E-LEDGER-GOLDEN-SCHEMA",
                    message: format!("verify golden {path_string} does not parse: {err}"),
                    severity: Severity::Error,
                });
                return GoldenOutcome {
                    path: path_string,
                    drifts: Vec::new(),
                    diagnostics,
                };
            }
        },
        Err(diagnostic) => {
            return GoldenOutcome {
                path: path_string,
                drifts: Vec::new(),
                diagnostics: vec![diagnostic],
            };
        }
    };

    let mut drifts = Vec::new();
    for (name, bytes) in render_pass(root, evidence) {
        let actual_sha = sha256_hex(bytes.as_bytes());
        match golden.checks.iter().find(|check| check.name == name) {
            Some(check) => {
                if check.sha256 != actual_sha || check.bytes != bytes.len() {
                    drifts.push(GoldenDrift {
                        name: name.to_string(),
                        golden_bytes: check.bytes,
                        golden_sha256: check.sha256.clone(),
                        actual_bytes: bytes.len(),
                        actual_sha256: actual_sha,
                    });
                }
            }
            None => diagnostics.push(Diagnostic {
                code: "E-LEDGER-GOLDEN-SCHEMA",
                message: format!(
                    "verify golden {path_string} pins no digest for check {name:?} — regenerate it"
                ),
                severity: Severity::Error,
            }),
        }
    }
    GoldenOutcome {
        path: path_string,
        drifts,
        diagnostics,
    }
}

/// Renders the human-readable golden report (write face or compare face).
#[must_use]
pub fn format_golden_text(outcome: &GoldenOutcome) -> String {
    let mut text = String::new();
    let _ = writeln!(
        text,
        "Verify golden report for {} (plan M1.13 reproducibility)",
        outcome.path
    );
    for drift in &outcome.drifts {
        let _ = writeln!(
            text,
            "  DRIFT {}: golden {} bytes sha256:{} vs actual {} bytes sha256:{}",
            drift.name,
            drift.golden_bytes,
            drift.golden_sha256,
            drift.actual_bytes,
            drift.actual_sha256
        );
    }
    for diagnostic in &outcome.diagnostics {
        let _ = writeln!(
            text,
            "  {}: {}",
            diagnostic.severity.as_str(),
            diagnostic.message
        );
    }
    let _ = writeln!(
        text,
        "Gate: {}",
        if outcome.is_ok() {
            "every rendered check matches the committed golden — renders are reproducible across machines"
        } else {
            "golden drift or a load failure — regenerate with `verify --write-golden` only when the change is intended"
        }
    );
    text
}

/// JSON document rendered by [`format_golden_json`].
#[derive(Serialize)]
struct GoldenJson<'a> {
    schema: &'static str,
    path: &'a str,
    ok: bool,
    drifts: &'a [GoldenDrift],
}

/// Renders the `glyim.ledger-golden-verdict/1` JSON contract.
#[must_use]
pub fn format_golden_json(outcome: &GoldenOutcome) -> String {
    let document = GoldenJson {
        schema: "glyim.ledger-golden-verdict/1",
        path: &outcome.path,
        ok: outcome.is_ok(),
        drifts: &outcome.drifts,
    };
    let mut text = serde_json::to_string_pretty(&document)
        .unwrap_or_else(|err| format!("{{\"error\":\"{err}\"}}"));
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledger_schema::Severity;
    use std::path::PathBuf;

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

    /// M1.13 two-machine half, locally provable: a golden written from
    /// one render pass matches a fresh render pass byte-for-byte, and
    /// any tampering with the golden produces a named drift.
    #[test]
    fn golden_round_trips_and_detects_tampering() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("verify-golden.json");

        let written = write_verify_golden(dir.path(), None, &path);
        assert!(
            written.is_ok(),
            "a clean synthetic root writes its golden: {:?}",
            written.diagnostics
        );
        assert!(path.exists(), "the golden artifact lands on disk");

        // The freshly written golden matches a fresh render — the
        // cross-machine comparison, exercised in-process.
        let verdict = verify_against_golden_path(dir.path(), None, &path);
        assert!(
            verdict.is_ok(),
            "the committed golden must match the renders: drifts={:?} diags={:?}",
            verdict.drifts,
            verdict.diagnostics
        );

        // Tampering with one digest is a named drift, not a pass.
        let mut tampered: VerifyGolden =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read golden"))
                .expect("parse golden");
        tampered.checks[0].sha256 = "0".repeat(64);
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&tampered).expect("serialize"),
        )
        .expect("write tampered");
        let verdict = verify_against_golden_path(dir.path(), None, &path);
        assert!(!verdict.is_ok(), "a tampered digest must fail the gate");
        assert_eq!(verdict.drifts.len(), 1);
        assert_eq!(verdict.drifts[0].name, CHECK_NAMES[0]);
    }

    #[test]
    fn golden_refuses_a_missing_or_foreign_artifact() {
        let dir = tempfile::tempdir().expect("tmp");
        let missing = verify_against_golden_path(dir.path(), None, &dir.path().join("absent.json"));
        assert!(!missing.is_ok(), "a missing golden fails the gate");
        assert!(
            missing
                .diagnostics
                .iter()
                .any(|d| d.code == "E-LEDGER-GOLDEN-LOAD")
        );

        let foreign = dir.path().join("foreign.json");
        std::fs::write(&foreign, "{\"schema\":\"glyim.status/1\",\"checks\":[]}").expect("write");
        let verdict = verify_against_golden_path(dir.path(), None, &foreign);
        assert!(!verdict.is_ok(), "a foreign schema fails the gate");
        assert!(
            verdict
                .diagnostics
                .iter()
                .any(|d| d.code == "E-LEDGER-GOLDEN-SCHEMA")
        );
    }

    /// The committed golden in the repository tree matches the renders —
    /// the same comparison the reproducibility CI job runs.
    #[test]
    fn the_committed_verify_golden_matches_the_renders() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("ledger crate lives under <repo>/tools/")
            .to_path_buf();
        let golden_path = repo_root.join("ledger/verify-golden.json");
        assert!(
            golden_path.exists(),
            "the committed golden must exist (regenerate with `ledger verify --write-golden`)"
        );
        let verdict = verify_against_golden_path(&repo_root, None, &golden_path);
        assert!(
            verdict.is_ok(),
            "committed golden drifted: {:?} {:?}",
            verdict.drifts,
            verdict.diagnostics
        );
    }
}
