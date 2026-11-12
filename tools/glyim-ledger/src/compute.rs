//! `ledger compute` — the M1.8 level-computation engine (plan §6 M1.8, §3.2).
//!
//! A pure function of `(registry, evidence, policy)`: for every `[[feature]]`
//! entry the engine computes the readiness level L0–L4 the recorded evidence
//! supports (L5 is governed by the policy's hardening thresholds and is
//! reported as *unmeasured* until a hardening evidence source exists), applies
//! the gap caps declared by every open `[[gap]]` that affects the feature, and
//! reports one [`FeatureLevel`] per feature plus sorted diagnostics.
//!
//! # Determinism contract (plan C1: status is computed, never typed)
//!
//! The engine reads no clock, no environment, no randomness: duplicate
//! evidence lines collapse, counting is set-based (distinct fixture hashes),
//! entries are sorted by feature ID, and diagnostics are sorted and
//! de-duplicated exactly like [`crate::run_check`]. Running [`compute_levels`]
//! twice on the same inputs yields identical bytes through both renderers —
//! pinned by the golden and shuffled-input tests in this module.
//!
//! # Monotonicity contract
//!
//! Evidence only ever raises a computed level (the ladder is a prefix: level N
//! requires every rule of levels L1..N to hold), and gaps only ever lower one
//! (a cap is a `min`). Both directions are property-tested over randomly
//! generated evidence sets.
//!
//! # Seams
//!
//! [`EvidenceSource`] decouples the engine from where `glyim.evidence/1`
//! records come from: [`JsonlEvidenceSource`] and [`ProjectEvidenceSource`]
//! adapt files today (the canonical writer is `glyim-test`'s harness, which
//! the ledger deliberately does not depend on), and future sources — CI-run
//! evidence stores, the M2 instruction counters — plug in without touching
//! the engine. The policy schema lives in `ledger-schema` ([`LevelPolicy`])
//! beside the other strict registry schemas.

use std::collections::{BTreeSet, HashSet};
use std::fmt;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use ledger_schema::policy::{EvidenceRule, LevelPolicy};
use ledger_schema::{
    Backend, CANONICAL_BACKENDS, CANONICAL_MODES, Diagnostic, FeatureEntry, GapEntry, Level,
    Registry, Severity, Sourced, Tier, load_project_registry,
};
use serde::{Deserialize, Serialize};

/// Schema tag of the compute output JSON contract.
pub const COMPUTE_SCHEMA: &str = "glyim.ledger-compute/1";

/// Schema tag the ledger accepts for evidence input — byte-identical to the
/// tag `glyim-test`'s harness writes (`EVIDENCE_SCHEMA`); a mismatched tag is
/// rejected instead of guessed at.
pub const EVIDENCE_WIRE_SCHEMA: &str = "glyim.evidence/1";

/// Unmeasured-hardening axis: fuzz CPU hours (plan M1.8 "CPU-hour thresholds";
/// no evidence kind records it yet).
pub const HARDENING_FUZZ_HOURS: &str = "fuzz_cpu_hours";

/// Unmeasured-hardening axis: mutation kill rate (the `glyim-pilot`
/// mutation-gate threshold; no evidence kind records it yet).
pub const HARDENING_MUTATION_RATE: &str = "mutation_kill_rate_pct";

// -- diagnostics -----------------------------------------------------------

/// An evidence file could not be read.
pub const CODE_EVIDENCE_IO: &str = "LEDGER-EVIDENCE-IO";
/// An evidence line is not a valid `glyim.evidence/1` record.
pub const CODE_EVIDENCE_INVALID: &str = "LEDGER-EVIDENCE-INVALID";
/// An evidence record carries a foreign `schema` tag.
pub const CODE_EVIDENCE_SCHEMA: &str = "LEDGER-EVIDENCE-SCHEMA";
/// An evidence record names a feature ID absent from the registry.
pub const CODE_EVIDENCE_UNKNOWN_FEATURE: &str = "LEDGER-EVIDENCE-UNKNOWN-FEATURE";

/// Policy diagnostics codes re-exported for callers that treat the compute
/// module as the engine's single façade (the codes are produced by
/// `ledger-schema`'s policy loader).
pub use ledger_schema::{CODE_POLICY_LOAD, CODE_POLICY_RULE, CODE_POLICY_SCHEMA};

fn diagnostic(code: &'static str, severity: Severity, message: String) -> Diagnostic {
    Diagnostic {
        code,
        message,
        severity,
    }
}

// -- evidence wire format --------------------------------------------------

/// The observed outcome of one fixture execution (ledger-side mirror of the
/// harness's `EvidenceOutcome`; serde spellings are the wire contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EvidenceOutcome {
    /// The fixture passed its strategy checks.
    Passed,
    /// The fixture failed with a reason class.
    Failed,
    /// The fixture was skipped (unmet `needs-llvm` / `only-target` / ignore).
    Ignored,
}

impl EvidenceOutcome {
    /// Canonical wire spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Ignored => "ignored",
        }
    }
}

impl fmt::Display for EvidenceOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One `glyim.evidence/1` record, read strictly on the ledger side.
///
/// Field-for-field mirror of `glyim_test::harness::evidence::EvidenceRecord`
/// (the canonical writer). The mirror keeps the ledger dependency-free of the
/// test-harness crate; the schema tag ([`EVIDENCE_WIRE_SCHEMA`]) and the
/// strict (`deny_unknown_fields`) parse pin the two in lockstep — when the
/// writer evolves the record (append-only, new schema tag), this reader must
/// be updated in the same change, exactly like any other consumer.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LedgerEvidence {
    /// Schema tag; must equal [`EVIDENCE_WIRE_SCHEMA`].
    pub schema: String,
    /// Fixture name as the harness reports it.
    pub name: String,
    /// Fixture path relative to the collection root.
    pub fixture: String,
    /// SHA-256 (hex) of the fixture source bytes.
    pub fixture_sha256: String,
    /// Revision tag the fixture ran under (empty = default).
    pub revision: String,
    /// Canonical mode name (`compile-pass` … `run-fail`).
    pub mode: String,
    /// Observed outcome.
    pub outcome: EvidenceOutcome,
    /// Stable short reason class for failures.
    pub reason: Option<String>,
    /// Backend the fixture actually ran on.
    pub backend: Backend,
    /// Optimisation level derived from the fixture's compile flags.
    pub opt: u8,
    /// Ledger feature IDs the fixture declares.
    pub features: Vec<String>,
    /// Backend IDs the fixture declares.
    pub declared_backends: Vec<String>,
    /// Reserved for M2: instruction count.
    pub instructions: Option<u64>,
    /// Reserved for M2: peak RSS in KiB.
    pub rss_kb: Option<u64>,
}

impl LedgerEvidence {
    /// Parses one JSONL line strictly (`deny_unknown_fields`).
    ///
    /// # Errors
    ///
    /// Returns the serde error text (which names the offending field) when
    /// the line is not a `glyim.evidence/1` record.
    pub fn from_jsonl_line(line: &str) -> Result<Self, String> {
        serde_json::from_str(line).map_err(|e| e.to_string())
    }

    /// Checks the invariants serde cannot express: hash format, canonical
    /// mode, and declared-backend membership. The `schema` tag is checked
    /// separately by the run entry point so a foreign tag gets its own
    /// diagnostic code.
    ///
    /// Returns human-readable issues (empty = valid); the compute run turns
    /// every issue into an error diagnostic and excludes the record from all
    /// predicates, so a schema bug upstream cannot silently poison levels.
    #[must_use]
    pub fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();
        let hex_ok = self.fixture_sha256.len() == 64
            && self
                .fixture_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !hex_ok {
            issues.push(format!(
                "fixture_sha256 must be 64 lowercase hex chars, got {:?}",
                self.fixture_sha256
            ));
        }
        if !CANONICAL_MODES.contains(&self.mode.as_str()) {
            issues.push(format!(
                "mode {:?} is outside the canonical set {CANONICAL_MODES:?}",
                self.mode
            ));
        }
        for backend in &self.declared_backends {
            if !CANONICAL_BACKENDS.contains(&backend.as_str()) {
                issues.push(format!(
                    "declared backend {backend:?} is outside the ledger set {CANONICAL_BACKENDS:?}"
                ));
            }
        }
        issues
    }
}

// -- evidence sources ------------------------------------------------------

/// What an [`EvidenceSource`] hands the engine: the loaded records (file
/// order; the engine is order-insensitive) plus one diagnostic per load or
/// parse failure. Failures are diagnostics, never panics, so a broken
/// evidence store degrades to L0 everywhere instead of killing the report.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EvidenceBatch {
    /// Loaded records (may be empty).
    pub records: Vec<LedgerEvidence>,
    /// Load/parse diagnostics (sorted like every other diagnostic stream by
    /// the run entry point).
    pub diagnostics: Vec<Diagnostic>,
}

/// Source of `glyim.evidence/1` records for [`compute_levels`].
///
/// The seam that keeps the engine independent of where evidence lives; the
/// file-based adapters below cover today's layouts, and future sources (CI
/// artifact stores, M2 instruction counters) implement this trait without
/// the engine changing shape.
pub trait EvidenceSource {
    /// Loads the records plus loading diagnostics.
    fn load(&self) -> EvidenceBatch;
}

/// Evidence from an explicit list of JSONL files (used by `--evidence PATH`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonlEvidenceSource {
    /// JSONL files to read, in order.
    pub paths: Vec<PathBuf>,
}

impl JsonlEvidenceSource {
    /// Reads exactly the given paths (duplicates collapse downstream).
    #[must_use]
    pub fn new(paths: Vec<PathBuf>) -> Self {
        Self { paths }
    }
}

impl EvidenceSource for JsonlEvidenceSource {
    fn load(&self) -> EvidenceBatch {
        let mut batch = EvidenceBatch::default();
        for path in &self.paths {
            let lines = match fs::read_to_string(path) {
                Ok(text) => text,
                Err(err) => {
                    batch.diagnostics.push(diagnostic(
                        CODE_EVIDENCE_IO,
                        Severity::Error,
                        format!("cannot read evidence file {}: {err}", path.display()),
                    ));
                    continue;
                }
            };
            for (index, line) in lines.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                let where_ = format!("{}:{}", path.display(), index + 1);
                match LedgerEvidence::from_jsonl_line(line) {
                    Ok(record) => batch.records.push(record),
                    Err(err) => batch.diagnostics.push(diagnostic(
                        CODE_EVIDENCE_INVALID,
                        Severity::Error,
                        format!("{where_}: {err}"),
                    )),
                }
            }
        }
        batch
    }
}

/// Evidence for a project run: the `--evidence PATH` override when given,
/// otherwise every `ledger/evidence/*.jsonl` file (sorted; an absent
/// directory is an empty batch, not an error — a registry with no recorded
/// evidence computes honest L0s).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectEvidenceSource {
    /// Project directory containing `ledger/`.
    pub root: PathBuf,
    /// Explicit override path (`--evidence`), if any.
    pub override_path: Option<PathBuf>,
}

impl ProjectEvidenceSource {
    /// Scans `root` with the given override.
    #[must_use]
    pub fn new(root: &Path, override_path: Option<&Path>) -> Self {
        Self {
            root: root.to_path_buf(),
            override_path: override_path.map(Path::to_path_buf),
        }
    }
}

impl EvidenceSource for ProjectEvidenceSource {
    fn load(&self) -> EvidenceBatch {
        let paths = match &self.override_path {
            Some(path) => vec![path.clone()],
            None => {
                let dir = self.root.join("ledger").join("evidence");
                let Ok(entries) = fs::read_dir(&dir) else {
                    // Absent evidence directory: no evidence recorded yet —
                    // an honest L0-everywhere run, not a misconfiguration.
                    return EvidenceBatch::default();
                };
                let mut paths: Vec<PathBuf> = entries
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
                    .collect();
                paths.sort();
                paths
            }
        };
        JsonlEvidenceSource::new(paths).load()
    }
}

/// Evidence from an in-memory list (tests and examples).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InMemoryEvidenceSource {
    /// The records to hand the engine.
    pub records: Vec<LedgerEvidence>,
}

impl InMemoryEvidenceSource {
    /// Wraps the given records.
    #[must_use]
    pub fn new(records: Vec<LedgerEvidence>) -> Self {
        Self { records }
    }
}

impl EvidenceSource for InMemoryEvidenceSource {
    fn load(&self) -> EvidenceBatch {
        EvidenceBatch {
            records: self.records.clone(),
            diagnostics: Vec::new(),
        }
    }
}

// -- compute output --------------------------------------------------------

/// Per-feature readiness computed from evidence (one JSON `entries` item).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FeatureLevel {
    /// Registry feature ID.
    pub id: String,
    /// Maturity tier (echoed for consumers sorting by tier).
    pub tier: Tier,
    /// The level the registry demands.
    pub target_level: Level,
    /// The level the evidence supports after caps.
    pub computed_level: Level,
    /// Gap IDs that capped this feature (sorted; empty when uncapped).
    pub capped_by: Vec<String>,
    /// Hardening axes with a policy threshold but no evidence source yet
    /// (`target_level == L5` only).
    pub unmeasured: Vec<&'static str>,
    /// Deterministic human reasons the level sits where it does, one per
    /// unmet rule, ordered by level then rule.
    pub missing: Vec<String>,
}

/// Result of a full `ledger compute` run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComputeOutcome {
    /// Per-feature entries, sorted by ID.
    pub entries: Vec<FeatureLevel>,
    /// Registry, policy, and evidence diagnostics, sorted and de-duplicated.
    pub diagnostics: Vec<Diagnostic>,
    /// Feature entries loaded.
    pub features: usize,
    /// Evidence records that entered the computation (after exact-duplicate
    /// collapse).
    pub records: usize,
}

impl ComputeOutcome {
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

// -- engine ----------------------------------------------------------------

/// Counts the distinct fixtures whose records satisfy `rule` for `feature`.
fn count_matches(
    evidence: &[&LedgerEvidence],
    feature: &FeatureEntry,
    rule: &EvidenceRule,
) -> usize {
    let matched: BTreeSet<&str> = evidence
        .iter()
        .filter(|record| {
            record.features.contains(&feature.id)
                && record.outcome.as_str() == rule.outcome
                && record.mode == rule.mode
                && rule.backend.is_none_or(|backend| record.backend == backend)
        })
        .map(|record| record.fixture_sha256.as_str())
        .collect();
    matched.len()
}

/// A policy rule applies to a feature when the rule pins no backend, or the
/// feature declares that backend (a bytecode-only feature is not asked for
/// LLVM evidence).
fn rule_applies(rule: &EvidenceRule, feature: &FeatureEntry) -> bool {
    rule.backend
        .is_none_or(|backend| feature.backends.contains(&backend))
}

/// Computes one feature's entry. Pure: everything derives from the slices.
fn compute_feature(
    feature: &Sourced<FeatureEntry>,
    gaps: &[Sourced<GapEntry>],
    policy: &LevelPolicy,
    evidence: &[&LedgerEvidence],
) -> FeatureLevel {
    let entry = &feature.entry;
    let mut computed = Level::L0;
    let mut missing = Vec::new();
    for level in [Level::L1, Level::L2, Level::L3, Level::L4] {
        let Some(rules) = policy.levels.get(&level) else {
            break;
        };
        let mut unmet = Vec::new();
        let mut satisfied = true;
        let mut applicable = false;
        for rule in rules {
            if !rule_applies(rule, entry) {
                continue;
            }
            applicable = true;
            let have = count_matches(evidence, entry, rule);
            if have >= rule.count {
                continue;
            }
            satisfied = false;
            let backend_note = rule
                .backend
                .map_or_else(String::new, |b| format!(" on {}", b.as_str()));
            unmet.push(format!(
                "{}: {} {} {}{} required, {have} distinct seen",
                level.as_str(),
                rule.count,
                rule.outcome,
                rule.mode,
                backend_note,
            ));
        }
        // A level whose rules are all backend-gated away (e.g. the L4
        // llvm rule for a gam-only feature) cannot be *evidenced* — the
        // ladder honestly stops below it instead of advancing vacuously.
        if satisfied && applicable {
            computed = level;
        } else {
            missing.extend(unmet);
            break;
        }
    }
    let mut unmeasured = Vec::new();
    if entry.target_level == Level::L5 {
        unmeasured.push(HARDENING_FUZZ_HOURS);
        unmeasured.push(HARDENING_MUTATION_RATE);
        if computed > Level::L4 {
            computed = Level::L4;
        }
    }
    let mut capped_by = Vec::new();
    if policy.gap_caps_enabled {
        for gap in gaps {
            if !gap.entry.affects.contains(&entry.id) {
                continue;
            }
            capped_by.push(gap.id().to_string());
            if gap.entry.caps < computed {
                computed = gap.entry.caps;
            }
        }
    }
    capped_by.sort();
    capped_by.dedup();
    FeatureLevel {
        id: entry.id.clone(),
        tier: entry.tier,
        target_level: entry.target_level,
        computed_level: computed,
        capped_by,
        unmeasured,
        missing,
    }
}

/// Deduplicates identical records (the harness may append across runs);
/// conflicting records (same fixture, different outcome) both stay, so the
/// `passed` side still counts toward its rule.
fn dedup_records(evidence: &[LedgerEvidence]) -> Vec<&LedgerEvidence> {
    let mut seen: HashSet<&LedgerEvidence> = HashSet::with_capacity(evidence.len());
    evidence
        .iter()
        .filter(|record| seen.insert(record))
        .collect()
}

/// Computes readiness levels for every feature in `registry` from `evidence`
/// under `policy`.
///
/// Pure and order-insensitive: the evidence order never changes the output
/// (identical records collapse, counting is set-based), entries are sorted by
/// feature ID, and running twice yields identical values (pinned by tests).
/// Records are trusted here — wire/invariant validation is the run entry
/// point's job ([`run_compute`]), so the engine stays a pure function.
#[must_use]
pub fn compute_levels(
    registry: &Registry,
    policy: &LevelPolicy,
    evidence: &[LedgerEvidence],
) -> Vec<FeatureLevel> {
    let unique = dedup_records(evidence);
    let mut entries: Vec<FeatureLevel> = registry
        .features()
        .iter()
        .map(|feature| compute_feature(feature, registry.gaps(), policy, &unique))
        .collect();
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    entries
}

/// Runs a full `ledger compute` pass over the project at `root` (the
/// directory that contains `ledger/`): loads and validates the registry,
/// loads `ledger/policy.toml`, loads evidence (`--evidence` override or
/// `ledger/evidence/*.jsonl`), cross-checks the evidence against the
/// registry, and computes the per-feature levels.
///
/// Every failure — unreadable files, invalid records, unknown feature IDs,
/// a missing or invalid policy — becomes a sorted error diagnostic; the
/// exit-code contract lives in `main.rs` like every other subcommand.
#[must_use]
pub fn run_compute(root: &Path, evidence: Option<&Path>) -> ComputeOutcome {
    let span = tracing::info_span!("compute", root = %root.display());
    let _enter = span.enter();

    let loaded = load_project_registry(root);
    let mut diagnostics = loaded.diagnostics;
    diagnostics.extend(loaded.registry.validate());

    let batch = ProjectEvidenceSource::new(root, evidence).load();
    diagnostics.extend(batch.diagnostics);

    // Evidence triage: identical records collapse first, then every
    // remaining record is schema- and invariant-checked; invalid records
    // are excluded from all predicates.
    let mut valid: Vec<LedgerEvidence> = Vec::with_capacity(batch.records.len());
    for record in dedup_records(&batch.records).into_iter().cloned() {
        if record.schema != EVIDENCE_WIRE_SCHEMA {
            diagnostics.push(diagnostic(
                CODE_EVIDENCE_SCHEMA,
                Severity::Error,
                format!(
                    "evidence record {} carries schema {:?}, expected {EVIDENCE_WIRE_SCHEMA:?}",
                    record.name, record.schema
                ),
            ));
            continue;
        }
        let issues = record.validate();
        if issues.is_empty() {
            valid.push(record);
            continue;
        }
        for issue in issues {
            diagnostics.push(diagnostic(
                CODE_EVIDENCE_INVALID,
                Severity::Error,
                format!("evidence record {} is invalid: {issue}", record.name),
            ));
        }
    }

    // Unknown feature IDs in evidence are misconfigurations (a renamed or
    // deleted registry entry still claimed by fixtures), reported even when
    // the policy failed to load so the report is actionable in one pass.
    let mut known_ids: BTreeSet<&str> = BTreeSet::new();
    for record in &valid {
        for id in &record.features {
            if loaded.registry.feature_by_id(id).is_none() && known_ids.insert(id) {
                diagnostics.push(diagnostic(
                    CODE_EVIDENCE_UNKNOWN_FEATURE,
                    Severity::Error,
                    format!(
                        "evidence record {} names unknown feature ID {id:?} (fixture {:?})",
                        record.name, record.fixture
                    ),
                ));
            }
        }
    }

    let (policy, policy_diagnostics) = ledger_schema::load_policy(root);
    diagnostics.extend(policy_diagnostics);

    let entries = match &policy {
        Some(policy) => compute_levels(&loaded.registry, policy, &valid),
        None => Vec::new(),
    };

    tracing::debug!(
        features = entries.len(),
        records = valid.len(),
        diagnostics = diagnostics.len(),
        "compute pass finished"
    );

    diagnostics.sort_by(|a, b| a.code.cmp(b.code).then(a.message.cmp(&b.message)));
    diagnostics.dedup();
    ComputeOutcome {
        entries,
        diagnostics,
        features: loaded.registry.feature_count(),
        records: valid.len(),
    }
}

// -- renderers -------------------------------------------------------------

/// Renders the text report (stdout form of `ledger compute`).
#[must_use]
pub fn format_compute_report(outcome: &ComputeOutcome) -> String {
    let mut lines = vec![format!(
        "compute: {} features, {} evidence record(s), {} diagnostic(s)",
        outcome.features,
        outcome.records,
        outcome.diagnostics.len(),
    )];
    for entry in &outcome.entries {
        let mut line = format!(
            "  {:<32} {} / {}",
            entry.id,
            entry.computed_level.as_str(),
            entry.target_level.as_str(),
        );
        if !entry.capped_by.is_empty() {
            let _ = write!(line, "  capped by {}", entry.capped_by.join(", "));
        }
        if !entry.unmeasured.is_empty() {
            let _ = write!(line, "  unmeasured: {}", entry.unmeasured.join(", "));
        }
        lines.push(line);
        for reason in &entry.missing {
            lines.push(format!("      missing: {reason}"));
        }
    }
    if outcome.is_ok() {
        lines.push(format!(
            "ledger compute OK: {} features, {} records, {} error(s), {} warning(s)",
            outcome.features,
            outcome.records,
            outcome.errors(),
            outcome.warnings(),
        ));
    } else {
        lines.push(format!(
            "ledger compute FAILED: {} error(s), {} warning(s)",
            outcome.errors(),
            outcome.warnings(),
        ));
        lines.push(format!(
            "registry: {} features, {} records",
            outcome.features, outcome.records,
        ));
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// JSON document rendered by [`format_compute_json`].
#[derive(Serialize)]
struct ComputeJson<'a> {
    schema: &'static str,
    features: usize,
    records: usize,
    entries: &'a [FeatureLevel],
}

/// Renders the `glyim.ledger-compute/1` JSON contract (pretty-printed).
#[must_use]
pub fn format_compute_json(outcome: &ComputeOutcome) -> String {
    let document = ComputeJson {
        schema: COMPUTE_SCHEMA,
        features: outcome.features,
        records: outcome.records,
        entries: &outcome.entries,
    };
    let mut text = serde_json::to_string_pretty(&document).unwrap_or_else(|err| {
        // Field sets are fixed and values are plain strings/numbers, so
        // serialization is infallible by construction; this arm keeps the
        // renderer total without introducing a Result into the format_*
        // family contract (same shape as `format_next_json`).
        format!("{{\"schema\":\"{COMPUTE_SCHEMA}\",\"error\":\"{err}\"}}")
    });
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The seed policy (mirrors `ledger/policy.toml`); one literal the
    /// project-seeding helper writes and golden tests pin.
    const SEED_POLICY: &str = r#"schema = "glyim.ledger-policy/1"

# Per-level evidence rules: a rule is a minimum count of distinct fixtures
# whose `glyim.evidence/1` records match the mode/outcome/backend triple.
[levels.L1]
rules = [{ mode = "compile-pass", outcome = "passed", count = 1 }]

[levels.L2]
rules = [
    { mode = "compile-pass", outcome = "passed", count = 1 },
    { mode = "compile-fail", outcome = "passed", count = 1 },
]

[levels.L3]
rules = [{ mode = "run-pass", outcome = "passed", backend = "gam", count = 1 }]

[levels.L4]
rules = [{ mode = "run-pass", outcome = "passed", backend = "llvm", count = 1 }]

[gap_caps]
enabled = true

[hardening]
fuzz_cpu_hours = 24
mutation_kill_rate_pct = 75
"#;

    // -- helpers ------------------------------------------------------------

    /// A distinct-looking fixture hash (64 lowercase hex chars).
    fn sha(tag: usize) -> String {
        let digits = tag.to_string();
        let body = "0".repeat(64 - digits.len() - 1);
        format!("a{body}{digits}")
    }

    /// Builds one evidence record with the given shape.
    fn rec(
        name: &str,
        fixture_sha: String,
        mode: &str,
        outcome: EvidenceOutcome,
        backend: Backend,
        features: &[&str],
    ) -> LedgerEvidence {
        LedgerEvidence {
            schema: EVIDENCE_WIRE_SCHEMA.to_string(),
            name: name.to_string(),
            fixture: format!("tests/fixtures/{name}.g"),
            fixture_sha256: fixture_sha,
            revision: String::new(),
            mode: mode.to_string(),
            outcome,
            reason: None,
            backend,
            opt: 0,
            features: features.iter().map(|id| (*id).to_string()).collect(),
            declared_backends: vec![backend.as_str().to_string()],
            instructions: None,
            rss_kb: None,
        }
    }

    /// Feature/gap seeds for the project-seeding helper.
    struct FeatureSeed {
        id: &'static str,
        tier: &'static str,
        target: &'static str,
        backends: &'static str,
    }

    struct GapSeed {
        id: &'static str,
        affects: &'static str,
        caps: &'static str,
    }

    /// Writes a project-shaped directory: one feature file, one gap file,
    /// the spec anchor, and the policy when `policy` is `Some`.
    fn seed_project(
        features: &[FeatureSeed],
        gaps: &[GapSeed],
        policy: Option<&str>,
    ) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::create_dir_all(dir.path().join("docs/specs")).expect("mkdir specs");
        std::fs::create_dir_all(dir.path().join("ledger/features")).expect("mkdir features");
        std::fs::create_dir_all(dir.path().join("ledger/gaps")).expect("mkdir gaps");
        std::fs::write(dir.path().join("docs/specs/v0.1.0.md"), "# spec fixture\n")
            .expect("write spec");
        let mut feature_toml = String::new();
        for f in features {
            let _ = write!(
                feature_toml,
                "[[feature]]\n\
                 id = \"{}\"\n\
                 title = \"seed feature {}\"\n\
                 area = \"lang.control-flow\"\n\
                 tier = \"{}\"\n\
                 spec = \"docs/specs/v0.1.0.md\"\n\
                 target_level = \"{}\"\n\
                 backends = [{}]\n\
                 owner_lane = \"F\"\n\n",
                f.id,
                f.id,
                f.tier,
                f.target,
                f.backends
                    .split(',')
                    .map(|b| format!("\"{}\"", b.trim()))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
        std::fs::write(dir.path().join("ledger/features/seed.toml"), feature_toml)
            .expect("write features");
        if !gaps.is_empty() {
            // An empty gap file would trip LEDGER-EMPTY-FILE; only write
            // the file when the seed actually declares gaps.
            let mut gap_toml = String::new();
            for g in gaps {
                let _ = write!(
                    gap_toml,
                    "[[gap]]\n\
                     id = \"{}\"\n\
                     class = \"b\"\n\
                     title = \"seed gap {}\"\n\
                     affects = [\"{}\"]\n\
                     caps = \"{}\"\n\
                     opened_in = \"v0.3.0\"\n\
                     status = \"unwitnessed\"\n\n",
                    g.id, g.id, g.affects, g.caps,
                );
            }
            std::fs::write(dir.path().join("ledger/gaps/seed.toml"), gap_toml).expect("write gaps");
        }
        if let Some(policy) = policy {
            std::fs::write(dir.path().join("ledger/policy.toml"), policy).expect("write policy");
        }
        dir
    }

    fn registry_of(dir: &tempfile::TempDir) -> Registry {
        let loaded = load_project_registry(dir.path());
        assert!(
            loaded.diagnostics.is_empty(),
            "seed registry must parse cleanly: {:?}",
            loaded.diagnostics
        );
        loaded.registry
    }

    fn policy_of(dir: &tempfile::TempDir) -> LevelPolicy {
        let (policy, diagnostics) = ledger_schema::load_policy(dir.path());
        assert!(
            diagnostics.is_empty(),
            "seed policy must load cleanly: {diagnostics:?}"
        );
        policy.expect("policy loaded")
    }

    fn entry<'a>(entries: &'a [FeatureLevel], id: &str) -> &'a FeatureLevel {
        entries
            .iter()
            .find(|e| e.id == id)
            .unwrap_or_else(|| panic!("entry {id} missing"))
    }

    /// Deterministic xorshift64 PRNG for the monotonicity property test.
    struct XorShift(u64);

    impl XorShift {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
    }

    // -- wire format --------------------------------------------------------

    #[test]
    fn evidence_line_parses_field_for_field() {
        // Shape mirrors a real `glyim.evidence/1` line as the harness
        // writes it (serde_json field order is declaration order).
        let line = format!(
            r#"{{"schema":"{EVIDENCE_WIRE_SCHEMA}","name":"for_loop_over_slice","fixture":"behavior/for_loop_over_slice.g","fixture_sha256":"{}","revision":"","mode":"run-pass","outcome":"passed","reason":null,"backend":"gam","opt":2,"features":["LANG-FOR-SLICE"],"declared_backends":["gam"],"instructions":null,"rss_kb":null}}"#,
            "a".repeat(64)
        );
        let record = LedgerEvidence::from_jsonl_line(&line).expect("parse");
        assert_eq!(record.schema, EVIDENCE_WIRE_SCHEMA);
        assert_eq!(record.mode, "run-pass");
        assert_eq!(record.outcome, EvidenceOutcome::Passed);
        assert_eq!(record.backend, Backend::Gam);
        assert_eq!(record.opt, 2);
        assert_eq!(record.features, vec!["LANG-FOR-SLICE".to_string()]);
        assert!(record.validate().is_empty());
    }

    #[test]
    fn evidence_rejects_unknown_fields_and_bad_json() {
        let junk = r#"{"schema":"glyim.evidence/1","name":"x","fixture":"f","fixture_sha256":"00","wall_ms":42}"#;
        assert!(LedgerEvidence::from_jsonl_line(junk).is_err());
        assert!(LedgerEvidence::from_jsonl_line("{").is_err());
        // A *complete* record with a foreign tag parses structurally (the
        // tag is a plain string field); the run entry point rejects it by
        // diagnostic code, not by parse failure.
        let mut foreign = rec(
            "x",
            sha(1),
            "run-pass",
            EvidenceOutcome::Passed,
            Backend::Gam,
            &["A"],
        );
        foreign.schema = "glyim.evidence/2".to_string();
        assert!(
            LedgerEvidence::from_jsonl_line(&serde_json::to_string(&foreign).expect("serialize"))
                .is_ok()
        );
    }

    #[test]
    fn evidence_validate_flags_hash_mode_and_declared_backend() {
        let mut record = rec(
            "r",
            sha(1),
            "run-pass",
            EvidenceOutcome::Passed,
            Backend::Gam,
            &["A"],
        );
        record.fixture_sha256 = "00".to_string();
        record.mode = "parse-pass".to_string();
        record.declared_backends = vec!["wasm".to_string()];
        let issues = record.validate();
        assert_eq!(issues.len(), 3, "{issues:?}");
    }

    // -- engine: level ladder ------------------------------------------------

    fn ladder_project() -> tempfile::TempDir {
        seed_project(
            &[FeatureSeed {
                id: "LANG-LADDER-A",
                tier: "core",
                target: "L5",
                backends: "gam, llvm",
            }],
            &[],
            Some(SEED_POLICY),
        )
    }

    #[test]
    fn ladder_advances_only_with_matching_evidence() {
        let dir = ladder_project();
        let registry = registry_of(&dir);
        let policy = policy_of(&dir);
        let records = [
            rec(
                "cp",
                sha(1),
                "compile-pass",
                EvidenceOutcome::Passed,
                Backend::Gam,
                &["LANG-LADDER-A"],
            ),
            rec(
                "cf",
                sha(2),
                "compile-fail",
                EvidenceOutcome::Passed,
                Backend::Gam,
                &["LANG-LADDER-A"],
            ),
            rec(
                "rp-gam",
                sha(3),
                "run-pass",
                EvidenceOutcome::Passed,
                Backend::Gam,
                &["LANG-LADDER-A"],
            ),
            rec(
                "rp-llvm",
                sha(4),
                "run-pass",
                EvidenceOutcome::Passed,
                Backend::Llvm,
                &["LANG-LADDER-A"],
            ),
        ];
        let computed = |evidence: &[LedgerEvidence]| {
            entry(
                &compute_levels(&registry, &policy, evidence),
                "LANG-LADDER-A",
            )
            .computed_level
        };
        assert_eq!(computed(&[]), Level::L0);
        assert_eq!(computed(&records[..1]), Level::L1);
        assert_eq!(computed(&records[..2]), Level::L2);
        assert_eq!(computed(&records[..3]), Level::L3);
        assert_eq!(computed(&records), Level::L4);
    }

    #[test]
    fn ladder_stops_at_first_unmet_level_and_reports_the_reason() {
        let dir = ladder_project();
        let registry = registry_of(&dir);
        let policy = policy_of(&dir);
        // L1 and L3/L4 evidence, but no compile-fail (L2): the ladder is a
        // prefix, so the feature sits at L1 and the reason names L2.
        let records = [
            rec(
                "cp",
                sha(1),
                "compile-pass",
                EvidenceOutcome::Passed,
                Backend::Gam,
                &["LANG-LADDER-A"],
            ),
            rec(
                "rp-gam",
                sha(3),
                "run-pass",
                EvidenceOutcome::Passed,
                Backend::Gam,
                &["LANG-LADDER-A"],
            ),
            rec(
                "rp-llvm",
                sha(4),
                "run-pass",
                EvidenceOutcome::Passed,
                Backend::Llvm,
                &["LANG-LADDER-A"],
            ),
        ];
        let levels = compute_levels(&registry, &policy, &records);
        let level = entry(&levels, "LANG-LADDER-A");
        assert_eq!(level.computed_level, Level::L1);
        assert_eq!(level.missing.len(), 1);
        assert!(
            level.missing[0].starts_with("L2: 1 passed compile-fail required, 0 distinct seen"),
            "{}",
            level.missing[0]
        );
    }

    #[test]
    fn backend_rule_is_skipped_for_features_without_that_backend() {
        let dir = seed_project(
            &[FeatureSeed {
                id: "LANG-LADDER-GAM",
                tier: "tooling",
                target: "L3",
                backends: "gam",
            }],
            &[],
            Some(SEED_POLICY),
        );
        let registry = registry_of(&dir);
        let policy = policy_of(&dir);
        let records = [
            rec(
                "cp",
                sha(1),
                "compile-pass",
                EvidenceOutcome::Passed,
                Backend::Gam,
                &["LANG-LADDER-GAM"],
            ),
            rec(
                "cf",
                sha(2),
                "compile-fail",
                EvidenceOutcome::Passed,
                Backend::Gam,
                &["LANG-LADDER-GAM"],
            ),
            rec(
                "rp-gam",
                sha(3),
                "run-pass",
                EvidenceOutcome::Passed,
                Backend::Gam,
                &["LANG-LADDER-GAM"],
            ),
        ];
        let levels = compute_levels(&registry, &policy, &records);
        let level = entry(&levels, "LANG-LADDER-GAM");
        // The L4 rule pins `llvm`, which this feature does not declare —
        // the rule is vacuous, so nothing blocks the gam-only target L3.
        assert_eq!(level.computed_level, Level::L3);
        assert!(level.missing.is_empty());
    }

    #[test]
    fn duplicate_evidence_counts_once_but_conflicts_keep_both_sides() {
        let dir = ladder_project();
        let registry = registry_of(&dir);
        let policy = policy_of(&dir);
        let cp = rec(
            "cp",
            sha(1),
            "compile-pass",
            EvidenceOutcome::Passed,
            Backend::Gam,
            &["LANG-LADDER-A"],
        );
        // Same content, different fixture hash: two distinct fixtures.
        let cp2 = rec(
            "cp-again",
            sha(2),
            "compile-pass",
            EvidenceOutcome::Passed,
            Backend::Gam,
            &["LANG-LADDER-A"],
        );
        // The failing twin of the same fixture content: a conflict that
        // must not lower the passed side's contribution.
        let failed_twin = rec(
            "cp",
            sha(1),
            "compile-pass",
            EvidenceOutcome::Failed,
            Backend::Gam,
            &["LANG-LADDER-A"],
        );

        let once = compute_levels(&registry, &policy, &[cp.clone(), cp.clone()]);
        assert_eq!(once[0].computed_level, Level::L1);
        assert_eq!(once[0].missing.len(), 1); // L2 unmet: only 1 distinct fixture

        let twice = compute_levels(&registry, &policy, &[cp.clone(), cp2.clone()]);
        assert_eq!(twice[0].computed_level, Level::L1);

        let with_conflict = compute_levels(&registry, &policy, &[cp2, failed_twin]);
        assert_eq!(with_conflict[0].computed_level, Level::L1);
    }

    // -- engine: caps and hardening ------------------------------------------

    #[test]
    fn gap_cap_lowers_level_and_records_the_capper() {
        let dir = seed_project(
            &[FeatureSeed {
                id: "LANG-CAPPED-A",
                tier: "core",
                target: "L5",
                backends: "gam, llvm",
            }],
            &[GapSeed {
                id: "GAP-0009",
                affects: "LANG-CAPPED-A",
                caps: "L2",
            }],
            Some(SEED_POLICY),
        );
        let registry = registry_of(&dir);
        let policy = policy_of(&dir);
        let records = [
            rec(
                "cp",
                sha(1),
                "compile-pass",
                EvidenceOutcome::Passed,
                Backend::Gam,
                &["LANG-CAPPED-A"],
            ),
            rec(
                "cf",
                sha(2),
                "compile-fail",
                EvidenceOutcome::Passed,
                Backend::Gam,
                &["LANG-CAPPED-A"],
            ),
            rec(
                "rp-gam",
                sha(3),
                "run-pass",
                EvidenceOutcome::Passed,
                Backend::Gam,
                &["LANG-CAPPED-A"],
            ),
            rec(
                "rp-llvm",
                sha(4),
                "run-pass",
                EvidenceOutcome::Passed,
                Backend::Llvm,
                &["LANG-CAPPED-A"],
            ),
        ];
        let levels = compute_levels(&registry, &policy, &records);
        let level = entry(&levels, "LANG-CAPPED-A");
        assert_eq!(level.computed_level, Level::L2);
        assert_eq!(level.capped_by, vec!["GAP-0009".to_string()]);
    }

    #[test]
    fn gap_caps_disabled_policy_skips_capping() {
        let dir = seed_project(
            &[FeatureSeed {
                id: "LANG-CAPPED-A",
                tier: "core",
                target: "L5",
                backends: "gam, llvm",
            }],
            &[GapSeed {
                id: "GAP-0009",
                affects: "LANG-CAPPED-A",
                caps: "L2",
            }],
            Some(&SEED_POLICY.replace("[gap_caps]\nenabled = true", "[gap_caps]\nenabled = false")),
        );
        let registry = registry_of(&dir);
        let policy = policy_of(&dir);
        let records = [
            rec(
                "cp",
                sha(1),
                "compile-pass",
                EvidenceOutcome::Passed,
                Backend::Gam,
                &["LANG-CAPPED-A"],
            ),
            rec(
                "cf",
                sha(2),
                "compile-fail",
                EvidenceOutcome::Passed,
                Backend::Gam,
                &["LANG-CAPPED-A"],
            ),
            rec(
                "rp-gam",
                sha(3),
                "run-pass",
                EvidenceOutcome::Passed,
                Backend::Gam,
                &["LANG-CAPPED-A"],
            ),
            rec(
                "rp-llvm",
                sha(4),
                "run-pass",
                EvidenceOutcome::Passed,
                Backend::Llvm,
                &["LANG-CAPPED-A"],
            ),
        ];
        let levels = compute_levels(&registry, &policy, &records);
        let level = entry(&levels, "LANG-CAPPED-A");
        assert_eq!(level.computed_level, Level::L4);
        assert!(level.capped_by.is_empty());
    }

    #[test]
    fn l5_target_reports_unmeasured_hardening_axes() {
        let dir = ladder_project();
        let registry = registry_of(&dir);
        let policy = policy_of(&dir);
        let records = [
            rec(
                "cp",
                sha(1),
                "compile-pass",
                EvidenceOutcome::Passed,
                Backend::Gam,
                &["LANG-LADDER-A"],
            ),
            rec(
                "cf",
                sha(2),
                "compile-fail",
                EvidenceOutcome::Passed,
                Backend::Gam,
                &["LANG-LADDER-A"],
            ),
            rec(
                "rp-gam",
                sha(3),
                "run-pass",
                EvidenceOutcome::Passed,
                Backend::Gam,
                &["LANG-LADDER-A"],
            ),
            rec(
                "rp-llvm",
                sha(4),
                "run-pass",
                EvidenceOutcome::Passed,
                Backend::Llvm,
                &["LANG-LADDER-A"],
            ),
        ];
        let levels = compute_levels(&registry, &policy, &records);
        let level = entry(&levels, "LANG-LADDER-A");
        // All L4 evidence is in; L5 stays unreachable until a hardening
        // evidence source exists, and the axes are named, not hidden.
        assert_eq!(level.computed_level, Level::L4);
        assert_eq!(level.target_level, Level::L5);
        assert_eq!(
            level.unmeasured,
            vec![HARDENING_FUZZ_HOURS, HARDENING_MUTATION_RATE]
        );
    }

    // -- engine: determinism and monotonicity ---------------------------------

    #[test]
    fn compute_is_order_insensitive_and_deduplicates() {
        let dir = ladder_project();
        let registry = registry_of(&dir);
        let policy = policy_of(&dir);
        let mut records = vec![
            rec(
                "rp-llvm",
                sha(4),
                "run-pass",
                EvidenceOutcome::Passed,
                Backend::Llvm,
                &["LANG-LADDER-A"],
            ),
            rec(
                "cp",
                sha(1),
                "compile-pass",
                EvidenceOutcome::Passed,
                Backend::Gam,
                &["LANG-LADDER-A"],
            ),
            rec(
                "cp",
                sha(1),
                "compile-pass",
                EvidenceOutcome::Passed,
                Backend::Gam,
                &["LANG-LADDER-A"],
            ),
            rec(
                "cf",
                sha(2),
                "compile-fail",
                EvidenceOutcome::Passed,
                Backend::Gam,
                &["LANG-LADDER-A"],
            ),
            rec(
                "rp-gam",
                sha(3),
                "run-pass",
                EvidenceOutcome::Passed,
                Backend::Gam,
                &["LANG-LADDER-A"],
            ),
        ];
        let sorted = compute_levels(&registry, &policy, &records);
        records.reverse();
        let reversed = compute_levels(&registry, &policy, &records);
        assert_eq!(sorted, reversed);
        assert_eq!(sorted[0].computed_level, Level::L4);
    }

    #[test]
    fn computed_levels_never_decrease_under_growing_evidence() {
        let dir = seed_project(
            &[
                FeatureSeed {
                    id: "LANG-PROP-A",
                    tier: "core",
                    target: "L5",
                    backends: "gam, llvm",
                },
                FeatureSeed {
                    id: "LANG-PROP-B",
                    tier: "tooling",
                    target: "L3",
                    backends: "gam",
                },
                FeatureSeed {
                    id: "LANG-PROP-C",
                    tier: "std",
                    target: "L4",
                    backends: "gam, llvm",
                },
            ],
            &[GapSeed {
                id: "GAP-0001",
                affects: "LANG-PROP-C",
                caps: "L2",
            }],
            Some(SEED_POLICY),
        );
        let registry = registry_of(&dir);
        let policy = policy_of(&dir);
        let features = [
            "LANG-PROP-A",
            "LANG-PROP-B",
            "LANG-PROP-C",
            "LANG-PROP-GHOST",
        ];
        let modes = ["compile-pass", "compile-fail", "ui", "run-pass", "run-fail"];
        let backends = [
            Backend::Gam,
            Backend::Llvm,
            Backend::Bytecode,
            Backend::Cranelift,
        ];
        let outcomes = [
            EvidenceOutcome::Passed,
            EvidenceOutcome::Failed,
            EvidenceOutcome::Ignored,
        ];
        let mut rng = XorShift(0x2545_F491_4F6C_DD1D);
        let mut evidence: Vec<LedgerEvidence> = Vec::new();
        let mut previous: Vec<FeatureLevel> = compute_levels(&registry, &policy, &evidence);
        for step in 0..96u64 {
            let name = format!("fixture-{step}");
            let record = rec(
                &name,
                sha(usize::try_from(step + 1).expect("small step")),
                modes[(rng.next() % modes.len() as u64) as usize],
                outcomes[(rng.next() % outcomes.len() as u64) as usize],
                backends[(rng.next() % backends.len() as u64) as usize],
                &[features[(rng.next() % features.len() as u64) as usize]],
            );
            evidence.push(record);
            let current = compute_levels(&registry, &policy, &evidence);
            for (before, after) in previous.iter().zip(current.iter()) {
                assert_eq!(before.id, after.id);
                assert!(
                    after.computed_level >= before.computed_level,
                    "evidence growth lowered {} from {} to {}",
                    after.id,
                    before.computed_level,
                    after.computed_level
                );
            }
            previous = current;
        }
    }

    // -- run entry point (end-to-end) ------------------------------------------

    #[test]
    fn run_compute_end_to_end_green() {
        let dir = ladder_project();
        let evidence = dir.path().join("ledger/evidence/run.jsonl");
        std::fs::create_dir_all(evidence.parent().expect("parent")).expect("mkdir evidence");
        let records = [
            rec(
                "cp",
                sha(1),
                "compile-pass",
                EvidenceOutcome::Passed,
                Backend::Gam,
                &["LANG-LADDER-A"],
            ),
            rec(
                "cf",
                sha(2),
                "compile-fail",
                EvidenceOutcome::Passed,
                Backend::Gam,
                &["LANG-LADDER-A"],
            ),
            rec(
                "rp-gam",
                sha(3),
                "run-pass",
                EvidenceOutcome::Passed,
                Backend::Gam,
                &["LANG-LADDER-A"],
            ),
            rec(
                "rp-llvm",
                sha(4),
                "run-pass",
                EvidenceOutcome::Passed,
                Backend::Llvm,
                &["LANG-LADDER-A"],
            ),
        ];
        let mut text = String::new();
        for record in &records {
            text.push_str(&serde_json::to_string(record).expect("serialize"));
            text.push('\n');
        }
        std::fs::write(&evidence, text).expect("write evidence");
        let outcome = run_compute(dir.path(), None);
        assert!(outcome.is_ok(), "{:?}", outcome.diagnostics);
        assert_eq!(outcome.features, 1);
        assert_eq!(outcome.records, 4);
        assert_eq!(outcome.entries.len(), 1);
        assert_eq!(outcome.entries[0].computed_level, Level::L4);

        let json = format_compute_json(&outcome);
        assert!(
            json.contains("\"schema\": \"glyim.ledger-compute/1\""),
            "{json}"
        );
        assert!(json.contains("\"computed_level\": \"L4\""), "{json}");
        let report = format_compute_report(&outcome);
        assert!(report.contains("ledger compute OK"), "{report}");
        // Determinism: the same run twice renders identical bytes.
        assert_eq!(run_compute(dir.path(), None), outcome);
    }

    #[test]
    fn run_compute_reports_unknown_feature_and_invalid_records() {
        let dir = ladder_project();
        let evidence = dir.path().join("ledger/evidence/run.jsonl");
        std::fs::create_dir_all(evidence.parent().expect("parent")).expect("mkdir evidence");
        let good = rec(
            "cp",
            sha(1),
            "compile-pass",
            EvidenceOutcome::Passed,
            Backend::Gam,
            &["LANG-LADDER-A"],
        );
        let ghost = rec(
            "ghost",
            sha(2),
            "compile-pass",
            EvidenceOutcome::Passed,
            Backend::Gam,
            &["LANG-GHOST"],
        );
        let mut bad_sha = rec(
            "badsha",
            sha(3),
            "compile-pass",
            EvidenceOutcome::Passed,
            Backend::Gam,
            &["LANG-LADDER-A"],
        );
        bad_sha.fixture_sha256 = "xyz".to_string();
        let mut text = String::new();
        for record in [&good, &ghost, &bad_sha] {
            text.push_str(&serde_json::to_string(record).expect("serialize"));
            text.push('\n');
        }
        std::fs::write(&evidence, text).expect("write evidence");
        let outcome = run_compute(dir.path(), None);
        assert!(!outcome.is_ok());
        let codes: Vec<&str> = outcome.diagnostics.iter().map(|d| d.code).collect();
        assert!(codes.contains(&CODE_EVIDENCE_UNKNOWN_FEATURE), "{codes:?}");
        assert!(codes.contains(&CODE_EVIDENCE_INVALID), "{codes:?}");
        // The invalid record is excluded; the ghost record stays (it is
        // well-formed, it just names an unregistered feature) — so two
        // records survive triage, and only the good one feeds the feature.
        assert_eq!(outcome.records, 2);
        assert_eq!(outcome.entries[0].computed_level, Level::L1);
    }

    #[test]
    fn run_compute_rejects_foreign_schema_tag() {
        let dir = ladder_project();
        let evidence = dir.path().join("ledger/evidence/run.jsonl");
        std::fs::create_dir_all(evidence.parent().expect("parent")).expect("mkdir evidence");
        let mut foreign = rec(
            "cp",
            sha(1),
            "compile-pass",
            EvidenceOutcome::Passed,
            Backend::Gam,
            &["LANG-LADDER-A"],
        );
        foreign.schema = "glyim.evidence/2".to_string();
        std::fs::write(
            &evidence,
            serde_json::to_string(&foreign).expect("serialize"),
        )
        .expect("write evidence");
        let outcome = run_compute(dir.path(), None);
        assert!(!outcome.is_ok());
        assert!(
            matches!(
                outcome.diagnostics.first().map(|d| d.code),
                Some(CODE_EVIDENCE_SCHEMA)
            ),
            "{:?}",
            outcome.diagnostics
        );
        assert_eq!(outcome.records, 0);
    }

    #[test]
    fn run_compute_missing_policy_fails_with_no_entries() {
        let dir = seed_project(
            &[FeatureSeed {
                id: "LANG-NOPOLICY",
                tier: "core",
                target: "L5",
                backends: "gam",
            }],
            &[],
            None,
        );
        let outcome = run_compute(dir.path(), None);
        assert!(!outcome.is_ok());
        assert!(
            matches!(
                outcome.diagnostics.first().map(|d| d.code),
                Some(CODE_POLICY_LOAD)
            ),
            "{:?}",
            outcome.diagnostics
        );
        assert!(outcome.entries.is_empty());
    }

    #[test]
    fn run_compute_overrides_evidence_path() {
        let dir = ladder_project();
        let custom = dir.path().join("custom-evidence.jsonl");
        let record = rec(
            "cp",
            sha(1),
            "compile-pass",
            EvidenceOutcome::Passed,
            Backend::Gam,
            &["LANG-LADDER-A"],
        );
        std::fs::write(&custom, serde_json::to_string(&record).expect("serialize"))
            .expect("write custom evidence");
        let outcome = run_compute(dir.path(), Some(&custom));
        assert!(outcome.is_ok(), "{:?}", outcome.diagnostics);
        assert_eq!(outcome.records, 1);
        assert_eq!(outcome.entries[0].computed_level, Level::L1);
    }

    // -- golden report ----------------------------------------------------------

    #[test]
    fn golden_report_text_is_stable() {
        let dir = seed_project(
            &[
                FeatureSeed {
                    id: "LANG-GOLDEN-A",
                    tier: "core",
                    target: "L5",
                    backends: "gam, llvm",
                },
                FeatureSeed {
                    id: "LANG-GOLDEN-B",
                    tier: "tooling",
                    target: "L3",
                    backends: "gam",
                },
            ],
            &[GapSeed {
                id: "GAP-0001",
                affects: "LANG-GOLDEN-B",
                caps: "L1",
            }],
            Some(SEED_POLICY),
        );
        let registry = registry_of(&dir);
        let policy = policy_of(&dir);
        let records = [
            rec(
                "cp",
                sha(1),
                "compile-pass",
                EvidenceOutcome::Passed,
                Backend::Gam,
                &["LANG-GOLDEN-A"],
            ),
            rec(
                "cf",
                sha(2),
                "compile-fail",
                EvidenceOutcome::Passed,
                Backend::Gam,
                &["LANG-GOLDEN-A"],
            ),
        ];
        let outcome = ComputeOutcome {
            entries: compute_levels(&registry, &policy, &records),
            diagnostics: Vec::new(),
            features: registry.feature_count(),
            records: records.len(),
        };
        let expected = "\
compute: 2 features, 2 evidence record(s), 0 diagnostic(s)
  LANG-GOLDEN-A                    L2 / L5  unmeasured: fuzz_cpu_hours, mutation_kill_rate_pct
      missing: L3: 1 passed run-pass on gam required, 0 distinct seen
  LANG-GOLDEN-B                    L0 / L3  capped by GAP-0001
      missing: L1: 1 passed compile-pass required, 0 distinct seen
ledger compute OK: 2 features, 2 records, 0 error(s), 0 warning(s)
";
        assert_eq!(format_compute_report(&outcome), expected);
    }
}
