//! `ledger evidence` — the evidence-store curation face (plan §3.4/§6 M1.10
//! remainder).
//!
//! The M1.8 engine reads `glyim.evidence/1` records through
//! [`crate::compute::EvidenceSource`], but nothing owns the *store* those
//! records are committed into: `ledger/evidence/*.jsonl` is discovered by
//! [`crate::compute::ProjectEvidenceSource`], yet a harness run's records
//! reach it only by hand-copying JSONL out of `target/`. This module closes
//! that gap with two subcommands and one storage seam:
//!
//! - `glyim-ledger evidence record [--out PATH] [--format text|json]
//!   <SRC...>` — validate harness-emitted records against the registry,
//!   merge them into the committed store (whole-record dedupe, canonical
//!   order), and persist the result. Invalid records are loud error
//!   diagnostics and never reach the store.
//! - `glyim-ledger evidence check [ROOT] [--format text|json]` — validate
//!   the committed store itself (schema tag, record invariants, unknown
//!   feature IDs) and report its coverage; any error diagnostic fails the
//!   exit code (the CI gate face).
//! - [`EvidenceStore`] — the storage seam (the write counterpart of the
//!   read-side [`crate::compute::EvidenceSource`]): [`FileEvidenceStore`]
//!   for the local JSONL layout, [`InMemoryEvidenceStore`] for tests and
//!   in-process consumers.
//!
//! Determinism contract (plan C1/M1.13): the store is a set of whole
//! records in canonical (schema-serialized) order — merging is
//! order-insensitive and idempotent, so re-recording the same harness run
//! produces byte-identical files.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use ledger_schema::{Diagnostic, Severity};
use serde::Serialize;

use crate::LedgerCliError;
use crate::compute::{
    CODE_EVIDENCE_INVALID, CODE_EVIDENCE_SCHEMA, CODE_EVIDENCE_UNKNOWN_FEATURE,
    EVIDENCE_WIRE_SCHEMA, EvidenceBatch, EvidenceOutcome, EvidenceSource, JsonlEvidenceSource,
    LedgerEvidence, ProjectEvidenceSource, dedup_records, diagnostic,
};

/// Diagnostic code for store-level IO failures (a store file that cannot
/// be read or written is a different failure class from a malformed record
/// line, which [`crate::compute::CODE_EVIDENCE_IO`] / `-INVALID` cover).
pub const CODE_EVIDENCE_STORE_IO: &str = "LEDGER-EVIDENCE-STORE-IO";

/// Schema tag of the `evidence record` JSON contract.
pub const EVIDENCE_RECORD_SCHEMA: &str = "glyim.ledger-evidence-record/1";

/// Schema tag of the `evidence check` JSON contract.
pub const EVIDENCE_CHECK_SCHEMA: &str = "glyim.ledger-evidence-check/1";

/// Default `evidence record` target (relative to the working directory):
/// the store file [`crate::compute::ProjectEvidenceSource`] discovers.
pub const DEFAULT_RECORD_TARGET: &str = "ledger/evidence/recorded.jsonl";

// -- canonical records -------------------------------------------------------

/// One record's canonical wire form: the compact `glyim.evidence/1` JSON
/// line (field order is the struct's declaration order, so serialization
/// is a total, stable order over records).
///
/// # Panics
///
/// Never for a constructed record: `LedgerEvidence` is a plain data
/// structure, so serialization is an invariant site, not an input path
/// (same contract as the harness writer's `to_jsonl_line`).
pub fn canonical_line(record: &LedgerEvidence) -> String {
    serde_json::to_string(record)
        .expect("LedgerEvidence is a plain data structure and cannot fail to serialize")
}

/// Merges two record sets into the canonical store form: whole-record
/// duplicates collapse (the engine's set-based counting needs no exact
/// duplicates committed) and the remainder is sorted by canonical line so
/// any merge order — and any machine — produces identical bytes.
#[must_use]
pub fn merge_records(
    existing: &[LedgerEvidence],
    incoming: &[LedgerEvidence],
) -> Vec<LedgerEvidence> {
    let mut seen: HashSet<&LedgerEvidence> =
        HashSet::with_capacity(existing.len() + incoming.len());
    let mut keyed: Vec<(String, &LedgerEvidence)> = existing
        .iter()
        .chain(incoming.iter())
        .filter(|record| seen.insert(record))
        .map(|record| (canonical_line(record), record))
        .collect();
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    keyed
        .into_iter()
        .map(|(_, record)| record.clone())
        .collect()
}

// -- storage seam ------------------------------------------------------------

/// Where curated evidence records come from and go.
///
/// The write counterpart of the read-side
/// [`crate::compute::EvidenceSource`] seam: run faces (and their tests)
/// talk to this trait, and today's two stores — the committed JSONL
/// layout and an in-process map — implement it without the validation
/// logic knowing which is live.
pub trait EvidenceStore {
    /// Loads one store file; failures are [`CODE_EVIDENCE_STORE_IO`] /
    /// [`crate::compute::CODE_EVIDENCE_INVALID`] diagnostics inside the
    /// returned batch (a corrupt line is diagnosed, not fatal, mirroring
    /// the engine's evidence loading).
    fn load(&self, path: &Path) -> EvidenceBatch;

    /// Persists `records` as the store at `path` in canonical form
    /// (deduplicated, sorted). The file's parent directory is created if
    /// missing — the record face's first run lands in a repository that
    /// has never had `ledger/evidence/`, and "create the directory" is
    /// the remedy a first user needs, not an error.
    ///
    /// # Errors
    ///
    /// A [`CODE_EVIDENCE_STORE_IO`] diagnostic naming the failing path.
    fn save(&self, path: &Path, records: &[LedgerEvidence]) -> Result<(), Diagnostic>;
}

/// The default store: the committed `ledger/evidence/*.jsonl` layout on
/// the local filesystem (one canonical record per line, trailing newline).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileEvidenceStore;

impl EvidenceStore for FileEvidenceStore {
    fn load(&self, path: &Path) -> EvidenceBatch {
        let mut batch = EvidenceBatch::default();
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            // A missing store file is a fresh store, not a misconfiguration
            // (the record face's first run lands before the file exists —
            // the same honest-absence contract as `ProjectEvidenceSource`
            // discovering no `ledger/evidence/` directory).
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return batch;
            }
            Err(err) => {
                batch.diagnostics.push(diagnostic(
                    CODE_EVIDENCE_STORE_IO,
                    Severity::Error,
                    format!("cannot read evidence store {}: {err}", path.display()),
                ));
                return batch;
            }
        };
        for (index, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            match LedgerEvidence::from_jsonl_line(line) {
                Ok(record) => batch.records.push(record),
                Err(err) => batch.diagnostics.push(diagnostic(
                    crate::compute::CODE_EVIDENCE_INVALID,
                    Severity::Error,
                    format!("{}:{}: {err}", path.display(), index + 1),
                )),
            }
        }
        batch
    }

    fn save(&self, path: &Path, records: &[LedgerEvidence]) -> Result<(), Diagnostic> {
        let canonical = merge_records(&[], records);
        let mut body = String::with_capacity(canonical.len() * 160);
        for record in &canonical {
            body.push_str(&canonical_line(record));
            body.push('\n');
        }
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|err| {
                    store_io(path, format!("cannot create the store directory: {err}"))
                })?;
            }
        }
        fs::write(path, body)
            .map_err(|err| store_io(path, format!("cannot write the evidence store: {err}")))
    }
}

/// In-memory store: a `path -> records` map for tests and in-process
/// consumers. `save` runs the same canonicalization contract as disk, so
/// a consumer cannot behave differently against either store.
#[derive(Debug, Default)]
pub struct InMemoryEvidenceStore {
    stores: RefCell<BTreeMap<PathBuf, Vec<LedgerEvidence>>>,
}

impl InMemoryEvidenceStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of stored store files.
    #[must_use]
    pub fn len(&self) -> usize {
        self.stores.borrow().len()
    }

    /// Whether the store holds no store files.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.stores.borrow().is_empty()
    }
}

impl EvidenceStore for InMemoryEvidenceStore {
    fn load(&self, path: &Path) -> EvidenceBatch {
        match self.stores.borrow().get(path) {
            Some(records) => EvidenceBatch {
                records: records.clone(),
                diagnostics: Vec::new(),
            },
            None => EvidenceBatch {
                records: Vec::new(),
                diagnostics: vec![store_io(
                    path,
                    "not found in the in-memory store".to_string(),
                )],
            },
        }
    }

    fn save(&self, path: &Path, records: &[LedgerEvidence]) -> Result<(), Diagnostic> {
        self.stores
            .borrow_mut()
            .insert(path.to_path_buf(), merge_records(&[], records));
        Ok(())
    }
}

fn store_io(path: &Path, detail: String) -> Diagnostic {
    diagnostic(
        CODE_EVIDENCE_STORE_IO,
        Severity::Error,
        format!("cannot use evidence store {}: {detail}", path.display()),
    )
}

// -- shared triage -----------------------------------------------------------

/// Schema-, invariant-, and registry-checks `records`, returning the valid
/// remainder and pushing one error diagnostic per rejected record (the
/// same triage `run_compute` applies to its evidence input — the store
/// must never hold records the engine would reject).
fn triage_records(
    records: &[LedgerEvidence],
    registry: &ledger_schema::Registry,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<LedgerEvidence> {
    let mut valid = Vec::with_capacity(records.len());
    let mut unknown_ids: BTreeSet<&str> = BTreeSet::new();
    for record in records {
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
        if !issues.is_empty() {
            for issue in issues {
                diagnostics.push(diagnostic(
                    CODE_EVIDENCE_INVALID,
                    Severity::Error,
                    format!("evidence record {} is invalid: {issue}", record.name),
                ));
            }
            continue;
        }
        let mut unknown = false;
        for id in &record.features {
            if registry.feature_by_id(id).is_none() && unknown_ids.insert(id) {
                unknown = true;
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
        if !unknown {
            valid.push(record.clone());
        }
    }
    valid
}

fn finish_diagnostics(diagnostics: &mut Vec<Diagnostic>) {
    diagnostics.sort_by(|a, b| a.code.cmp(b.code).then(a.message.cmp(&b.message)));
    diagnostics.dedup();
}

// -- record face -------------------------------------------------------------

/// Counters rendered by the `evidence record` report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RecordStats {
    /// Source JSONL files read.
    pub sources: usize,
    /// Records parsed from the sources (before triage and dedupe).
    pub parsed: usize,
    /// Source lines that failed to parse (one diagnostic each).
    pub invalid_lines: usize,
    /// Incoming records that passed triage.
    pub incoming: usize,
    /// Incoming records already in the store (or duplicated within the
    /// batch): `incoming + existing - merged`.
    pub duplicates_dropped: usize,
    /// Records already committed before this run.
    pub existing: usize,
    /// Canonical store size after the merge.
    pub merged: usize,
    /// Bytes persisted (canonical lines plus one newline each).
    pub bytes: u64,
}

/// Result of a full `evidence record` run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordOutcome {
    /// The store file the merge landed in.
    pub out: PathBuf,
    /// The run's counters.
    pub stats: RecordStats,
    /// Load, triage, and registry diagnostics (sorted, de-duplicated).
    pub diagnostics: Vec<Diagnostic>,
}

impl RecordOutcome {
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

/// Runs a full `evidence record` pass: load the registry at `root`, read
/// every `sources` JSONL file, triage the records against it, merge them
/// into the store at `out`, and persist the canonical result.
///
/// # Errors
///
/// [`LedgerCliError::EvidenceWrite`] when the store cannot be persisted
/// (the operator asked for a document that did not land — the same seam
/// as `compute --snapshot`); everything else is a diagnostic in the
/// returned outcome, so one run reports all invalid records, not the
/// first.
pub fn run_evidence_record(
    root: &Path,
    sources: &[PathBuf],
    out: &Path,
    store: &dyn EvidenceStore,
) -> Result<RecordOutcome, LedgerCliError> {
    let span = tracing::info_span!(
        "evidence_record",
        root = %root.display(),
        out = %out.display(),
        sources = sources.len()
    );
    let _enter = span.enter();

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let loaded = ledger_schema::load_project_registry(root);
    diagnostics.extend(loaded.diagnostics);
    diagnostics.extend(loaded.registry.validate());

    let batch = JsonlEvidenceSource::new(sources.to_vec()).load();
    let parsed = batch.records.len();
    let invalid_lines = batch
        .diagnostics
        .iter()
        .filter(|d| d.code == crate::compute::CODE_EVIDENCE_INVALID)
        .count();
    diagnostics.extend(batch.diagnostics);

    let incoming = triage_records(&batch.records, &loaded.registry, &mut diagnostics);

    let existing_batch = store.load(out);
    diagnostics.extend(existing_batch.diagnostics);

    let merged = merge_records(&existing_batch.records, &incoming);
    let bytes: u64 = merged
        .iter()
        .map(|record| canonical_line(record).len() as u64 + 1)
        .sum();
    store
        .save(out, &merged)
        .map_err(|failure| LedgerCliError::EvidenceWrite {
            path: out.display().to_string(),
            detail: failure.message,
        })?;

    tracing::debug!(
        records = merged.len(),
        bytes,
        "evidence record pass finished"
    );

    finish_diagnostics(&mut diagnostics);
    Ok(RecordOutcome {
        out: out.to_path_buf(),
        stats: RecordStats {
            sources: sources.len(),
            parsed,
            invalid_lines,
            incoming: incoming.len(),
            duplicates_dropped: incoming.len() + existing_batch.records.len() - merged.len(),
            existing: existing_batch.records.len(),
            merged: merged.len(),
            bytes,
        },
        diagnostics,
    })
}

// -- check face --------------------------------------------------------------

/// Per-outcome record counts in the `evidence check` report.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct OutcomeHistogram {
    /// Records whose fixture passed its strategy checks.
    pub passed: usize,
    /// Records whose fixture failed (with a stable reason class).
    pub failed: usize,
    /// Records whose fixture was skipped (`needs-llvm` / `only-target`).
    pub ignored: usize,
}

/// Result of a full `evidence check` run (the CI gate face).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    /// Committed store files discovered under `ledger/evidence/`.
    pub files: usize,
    /// Distinct committed records (after exact-duplicate collapse).
    pub records: usize,
    /// Exact-duplicate lines found in the committed store (the canonical
    /// form never holds them, so a non-zero count means a hand-edited or
    /// foreign store).
    pub duplicates: usize,
    /// Distinct fixtures the records observe.
    pub distinct_fixtures: usize,
    /// Distinct registry feature IDs the records reference.
    pub features_covered: usize,
    /// Per-outcome histogram over the committed records.
    pub by_outcome: OutcomeHistogram,
    /// Load, triage, and registry diagnostics (sorted, de-duplicated).
    pub diagnostics: Vec<Diagnostic>,
}

impl CheckReport {
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

/// Runs a full `evidence check` pass: validate every committed
/// `ledger/evidence/*.jsonl` record against the registry and report the
/// store's coverage. An absent evidence directory is a valid, honest
/// zero-coverage report (mirroring [`ProjectEvidenceSource`]).
#[must_use]
pub fn run_evidence_check(root: &Path) -> CheckReport {
    let span = tracing::info_span!("evidence_check", root = %root.display());
    let _enter = span.enter();

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let evidence_dir = root.join("ledger").join("evidence");
    let files = match fs::read_dir(&evidence_dir) {
        Ok(entries) => {
            let paths: Vec<PathBuf> = entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
                .collect();
            paths.len()
        }
        // Absent store directory: no evidence recorded yet — an honest
        // zero report, not a misconfiguration (same contract as compute).
        Err(_) => 0,
    };

    let loaded = ledger_schema::load_project_registry(root);
    diagnostics.extend(loaded.diagnostics);
    diagnostics.extend(loaded.registry.validate());

    let batch = ProjectEvidenceSource::new(root, None).load();
    diagnostics.extend(batch.diagnostics);

    let unique = dedup_records(&batch.records);
    let duplicates = batch.records.len() - unique.len();
    let valid = triage_records(
        &unique.into_iter().cloned().collect::<Vec<_>>(),
        &loaded.registry,
        &mut diagnostics,
    );

    let mut fixtures: BTreeSet<&str> = BTreeSet::new();
    let mut features: BTreeSet<&str> = BTreeSet::new();
    let mut by_outcome = OutcomeHistogram::default();
    for record in &valid {
        fixtures.insert(&record.fixture);
        features.extend(record.features.iter().map(String::as_str));
        match record.outcome {
            EvidenceOutcome::Passed => by_outcome.passed += 1,
            EvidenceOutcome::Failed => by_outcome.failed += 1,
            EvidenceOutcome::Ignored => by_outcome.ignored += 1,
        }
    }

    tracing::debug!(
        files,
        records = valid.len(),
        diagnostics = diagnostics.len(),
        "evidence check pass finished"
    );

    finish_diagnostics(&mut diagnostics);
    CheckReport {
        files,
        records: valid.len(),
        duplicates,
        distinct_fixtures: fixtures.len(),
        features_covered: features.len(),
        by_outcome,
        diagnostics,
    }
}

// -- report rendering --------------------------------------------------------

/// JSON shape of one diagnostic in the report contracts (severity is
/// rendered through [`Severity::as_str`], the ledger's wire spelling).
#[derive(Serialize)]
struct DiagnosticJson<'a> {
    code: &'a str,
    severity: &'a str,
    message: &'a str,
}

fn diagnostics_json(diagnostics: &[Diagnostic]) -> Vec<DiagnosticJson<'_>> {
    diagnostics
        .iter()
        .map(|d| DiagnosticJson {
            code: d.code,
            severity: d.severity.as_str(),
            message: &d.message,
        })
        .collect()
}

/// Renders the `glyim.ledger-evidence-record/1` JSON contract.
#[must_use]
pub fn format_evidence_record_json(outcome: &RecordOutcome) -> String {
    #[derive(Serialize)]
    struct RecordJson<'a> {
        schema: &'static str,
        out: &'a str,
        stats: &'a RecordStats,
        diagnostics: Vec<DiagnosticJson<'a>>,
    }
    let document = RecordJson {
        schema: EVIDENCE_RECORD_SCHEMA,
        out: &outcome.out.display().to_string(),
        stats: &outcome.stats,
        diagnostics: diagnostics_json(&outcome.diagnostics),
    };
    let mut text = serde_json::to_string_pretty(&document).unwrap_or_else(|err| {
        // Field sets are fixed and values are plain strings/numbers, so
        // serialization is infallible by construction; this arm keeps the
        // renderer total without introducing a Result into the format_*
        // family contract (same shape as `format_status_json`).
        format!("{{\"schema\":\"{EVIDENCE_RECORD_SCHEMA}\",\"error\":\"{err}\"}}")
    });
    text.push('\n');
    text
}

/// Renders the human-readable `evidence record` report.
#[must_use]
pub fn format_evidence_record_text(outcome: &RecordOutcome) -> String {
    use std::fmt::Write as _;
    let mut text = String::new();
    let _ = writeln!(
        text,
        "evidence record: merged {} record(s) into {} ({} bytes)",
        outcome.stats.merged,
        outcome.out.display(),
        outcome.stats.bytes,
    );
    let _ = writeln!(
        text,
        "  sources: {} file(s), {} parsed, {} invalid line(s)",
        outcome.stats.sources, outcome.stats.parsed, outcome.stats.invalid_lines,
    );
    let _ = writeln!(
        text,
        "  store: {} existing, {} incoming, {} duplicate(s) dropped, {} merged",
        outcome.stats.existing,
        outcome.stats.incoming,
        outcome.stats.duplicates_dropped,
        outcome.stats.merged,
    );
    let _ = writeln!(
        text,
        "  diagnostics: {} error(s), {} warning(s)",
        outcome.errors(),
        outcome.warnings(),
    );
    for failure in &outcome.diagnostics {
        let _ = writeln!(text, "  {failure}");
    }
    text
}

/// Renders the `glyim.ledger-evidence-check/1` JSON contract.
#[must_use]
pub fn format_evidence_check_json(report: &CheckReport) -> String {
    #[derive(Serialize)]
    struct CheckJson<'a> {
        schema: &'static str,
        files: usize,
        records: usize,
        duplicates: usize,
        distinct_fixtures: usize,
        features_covered: usize,
        by_outcome: &'a OutcomeHistogram,
        diagnostics: Vec<DiagnosticJson<'a>>,
    }
    let document = CheckJson {
        schema: EVIDENCE_CHECK_SCHEMA,
        files: report.files,
        records: report.records,
        duplicates: report.duplicates,
        distinct_fixtures: report.distinct_fixtures,
        features_covered: report.features_covered,
        by_outcome: &report.by_outcome,
        diagnostics: diagnostics_json(&report.diagnostics),
    };
    let mut text = serde_json::to_string_pretty(&document).unwrap_or_else(|err| {
        format!("{{\"schema\":\"{EVIDENCE_CHECK_SCHEMA}\",\"error\":\"{err}\"}}")
    });
    text.push('\n');
    text
}

/// Renders the human-readable `evidence check` report.
#[must_use]
pub fn format_evidence_check_text(report: &CheckReport) -> String {
    use std::fmt::Write as _;
    let mut text = String::new();
    let _ = writeln!(
        text,
        "evidence check: {} store file(s), {} record(s) ({} duplicate(s))",
        report.files, report.records, report.duplicates,
    );
    let _ = writeln!(
        text,
        "  coverage: {} distinct fixture(s), {} feature(s) referenced",
        report.distinct_fixtures, report.features_covered,
    );
    let _ = writeln!(
        text,
        "  outcomes: {} passed, {} failed, {} ignored",
        report.by_outcome.passed, report.by_outcome.failed, report.by_outcome.ignored,
    );
    let _ = writeln!(
        text,
        "  diagnostics: {} error(s), {} warning(s)",
        report.errors(),
        report.warnings(),
    );
    for failure in &report.diagnostics {
        let _ = writeln!(text, "  {failure}");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compute::run_compute;
    use crate::{Command, EvidenceFormat};
    use ledger_schema::Backend;
    use std::fs;

    /// Seed policy text (mirrors `ledger/policy.toml`, same fixture the
    /// `status` bench uses).
    const POLICY: &str = r#"schema = "glyim.ledger-policy/1"

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

    /// Writes a minimal valid registry: two features over the spec anchor
    /// plus the seed policy (no gaps).
    fn write_project(root: &Path) {
        let features = root.join("ledger/features");
        fs::create_dir_all(&features).expect("mkdir features");
        fs::create_dir_all(root.join("docs/specs")).expect("mkdir specs");
        fs::write(
            root.join("docs/specs/v0.1.0.md"),
            "# spec fixture\n\n## evidence\n",
        )
        .expect("write spec");
        fs::write(root.join("ledger/policy.toml"), POLICY).expect("write policy");
        fs::write(
            features.join("evidence.toml"),
            "[[feature]]\n\
             id = \"LANG-EVIDENCE-A\"\n\
             title = \"evidence fixture feature A\"\n\
             area = \"lang.control-flow\"\n\
             tier = \"core\"\n\
             spec = \"docs/specs/v0.1.0.md\"\n\
             target_level = \"L5\"\n\
             backends = [\"gam\"]\n\
             owner_lane = \"M\"\n\n\
             [[feature]]\n\
             id = \"LANG-EVIDENCE-B\"\n\
             title = \"evidence fixture feature B\"\n\
             area = \"lang.control-flow\"\n\
             tier = \"core\"\n\
             spec = \"docs/specs/v0.1.0.md\"\n\
             target_level = \"L5\"\n\
             backends = [\"gam\", \"llvm\"]\n\
             owner_lane = \"M\"\n",
        )
        .expect("write features");
    }

    /// A valid `glyim.evidence/1` record for fixture `name` (the fixture
    /// hash is a deterministic 64-hex digest derived from the name, so
    /// every record carries a well-formed identity).
    fn record(
        name: &str,
        mode: &str,
        outcome: EvidenceOutcome,
        features: &[&str],
    ) -> LedgerEvidence {
        let seed = name.bytes().fold(0u64, |acc, byte| {
            acc.wrapping_mul(31).wrapping_add(u64::from(byte))
        });
        LedgerEvidence {
            schema: EVIDENCE_WIRE_SCHEMA.to_string(),
            name: name.to_string(),
            fixture: format!("tests/{name}.g"),
            fixture_sha256: format!("{seed:064x}"),
            revision: String::new(),
            mode: mode.to_string(),
            outcome,
            reason: None,
            backend: Backend::Gam,
            opt: 0,
            features: features.iter().map(|id| (*id).to_string()).collect(),
            declared_backends: vec!["gam".to_string()],
            instructions: None,
            rss_kb: None,
        }
    }

    /// Serializes a record to one JSONL line (the wire form).
    fn line(record: &LedgerEvidence) -> String {
        serde_json::to_string(record).expect("record is a plain data structure") + "\n"
    }

    /// Runs the record face over `records` written to a temporary source
    /// file, storing into `<dir>/ledger/evidence/test.jsonl`.
    fn run_record(dir: &Path, records: &[LedgerEvidence]) -> RecordOutcome {
        let source = dir.join("run.jsonl");
        fs::write(&source, records.iter().map(line).collect::<String>()).expect("write source");
        let out = dir.join("ledger/evidence/test.jsonl");
        run_evidence_record(dir, &[source], &out, &FileEvidenceStore).expect("record run")
    }

    // -- record face -------------------------------------------------------

    #[test]
    fn record_face_merges_and_canonicalizes_the_store() {
        let dir = tempfile::tempdir().expect("tmp");
        write_project(dir.path());
        let out = dir.path().join("ledger/evidence/test.jsonl");
        fs::create_dir_all(out.parent().expect("parent")).expect("mkdir");
        let a = record(
            "alpha",
            "compile-pass",
            EvidenceOutcome::Passed,
            &["LANG-EVIDENCE-A"],
        );
        let b = record(
            "bravo",
            "run-pass",
            EvidenceOutcome::Passed,
            &["LANG-EVIDENCE-A"],
        );
        let c = record(
            "charlie",
            "compile-fail",
            EvidenceOutcome::Passed,
            &["LANG-EVIDENCE-B"],
        );
        fs::write(&out, format!("{}{}", line(&a), line(&b))).expect("seed store");

        let outcome = run_record(dir.path(), &[b.clone(), c]);

        assert!(outcome.is_ok(), "{:?}", outcome.diagnostics);
        // The store had `a` and `b`; incoming `b` (duplicate) + `c` (new):
        // three canonical records, one duplicate dropped.
        assert_eq!(outcome.stats.merged, 3);
        assert_eq!(outcome.stats.duplicates_dropped, 1);
        let committed = fs::read_to_string(&out).expect("read store");
        let lines: Vec<&str> = committed.lines().collect();
        assert_eq!(lines.len(), 3);
        let mut sorted = lines.clone();
        sorted.sort_unstable();
        assert_eq!(lines, sorted, "store lines are in canonical order");
    }

    #[test]
    fn record_face_is_idempotent_and_order_insensitive() {
        let dir = tempfile::tempdir().expect("tmp");
        write_project(dir.path());
        let a = record(
            "alpha",
            "compile-pass",
            EvidenceOutcome::Passed,
            &["LANG-EVIDENCE-A"],
        );
        let b = record(
            "bravo",
            "run-pass",
            EvidenceOutcome::Passed,
            &["LANG-EVIDENCE-A"],
        );

        let first = run_record(dir.path(), &[a.clone(), b.clone()]);
        let second = run_record(dir.path(), &[b.clone(), a.clone()]);
        assert!(first.is_ok() && second.is_ok());
        let store = dir.path().join("ledger/evidence/test.jsonl");
        let committed = fs::read_to_string(&store).expect("read store");
        assert_eq!(first.stats.merged, 2);
        assert_eq!(second.stats.merged, 2, "input order never changes the set");
        assert_eq!(
            fs::read_to_string(&store).expect("read store"),
            committed,
            "shuffled input merges to byte-identical canonical order"
        );
        // Re-recording the same records changes nothing (idempotence).
        let rerun = run_record(dir.path(), &[a]);
        assert_eq!(rerun.stats.merged, 2);
        assert_eq!(rerun.stats.duplicates_dropped, 1);
        assert_eq!(
            fs::read_to_string(&store).expect("read store"),
            committed,
            "idempotent re-record leaves the store byte-identical"
        );
    }

    #[test]
    fn record_face_diagnoses_an_invalid_line_and_persists_the_rest() {
        let dir = tempfile::tempdir().expect("tmp");
        write_project(dir.path());
        let source = dir.path().join("run.jsonl");
        let good = record(
            "alpha",
            "compile-pass",
            EvidenceOutcome::Passed,
            &["LANG-EVIDENCE-A"],
        );
        fs::write(&source, format!("{{\"broken\": true}}\n{}", line(&good))).expect("write source");
        let out = dir.path().join("ledger/evidence/test.jsonl");

        let outcome = run_evidence_record(dir.path(), &[source], &out, &FileEvidenceStore)
            .expect("record run");

        assert!(!outcome.is_ok());
        assert_eq!(outcome.stats.invalid_lines, 1);
        assert_eq!(outcome.stats.merged, 1, "only the valid record lands");
        assert!(
            outcome
                .diagnostics
                .iter()
                .any(|d| d.code == CODE_EVIDENCE_INVALID),
            "{:?}",
            outcome.diagnostics
        );
    }

    #[test]
    fn record_face_rejects_a_foreign_schema_tag() {
        let dir = tempfile::tempdir().expect("tmp");
        write_project(dir.path());
        let mut foreign = record(
            "alpha",
            "compile-pass",
            EvidenceOutcome::Passed,
            &["LANG-EVIDENCE-A"],
        );
        foreign.schema = "glyim.evidence/0".to_string();

        let outcome = run_record(dir.path(), &[foreign]);

        assert!(!outcome.is_ok());
        assert_eq!(outcome.stats.merged, 0);
        assert!(
            outcome
                .diagnostics
                .iter()
                .any(|d| d.code == CODE_EVIDENCE_SCHEMA),
            "{:?}",
            outcome.diagnostics
        );
    }

    #[test]
    fn record_face_rejects_unknown_feature_ids() {
        let dir = tempfile::tempdir().expect("tmp");
        write_project(dir.path());
        let unknown = record(
            "alpha",
            "compile-pass",
            EvidenceOutcome::Passed,
            &["LANG-EVIDENCE-NOPE"],
        );

        let outcome = run_record(dir.path(), &[unknown]);

        assert!(!outcome.is_ok());
        assert_eq!(outcome.stats.merged, 0);
        assert!(
            outcome
                .diagnostics
                .iter()
                .any(|d| d.code == CODE_EVIDENCE_UNKNOWN_FEATURE),
            "{:?}",
            outcome.diagnostics
        );
    }

    #[test]
    fn record_face_creates_missing_parent_directories() {
        let dir = tempfile::tempdir().expect("tmp");
        write_project(dir.path());
        let out = dir.path().join("ledger/evidence/nested/deep/test.jsonl");
        let good = record(
            "alpha",
            "compile-pass",
            EvidenceOutcome::Passed,
            &["LANG-EVIDENCE-A"],
        );
        let source = dir.path().join("run.jsonl");
        fs::write(&source, line(&good)).expect("write source");

        let outcome = run_evidence_record(dir.path(), &[source], &out, &FileEvidenceStore)
            .expect("record run");

        assert!(outcome.is_ok(), "{:?}", outcome.diagnostics);
        assert!(out.is_file(), "the nested store file exists");
    }

    #[test]
    fn record_face_json_contract_is_stable() {
        let dir = tempfile::tempdir().expect("tmp");
        write_project(dir.path());
        let good = record(
            "alpha",
            "compile-pass",
            EvidenceOutcome::Passed,
            &["LANG-EVIDENCE-A"],
        );

        let outcome = run_record(dir.path(), &[good]);

        let json = format_evidence_record_json(&outcome);
        assert!(
            json.contains("\"schema\": \"glyim.ledger-evidence-record/1\""),
            "{json}"
        );
        assert!(json.contains("\"merged\": 1"), "{json}");
        assert!(json.contains("\"parsed\": 1"), "{json}");
        let text = format_evidence_record_text(&outcome);
        assert!(text.contains("merged 1 record(s)"), "{text}");
    }

    // -- check face --------------------------------------------------------

    #[test]
    fn check_face_reports_an_absent_store_as_ok() {
        let dir = tempfile::tempdir().expect("tmp");
        write_project(dir.path());

        let report = run_evidence_check(dir.path());

        assert!(report.is_ok(), "{:?}", report.diagnostics);
        assert_eq!(report.files, 0);
        assert_eq!(report.records, 0);
        let text = format_evidence_check_text(&report);
        assert!(text.contains("0 store file(s)"), "{text}");
    }

    #[test]
    fn check_face_counts_and_validates_a_committed_store() {
        let dir = tempfile::tempdir().expect("tmp");
        write_project(dir.path());
        let store = dir.path().join("ledger/evidence/test.jsonl");
        fs::create_dir_all(store.parent().expect("parent")).expect("mkdir");
        let passed = record(
            "alpha",
            "run-pass",
            EvidenceOutcome::Passed,
            &["LANG-EVIDENCE-A"],
        );
        let mut failed = record(
            "bravo",
            "run-pass",
            EvidenceOutcome::Failed,
            &["LANG-EVIDENCE-B"],
        );
        failed.reason = Some("run failed".to_string());
        let ignored = record(
            "charlie",
            "run-pass",
            EvidenceOutcome::Ignored,
            &["LANG-EVIDENCE-A"],
        );
        fs::write(
            &store,
            format!("{}{}{}", line(&passed), line(&failed), line(&ignored)),
        )
        .expect("write store");

        let report = run_evidence_check(dir.path());

        assert!(report.is_ok(), "{:?}", report.diagnostics);
        assert_eq!(report.files, 1);
        assert_eq!(report.records, 3);
        assert_eq!(report.distinct_fixtures, 3);
        assert_eq!(report.features_covered, 2);
        assert_eq!(report.by_outcome.passed, 1);
        assert_eq!(report.by_outcome.failed, 1);
        assert_eq!(report.by_outcome.ignored, 1);
        let json = format_evidence_check_json(&report);
        assert!(
            json.contains("\"schema\": \"glyim.ledger-evidence-check/1\""),
            "{json}"
        );
    }

    #[test]
    fn check_face_fails_on_an_invalid_committed_record() {
        let dir = tempfile::tempdir().expect("tmp");
        write_project(dir.path());
        let store = dir.path().join("ledger/evidence/test.jsonl");
        fs::create_dir_all(store.parent().expect("parent")).expect("mkdir");
        let mut corrupt = record(
            "alpha",
            "compile-pass",
            EvidenceOutcome::Passed,
            &["LANG-EVIDENCE-A"],
        );
        corrupt.fixture_sha256 = "not-a-hash".to_string();
        fs::write(&store, line(&corrupt)).expect("write store");

        let report = run_evidence_check(dir.path());

        assert!(!report.is_ok());
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.code == CODE_EVIDENCE_INVALID),
            "{:?}",
            report.diagnostics
        );
    }

    #[test]
    fn check_face_fails_on_unknown_feature_ids() {
        let dir = tempfile::tempdir().expect("tmp");
        write_project(dir.path());
        let store = dir.path().join("ledger/evidence/test.jsonl");
        fs::create_dir_all(store.parent().expect("parent")).expect("mkdir");
        let unknown = record(
            "alpha",
            "compile-pass",
            EvidenceOutcome::Passed,
            &["LANG-EVIDENCE-NOPE"],
        );
        fs::write(&store, line(&unknown)).expect("write store");

        let report = run_evidence_check(dir.path());

        assert!(!report.is_ok());
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.code == CODE_EVIDENCE_UNKNOWN_FEATURE),
            "{:?}",
            report.diagnostics
        );
    }

    // -- storage seam ------------------------------------------------------

    #[test]
    fn in_memory_store_round_trips_and_diagnoses_misses() {
        let store = InMemoryEvidenceStore::new();
        let a = record(
            "alpha",
            "compile-pass",
            EvidenceOutcome::Passed,
            &["LANG-EVIDENCE-A"],
        );
        let path = Path::new("/virtual/store.jsonl");
        store.save(path, std::slice::from_ref(&a)).expect("save");
        let batch = store.load(path);
        assert_eq!(batch.records, vec![a.clone()]);
        assert!(batch.diagnostics.is_empty());

        let missing = store.load(Path::new("/virtual/absent.jsonl"));
        assert!(missing.records.is_empty());
        assert_eq!(missing.diagnostics.len(), 1);
        assert_eq!(
            missing.diagnostics[0].severity,
            ledger_schema::Severity::Error
        );
    }

    #[test]
    fn file_store_save_canonicalizes_any_input_order() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("store.jsonl");
        let a = record(
            "alpha",
            "compile-pass",
            EvidenceOutcome::Passed,
            &["LANG-EVIDENCE-A"],
        );
        let b = record(
            "bravo",
            "compile-pass",
            EvidenceOutcome::Passed,
            &["LANG-EVIDENCE-A"],
        );
        FileEvidenceStore
            .save(&path, &[b.clone(), a.clone(), a.clone()])
            .expect("save");
        let committed = fs::read_to_string(&path).expect("read store");
        let lines: Vec<&str> = committed.lines().collect();
        assert_eq!(lines.len(), 2, "duplicates collapse");
        let mut sorted = lines.clone();
        sorted.sort_unstable();
        assert_eq!(lines, sorted);
    }

    #[test]
    fn merge_records_collapses_duplicates_set_based() {
        let a = record(
            "alpha",
            "compile-pass",
            EvidenceOutcome::Passed,
            &["LANG-EVIDENCE-A"],
        );
        let b = record(
            "bravo",
            "run-pass",
            EvidenceOutcome::Passed,
            &["LANG-EVIDENCE-A"],
        );
        let merged = merge_records(&[a.clone(), a.clone()], &[a.clone(), b.clone()]);
        assert_eq!(merged, vec![a.clone(), b.clone()]);
        // Order-insensitive: swapping the inputs merges to the same set.
        let swapped = merge_records(&[b.clone()], &[a.clone(), b]);
        assert_eq!(swapped, merged);
        let _ = a;
    }

    // -- CLI parse ---------------------------------------------------------

    #[test]
    fn parse_evidence_record_minimal() {
        let command = crate::parse_args(
            [
                "evidence".to_string(),
                "record".to_string(),
                "run.jsonl".to_string(),
            ]
            .into_iter(),
        )
        .expect("parses");
        match command {
            Command::EvidenceRecord { sources, out, .. } => {
                assert_eq!(sources, vec![PathBuf::from("run.jsonl")]);
                assert_eq!(
                    out,
                    PathBuf::from("ledger/evidence/recorded.jsonl"),
                    "the documented default target"
                );
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_evidence_record_flags() {
        let command = crate::parse_args(
            [
                "evidence".to_string(),
                "record".to_string(),
                "a.jsonl".to_string(),
                "b.jsonl".to_string(),
                "--out".to_string(),
                "store.jsonl".to_string(),
                "--format".to_string(),
                "json".to_string(),
            ]
            .into_iter(),
        )
        .expect("parses");
        match command {
            Command::EvidenceRecord {
                sources,
                out,
                format,
            } => {
                assert_eq!(sources.len(), 2);
                assert_eq!(out, PathBuf::from("store.jsonl"));
                assert_eq!(format, EvidenceFormat::Json);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_evidence_record_requires_a_source() {
        let err = crate::parse_args(["evidence".to_string(), "record".to_string()].into_iter())
            .expect_err("no sources");
        assert!(matches!(err, LedgerCliError::EvidenceMissingSources { .. }));
    }

    #[test]
    fn parse_evidence_rejects_unknown_subcommands_and_flags() {
        let err = crate::parse_args(["evidence".to_string(), "groom".to_string()].into_iter())
            .expect_err("unknown evidence subcommand");
        assert!(matches!(err, LedgerCliError::EvidenceSubcommand { .. }));
        let err = crate::parse_args(
            [
                "evidence".to_string(),
                "record".to_string(),
                "--out".to_string(),
            ]
            .into_iter(),
        )
        .expect_err("--out without value");
        assert!(matches!(
            err,
            LedgerCliError::EvidenceMissingValue { flag, .. } if flag == "--out"
        ));
        let err = crate::parse_args(
            [
                "evidence".to_string(),
                "record".to_string(),
                "run.jsonl".to_string(),
                "--format".to_string(),
                "yaml".to_string(),
            ]
            .into_iter(),
        )
        .expect_err("bad format");
        assert!(matches!(err, LedgerCliError::EvidenceBadFormat { .. }));
        let err = crate::parse_args(
            [
                "evidence".to_string(),
                "check".to_string(),
                "a".to_string(),
                "b".to_string(),
            ]
            .into_iter(),
        )
        .expect_err("two roots");
        assert!(matches!(err, LedgerCliError::ExtraArgument { .. }));
    }

    #[test]
    fn parse_evidence_check_with_root_and_format() {
        let command = crate::parse_args(
            [
                "evidence".to_string(),
                "check".to_string(),
                "/repo".to_string(),
                "--format".to_string(),
                "json".to_string(),
            ]
            .into_iter(),
        )
        .expect("parses");
        match command {
            Command::EvidenceCheck { root, format } => {
                assert_eq!(root, PathBuf::from("/repo"));
                assert_eq!(format, EvidenceFormat::Json);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    // -- end-to-end: the store feeds the engine -----------------------------

    #[test]
    fn recorded_store_feeds_compute_and_status() {
        let dir = tempfile::tempdir().expect("tmp");
        write_project(dir.path());
        // The level ladder is a prefix: L3 (one run-pass gam pass) needs
        // the L1/L2 compile faces satisfied first, so the curated run
        // carries all three faces for LANG-EVIDENCE-A.
        let records = [
            record(
                "alpha",
                "compile-pass",
                EvidenceOutcome::Passed,
                &["LANG-EVIDENCE-A"],
            ),
            record(
                "bravo",
                "compile-fail",
                EvidenceOutcome::Passed,
                &["LANG-EVIDENCE-A"],
            ),
            record(
                "charlie",
                "run-pass",
                EvidenceOutcome::Passed,
                &["LANG-EVIDENCE-A"],
            ),
        ];

        let outcome = run_record(dir.path(), &records);

        assert!(outcome.is_ok(), "{:?}", outcome.diagnostics);
        // The committed store is discovered by the engine with no explicit
        // override — the whole point of the curation face.
        let compute = run_compute(dir.path(), None);
        assert!(compute.is_ok(), "{:?}", compute.diagnostics);
        assert_eq!(compute.records, 3);
        let feature = compute
            .entries
            .iter()
            .find(|entry| entry.id == "LANG-EVIDENCE-A")
            .expect("feature entry");
        assert_eq!(feature.computed_level, ledger_schema::Level::L3);
    }
}
