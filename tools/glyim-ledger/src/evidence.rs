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
//! - `glyim-ledger evidence audit [ROOT] [--format text|json]` — audit
//!   the committed store against the filesystem: recompute every record's
//!   `fixture_sha256` from the fixture bytes at its resolved path and
//!   report per-record verdicts (`ok` / `missing` / `drift` /
//!   `unreadable`). `check` proves what the store *says* is consistent;
//!   audit proves what the store *points at* still exists and still
//!   hashes to what was recorded — a store that drifted from its fixtures
//!   fails the exit code (the trust-loop closure the check face cannot
//!   express).
//! - [`EvidenceStore`] — the storage seam (the write counterpart of the
//!   read-side [`crate::compute::EvidenceSource`]): [`FileEvidenceStore`]
//!   for the local JSONL layout, [`InMemoryEvidenceStore`] for tests and
//!   in-process consumers.
//! - [`FixtureFingerprint`] — the hashing seam: [`FileFixtureFingerprint`]
//!   reads the fixture bytes and digests them, [`InMemoryFixtureFingerprint`]
//!   serves precomputed digests for tests and programmatic consumers.
//!
//! Determinism contract (plan C1/M1.13): the store is a set of whole
//! records in canonical (schema-serialized) order — merging is
//! order-insensitive and idempotent, so re-recording the same harness run
//! produces byte-identical files.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Write as _;
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

// -- audit face --------------------------------------------------------------

/// Schema tag of the `evidence audit` JSON contract.
pub const EVIDENCE_AUDIT_SCHEMA: &str = "glyim.ledger-evidence-audit/1";

/// Why one fixture's bytes could not be produced: the path does not
/// exist ([`FingerprintError::NotFound`]) or exists but cannot be read
/// ([`FingerprintError::Unreadable`]). The engine maps these to verdicts,
/// so the seam stays IO-shaped (the [`FixtureFingerprint`] counterpart of
/// the `Result<_, String>` seams).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FingerprintError {
    /// The fixture path does not exist (an honest finding, not an
    /// engine failure — the store survived, the tree moved on).
    NotFound,
    /// The fixture exists but its bytes could not be read.
    Unreadable(String),
}

/// The hashing seam: produce one fixture file's `fixture_sha256` hex
/// digest. The engine consumes the trait, so tests inject precomputed
/// digests ([`InMemoryFixtureFingerprint`]) and the CLI wires the
/// file-backed implementation ([`FileFixtureFingerprint`]). New digest
/// sources plug in as one more impl, never as outcome fields.
pub trait FixtureFingerprint {
    /// Digests the fixture at `path` (64 lowercase hex chars) or reports
    /// why its bytes could not be produced.
    fn sha256_hex(&self, path: &Path) -> Result<String, FingerprintError>;
}

/// Digests fixture files on disk (`sha256` over the raw bytes): the
/// committed store's fixture paths resolve against the project root, and
/// the recomputed digest is what the drift verdict compares against.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileFixtureFingerprint;

impl FixtureFingerprint for FileFixtureFingerprint {
    fn sha256_hex(&self, path: &Path) -> Result<String, FingerprintError> {
        use sha2::{Digest, Sha256};
        use std::fmt::Write as _;
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(FingerprintError::NotFound);
            }
            Err(err) => return Err(FingerprintError::Unreadable(err.to_string())),
        };
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let digest = hasher.finalize();
        let mut hex = String::with_capacity(64);
        for byte in &digest {
            let _ = write!(hex, "{byte:02x}");
        }
        Ok(hex)
    }
}

/// Serves caller-pinned digests (tests and programmatic consumers); the
/// in-memory twin of [`FileFixtureFingerprint`]. A path absent from the
/// map reports [`FingerprintError::NotFound`] — the same verdict the
/// file-backed seam produces for a fixture the tree no longer carries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InMemoryFixtureFingerprint {
    /// `resolved fixture path -> digest` (64 lowercase hex chars).
    pub digests: BTreeMap<PathBuf, String>,
}

impl FixtureFingerprint for InMemoryFixtureFingerprint {
    fn sha256_hex(&self, path: &Path) -> Result<String, FingerprintError> {
        match self.digests.get(path) {
            Some(digest) => Ok(digest.clone()),
            None => Err(FingerprintError::NotFound),
        }
    }
}

/// The per-record audit verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FixtureVerdict {
    /// The recomputed digest matches the recorded one.
    Ok,
    /// The fixture file no longer exists at its resolved path.
    Missing,
    /// The fixture exists but hashes to a different digest — the record
    /// predates a fixture edit and no longer describes the tree.
    Drift,
    /// The fixture exists but its bytes could not be read.
    Unreadable,
}

impl FixtureVerdict {
    /// Canonical wire and table spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Missing => "missing",
            Self::Drift => "drift",
            Self::Unreadable => "unreadable",
        }
    }
}

/// One audited record (the JSON contract's row shape).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FixtureRow {
    /// The audited record's name (echoed verbatim).
    pub name: String,
    /// The fixture path as the record carries it (echoed verbatim).
    pub fixture: String,
    /// The recorded digest (echoed verbatim).
    pub expected_sha256: String,
    /// The recomputed digest; `None` when the fixture produced no bytes.
    pub actual_sha256: Option<String>,
    /// The computed verdict.
    pub verdict: FixtureVerdict,
    /// Human-readable detail for the non-`ok` verdicts (rendered in the
    /// text report).
    pub detail: Option<String>,
}

/// Result of a full `evidence audit` run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceAuditOutcome {
    /// Records audited (equal to [`Self::rows`] length).
    pub records: usize,
    /// Rows whose verdict is [`FixtureVerdict::Ok`].
    pub ok: usize,
    /// Rows whose verdict is [`FixtureVerdict::Missing`].
    pub missing: usize,
    /// Rows whose verdict is [`FixtureVerdict::Drift`].
    pub drift: usize,
    /// Rows whose verdict is [`FixtureVerdict::Unreadable`].
    pub unreadable: usize,
    /// One row per audited record, sorted by (fixture, name).
    pub rows: Vec<FixtureRow>,
    /// Load diagnostics (registry + store), sorted and de-duplicated —
    /// the same diagnostics `evidence check` would emit.
    pub diagnostics: Vec<Diagnostic>,
}

impl EvidenceAuditOutcome {
    /// Number of error-severity load diagnostics.
    #[must_use]
    pub fn errors(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    }

    /// True when the audit gate passes: no error-severity load diagnostic
    /// and no row whose verdict is anything but [`FixtureVerdict::Ok`].
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.errors() == 0 && self.missing == 0 && self.drift == 0 && self.unreadable == 0
    }
}

/// Resolves a record's fixture path against the project root: an
/// absolute path is the recording machine's own spelling and is audited
/// verbatim, a relative path hangs off `root` (the store's committed
/// spelling).
#[must_use]
pub fn resolve_fixture(root: &Path, fixture: &str) -> PathBuf {
    let path = Path::new(fixture);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

/// Audits triaged records against `fingerprint` (the pure verdict
/// function the run face consumes). Fixture paths resolve against
/// `root`; rows render in (fixture, name) order so the face is
/// deterministic for every input path.
#[must_use]
pub fn audit_records(
    root: &Path,
    records: &[LedgerEvidence],
    fingerprint: &dyn FixtureFingerprint,
) -> EvidenceAuditOutcome {
    let mut rows = Vec::with_capacity(records.len());
    let (mut ok, mut missing, mut drift, mut unreadable) = (0usize, 0usize, 0usize, 0usize);
    for record in records {
        let path = resolve_fixture(root, &record.fixture);
        let (verdict, actual, detail) = match fingerprint.sha256_hex(&path) {
            Ok(digest) => {
                if digest == record.fixture_sha256 {
                    (FixtureVerdict::Ok, Some(digest), None)
                } else {
                    (
                        FixtureVerdict::Drift,
                        Some(digest),
                        Some(format!(
                            "recorded {} but the fixture hashes to a different digest",
                            record.fixture_sha256
                        )),
                    )
                }
            }
            Err(FingerprintError::NotFound) => (
                FixtureVerdict::Missing,
                None,
                Some(format!(
                    "no fixture at {} (the store outlived the tree it described)",
                    path.display()
                )),
            ),
            Err(FingerprintError::Unreadable(why)) => (
                FixtureVerdict::Unreadable,
                None,
                Some(format!("cannot read {}: {why}", path.display())),
            ),
        };
        match verdict {
            FixtureVerdict::Ok => ok += 1,
            FixtureVerdict::Missing => missing += 1,
            FixtureVerdict::Drift => drift += 1,
            FixtureVerdict::Unreadable => unreadable += 1,
        }
        rows.push(FixtureRow {
            name: record.name.clone(),
            fixture: record.fixture.clone(),
            expected_sha256: record.fixture_sha256.clone(),
            actual_sha256: actual,
            verdict,
            detail,
        });
    }
    rows.sort_by(|a, b| a.fixture.cmp(&b.fixture).then(a.name.cmp(&b.name)));
    EvidenceAuditOutcome {
        records: rows.len(),
        ok,
        missing,
        drift,
        unreadable,
        rows,
        diagnostics: Vec::new(),
    }
}

/// Runs a full `evidence audit` pass over `root` through `source` and
/// `fingerprint`: loads the registry and the committed store exactly like
/// [`run_evidence_check`] (so the two faces can never disagree about what
/// the store contains), triages the records, and fingerprints every
/// fixture the surviving records point at.
#[must_use]
pub fn run_evidence_audit(
    root: &Path,
    source: &dyn EvidenceSource,
    fingerprint: &dyn FixtureFingerprint,
) -> EvidenceAuditOutcome {
    let span = tracing::info_span!("evidence_audit", root = %root.display());
    let _enter = span.enter();

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let loaded = ledger_schema::load_project_registry(root);
    diagnostics.extend(loaded.diagnostics);
    diagnostics.extend(loaded.registry.validate());

    let batch = source.load();
    diagnostics.extend(batch.diagnostics);

    let unique = dedup_records(&batch.records);
    let valid = triage_records(
        &unique.into_iter().cloned().collect::<Vec<_>>(),
        &loaded.registry,
        &mut diagnostics,
    );

    let mut outcome = audit_records(root, &valid, fingerprint);
    outcome.diagnostics = diagnostics;
    finish_diagnostics(&mut outcome.diagnostics);
    tracing::debug!(
        records = outcome.records,
        ok = outcome.ok,
        missing = outcome.missing,
        drift = outcome.drift,
        unreadable = outcome.unreadable,
        diagnostics = outcome.diagnostics.len(),
        "evidence audit pass finished"
    );
    outcome
}

/// Renders the `glyim.ledger-evidence-audit/1` JSON contract.
#[must_use]
pub fn format_evidence_audit_json(outcome: &EvidenceAuditOutcome) -> String {
    #[derive(Serialize)]
    struct AuditJson<'a> {
        schema: &'static str,
        records: usize,
        ok: usize,
        missing: usize,
        drift: usize,
        unreadable: usize,
        passed: bool,
        rows: &'a [FixtureRow],
    }
    let document = AuditJson {
        schema: EVIDENCE_AUDIT_SCHEMA,
        records: outcome.records,
        ok: outcome.ok,
        missing: outcome.missing,
        drift: outcome.drift,
        unreadable: outcome.unreadable,
        passed: outcome.is_ok(),
        rows: &outcome.rows,
    };
    let mut text = serde_json::to_string_pretty(&document)
        .expect("the glyim.ledger-evidence-audit/1 document is a serialisation-only struct");
    text.push('\n');
    text
}

/// Renders the audit outcome's text face (the [`format_evidence_check_text`]
/// shape: summary line, one line per non-`ok` row, diagnostics last).
#[must_use]
pub fn format_evidence_audit_text(outcome: &EvidenceAuditOutcome) -> String {
    let mut text = String::new();
    let _ = writeln!(
        text,
        "audit: {} record(s), {} ok, {} missing, {} drift, {} unreadable, \
         {} diagnostic(s), {}",
        outcome.records,
        outcome.ok,
        outcome.missing,
        outcome.drift,
        outcome.unreadable,
        outcome.diagnostics.len(),
        if outcome.is_ok() { "passed" } else { "FAILED" },
    );
    for row in &outcome.rows {
        if row.verdict == FixtureVerdict::Ok {
            continue;
        }
        let _ = writeln!(
            text,
            "  {:<28} {:<10} {}",
            row.name,
            row.verdict.as_str(),
            row.detail.as_deref().unwrap_or_default(),
        );
    }
    for diagnostic in &outcome.diagnostics {
        let _ = writeln!(text, "{diagnostic}");
    }
    text
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

// -- retract face -------------------------------------------------------------

/// Schema tag of the `evidence retract` JSON contract.
pub const EVIDENCE_RETRACT_SCHEMA: &str = "glyim.ledger-evidence-retract/1";

/// The `evidence retract` request (the CLI flags, validated by
/// [`run_evidence_retract`]): remove every store record whose
/// `fixture_sha256` matches the recomputed digest of one fixture — the
/// curation counterpart of the record face. When a fixture's bytes
/// change, the audit face reports its committed records as `Drift`;
/// until this face the remedy was hand-editing the canonical JSONL store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceRetractRequest {
    /// The fixture whose records are retracted, relative to the project
    /// root — the same repo-relative spelling the committed records carry
    /// (`tests/alpha.g`).
    pub fixture: String,
    /// The store file to rewrite (the record face's `--out` shape;
    /// `DEFAULT_RECORD_TARGET` by default).
    pub out: PathBuf,
}

/// Result of an `evidence retract` run (also the
/// `glyim.ledger-evidence-retract/1` JSON face via
/// [`format_evidence_retract_json`]).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct EvidenceRetractOutcome {
    /// Schema tag, always [`EVIDENCE_RETRACT_SCHEMA`].
    pub schema: &'static str,
    /// The retracted fixture path (as requested).
    pub fixture: String,
    /// The recomputed digest the records were matched by (64 lowercase
    /// hex chars — the same spelling the audit face compares).
    pub fixture_sha256: String,
    /// The store file the retraction rewrote.
    pub out: PathBuf,
    /// Records removed.
    pub removed: usize,
    /// Records left in the store.
    pub remaining: usize,
    /// Bytes persisted (canonical lines plus one newline each).
    pub bytes: u64,
}

/// Retracts one fixture's records: validates the request, refuses to
/// touch a store that does not load cleanly, and rewrites the store
/// canonically without the matching records.
///
/// # Validation contract
///
/// - `fixture` must be a relative, backslash-free path that never escapes
///   the project root (`..`) — the same usable-path rule the witness face
///   applies, so an absolute or escaping path is a rejection before any
///   digest is computed;
/// - the fixture must exist under `root` (the fingerprint needs its
///   bytes; a phantom fixture would silently retract nothing);
/// - the recomputed digest must succeed (the CLI wires
///   [`FileFixtureFingerprint`]; tests inject precomputed digests);
/// - the store must load with zero error-severity diagnostics — a store
///   that does not parse is never rewritten (the waiver-face load rule);
/// - the fixture must have at least one record in the store: a fixture
///   with nothing committed is a rejection naming the digest, not a
///   silent no-op (the grant-face duplicate rule's counterpart — the
///   operator probably mistyped the path).
///
/// Nothing is written when validation fails; the surviving records are
/// persisted in canonical form (the same [`EvidenceStore::save`] the
/// record face uses), so the rewrite is byte-deterministic.
///
/// # Errors
///
/// Typed [`LedgerCliError`] variants for every rejected request
/// (`EvidenceRetractBadFixture`, `EvidenceRetractMissingFixture`,
/// `EvidenceRetractFingerprint`, `EvidenceRetractLoad`,
/// `EvidenceRetractUnknownFixture`) and for a failed persist
/// (`LedgerCliError::EvidenceWrite`, the record face's seam).
pub fn run_evidence_retract(
    root: &Path,
    request: &EvidenceRetractRequest,
    store: &dyn EvidenceStore,
    fingerprint: &dyn FixtureFingerprint,
) -> Result<EvidenceRetractOutcome, LedgerCliError> {
    let span = tracing::info_span!(
        "evidence_retract",
        root = %root.display(),
        fixture = %request.fixture,
    );
    let _enter = span.enter();

    // The same usable-path rule the witness face applies: relative,
    // backslash-free, no `..` component, no control characters.
    let usable = !request.fixture.trim().is_empty()
        && !request.fixture.chars().any(char::is_control)
        && !request.fixture.contains('\\')
        && !Path::new(&request.fixture).is_absolute()
        && Path::new(&request.fixture).components().all(|component| {
            matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        });
    if !usable {
        return Err(LedgerCliError::EvidenceRetractBadFixture {
            fixture: request.fixture.clone(),
        });
    }
    let resolved = resolve_fixture(root, &request.fixture);
    if !resolved.is_file() {
        return Err(LedgerCliError::EvidenceRetractMissingFixture {
            fixture: request.fixture.clone(),
            path: resolved.display().to_string(),
        });
    }
    let digest = fingerprint.sha256_hex(&resolved).map_err(|failure| {
        LedgerCliError::EvidenceRetractFingerprint {
            fixture: request.fixture.clone(),
            detail: match failure {
                FingerprintError::NotFound => {
                    format!("{} not found", resolved.display())
                }
                FingerprintError::Unreadable(detail) => detail,
            },
        }
    })?;

    // The load rule: a store that does not parse cleanly is never
    // rewritten (the waiver-face rule; the corrupt line would be silently
    // destroyed by the canonical rewrite otherwise).
    let batch = store.load(&request.out);
    let failures: Vec<String> = batch
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .map(ToString::to_string)
        .collect();
    if !failures.is_empty() {
        return Err(LedgerCliError::EvidenceRetractLoad {
            detail: failures.join("; "),
        });
    }

    let (remaining, removed): (Vec<LedgerEvidence>, Vec<LedgerEvidence>) = batch
        .records
        .iter()
        .cloned()
        .partition(|record| record.fixture_sha256 != digest);
    if removed.is_empty() {
        return Err(LedgerCliError::EvidenceRetractUnknownFixture {
            fixture: request.fixture.clone(),
            sha256: digest,
        });
    }

    let bytes: u64 = remaining
        .iter()
        .map(|record| canonical_line(record).len() as u64 + 1)
        .sum();
    store
        .save(&request.out, &remaining)
        .map_err(|failure| LedgerCliError::EvidenceWrite {
            path: request.out.display().to_string(),
            detail: failure.message,
        })?;
    tracing::debug!(
        removed = removed.len(),
        remaining = remaining.len(),
        "evidence retraction finished"
    );

    Ok(EvidenceRetractOutcome {
        schema: EVIDENCE_RETRACT_SCHEMA,
        fixture: request.fixture.clone(),
        fixture_sha256: digest,
        out: request.out.clone(),
        removed: removed.len(),
        remaining: remaining.len(),
        bytes,
    })
}

/// Renders the retract outcome's text face: one summary line naming the
/// retracted fixture, its digest, and the store it rewrote.
#[must_use]
pub fn format_evidence_retract_text(outcome: &EvidenceRetractOutcome) -> String {
    use std::fmt::Write as _;
    let mut text = String::new();
    let _ = writeln!(
        text,
        "evidence retract: retracted {} record(s) for {} (sha256 {}) from {}",
        outcome.removed,
        outcome.fixture,
        outcome.fixture_sha256,
        outcome.out.display(),
    );
    let _ = writeln!(
        text,
        "  remaining: {} record(s), {} canonical bytes",
        outcome.remaining, outcome.bytes,
    );
    text
}

/// Renders the `glyim.ledger-evidence-retract/1` JSON contract.
#[must_use]
pub fn format_evidence_retract_json(outcome: &EvidenceRetractOutcome) -> String {
    let mut text = serde_json::to_string_pretty(outcome)
        .expect("the glyim.ledger-evidence-retract/1 document is a serialisation-only struct");
    text.push('\n');
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
        let swapped = merge_records(std::slice::from_ref(&b), &[a.clone(), b.clone()]);
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

    // -- audit face ---------------------------------------------------------

    use super::{
        EVIDENCE_AUDIT_SCHEMA, FileFixtureFingerprint, FixtureFingerprint, FixtureVerdict,
        InMemoryFixtureFingerprint, resolve_fixture, run_evidence_audit,
    };
    use crate::compute::ProjectEvidenceSource as AuditProjectSource;
    use crate::parse_args;
    use std::collections::BTreeMap;

    /// Seeds one fixture file under `<root>/tests/` and returns its real
    /// sha256 digest (the audit compares recomputed bytes, so the test
    /// fixtures carry real digests, not name-derived ones).
    fn seed_fixture(root: &Path, name: &str, body: &str) -> String {
        use sha2::{Digest, Sha256};
        use std::fmt::Write as _;
        fs::create_dir_all(root.join("tests")).expect("mkdir tests");
        fs::write(root.join("tests").join(name), body).expect("write fixture");
        let mut hasher = Sha256::new();
        hasher.update(body.as_bytes());
        let digest = hasher.finalize();
        digest
            .iter()
            .fold(String::with_capacity(64), |mut out, byte| {
                let _ = write!(out, "{byte:02x}");
                out
            })
    }

    /// A committed store carrying one record per fixture body, digested
    /// for real (the store the audit face reads).
    fn audited_root(root: &Path, fixtures: &[(&str, &str)]) -> Vec<super::FixtureRow> {
        let mut records = Vec::new();
        for (index, (name, body)) in fixtures.iter().enumerate() {
            let digest = seed_fixture(root, name, body);
            let mut entry = record(
                name,
                "compile-pass",
                EvidenceOutcome::Passed,
                &["LANG-EVIDENCE-A"],
            );
            entry.fixture = format!("tests/{name}");
            entry.fixture_sha256 = digest;
            entry.opt = index as u8;
            records.push(entry);
        }
        let store = root.join("ledger/evidence/test.jsonl");
        fs::create_dir_all(store.parent().expect("parent")).expect("mkdir store");
        fs::write(&store, records.iter().map(line).collect::<String>()).expect("write store");
        run_evidence_audit(
            root,
            &AuditProjectSource::new(root, None),
            &FileFixtureFingerprint,
        )
        .rows
    }

    #[test]
    fn audit_verdict_matrix_over_in_memory_digests() {
        use super::resolve_fixture as resolve;
        let root = Path::new("/proj");
        let mut digests = BTreeMap::new();
        digests.insert(
            resolve(root, "tests/ok.g"),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        );
        digests.insert(
            resolve(root, "tests/drift.g"),
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
        );
        let fingerprint = InMemoryFixtureFingerprint { digests };
        let mut ok = record(
            "ok",
            "compile-pass",
            EvidenceOutcome::Passed,
            &["LANG-EVIDENCE-A"],
        );
        ok.fixture = "tests/ok.g".to_string();
        ok.fixture_sha256 =
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string();
        let mut drift = record(
            "drift",
            "compile-pass",
            EvidenceOutcome::Passed,
            &["LANG-EVIDENCE-A"],
        );
        drift.fixture = "tests/drift.g".to_string();
        drift.fixture_sha256 =
            "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_string();
        let mut absent = record(
            "absent",
            "compile-pass",
            EvidenceOutcome::Passed,
            &["LANG-EVIDENCE-A"],
        );
        absent.fixture = "tests/gone.g".to_string();
        absent.fixture_sha256 =
            "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd".to_string();

        let outcome = super::audit_records(root, &[ok, drift, absent], &fingerprint);
        assert_eq!(outcome.records, 3);
        assert_eq!(
            (outcome.ok, outcome.drift, outcome.missing),
            (1, 1, 1),
            "one row per verdict class"
        );
        assert!(!outcome.is_ok(), "drift and missing fail the gate");
        let verdicts: BTreeMap<&str, FixtureVerdict> = outcome
            .rows
            .iter()
            .map(|row| (row.name.as_str(), row.verdict))
            .collect();
        assert_eq!(verdicts["ok"], FixtureVerdict::Ok);
        assert_eq!(verdicts["drift"], FixtureVerdict::Drift);
        assert_eq!(verdicts["absent"], FixtureVerdict::Missing);
        let absent_row = outcome
            .rows
            .iter()
            .find(|row| row.name == "absent")
            .expect("the absent row");
        assert!(
            absent_row.actual_sha256.is_none(),
            "a missing fixture produces no digest"
        );
    }

    #[test]
    fn audit_rows_sort_by_fixture_then_name() {
        let mut digests = BTreeMap::new();
        digests.insert(
            resolve_fixture(Path::new("/proj"), "tests/a.g"),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        );
        let fingerprint = InMemoryFixtureFingerprint { digests };
        let mut second = record(
            "zeta",
            "compile-pass",
            EvidenceOutcome::Passed,
            &["LANG-EVIDENCE-A"],
        );
        second.fixture = "tests/a.g".to_string();
        second.fixture_sha256 =
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string();
        let mut first = record(
            "alpha",
            "run-pass",
            EvidenceOutcome::Passed,
            &["LANG-EVIDENCE-A"],
        );
        first.fixture = "tests/a.g".to_string();
        first.fixture_sha256 =
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string();
        let outcome = super::audit_records(Path::new("/proj"), &[second, first], &fingerprint);
        let names: Vec<&str> = outcome.rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(
            names,
            ["alpha", "zeta"],
            "deterministic (fixture, name) order"
        );
    }

    #[test]
    fn resolve_fixture_hangs_relative_paths_off_root() {
        assert_eq!(
            resolve_fixture(Path::new("/proj"), "tests/x.g"),
            PathBuf::from("/proj/tests/x.g")
        );
        assert_eq!(
            resolve_fixture(Path::new("/proj"), "/abs/y.g"),
            PathBuf::from("/abs/y.g"),
            "absolute paths are the recording machine's own spelling"
        );
    }

    #[test]
    fn audit_passes_on_a_store_that_matches_the_tree() {
        let dir = tempfile::tempdir().expect("tmp");
        write_project(dir.path());
        let rows = audited_root(
            dir.path(),
            &[
                ("alpha.g", "// alpha fixture\n"),
                ("bravo.g", "// bravo fixture\n"),
            ],
        );
        assert_eq!(rows.len(), 2);
        assert!(
            rows.iter().all(|row| row.verdict == FixtureVerdict::Ok),
            "both fixtures hash to their recorded digests"
        );
    }

    #[test]
    fn audit_fails_when_the_tree_drifts_or_a_fixture_vanishes() {
        let dir = tempfile::tempdir().expect("tmp");
        write_project(dir.path());
        let rows = audited_root(
            dir.path(),
            &[
                ("alpha.g", "// alpha fixture\n"),
                ("bravo.g", "// bravo fixture\n"),
            ],
        );
        // The store survives; the tree moved on: one fixture edited, one
        // deleted. Both verdicts are findings, not engine failures.
        fs::write(
            dir.path().join("tests/alpha.g"),
            "// alpha fixture, edited\n",
        )
        .expect("drift the fixture");
        fs::remove_file(dir.path().join("tests/bravo.g")).expect("remove the fixture");
        let outcome = run_evidence_audit(
            dir.path(),
            &AuditProjectSource::new(dir.path(), None),
            &FileFixtureFingerprint,
        );
        assert!(!outcome.is_ok(), "drift and missing both fail the gate");
        assert_eq!(outcome.ok, 0);
        let verdicts: BTreeMap<&str, FixtureVerdict> = outcome
            .rows
            .iter()
            .map(|row| (row.name.as_str(), row.verdict))
            .collect();
        assert_eq!(verdicts["alpha.g"], FixtureVerdict::Drift);
        assert_eq!(verdicts["bravo.g"], FixtureVerdict::Missing);
        // The verdict matrix is unchanged from the pre-drift pass except
        // for the two findings (rows stay one per record).
        assert_eq!(outcome.records, rows.len());
    }

    #[test]
    fn audit_reports_an_absent_store_as_a_honest_zero() {
        let dir = tempfile::tempdir().expect("tmp");
        write_project(dir.path());
        let outcome = run_evidence_audit(
            dir.path(),
            &AuditProjectSource::new(dir.path(), None),
            &FileFixtureFingerprint,
        );
        assert_eq!(outcome.records, 0);
        assert!(outcome.is_ok(), "no store is zero coverage, not a failure");
    }

    #[test]
    fn audit_text_and_json_faces_render_deterministically() {
        let dir = tempfile::tempdir().expect("tmp");
        write_project(dir.path());
        audited_root(dir.path(), &[("alpha.g", "// alpha fixture\n")]);
        fs::write(
            dir.path().join("tests/alpha.g"),
            "// alpha fixture, edited\n",
        )
        .expect("drift the fixture");
        let outcome = run_evidence_audit(
            dir.path(),
            &AuditProjectSource::new(dir.path(), None),
            &FileFixtureFingerprint,
        );

        let text_a = super::format_evidence_audit_text(&outcome);
        let text_b = super::format_evidence_audit_text(&outcome);
        assert_eq!(text_a, text_b);
        assert!(text_a.contains("1 record(s)"));
        assert!(text_a.contains("0 missing, 1 drift"));
        assert!(text_a.contains("FAILED"));
        assert!(text_a.contains("alpha"), "the drifting row is named");

        let json_a = super::format_evidence_audit_json(&outcome);
        let json_b = super::format_evidence_audit_json(&outcome);
        assert_eq!(json_a, json_b);
        let document: serde_json::Value = serde_json::from_str(&json_a).expect("valid JSON");
        assert_eq!(document["schema"], EVIDENCE_AUDIT_SCHEMA);
        assert_eq!(document["records"], 1);
        assert_eq!(document["drift"], 1);
        assert_eq!(document["passed"], false);
        assert_eq!(document["rows"][0]["verdict"], "drift");
        assert!(json_a.ends_with('\n'));
    }

    #[test]
    fn parse_args_accepts_evidence_audit_flags() {
        let command = parse_args(
            ["evidence", "audit", "/tmp/repo", "--format", "json"]
                .into_iter()
                .map(String::from),
        )
        .expect("the audit flags parse");
        assert!(matches!(
            command,
            Command::EvidenceAudit {
                ref root,
                format: EvidenceFormat::Json,
            } if root == &PathBuf::from("/tmp/repo")
        ));
        let err = parse_args(
            ["evidence", "audit", "--format", "yaml"]
                .into_iter()
                .map(String::from),
        )
        .expect_err("unknown format fails");
        assert!(matches!(
            err,
            crate::LedgerCliError::EvidenceBadFormat { .. }
        ));
    }

    #[test]
    fn file_fingerprint_maps_missing_and_unreadable_paths() {
        use super::FingerprintError;
        let dir = tempfile::tempdir().expect("tmp");
        let missing = FileFixtureFingerprint.sha256_hex(&dir.path().join("nope.g"));
        assert!(matches!(missing, Err(FingerprintError::NotFound)));
        let seeded = seed_fixture(dir.path(), "real.g", "// bytes\n");
        let hashed = FileFixtureFingerprint
            .sha256_hex(&dir.path().join("tests/real.g"))
            .expect("a readable fixture digests");
        assert_eq!(hashed, seeded, "the recomputed digest matches sha256");
    }

    // -- retract face ---------------------------------------------------------

    use super::{
        EVIDENCE_RETRACT_SCHEMA, EvidenceRetractRequest, FileEvidenceStore,
        format_evidence_retract_json, format_evidence_retract_text, run_evidence_retract,
    };
    use crate::LedgerCliError;

    /// A record whose `fixture_sha256` is the real digest of the seeded
    /// fixture bytes (the retract face matches records by recomputed
    /// digest, so the test records carry the same spelling the harness
    /// emitter writes).
    fn record_for(
        root: &Path,
        name: &str,
        body: &str,
        mode: &str,
        outcome: EvidenceOutcome,
        features: &[&str],
    ) -> LedgerEvidence {
        let digest = seed_fixture(root, name, body);
        let mut seeded = record(name, mode, outcome, features);
        seeded.fixture = format!("tests/{name}.g");
        seeded.fixture_sha256 = digest;
        seeded
    }

    fn retract_request(root: &Path, fixture: &str) -> EvidenceRetractRequest {
        EvidenceRetractRequest {
            fixture: fixture.to_string(),
            out: root.join("ledger/evidence/test.jsonl"),
        }
    }

    #[test]
    fn retract_face_removes_exactly_the_matching_records() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        write_project(root);
        let out = root.join("ledger/evidence/test.jsonl");
        fs::create_dir_all(out.parent().expect("parent")).expect("mkdir");
        let alpha = record_for(
            root,
            "alpha.g",
            "// alpha\n",
            "compile-pass",
            EvidenceOutcome::Passed,
            &["LANG-EVIDENCE-A"],
        );
        let charlie = record_for(
            root,
            "alpha.g",
            "// alpha\n",
            "run-pass",
            EvidenceOutcome::Passed,
            &["LANG-EVIDENCE-A"],
        );
        let bravo = record(
            "bravo",
            "run-pass",
            EvidenceOutcome::Passed,
            &["LANG-EVIDENCE-A"],
        );
        fs::write(
            &out,
            format!("{}{}{}", line(&alpha), line(&charlie), line(&bravo)),
        )
        .expect("seed store");

        // alpha and charlie are two records of the same fixture (the
        // harness emits one record per face), so the retraction removes
        // both; bravo carries a name-derived digest that matches no
        // seeded fixture and survives.
        let outcome = run_evidence_retract(
            root,
            &retract_request(root, "tests/alpha.g"),
            &FileEvidenceStore,
            &FileFixtureFingerprint,
        )
        .expect("the retraction lands");
        assert_eq!(outcome.schema, EVIDENCE_RETRACT_SCHEMA);
        assert_eq!(outcome.fixture, "tests/alpha.g");
        assert_eq!(outcome.fixture_sha256, alpha.fixture_sha256);
        assert_eq!(outcome.removed, 2);
        assert_eq!(outcome.remaining, 1);

        // The surviving record is byte-identical in canonical form, and
        // the reloaded store carries exactly it.
        assert_eq!(fs::read_to_string(&out).expect("read back"), line(&bravo),);
        let reloaded = FileEvidenceStore.load(&out);
        assert_eq!(reloaded.records.len(), 1);
        assert_eq!(reloaded.records[0], bravo);
    }

    #[test]
    fn retract_face_rejects_a_bad_fixture_path_without_writing() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        write_project(root);
        for fixture in ["/abs/alpha.g", "../escape.g", "tests\\alpha.g", ""] {
            let request = retract_request(root, fixture);
            let err =
                run_evidence_retract(root, &request, &FileEvidenceStore, &FileFixtureFingerprint)
                    .expect_err("an unusable fixture path is rejected");
            assert!(matches!(
                err,
                LedgerCliError::EvidenceRetractBadFixture { .. }
            ));
        }
    }

    #[test]
    fn retract_face_rejects_a_missing_fixture_file() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        write_project(root);
        let request = retract_request(root, "tests/ghost.g");
        let err = run_evidence_retract(root, &request, &FileEvidenceStore, &FileFixtureFingerprint)
            .expect_err("a phantom fixture is rejected");
        assert!(matches!(
            err,
            LedgerCliError::EvidenceRetractMissingFixture { .. }
        ));
    }

    #[test]
    fn retract_face_propagates_fingerprint_failures() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        write_project(root);
        fs::create_dir_all(root.join("tests")).expect("mkdir");
        fs::write(root.join("tests/alpha.g"), "// alpha\n").expect("write fixture");
        let fingerprint = InMemoryFixtureFingerprint::default();
        let err = run_evidence_retract(
            root,
            &retract_request(root, "tests/alpha.g"),
            &FileEvidenceStore,
            &fingerprint,
        )
        .expect_err("an absent digest is a fingerprint failure");
        assert!(matches!(
            err,
            LedgerCliError::EvidenceRetractFingerprint { .. }
        ));
    }

    #[test]
    fn retract_face_refuses_a_store_that_does_not_load_cleanly() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        write_project(root);
        seed_fixture(root, "alpha.g", "// alpha\n");
        let out = root.join("ledger/evidence/test.jsonl");
        fs::create_dir_all(out.parent().expect("parent")).expect("mkdir");
        fs::write(&out, "not-json-at-all\n").expect("write broken store");
        let request = retract_request(root, "tests/alpha.g");
        let err = run_evidence_retract(root, &request, &FileEvidenceStore, &FileFixtureFingerprint)
            .expect_err("a store that does not parse is refused");
        assert!(matches!(err, LedgerCliError::EvidenceRetractLoad { .. }));
        // The broken store is never rewritten.
        assert_eq!(
            fs::read_to_string(&out).expect("read back"),
            "not-json-at-all\n"
        );
    }

    #[test]
    fn retract_face_rejects_an_unknown_fixture_without_writing() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        write_project(root);
        let out = root.join("ledger/evidence/test.jsonl");
        fs::create_dir_all(out.parent().expect("parent")).expect("mkdir");
        let bravo = record(
            "bravo",
            "run-pass",
            EvidenceOutcome::Passed,
            &["LANG-EVIDENCE-A"],
        );
        fs::write(&out, line(&bravo)).expect("seed store");
        seed_fixture(root, "alpha.g", "// alpha\n");
        let request = retract_request(root, "tests/alpha.g");
        let err = run_evidence_retract(root, &request, &FileEvidenceStore, &FileFixtureFingerprint)
            .expect_err("a fixture with no records is a rejection, not a no-op");
        assert!(matches!(
            err,
            LedgerCliError::EvidenceRetractUnknownFixture { .. }
        ));
        assert_eq!(fs::read_to_string(&out).expect("read back"), line(&bravo));
    }

    #[test]
    fn format_evidence_retract_text_names_the_retracted_fixture() {
        let outcome = super::EvidenceRetractOutcome {
            schema: EVIDENCE_RETRACT_SCHEMA,
            fixture: "tests/alpha.g".to_string(),
            fixture_sha256: "a".repeat(64),
            out: PathBuf::from("ledger/evidence/test.jsonl"),
            removed: 2,
            remaining: 1,
            bytes: 128,
        };
        let text = format_evidence_retract_text(&outcome);
        assert!(text.contains("retracted 2 record(s)"));
        assert!(text.contains("tests/alpha.g"));
        assert!(text.contains("ledger/evidence/test.jsonl"));
    }

    #[test]
    fn format_evidence_retract_json_parses_back_with_schema() {
        let outcome = super::EvidenceRetractOutcome {
            schema: EVIDENCE_RETRACT_SCHEMA,
            fixture: "tests/alpha.g".to_string(),
            fixture_sha256: "a".repeat(64),
            out: PathBuf::from("ledger/evidence/test.jsonl"),
            removed: 2,
            remaining: 1,
            bytes: 128,
        };
        let json = format_evidence_retract_json(&outcome);
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("the JSON contract parses");
        assert_eq!(
            parsed["schema"].as_str(),
            Some("glyim.ledger-evidence-retract/1")
        );
        assert_eq!(parsed["removed"].as_u64(), Some(2));
        assert_eq!(parsed["remaining"].as_u64(), Some(1));
    }

    #[test]
    fn both_evidence_retract_formatters_render_twice_byte_identical() {
        let outcome = super::EvidenceRetractOutcome {
            schema: EVIDENCE_RETRACT_SCHEMA,
            fixture: "tests/alpha.g".to_string(),
            fixture_sha256: "a".repeat(64),
            out: PathBuf::from("ledger/evidence/test.jsonl"),
            removed: 2,
            remaining: 1,
            bytes: 128,
        };
        assert_eq!(
            format_evidence_retract_text(&outcome),
            format_evidence_retract_text(&outcome)
        );
        assert_eq!(
            format_evidence_retract_json(&outcome),
            format_evidence_retract_json(&outcome)
        );
    }

    #[test]
    fn evidence_retract_error_variants_carry_code_and_help() {
        let variants = [
            LedgerCliError::EvidenceRetractBadFixture {
                fixture: "../escape.g".to_string(),
            },
            LedgerCliError::EvidenceRetractMissingFixture {
                fixture: "tests/ghost.g".to_string(),
                path: "/tmp/ghost.g".to_string(),
            },
            LedgerCliError::EvidenceRetractFingerprint {
                fixture: "tests/alpha.g".to_string(),
                detail: "not found".to_string(),
            },
            LedgerCliError::EvidenceRetractLoad {
                detail: "1: not a record".to_string(),
            },
            LedgerCliError::EvidenceRetractUnknownFixture {
                fixture: "tests/alpha.g".to_string(),
                sha256: "a".repeat(64),
            },
        ];
        for err in &variants {
            let code = miette::Diagnostic::code(err)
                .expect("every typed error carries a code")
                .to_string();
            assert!(
                code.starts_with("glyim-ledger::evidence_retract_"),
                "unexpected code {code}"
            );
            assert!(
                miette::Diagnostic::help(err).is_some(),
                "missing help() on {code}"
            );
        }
    }

    #[test]
    fn usage_lists_evidence_retract() {
        assert!(crate::usage().contains("evidence retract"));
    }

    #[test]
    fn parse_args_accepts_evidence_retract() {
        let command = crate::parse_args(
            ["evidence", "retract", "--fixture", "tests/alpha.g"]
                .into_iter()
                .map(String::from),
        )
        .expect("the flags parse");
        assert!(matches!(
            command,
            Command::EvidenceRetract {
                ref root,
                ref fixture,
                ref out,
                format: EvidenceFormat::Text,
            } if root.as_path() == Path::new(".")
                && fixture == "tests/alpha.g"
                && out == &PathBuf::from(super::DEFAULT_RECORD_TARGET)
        ));
        let command = crate::parse_args(
            [
                "evidence",
                "retract",
                "/tmp/repo",
                "--fixture",
                "tests/alpha.g",
                "--out",
                "ledger/evidence/other.jsonl",
                "--format",
                "json",
            ]
            .into_iter()
            .map(String::from),
        )
        .expect("the flags parse");
        assert!(matches!(
            command,
            Command::EvidenceRetract {
                ref root,
                ref fixture,
                ref out,
                format: EvidenceFormat::Json,
            } if root == &PathBuf::from("/tmp/repo")
                && fixture == "tests/alpha.g"
                && out == &PathBuf::from("ledger/evidence/other.jsonl")
        ));
    }

    #[test]
    fn parse_args_rejects_evidence_retract_missing_fixture() {
        let err = crate::parse_args(["evidence", "retract"].into_iter().map(String::from))
            .expect_err("missing --fixture");
        assert!(matches!(
            err,
            LedgerCliError::EvidenceRetractMissingField { .. }
        ));
    }
}
