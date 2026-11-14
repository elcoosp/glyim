//! `ledger release-check` — the plan §7/§10 final release gate.
//!
//! The plan names the face three times: the CLI table lists
//! "`ledger release-check vX.Y.Z` — Final release gate", the release
//! criteria section (§10) defines its contract — "passes iff **all 24
//! readiness gates** are green on the release commit, evidence is
//! reproducible (RG-04/17), and no waiver is active" — and milestone
//! M12.7 requires it to pass before the v1.0 tag. Every verdict axis it
//! needs already shipped as a face (the M1.12 gate report, the M1.13
//! determinism self-check, the C9 waiver-health report), but no command
//! composed them into the one verdict a release commit needs; this face
//! is that composition, the `handoff` pattern applied to the release
//! contract.
//!
//! # The seams
//!
//! [`ReleaseCheckSource`] decouples the gating engine from where the
//! three face outcomes come from — the File/InMemory twin pattern
//! [`crate::waivers`] establishes for [`crate::WaiverSource`]:
//! [`FileReleaseCheckSource`] runs the real faces over the registry under
//! `ROOT`, [`InMemoryReleaseCheckSource`] hands the engine a pre-built
//! state for tests and in-process consumers. [`ReleaseCheckRenderer`]
//! mirrors [`crate::WaiversRenderer`]: the text report and the
//! `glyim.ledger-release-check/1` JSON contract render from one
//! [`ReleaseCheckOutcome`].
//!
//! # Gate semantics (plan §10, verbatim)
//!
//! [`ReleaseCheckOutcome::is_ok`] is the face's exit-code contract, and
//! every clause maps to the plan sentence:
//!
//! - **"all 24 readiness gates are green"** — every declared gate's
//!   verdict is [`crate::GateVerdict::Pass`]; a `fail` blocks, and an
//!   `unmeasured` verdict blocks too: unmeasured is *displayed, never
//!   silently passed* (plan M1.12), so a release with unmeasured gates is
//!   not releasable and the honest verdict says so;
//! - **"evidence is reproducible"** — the M1.13 self-check reports zero
//!   drifted checks and zero error diagnostics;
//! - **"no waiver is active"** — zero unexpired waivers. Expired
//!   waivers block through the composed C9 health contract the same way
//!   they fail the `waivers` face (constitution C9: an expired exception
//!   must fail CI even when no regression needs covering) — the release
//!   gate is never greener than the faces it composes.
//!
//! Error-severity diagnostics across the three composed faces block the
//! verdict; warnings are displayed, not failed — the same severity
//! contract every composed face applies. The debt is rendered, never
//! silent: a red release check is still a complete report.
//!
//! # Determinism (plan C1/M1.13)
//!
//! The engine is a pure function of the composed state and the parsed
//! version: no clock, no filesystem, no environment. The assessment date
//! rides in on the waivers outcome (the [`crate::handoff`] pattern — one
//! clock consult per run), gate rows are already in declaration order
//! (the M1.12 contract), and both renderers run twice byte-identical,
//! pinned by golden and round-trip tests.

use std::fmt;
use std::fmt::Write as _;

use serde::Serialize;

use crate::NaiveDate;
use crate::gates::{GateVerdict, GatesOutcome, run_gates};
use crate::verify::{VerifyOutcome, verify_determinism};
use crate::waivers::{FileWaiverSource, WaiversOutcome, run_waivers};
use ledger_schema::Severity;

/// Schema tag of the release-check JSON contract (pinned by the
/// round-trip test; bump the minor when adding fields, the major when
/// changing or removing them).
pub const RELEASE_CHECK_SCHEMA: &str = "glyim.ledger-release-check/1";

/// The parsed `<VERSION>` argument (`vMAJOR.MINOR.PATCH`, the `v`
/// prefix optional) the verdict names.
///
/// Hand-rolled like the rest of the CLI's argument handling (no `semver`
/// dependency — the tool's supply chain stays dependency-light): three
/// integer components separated by dots, each in `0..=u64::MAX`, no
/// build metadata, no pre-release tags (a release gate gates a plain
/// triple).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleaseVersion {
    /// Major version.
    pub major: u64,
    /// Minor version.
    pub minor: u64,
    /// Patch version.
    pub patch: u64,
}

impl ReleaseVersion {
    /// Parses `vMAJOR.MINOR.PATCH` (or `MAJOR.MINOR.PATCH`); `None` for
    /// anything else (wrong component count, non-numeric or oversized
    /// component, empty component).
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let digits = text.strip_prefix('v').unwrap_or(text);
        let mut parts = digits.split('.');
        let major = parts.next()?.parse::<u64>().ok()?;
        let minor = parts.next()?.parse::<u64>().ok()?;
        let patch = parts.next()?.parse::<u64>().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
        })
    }

    /// The unprefixed `SemVer` spelling (`1.2.3`).
    #[must_use]
    pub fn to_semver(self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl fmt::Display for ReleaseVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The three composed face outcomes the gate reads. Loaded by a
/// [`ReleaseCheckSource`], consumed by [`render_release_check`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseCheckState {
    /// The M1.12 release-gate verdicts over `ledger/gates.toml`.
    pub gates: GatesOutcome,
    /// The M1.13 determinism self-check (evidence reproducibility).
    pub verify: VerifyOutcome,
    /// The C9 waiver-health outcome (pins the assessment date).
    pub waivers: WaiversOutcome,
}

/// Source of the composed state for the release gate.
///
/// The seam that keeps [`render_release_check`] independent of where its
/// three face outcomes come from: the two implementations below cover the
/// live registry and in-process fixtures, and future sources (a CI
/// artifact, a pinned snapshot) plug in as one more impl without the
/// engine changing shape.
pub trait ReleaseCheckSource {
    /// Loads the composed state for the registry under `root`.
    fn load(&self, root: &std::path::Path) -> ReleaseCheckState;
}

/// Runs the real faces over the registry under `root`: the M1.12 gate
/// report, the M1.13 determinism self-check, and the C9 waiver health —
/// through the same executors and loaders those faces use, so a release
/// verdict can never disagree with the individual face runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileReleaseCheckSource;

impl ReleaseCheckSource for FileReleaseCheckSource {
    fn load(&self, root: &std::path::Path) -> ReleaseCheckState {
        ReleaseCheckState {
            gates: run_gates(root, None),
            verify: verify_determinism(root, None),
            waivers: run_waivers(root, &FileWaiverSource),
        }
    }
}

/// Hands the engine a pre-built state (tests and in-process consumers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InMemoryReleaseCheckSource {
    /// The state every `load` returns (cloned).
    pub state: ReleaseCheckState,
}

impl ReleaseCheckSource for InMemoryReleaseCheckSource {
    fn load(&self, _root: &std::path::Path) -> ReleaseCheckState {
        self.state.clone()
    }
}

/// Result of a full `ledger release-check` run: the composed verdict both
/// renderers consume. The JSON face re-shapes this into
/// [`ReleaseCheckDocument`] (the `HandoffDocument` split: the version as
/// its wire spelling, the gate as an explicit `ok` field).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseCheckOutcome {
    /// The JSON contract tag (echoed for machine consumers).
    pub schema: &'static str,
    /// The release the verdict names (the parsed `<VERSION>` argument).
    pub version: ReleaseVersion,
    /// The date the assessment ran against (pinned by the waivers
    /// outcome; no second clock consult).
    pub today: NaiveDate,
    /// Gates loaded from the declaration (rows; 0 when it failed to
    /// load).
    pub gates_total: usize,
    /// Gates whose verdict is [`GateVerdict::Pass`].
    pub gates_pass: usize,
    /// Gates whose verdict is [`GateVerdict::Fail`].
    pub gates_fail: usize,
    /// Gates whose verdict is [`GateVerdict::Unmeasured`] (displayed,
    /// never silently passed — an unmeasured gate blocks the release).
    pub gates_unmeasured: usize,
    /// Declared gate IDs that are not green (`fail` or `unmeasured`), in
    /// declaration order (the M1.12 rows order).
    pub blocking_gates: Vec<String>,
    /// Determinism self-checks run (the M1.13 face's fixed set).
    pub verify_checks: usize,
    /// Self-checks that rendered non-identical bytes.
    pub verify_drifted: usize,
    /// Waivers inside their horizon (the plan §10 release blocker).
    pub waivers_active: usize,
    /// Waivers past their horizon (C9 debt; fails the composed health
    /// face and the release gate with it).
    pub waivers_expired: usize,
    /// Error-severity diagnostics across the three composed faces,
    /// sorted-source order summed (the count drives the gate).
    pub error_diagnostics: usize,
}

impl ReleaseCheckOutcome {
    /// True when the release gate passes (plan §10, verbatim): every
    /// declared gate is green (no `fail`, no `unmeasured`), the
    /// determinism self-check reports no drift, no waiver is active or
    /// expired, and no error-severity diagnostic was produced across the
    /// composed faces.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.gates_fail == 0
            && self.gates_unmeasured == 0
            && self.verify_drifted == 0
            && self.waivers_active == 0
            && self.waivers_expired == 0
            && self.error_diagnostics == 0
    }
}

// -- engine ------------------------------------------------------------------

/// Composes the state into the release verdict: a pure function with no
/// clock, no filesystem, and no environment — the assessment date rides
/// in on the waivers outcome, blocking-gate order follows the M1.12
/// declaration order, and the counts are set-based.
#[must_use]
pub fn render_release_check(
    state: &ReleaseCheckState,
    version: ReleaseVersion,
) -> ReleaseCheckOutcome {
    let gates_total = state.gates.rows.len();
    let mut gates_pass = 0usize;
    let mut gates_fail = 0usize;
    let mut gates_unmeasured = 0usize;
    let mut blocking_gates = Vec::new();
    for row in &state.gates.rows {
        match row.verdict {
            GateVerdict::Pass => gates_pass += 1,
            GateVerdict::Fail => {
                gates_fail += 1;
                blocking_gates.push(row.id.clone());
            }
            GateVerdict::Unmeasured => {
                gates_unmeasured += 1;
                blocking_gates.push(row.id.clone());
            }
        }
    }

    let error_diagnostics = state.gates.errors()
        + state.waivers.errors()
        + state
            .verify
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == Severity::Error)
            .count();

    ReleaseCheckOutcome {
        schema: RELEASE_CHECK_SCHEMA,
        version,
        today: state.waivers.today,
        gates_total,
        gates_pass,
        gates_fail,
        gates_unmeasured,
        blocking_gates,
        verify_checks: state.verify.checks.len(),
        verify_drifted: state.verify.drifted(),
        waivers_active: state.waivers.valid,
        waivers_expired: state.waivers.expired,
        error_diagnostics,
    }
}

// -- renderers ---------------------------------------------------------------

/// The `glyim.ledger-release-check/1` JSON document (the serialization-only
/// re-shape of [`ReleaseCheckOutcome`]: the version as its wire spelling,
/// the date as `YYYY-MM-DD`, the gate as an explicit `ok` field — the
/// `HandoffDocument` split).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReleaseCheckDocument {
    /// Schema tag, always [`RELEASE_CHECK_SCHEMA`].
    pub schema: &'static str,
    /// The release the verdict names (`vMAJOR.MINOR.PATCH`).
    pub version: String,
    /// The assessment date (`YYYY-MM-DD`).
    pub today: String,
    /// Gates loaded from the declaration.
    pub gates_total: usize,
    /// Gates whose verdict is `pass`.
    pub gates_pass: usize,
    /// Gates whose verdict is `fail`.
    pub gates_fail: usize,
    /// Gates whose verdict is `unmeasured`.
    pub gates_unmeasured: usize,
    /// Declared gate IDs that are not green, declaration order.
    pub blocking_gates: Vec<String>,
    /// Determinism self-checks run.
    pub verify_checks: usize,
    /// Self-checks that drifted.
    pub verify_drifted: usize,
    /// Waivers inside their horizon (release blockers).
    pub waivers_active: usize,
    /// Waivers past their horizon (C9 debt).
    pub waivers_expired: usize,
    /// Error-severity diagnostics across the composed faces.
    pub error_diagnostics: usize,
    /// Whether the release gate passed ([`ReleaseCheckOutcome::is_ok`]).
    pub ok: bool,
}

/// Renders a [`ReleaseCheckOutcome`] into one of the two release faces.
///
/// The seam that keeps both renderers interchangeable for consumers:
/// tests and benches drive them through this trait exactly like the
/// `HandoffRenderer` and `WaiversRenderer` seams.
pub trait ReleaseCheckRenderer {
    /// Renders the face (complete bytes, including trailing newline).
    fn render(&self, outcome: &ReleaseCheckOutcome) -> String;
}

/// The human-readable release report (the plain text face — the
/// `gates`/`verify` report shape, no committed document).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TextReleaseCheckRenderer;

impl ReleaseCheckRenderer for TextReleaseCheckRenderer {
    fn render(&self, outcome: &ReleaseCheckOutcome) -> String {
        let mut text = String::new();
        let _ = writeln!(text, "# Release check — {}", outcome.version);
        let _ = writeln!(text, "assessed {}", outcome.today.to_iso());
        let _ = writeln!(text);
        let _ = writeln!(text, "## Gates (plan §3.10)");
        let _ = writeln!(
            text,
            "- declared {}, pass {}, fail {}, unmeasured {}",
            outcome.gates_total, outcome.gates_pass, outcome.gates_fail, outcome.gates_unmeasured,
        );
        if outcome.blocking_gates.is_empty() {
            let _ = writeln!(text, "- blocking gates: none");
        } else {
            let _ = writeln!(
                text,
                "- blocking gates: {}",
                outcome.blocking_gates.join(", ")
            );
        }
        let _ = writeln!(text);
        let _ = writeln!(text, "## Evidence reproducibility (plan M1.13)");
        let _ = writeln!(
            text,
            "- checks {}, drifted {}",
            outcome.verify_checks, outcome.verify_drifted,
        );
        let _ = writeln!(text);
        let _ = writeln!(text, "## Waivers (constitution C9)");
        let _ = writeln!(
            text,
            "- active {}, expired {}",
            outcome.waivers_active, outcome.waivers_expired,
        );
        if outcome.waivers_active > 0 {
            let _ = writeln!(text, "- an active waiver blocks the release (plan §10)");
        }
        if outcome.waivers_expired > 0 {
            let _ = writeln!(
                text,
                "- an expired waiver fails the C9 health gate (constitution C9)"
            );
        }
        let _ = writeln!(text);
        let _ = writeln!(text, "## Verdict");
        if outcome.is_ok() {
            let _ = writeln!(text, "- PASS — {} is releasable", outcome.version);
        } else {
            let _ = writeln!(text, "- FAIL — {} is not releasable", outcome.version);
        }
        text
    }
}

/// The `glyim.ledger-release-check/1` JSON face.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JsonReleaseCheckRenderer;

impl ReleaseCheckRenderer for JsonReleaseCheckRenderer {
    fn render(&self, outcome: &ReleaseCheckOutcome) -> String {
        let document = ReleaseCheckDocument {
            schema: RELEASE_CHECK_SCHEMA,
            version: outcome.version.to_string(),
            today: outcome.today.to_iso(),
            gates_total: outcome.gates_total,
            gates_pass: outcome.gates_pass,
            gates_fail: outcome.gates_fail,
            gates_unmeasured: outcome.gates_unmeasured,
            blocking_gates: outcome.blocking_gates.clone(),
            verify_checks: outcome.verify_checks,
            verify_drifted: outcome.verify_drifted,
            waivers_active: outcome.waivers_active,
            waivers_expired: outcome.waivers_expired,
            error_diagnostics: outcome.error_diagnostics,
            ok: outcome.is_ok(),
        };
        let mut text = serde_json::to_string_pretty(&document)
            .expect("the glyim.ledger-release-check/1 document is a serialisation-only struct");
        text.push('\n');
        text
    }
}

/// Renders the text release report (the [`TextReleaseCheckRenderer`]
/// face).
#[must_use]
pub fn format_release_check_text(outcome: &ReleaseCheckOutcome) -> String {
    TextReleaseCheckRenderer.render(outcome)
}

/// Renders the `glyim.ledger-release-check/1` JSON contract (the
/// [`JsonReleaseCheckRenderer`] face).
#[must_use]
pub fn format_release_check_json(outcome: &ReleaseCheckOutcome) -> String {
    JsonReleaseCheckRenderer.render(outcome)
}

// -- run face ----------------------------------------------------------------

/// Runs a full `ledger release-check` pass: the composed state from
/// `source` (the CLI wires [`FileReleaseCheckSource`]; tests and
/// in-process consumers wire [`InMemoryReleaseCheckSource`]) rendered
/// through the engine.
#[must_use]
pub fn run_release_check(
    root: &std::path::Path,
    version: ReleaseVersion,
    source: &dyn ReleaseCheckSource,
) -> ReleaseCheckOutcome {
    let span = tracing::info_span!(
        "release_check",
        root = %root.display(),
        version = %version,
    );
    let _enter = span.enter();
    let state = source.load(root);
    let outcome = render_release_check(&state, version);
    tracing::debug!(
        gates_total = outcome.gates_total,
        blocking_gates = outcome.blocking_gates.len(),
        verify_drifted = outcome.verify_drifted,
        waivers_active = outcome.waivers_active,
        "release check composed"
    );
    outcome
}

// -- tests -------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, LedgerCliError, parse_args, usage};
    use ledger_schema::{Diagnostic, GateEvidence};
    use serde_json::Value;
    use std::path::PathBuf;

    /// The pinned assessment date (no clock in the tests).
    fn day() -> NaiveDate {
        NaiveDate::new_unchecked(2026, 11, 13)
    }

    /// The release version the fixtures gate.
    fn version() -> ReleaseVersion {
        ReleaseVersion {
            major: 1,
            minor: 0,
            patch: 0,
        }
    }

    /// One declared gate with its verdict (the M1.12 row shape, evidence
    /// spelled so the engine never inspects the criterion).
    fn gate(id: &str, title: &str, verdict: GateVerdict) -> crate::gates::GateVerdictRow {
        crate::gates::GateVerdictRow {
            id: id.to_string(),
            title: title.to_string(),
            criterion: "synthetic criterion line".to_string(),
            evidence: if verdict == GateVerdict::Unmeasured {
                GateEvidence::None
            } else {
                GateEvidence::Levels
            },
            verdict,
            detail: "synthetic detail line".to_string(),
        }
    }

    /// One determinism self-check row (the M1.13 shape).
    fn check(name: &str, identical: bool) -> crate::verify::DeterminismCheck {
        crate::verify::DeterminismCheck {
            name: name.to_string(),
            identical,
            bytes: 42,
        }
    }

    /// One assessed waiver row (the C9 shape).
    fn row(
        id: &str,
        verdict: crate::waivers::WaiverVerdict,
        expires: &str,
    ) -> crate::waivers::WaiverRow {
        crate::waivers::WaiverRow {
            id: id.to_string(),
            feature: "LANG-RELEASE-DEMO".to_string(),
            reason: "regressions covered while the fix lands".to_string(),
            issue: "https://github.com/elcoosp/glyim/issues/42".to_string(),
            expires: expires.to_string(),
            verdict,
            remaining_days: if verdict == crate::waivers::WaiverVerdict::Valid {
                7
            } else {
                -2
            },
        }
    }

    /// One error- or warning-severity diagnostic.
    fn diagnostic(code: &'static str, severity: Severity) -> Diagnostic {
        Diagnostic {
            code,
            message: "synthetic diagnostic".to_string(),
            severity,
        }
    }

    /// The composed fixture state: 3 declared gates (2 pass, 1 unmeasured),
    /// 6 clean self-checks, 1 valid + 1 expired waiver.
    fn state() -> ReleaseCheckState {
        ReleaseCheckState {
            gates: GatesOutcome {
                diagnostics: Vec::new(),
                rows: vec![
                    gate("RG-09", "Feature closure", GateVerdict::Pass),
                    gate("RG-10", "Gap closure", GateVerdict::Pass),
                    gate("RG-11", "Instruction counts", GateVerdict::Unmeasured),
                ],
                gates: 3,
                failing: 0,
                unmeasured: 1,
            },
            verify: VerifyOutcome {
                checks: vec![
                    check("compute-text", true),
                    check("compute-json", true),
                    check("compute-snapshot", true),
                    check("status-markdown", true),
                    check("gates-text", true),
                    check("gates-json", true),
                ],
                diagnostics: Vec::new(),
            },
            waivers: WaiversOutcome {
                today: day(),
                diagnostics: Vec::new(),
                rows: vec![
                    row(
                        "WVR-20261107-alpha",
                        crate::waivers::WaiverVerdict::Valid,
                        "2026-11-20",
                    ),
                    row(
                        "WVR-20261105-gamma",
                        crate::waivers::WaiverVerdict::Expired,
                        "2026-11-11",
                    ),
                ],
                waivers: 2,
                valid: 1,
                expired: 1,
            },
        }
    }

    /// The plan §10 green state: all gates pass, no drift, no waivers, no
    /// diagnostics.
    fn green_state() -> ReleaseCheckState {
        let mut state = state();
        state.gates.rows[2].verdict = GateVerdict::Pass;
        state.gates.rows[2].evidence = GateEvidence::Levels;
        state.gates.unmeasured = 0;
        state.waivers.rows.clear();
        state.waivers.waivers = 0;
        state.waivers.valid = 0;
        state.waivers.expired = 0;
        state
    }

    #[test]
    fn render_counts_verdicts_waivers_and_diagnostics() {
        let outcome = render_release_check(&state(), version());
        assert_eq!(outcome.schema, RELEASE_CHECK_SCHEMA);
        assert_eq!(outcome.version, version());
        assert_eq!(outcome.today, day());
        assert_eq!(outcome.gates_total, 3);
        assert_eq!(outcome.gates_pass, 2);
        assert_eq!(outcome.gates_fail, 0);
        assert_eq!(outcome.gates_unmeasured, 1);
        assert_eq!(outcome.blocking_gates, ["RG-11"]);
        assert_eq!(outcome.verify_checks, 6);
        assert_eq!(outcome.verify_drifted, 0);
        assert_eq!(outcome.waivers_active, 1);
        assert_eq!(outcome.waivers_expired, 1);
        assert_eq!(outcome.error_diagnostics, 0);
    }

    #[test]
    fn render_is_a_pure_function_of_state_and_version() {
        let a = render_release_check(&state(), version());
        let b = render_release_check(&state(), version());
        assert_eq!(a, b, "the engine consults no clock, fs, or environment");
        let other = render_release_check(&state(), ReleaseVersion::parse("v0.2.1").expect("parse"));
        assert_ne!(a, other, "the verdict names its release");
    }

    #[test]
    fn blocking_gates_list_fail_and_unmeasured_in_declaration_order() {
        let mut mixed = state();
        mixed.gates.rows[1].verdict = GateVerdict::Fail;
        mixed.gates.failing = 1;
        let outcome = render_release_check(&mixed, version());
        assert_eq!(outcome.blocking_gates, ["RG-10", "RG-11"]);
        assert_eq!(outcome.gates_fail, 1);
        assert_eq!(outcome.gates_unmeasured, 1);
        assert_eq!(outcome.gates_pass, 1);
    }

    #[test]
    fn is_ok_follows_plan_section_10_clauses() {
        assert!(
            render_release_check(&green_state(), version()).is_ok(),
            "all gates green, reproducible evidence, no waiver: releasable"
        );

        // One failing gate blocks.
        let mut failed = green_state();
        failed.gates.rows[2].verdict = GateVerdict::Fail;
        failed.gates.failing = 1;
        assert!(
            !render_release_check(&failed, version()).is_ok(),
            "a failing gate blocks the release"
        );

        // One unmeasured gate blocks too: unmeasured is displayed, never
        // silently passed (plan M1.12) — a release with unmeasured gates
        // is not releasable.
        let mut unmeasured = green_state();
        unmeasured.gates.rows[2].verdict = GateVerdict::Unmeasured;
        unmeasured.gates.rows[2].evidence = GateEvidence::None;
        unmeasured.gates.unmeasured = 1;
        assert!(
            !render_release_check(&unmeasured, version()).is_ok(),
            "an unmeasured gate is not green"
        );

        // Evidence drift blocks (RG-04/17 reproducibility).
        let mut drifted = green_state();
        drifted.verify.checks[0].identical = false;
        assert!(
            !render_release_check(&drifted, version()).is_ok(),
            "a drifted determinism check blocks the release"
        );

        // An active (unexpired) waiver blocks (plan §10: no waiver is
        // active).
        let mut active = green_state();
        active.waivers.rows = vec![row(
            "WVR-20261107-alpha",
            crate::waivers::WaiverVerdict::Valid,
            "2026-11-20",
        )];
        active.waivers.waivers = 1;
        active.waivers.valid = 1;
        assert!(
            !render_release_check(&active, version()).is_ok(),
            "an active waiver blocks the release"
        );

        // An expired waiver blocks through the composed C9 contract — the
        // release gate is never greener than the faces it composes.
        let mut expired = green_state();
        expired.waivers.rows = vec![row(
            "WVR-20261105-gamma",
            crate::waivers::WaiverVerdict::Expired,
            "2026-11-11",
        )];
        expired.waivers.waivers = 1;
        expired.waivers.expired = 1;
        assert!(
            !render_release_check(&expired, version()).is_ok(),
            "an expired waiver fails the C9 gate"
        );

        // Error diagnostics in any composed face block; warnings display,
        // not fail.
        let mut errored = green_state();
        errored.gates.diagnostics = vec![diagnostic("LEDGER-GATES-LOAD", Severity::Error)];
        assert!(!render_release_check(&errored, version()).is_ok());
        let mut warned = green_state();
        warned.gates.diagnostics = vec![diagnostic("LEDGER-WARN", Severity::Warning)];
        assert!(
            render_release_check(&warned, version()).is_ok(),
            "warnings are displayed, not failed"
        );
    }

    #[test]
    fn error_diagnostics_sum_across_the_three_faces() {
        let mut errored = green_state();
        errored.gates.diagnostics = vec![diagnostic("LEDGER-GATES-LOAD", Severity::Error)];
        errored.verify.diagnostics = vec![diagnostic("LEDGER-VERIFY", Severity::Error)];
        errored.waivers.diagnostics = vec![diagnostic("LEDGER-WAIVER", Severity::Error)];
        let outcome = render_release_check(&errored, version());
        assert_eq!(outcome.error_diagnostics, 3);
    }

    #[test]
    fn text_golden_pins_the_release_face() {
        let golden = "\
# Release check — v1.0.0
assessed 2026-11-13

## Gates (plan §3.10)
- declared 3, pass 2, fail 0, unmeasured 1
- blocking gates: RG-11

## Evidence reproducibility (plan M1.13)
- checks 6, drifted 0

## Waivers (constitution C9)
- active 1, expired 1
- an active waiver blocks the release (plan §10)
- an expired waiver fails the C9 health gate (constitution C9)

## Verdict
- FAIL — v1.0.0 is not releasable
";
        let rendered = TextReleaseCheckRenderer.render(&render_release_check(&state(), version()));
        assert_eq!(rendered, golden);
    }

    #[test]
    fn text_golden_pins_the_green_face() {
        let golden = "\
# Release check — v1.0.0
assessed 2026-11-13

## Gates (plan §3.10)
- declared 3, pass 3, fail 0, unmeasured 0
- blocking gates: none

## Evidence reproducibility (plan M1.13)
- checks 6, drifted 0

## Waivers (constitution C9)
- active 0, expired 0

## Verdict
- PASS — v1.0.0 is releasable
";
        let rendered =
            TextReleaseCheckRenderer.render(&render_release_check(&green_state(), version()));
        assert_eq!(rendered, golden);
    }

    #[test]
    fn json_contract_parses_back_with_schema_and_counts() {
        let text = JsonReleaseCheckRenderer.render(&render_release_check(&state(), version()));
        let document: Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(document["schema"], RELEASE_CHECK_SCHEMA);
        assert_eq!(document["version"], "v1.0.0");
        assert_eq!(document["today"], "2026-11-13");
        assert_eq!(document["gates_total"], 3);
        assert_eq!(document["gates_pass"], 2);
        assert_eq!(document["gates_fail"], 0);
        assert_eq!(document["gates_unmeasured"], 1);
        assert_eq!(document["blocking_gates"][0], "RG-11");
        assert_eq!(document["verify_checks"], 6);
        assert_eq!(document["verify_drifted"], 0);
        assert_eq!(document["waivers_active"], 1);
        assert_eq!(document["waivers_expired"], 1);
        assert_eq!(document["error_diagnostics"], 0);
        assert_eq!(document["ok"], false);
    }

    #[test]
    fn json_face_reports_ok_for_the_green_state() {
        let text =
            JsonReleaseCheckRenderer.render(&render_release_check(&green_state(), version()));
        let document: Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(document["ok"], true);
        assert_eq!(document["blocking_gates"].as_array().map(Vec::len), Some(0));
        assert_eq!(document["waivers_active"], 0);
    }

    #[test]
    fn both_renderers_render_twice_is_byte_identical() {
        let outcome = render_release_check(&state(), version());
        let text_a = TextReleaseCheckRenderer.render(&outcome);
        let text_b = TextReleaseCheckRenderer.render(&outcome);
        let json_a = JsonReleaseCheckRenderer.render(&outcome);
        let json_b = JsonReleaseCheckRenderer.render(&outcome);
        assert_eq!(text_a, text_b);
        assert_eq!(json_a, json_b);
        assert!(json_a.ends_with('\n'), "the JSON face ends with a newline");
    }

    #[test]
    fn version_parse_accepts_prefixed_and_bare_triples() {
        let triple = ReleaseVersion {
            major: 1,
            minor: 2,
            patch: 3,
        };
        assert_eq!(ReleaseVersion::parse("v1.2.3"), Some(triple));
        assert_eq!(ReleaseVersion::parse("1.2.3"), Some(triple));
        assert_eq!(
            ReleaseVersion::parse("v0.0.0"),
            Some(ReleaseVersion {
                major: 0,
                minor: 0,
                patch: 0,
            })
        );
        assert_eq!(
            ReleaseVersion::parse("v1.2.3").map(|v| v.to_semver()),
            Some("1.2.3".to_string())
        );
        assert_eq!(version().to_string(), "v1.0.0");
    }

    #[test]
    fn version_parse_rejects_malformed_triples() {
        for text in [
            "", "v", "v1", "1", "v1.2", "1.2", "v1.2.3.4", "1.2.3.4", "vx.y.z", "v1.x.3",
            "v-1.2.3", "v1 .2.3",
        ] {
            assert!(
                ReleaseVersion::parse(text).is_none(),
                "{text:?} is not a release triple"
            );
        }
    }

    #[test]
    fn in_memory_source_returns_the_prebuilt_state() {
        let source = InMemoryReleaseCheckSource {
            state: green_state(),
        };
        let loaded = source.load(&PathBuf::from("."));
        assert_eq!(loaded, green_state());
    }

    #[test]
    fn file_source_composes_the_registry_faces() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("ledger/features")).expect("mkdir features");
        std::fs::create_dir_all(root.join("docs/specs")).expect("mkdir specs");
        std::fs::write(
            root.join("docs/specs/v0.1.0.md"),
            "# spec fixture\n\n## evidence\n",
        )
        .expect("write spec");
        std::fs::write(
            root.join("ledger/policy.toml"),
            "schema = \"glyim.ledger-policy/1\"\n\n[levels.L1]\nrules = [{ mode = \"compile-pass\", outcome = \"passed\", count = 1 }]\n\n[gap_caps]\nenabled = true\n\n[hardening]\nfuzz_cpu_hours = 24\nmutation_kill_rate_pct = 75\n",
        )
        .expect("write policy");
        std::fs::write(
            root.join("ledger/features/release.toml"),
            "[[feature]]\n\
             id = \"LANG-RELEASE-DEMO\"\n\
             title = \"release gate walkthrough feature\"\n\
             area = \"lang.control-flow\"\n\
             tier = \"core\"\n\
             spec = \"docs/specs/v0.1.0.md\"\n\
             target_level = \"L5\"\n\
             backends = [\"gam\"]\n\
             owner_lane = \"M\"\n",
        )
        .expect("write feature");
        // Two declared gates with `evidence = "none"`: the honest verdict
        // is unmeasured, and an unmeasured gate blocks the release (plan
        // M1.12 + §10) — the fixture pins exactly that composition.
        std::fs::write(
            root.join("ledger/gates.toml"),
            "schema = \"glyim.ledger-gates-registry/1\"\n\n\
             [[gate]]\n\
             id = \"RG-01\"\n\
             title = \"No ICE\"\n\
             criterion = \"0 ICEs over the full fuzz corpus\"\n\
             evidence = \"none\"\n\n\
             [[gate]]\n\
             id = \"RG-02\"\n\
             title = \"Backend agreement\"\n\
             criterion = \"0 disagreements GAM vs LLVM\"\n\
             evidence = \"none\"\n",
        )
        .expect("write gates");

        let outcome = render_release_check(&FileReleaseCheckSource.load(&root), version());
        assert_eq!(outcome.gates_total, 2, "both declared gates loaded");
        assert_eq!(
            outcome.gates_unmeasured, 2,
            "no evidence source: unmeasured"
        );
        assert_eq!(outcome.blocking_gates, ["RG-01", "RG-02"]);
        assert!(
            outcome.verify_checks > 0,
            "the self-check ran its fixed set"
        );
        assert_eq!(outcome.verify_drifted, 0, "the pipeline is deterministic");
        assert_eq!(
            outcome.waivers_active, 0,
            "no waivers.toml: no active waiver"
        );
        assert_eq!(outcome.error_diagnostics, 0, "the registry loads cleanly");
        assert!(!outcome.is_ok(), "unmeasured gates block the release");
    }

    #[test]
    fn parse_args_accepts_release_check() {
        let command = parse_args(["release-check", "v1.0.0"].into_iter().map(String::from))
            .expect("version only");
        assert_eq!(
            command,
            Command::ReleaseCheck {
                version: version(),
                root: PathBuf::from("."),
                format: crate::ReleaseCheckFormat::Text,
            }
        );
        let command = parse_args(
            ["release-check", "0.2.1", "/tmp/repo", "--format", "json"]
                .into_iter()
                .map(String::from),
        )
        .expect("root and format");
        assert_eq!(
            command,
            Command::ReleaseCheck {
                version: ReleaseVersion::parse("0.2.1").expect("parse"),
                root: PathBuf::from("/tmp/repo"),
                format: crate::ReleaseCheckFormat::Json,
            }
        );
    }

    #[test]
    fn parse_args_rejects_bad_release_check() {
        let err = parse_args(["release-check"].iter().map(|s| s.to_string()))
            .expect_err("missing version");
        assert!(matches!(
            err,
            LedgerCliError::ReleaseCheckMissingVersion { .. }
        ));

        for value in ["v1", "1.2", "v1.2.3.4", "vx.y.z"] {
            let err = parse_args(["release-check", value].iter().map(|s| s.to_string()))
                .expect_err("bad version");
            assert!(
                matches!(err, LedgerCliError::ReleaseCheckBadVersion { .. }),
                "{value:?} must fail with ReleaseCheckBadVersion"
            );
        }

        let err = parse_args(
            ["release-check", "v1.0.0", "--format", "yaml"]
                .iter()
                .map(|s| s.to_string()),
        )
        .expect_err("bad format");
        assert!(matches!(err, LedgerCliError::ReleaseCheckBadFormat { .. }));

        let err = parse_args(
            ["release-check", "v1.0.0", "--format"]
                .iter()
                .map(|s| s.to_string()),
        )
        .expect_err("missing value");
        assert!(matches!(
            err,
            LedgerCliError::ReleaseCheckMissingValue { .. }
        ));

        let err = parse_args(
            ["release-check", "v1.0.0", "--unknown"]
                .iter()
                .map(|s| s.to_string()),
        )
        .expect_err("unknown flag");
        assert!(matches!(err, LedgerCliError::UnknownFlag { .. }));

        let err = parse_args(
            ["release-check", "v1.0.0", "a", "b"]
                .iter()
                .map(|s| s.to_string()),
        )
        .expect_err("two positional roots");
        assert!(matches!(err, LedgerCliError::ExtraArgument { .. }));
    }

    #[test]
    fn release_check_error_variants_carry_code_and_help() {
        use miette::Diagnostic as _;
        let variants = [
            LedgerCliError::ReleaseCheckMissingVersion { usage: usage() },
            LedgerCliError::ReleaseCheckBadVersion {
                value: "v1.2".to_string(),
            },
            LedgerCliError::ReleaseCheckBadFormat {
                value: "yaml".to_string(),
                usage: usage(),
            },
            LedgerCliError::ReleaseCheckMissingValue {
                flag: "--format".to_string(),
                usage: usage(),
            },
        ];
        for variant in &variants {
            let code = variant.code().map(|c| c.to_string()).expect("code");
            assert!(code.starts_with("glyim-ledger::release_check"));
            assert!(variant.help().is_some(), "{code} carries a help line");
        }
    }

    #[test]
    fn usage_lists_release_check() {
        let text = usage();
        assert!(text.contains("glyim-ledger release-check <VERSION> [ROOT]"));
        assert!(text.contains("--format text|json"));
    }
}
