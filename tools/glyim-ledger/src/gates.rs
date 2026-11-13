//! `ledger gates` — the M1.12 release-gate report (plan §6 M1.12, §3.7).
//!
//! The plan's 24 release gates (`RG-01`…`RG-24`) are declared in
//! `ledger/gates.toml` (schema: [`ledger_schema::gates`]); this module is
//! the verdict engine over that declaration: for every declared gate it
//! computes the [`GateVerdict`] the current evidence supports and renders
//! the `glyim.ledger-gates/1` contract or the text table.
//!
//! # Verdict honesty contract (plan M1.12: "unmeasured allowed but
//! displayed")
//!
//! Three verdicts exist, and none of them is invented:
//!
//! - [`GateVerdict::Pass`] — the gate's evidence source exists and its
//!   criterion holds under the current evidence;
//! - [`GateVerdict::Fail`] — the evidence source exists and the criterion
//!   does not hold (the detail line names the counts that miss);
//! - [`GateVerdict::Unmeasured`] — no evidence source records the gate
//!   yet (its declaration says `evidence = "none"`). Unmeasured is
//!   *displayed*, never silently passed, and never failed by itself: the
//!   M1.12 exit gate is "`ledger gates` prints all 24", so the command
//!   exits non-zero only on load/validation diagnostics — release
//!   blocking is the `diff`-style gate's job, and wiring verdicts into CI
//!   blocking is deliberately left to the later RG wiring slices.
//!
//! # Determinism contract (plan C1/M1.13)
//!
//! The engine reads no clock, no environment, no randomness: rows render
//! in registry file order, counts are set-based, diagnostics are sorted
//! and de-duplicated exactly like [`crate::run_check`], and running
//! [`evaluate_gates`] twice on the same inputs yields identical bytes
//! through both renderers (pinned by golden tests).
//!
//! # Seams
//!
//! The declaration seam is [`ledger_schema::GateRegistry`] (loaded by
//! [`ledger_schema::load_gates`]); the evidence seams are the ones the
//! ledger already owns — the M1.8 compute outcome ([`FeatureLevel`]
//! entries) for the `levels` source and the M1.7 gap registry
//! ([`Sourced<GapEntry>`]) for the `gaps` source. New evidence sources
//! plug in as new [`ledger_schema::GateEvidence`] variants plus one rule
//! arm in [`evaluate_gates`], never as verdict fields.

use std::fmt::Write as _;
use std::path::Path;

use ledger_schema::{
    Diagnostic, GapClass, GapEntry, GapStatus, GateEntry, GateEvidence, GateRegistry, Level,
    Severity, Sourced, Tier, load_gates,
};
use serde::Serialize;

use crate::compute::{ComputeOutcome, FeatureLevel, run_compute};
use crate::{GapOutcome, run_gaps};

/// Schema tag of the gates output JSON contract.
pub const GATES_REPORT_SCHEMA: &str = "glyim.ledger-gates/1";

/// The verdict the engine computed for one declared gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GateVerdict {
    /// The gate's evidence source exists and its criterion holds.
    Pass,
    /// The evidence source exists and the criterion does not hold.
    Fail,
    /// No evidence source records this gate yet (displayed, not failed).
    Unmeasured,
}

impl GateVerdict {
    /// Canonical wire and table spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Unmeasured => "unmeasured",
        }
    }
}

/// One declared gate with its computed verdict and the deterministic
/// detail line explaining the verdict (the JSON contract's entry shape).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GateVerdictRow {
    /// Stable gate ID (`RG-NN`).
    pub id: String,
    /// The plan's gate name.
    pub title: String,
    /// The measurable pass condition (echoed from the declaration).
    pub criterion: String,
    /// The evidence source consulted for the verdict.
    pub evidence: GateEvidence,
    /// The computed verdict.
    pub verdict: GateVerdict,
    /// Counts or the honest absence note behind the verdict.
    pub detail: String,
}

/// Result of a full `ledger gates` run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatesOutcome {
    /// Load/validation diagnostics (declaration, registry, policy,
    /// evidence), sorted and de-duplicated.
    pub diagnostics: Vec<Diagnostic>,
    /// One row per declared gate, in declaration order.
    pub rows: Vec<GateVerdictRow>,
    /// Gates loaded from the declaration (0 when it failed to load).
    pub gates: usize,
    /// Rows whose verdict is [`GateVerdict::Fail`].
    pub failing: usize,
    /// Rows whose verdict is [`GateVerdict::Unmeasured`].
    pub unmeasured: usize,
}

impl GatesOutcome {
    /// Number of error-severity diagnostics.
    #[must_use]
    pub fn errors(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    }

    /// True when no error-severity diagnostic was produced. Verdicts do
    /// not enter this: `fail` rows are report content (the M1.12 exit
    /// gate is "prints all 24", see the module docs), not command exit
    /// codes.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.errors() == 0
    }
}

// -- engine -----------------------------------------------------------------

/// Computes one [`GateVerdictRow`] per declared gate.
///
/// The rule arms are per [`GateEvidence`] source; `None` renders
/// [`GateVerdict::Unmeasured`] with the honest absence note. Rows keep
/// declaration order — the plan's RG-01…RG-24 order — so consumers never
/// re-sort.
#[must_use]
pub fn evaluate_gates(
    registry: &GateRegistry,
    levels: &[FeatureLevel],
    gaps: &[Sourced<GapEntry>],
) -> Vec<GateVerdictRow> {
    registry
        .gates
        .iter()
        .map(|gate| evaluate_gate(gate, levels, gaps))
        .collect()
}

/// Verdict for a single declared gate (the per-gate arm of
/// [`evaluate_gates`]).
fn evaluate_gate(
    gate: &GateEntry,
    levels: &[FeatureLevel],
    gaps: &[Sourced<GapEntry>],
) -> GateVerdictRow {
    let (verdict, detail) = match gate.evidence {
        GateEvidence::None => (
            GateVerdict::Unmeasured,
            "no evidence source records this gate yet".to_string(),
        ),
        GateEvidence::Levels => levels_gate_verdict(levels),
        GateEvidence::Gaps => gaps_gate_verdict(gaps),
    };
    GateVerdictRow {
        id: gate.id.clone(),
        title: gate.title.clone(),
        criterion: gate.criterion.clone(),
        evidence: gate.evidence,
        verdict,
        detail,
    }
}

/// Rule arm for the `levels` evidence source (plan RG-09: "all core and
/// std features ≥ L4; core ≥ L5").
///
/// The second clause is evaluated against the same computed levels the
/// first clause uses: an L5 target can only be met when the compute
/// engine's hardening axes are measured, so a core feature stuck below L5
/// is reported as a fail naming the unmeasured axes — never as an
/// unmeasured gate (the *gate's* measurement exists; it is the levels
/// that miss).
fn levels_gate_verdict(levels: &[FeatureLevel]) -> (GateVerdict, String) {
    if levels.is_empty() {
        return (
            GateVerdict::Unmeasured,
            "no readiness entries to evaluate".to_string(),
        );
    }
    let below_l4: Vec<&FeatureLevel> = levels
        .iter()
        .filter(|entry| {
            matches!(entry.tier, Tier::Core | Tier::Std)
                && (entry.computed_level as usize) < (Level::L4 as usize)
        })
        .collect();
    if !below_l4.is_empty() {
        return (
            GateVerdict::Fail,
            format!(
                "{} core/std feature(s) below L4 (e.g. {})",
                below_l4.len(),
                below_l4
                    .iter()
                    .take(3)
                    .map(|entry| entry.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
    }
    let core_below_l5: Vec<&FeatureLevel> = levels
        .iter()
        .filter(|entry| {
            matches!(entry.tier, Tier::Core)
                && (entry.computed_level as usize) < (Level::L5 as usize)
        })
        .collect();
    if !core_below_l5.is_empty() {
        return (
            GateVerdict::Fail,
            format!(
                "{} core feature(s) below L5 — L5 hardening axes are unmeasured (e.g. {})",
                core_below_l5.len(),
                core_below_l5
                    .iter()
                    .take(3)
                    .map(|entry| entry.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
    }
    (
        GateVerdict::Pass,
        "every core/std feature at target".to_string(),
    )
}

/// Rule arm for the `gaps` evidence source (plan RG-10: "0 open class-b
/// gaps; every class-a gap has diagnostic + docs; witness debt = 0").
///
/// The registry holds only *open* gaps (closed ones retire their entry),
/// so every counted entry is open by construction.
fn gaps_gate_verdict(gaps: &[Sourced<GapEntry>]) -> (GateVerdict, String) {
    let class_b = gaps.iter().filter(|g| g.entry.class == GapClass::B).count();
    let class_a_without_diagnostic = gaps
        .iter()
        .filter(|g| g.entry.class == GapClass::A && g.entry.expect.is_none())
        .count();
    let witness_debt = gaps
        .iter()
        .filter(|g| g.entry.status == GapStatus::Unwitnessed)
        .count();
    let detail = format!(
        "{class_b} open class-b gap(s); {class_a_without_diagnostic} class-a gap(s) \
         without a pinned diagnostic; {witness_debt} unwitnessed gap(s)"
    );
    if class_b == 0 && class_a_without_diagnostic == 0 && witness_debt == 0 {
        (GateVerdict::Pass, detail)
    } else {
        (GateVerdict::Fail, detail)
    }
}

// -- executor ---------------------------------------------------------------

/// Runs a full `ledger gates` pass over `root`: loads the gate
/// declaration (`ledger/gates.toml`), recomputes the readiness levels
/// (same evidence contract as [`crate::run_compute`]) and the gap
/// registry, and evaluates every declared gate.
#[must_use]
pub fn run_gates(root: &Path, evidence: Option<&Path>) -> GatesOutcome {
    let span = tracing::info_span!("gates", root = %root.display());
    let _enter = span.enter();

    let mut diagnostics = Vec::new();
    let (registry, load) = load_gates(root);
    diagnostics.extend(load);

    let compute: ComputeOutcome = run_compute(root, evidence);
    diagnostics.extend(compute.diagnostics.iter().cloned());

    let gaps: GapOutcome = run_gaps(root);
    diagnostics.extend(gaps.diagnostics.iter().cloned());

    diagnostics.sort_by(|a, b| a.code.cmp(b.code).then(a.message.cmp(&b.message)));
    diagnostics.dedup();

    let Some(registry) = registry else {
        return GatesOutcome {
            diagnostics,
            rows: Vec::new(),
            gates: 0,
            failing: 0,
            unmeasured: 0,
        };
    };

    let rows = evaluate_gates(&registry, &compute.entries, &gaps.gaps);
    let failing = rows
        .iter()
        .filter(|row| row.verdict == GateVerdict::Fail)
        .count();
    let unmeasured = rows
        .iter()
        .filter(|row| row.verdict == GateVerdict::Unmeasured)
        .count();

    tracing::debug!(
        gates = rows.len(),
        failing,
        unmeasured,
        diagnostics = diagnostics.len(),
        "release-gate verdicts computed"
    );

    GatesOutcome {
        diagnostics,
        gates: rows.len(),
        failing,
        unmeasured,
        rows,
    }
}

// -- renderers ---------------------------------------------------------------

/// JSON document rendered by [`format_gates_json`].
#[derive(Serialize)]
struct GatesJson<'a> {
    schema: &'static str,
    gates: usize,
    pass: usize,
    fail: usize,
    unmeasured: usize,
    entries: &'a [GateVerdictRow],
}

/// Renders the `glyim.ledger-gates/1` JSON contract (pretty-printed).
#[must_use]
pub fn format_gates_json(outcome: &GatesOutcome) -> String {
    let pass = outcome.gates - outcome.failing - outcome.unmeasured;
    let document = GatesJson {
        schema: GATES_REPORT_SCHEMA,
        gates: outcome.gates,
        pass,
        fail: outcome.failing,
        unmeasured: outcome.unmeasured,
        entries: &outcome.rows,
    };
    let mut text = serde_json::to_string_pretty(&document).unwrap_or_else(|err| {
        // Field sets are fixed and values are plain strings/numbers, so
        // serialization is infallible by construction; this arm keeps the
        // renderer total without introducing a Result into the format_*
        // family contract (same shape as `format_status_json`).
        format!("{{\"schema\":\"{GATES_REPORT_SCHEMA}\",\"error\":\"{err}\"}}")
    });
    text.push('\n');
    text
}

/// Renders the text release-gate table: a summary line, then one line per
/// gate (`ID  verdict  [evidence]  title`) with the criterion indented
/// underneath and the detail line last, then any diagnostics.
#[must_use]
pub fn format_gates_text(outcome: &GatesOutcome) -> String {
    let pass = outcome.gates - outcome.failing - outcome.unmeasured;
    let mut text = String::new();
    let _ = writeln!(
        text,
        "gates: {} declared, {pass} pass, {} fail, {} unmeasured, {} diagnostic(s)",
        outcome.gates,
        outcome.failing,
        outcome.unmeasured,
        outcome.diagnostics.len(),
    );
    for row in &outcome.rows {
        let _ = writeln!(
            text,
            "  {:<6} {:<10} [{}] {}",
            row.id,
            row.verdict.as_str(),
            row.evidence.as_str(),
            row.title,
        );
        let _ = writeln!(text, "         criterion: {}", row.criterion);
        let _ = writeln!(text, "         detail: {}", row.detail);
    }
    for diagnostic in &outcome.diagnostics {
        let _ = writeln!(text, "{diagnostic}");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledger_schema::GapExpectation;

    fn gate(id: &str, evidence: GateEvidence) -> GateEntry {
        GateEntry {
            id: id.to_string(),
            title: format!("gate {id}"),
            criterion: "criterion".to_string(),
            evidence,
        }
    }

    fn registry(gates: &[GateEntry]) -> GateRegistry {
        GateRegistry {
            gates: gates.to_vec(),
        }
    }

    fn feature(id: &str, tier: Tier, computed: Level) -> FeatureLevel {
        FeatureLevel {
            id: id.to_string(),
            tier,
            target_level: Level::L5,
            computed_level: computed,
            capped_by: Vec::new(),
            unmeasured: Vec::new(),
            missing: Vec::new(),
        }
    }

    fn gap(
        id: &str,
        class: GapClass,
        witnessed: bool,
        diagnostic: Option<&str>,
    ) -> Sourced<GapEntry> {
        Sourced {
            path: "ledger/gaps/known/test.toml".into(),
            entry: GapEntry {
                id: id.to_string(),
                class,
                title: format!("gap {id}"),
                affects: Vec::new(),
                caps: Level::L2,
                witness: witnessed.then(|| "fixture.g".to_string()),
                expect: diagnostic.map(|code| GapExpectation {
                    diagnostic: code.to_string(),
                }),
                opened_in: "v0.3.0".to_string(),
                target_milestone: None,
                prevention: Vec::new(),
                status: if witnessed {
                    GapStatus::Witnessed
                } else {
                    GapStatus::Unwitnessed
                },
            },
        }
    }

    fn outcome(rows: Vec<GateVerdictRow>) -> GatesOutcome {
        let failing = rows
            .iter()
            .filter(|row| row.verdict == GateVerdict::Fail)
            .count();
        let unmeasured = rows
            .iter()
            .filter(|row| row.verdict == GateVerdict::Unmeasured)
            .count();
        GatesOutcome {
            diagnostics: Vec::new(),
            gates: rows.len(),
            failing,
            unmeasured,
            rows,
        }
    }

    // -- verdict rules -------------------------------------------------------

    #[test]
    fn gates_without_evidence_are_unmeasured_and_displayed() {
        let rows = evaluate_gates(&registry(&[gate("RG-01", GateEvidence::None)]), &[], &[]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].verdict, GateVerdict::Unmeasured);
        assert!(
            rows[0].detail.contains("no evidence source"),
            "the absence must be named: {}",
            rows[0].detail
        );
        assert_eq!(GateVerdict::Unmeasured.as_str(), "unmeasured");
    }

    #[test]
    fn rg10_passes_on_an_empty_gap_registry() {
        let rows = evaluate_gates(&registry(&[gate("RG-10", GateEvidence::Gaps)]), &[], &[]);
        assert_eq!(rows[0].verdict, GateVerdict::Pass, "{}", rows[0].detail);
    }

    #[test]
    fn rg10_fails_on_open_class_b_gaps() {
        let rows = evaluate_gates(
            &registry(&[gate("RG-10", GateEvidence::Gaps)]),
            &[],
            &[gap("GAP-0002", GapClass::B, true, None)],
        );
        assert_eq!(rows[0].verdict, GateVerdict::Fail);
        assert!(
            rows[0].detail.contains("1 open class-b gap(s)"),
            "{}",
            rows[0].detail
        );
    }

    #[test]
    fn rg10_fails_when_a_class_a_gap_lacks_a_pinned_diagnostic() {
        let rows = evaluate_gates(
            &registry(&[gate("RG-10", GateEvidence::Gaps)]),
            &[],
            &[gap("GAP-0009", GapClass::A, true, None)],
        );
        assert_eq!(rows[0].verdict, GateVerdict::Fail);
        assert!(
            rows[0]
                .detail
                .contains("1 class-a gap(s) without a pinned diagnostic"),
            "{}",
            rows[0].detail
        );
    }

    #[test]
    fn rg10_fails_on_witness_debt() {
        let rows = evaluate_gates(
            &registry(&[gate("RG-10", GateEvidence::Gaps)]),
            &[],
            &[gap("GAP-0002", GapClass::B, false, None)],
        );
        assert_eq!(rows[0].verdict, GateVerdict::Fail);
        assert!(
            rows[0].detail.contains("1 unwitnessed gap(s)"),
            "{}",
            rows[0].detail
        );
    }

    #[test]
    fn rg10_passes_when_every_gap_is_diagnosed_and_witnessed() {
        let rows = evaluate_gates(
            &registry(&[gate("RG-10", GateEvidence::Gaps)]),
            &[],
            &[
                gap("GAP-0001", GapClass::A, true, Some("T0060")),
                gap("GAP-0003", GapClass::C, true, None),
            ],
        );
        assert_eq!(rows[0].verdict, GateVerdict::Pass, "{}", rows[0].detail);
    }

    #[test]
    fn rg09_fails_naming_core_std_features_below_l4() {
        let rows = evaluate_gates(
            &registry(&[gate("RG-09", GateEvidence::Levels)]),
            &[
                feature("LANG-TRAITS", Tier::Core, Level::L2),
                feature("STD-VEC", Tier::Std, Level::L4),
            ],
            &[],
        );
        assert_eq!(rows[0].verdict, GateVerdict::Fail);
        assert!(
            rows[0].detail.contains("1 core/std feature(s) below L4"),
            "{}",
            rows[0].detail
        );
        assert!(rows[0].detail.contains("LANG-TRAITS"));
    }

    #[test]
    fn rg09_fails_on_core_features_below_l5_even_when_l4_is_met() {
        let rows = evaluate_gates(
            &registry(&[gate("RG-09", GateEvidence::Levels)]),
            &[
                feature("LANG-TRAITS", Tier::Core, Level::L4),
                feature("STD-VEC", Tier::Std, Level::L4),
            ],
            &[],
        );
        assert_eq!(rows[0].verdict, GateVerdict::Fail);
        assert!(rows[0].detail.contains("below L5"), "{}", rows[0].detail);
    }

    #[test]
    fn rg09_ignores_tooling_and_experimental_tiers() {
        let rows = evaluate_gates(
            &registry(&[gate("RG-09", GateEvidence::Levels)]),
            &[
                feature("TOOL-LSP", Tier::Tooling, Level::L0),
                feature("EXP-GSX", Tier::Experimental, Level::L0),
            ],
            &[],
        );
        assert_eq!(rows[0].verdict, GateVerdict::Pass, "{}", rows[0].detail);
    }

    #[test]
    fn rg09_is_unmeasured_without_readiness_entries() {
        let rows = evaluate_gates(&registry(&[gate("RG-09", GateEvidence::Levels)]), &[], &[]);
        assert_eq!(rows[0].verdict, GateVerdict::Unmeasured);
    }

    #[test]
    fn rg09_passes_only_when_core_reaches_l5() {
        let rows = evaluate_gates(
            &registry(&[gate("RG-09", GateEvidence::Levels)]),
            &[
                feature("LANG-TRAITS", Tier::Core, Level::L5),
                feature("STD-VEC", Tier::Std, Level::L4),
            ],
            &[],
        );
        assert_eq!(rows[0].verdict, GateVerdict::Pass, "{}", rows[0].detail);
    }

    #[test]
    fn rows_keep_declaration_order() {
        let rows = evaluate_gates(
            &registry(&[
                gate("RG-09", GateEvidence::Levels),
                gate("RG-01", GateEvidence::None),
                gate("RG-10", GateEvidence::Gaps),
            ]),
            &[],
            &[],
        );
        let ids: Vec<&str> = rows.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, ["RG-09", "RG-01", "RG-10"]);
    }

    #[test]
    fn evaluation_is_deterministic_under_repeated_calls() {
        let reg = registry(&[gate("RG-10", GateEvidence::Gaps)]);
        let gaps = vec![gap("GAP-0002", GapClass::B, true, None)];
        let first = evaluate_gates(&reg, &[], &gaps);
        let second = evaluate_gates(&reg, &[], &gaps);
        assert_eq!(first, second);
    }

    // -- outcome ---------------------------------------------------------

    #[test]
    fn outcome_counts_fail_and_unmeasured_rows() {
        let parsed = outcome(vec![
            GateVerdictRow {
                id: "RG-09".to_string(),
                title: "t".to_string(),
                criterion: "c".to_string(),
                evidence: GateEvidence::Levels,
                verdict: GateVerdict::Fail,
                detail: "d".to_string(),
            },
            GateVerdictRow {
                id: "RG-01".to_string(),
                title: "t".to_string(),
                criterion: "c".to_string(),
                evidence: GateEvidence::None,
                verdict: GateVerdict::Unmeasured,
                detail: "d".to_string(),
            },
            GateVerdictRow {
                id: "RG-10".to_string(),
                title: "t".to_string(),
                criterion: "c".to_string(),
                evidence: GateEvidence::Gaps,
                verdict: GateVerdict::Pass,
                detail: "d".to_string(),
            },
        ]);
        assert_eq!(parsed.gates, 3);
        assert_eq!(parsed.failing, 1);
        assert_eq!(parsed.unmeasured, 1);
        assert!(parsed.is_ok());
    }

    // -- renderers -------------------------------------------------------

    #[test]
    fn text_table_renders_summary_rows_and_details() {
        let parsed = outcome(vec![GateVerdictRow {
            id: "RG-01".to_string(),
            title: "No ICE".to_string(),
            criterion: "0 ICEs".to_string(),
            evidence: GateEvidence::None,
            verdict: GateVerdict::Unmeasured,
            detail: "no evidence source records this gate yet".to_string(),
        }]);
        let text = format_gates_text(&parsed);
        assert!(
            text.contains("gates: 1 declared, 0 pass, 0 fail, 1 unmeasured"),
            "{text}"
        );
        assert!(text.contains("RG-01"), "{text}");
        assert!(text.contains("[none]"), "{text}");
        assert!(text.contains("criterion: 0 ICEs"), "{text}");
        assert!(text.contains("detail: no evidence source"), "{text}");
    }

    #[test]
    fn json_contract_carries_schema_counts_and_rows() {
        let parsed = outcome(vec![GateVerdictRow {
            id: "RG-10".to_string(),
            title: "Gap closure".to_string(),
            criterion: "0 open class-b gaps".to_string(),
            evidence: GateEvidence::Gaps,
            verdict: GateVerdict::Fail,
            detail: "1 open class-b gap(s)".to_string(),
        }]);
        let json = format_gates_json(&parsed);
        let value: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(value["schema"], GATES_REPORT_SCHEMA);
        assert_eq!(value["gates"], 1);
        assert_eq!(value["fail"], 1);
        assert_eq!(value["pass"], 0);
        assert_eq!(value["entries"][0]["id"], "RG-10");
        assert_eq!(value["entries"][0]["verdict"], "fail");
        assert_eq!(value["entries"][0]["evidence"], "gaps");
    }

    #[test]
    fn renderers_are_byte_identical_on_repeat_runs() {
        let parsed = outcome(vec![GateVerdictRow {
            id: "RG-01".to_string(),
            title: "No ICE".to_string(),
            criterion: "0 ICEs".to_string(),
            evidence: GateEvidence::None,
            verdict: GateVerdict::Unmeasured,
            detail: "d".to_string(),
        }]);
        assert_eq!(format_gates_text(&parsed), format_gates_text(&parsed));
        assert_eq!(format_gates_json(&parsed), format_gates_json(&parsed));
    }
}
