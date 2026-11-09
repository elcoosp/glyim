//! Evidence records for the Readiness Ledger (plan §6 M1.5).
//!
//! `glyim-test` emits one JSONL record per executed fixture when
//! `GLYIM_TEST_EVIDENCE` names an output path. Records are the
//! machine-checked input of `ledger compute` (plan §3.4): status is
//! computed from what the machine observed, never typed by a human.
//!
//! Two properties are load-bearing and tested in
//! `src/tests/evidence_tests.rs`:
//!
//! 1. **Hermeticity** — a record contains no wall-clock or PRNG-derived
//!    data. `TestResult::duration` is a *reporting* convenience only and
//!    never reaches evidence; two runs of the same fixtures against the
//!    same compiler produce byte-identical JSONL, which is what the M1.8
//!    deterministic `compute` and the M1.13 reproducibility job need.
//! 2. **Strict schema** — records deserialise with `deny_unknown_fields`
//!    (same discipline as `ledger-schema`), outcome/backend are typed
//!    enums from the ledger's backend set (M1.4 `//@ backends:`), and
//!    fixture identity is a SHA-256 of the exact source bytes, so a
//!    stale record cannot be silently joined against a changed fixture.
//!
//! `instructions` / `rss_kb` are reserved for the M2 counting-allocator
//! instrumentation; M1.5 keeps them `None` so the schema is stable when
//! the numbers arrive.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::info_span;

use crate::harness::executor::{TestOutcome, TestResult};

/// Environment variable naming the JSONL evidence output path.
pub const EVIDENCE_ENV: &str = "GLYIM_TEST_EVIDENCE";

/// Schema tag pinned for the M1.5 record shape.
pub const EVIDENCE_SCHEMA: &str = "glyim.evidence/1";

/// The backend set the ledger registry declares for `//@ backends:` —
/// identical to the M1.4 annotation grammar's accepted values.
pub const LEDGER_BACKENDS: [&str; 4] = ["gam", "llvm", "cranelift", "bytecode"];

/// The backend a fixture actually ran on (typed; JSON spells it exactly
/// like the ledger set).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EvidenceBackend {
    /// The MIR interpreter ("Glyim Abstract Machine") oracle.
    Gam,
    /// The real LLVM codegen path.
    Llvm,
    /// Reserved backend tag from the ledger registry.
    Cranelift,
    /// The bytecode backend.
    Bytecode,
}

/// The observed outcome of one fixture execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EvidenceOutcome {
    /// The fixture passed its strategy checks.
    Passed,
    /// The fixture failed with a reason class.
    Failed,
    /// The fixture was skipped (unmet `needs-llvm` / `only-target` /
    /// `// ignore`).
    Ignored,
}

/// One fixture execution as observed by the harness.
///
/// Field order is load-bearing: `serde_json` serialises in declaration
/// order, and byte-identical output across runs requires the order to
/// be stable. Append-only evolution: add fields at the end, bump the
/// schema tag, never reorder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRecord {
    /// Schema tag (`glyim.evidence/1`); lets consumers reject records
    /// from a different layout instead of guessing.
    pub schema: String,
    /// Fixture name as the harness reports it (path-derived).
    pub name: String,
    /// Fixture path relative to the collection root.
    pub fixture: String,
    /// SHA-256 (hex) of the exact fixture source bytes.
    pub fixture_sha256: String,
    /// Revision tag the fixture ran under (empty = default revision).
    pub revision: String,
    /// Canonical mode name (`TestMode::dir_name`).
    pub mode: String,
    /// The observed outcome.
    pub outcome: EvidenceOutcome,
    /// Stable short reason class for failures (`reason_class`);
    /// deliberately *not* the free-form compiler output.
    pub reason: Option<String>,
    /// Backend the fixture actually ran on.
    pub backend: EvidenceBackend,
    /// Optimisation level derived from the fixture's compile flags.
    pub opt: u8,
    /// Ledger feature IDs the fixture declares (`//@ feature:`).
    pub features: Vec<String>,
    /// Backend IDs the fixture declares (`//@ backends:`).
    pub declared_backends: Vec<String>,
    /// Reserved for M2: instruction count (counting allocator).
    pub instructions: Option<u64>,
    /// Reserved for M2: peak RSS in KiB.
    pub rss_kb: Option<u64>,
}

impl EvidenceRecord {
    /// Builds the evidence record for one executed fixture.
    ///
    /// `backend`/`opt` describe the *execution configuration* the harness
    /// chose (backend from the compiler the executor built, opt level
    /// from the fixture's compile flags via [`opt_level_from_flags`]).
    /// Everything else is read from the result itself; the wall-clock
    /// `TestResult::duration` is deliberately ignored (hermeticity).
    #[must_use]
    pub fn from_result(result: &TestResult, backend: EvidenceBackend, opt: u8) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(result.test.source.as_bytes());
        let digest = hasher.finalize();
        let fixture_sha256: String =
            digest
                .iter()
                .fold(String::with_capacity(64), |mut out, byte| {
                    let _ = write!(out, "{byte:02x}");
                    out
                });
        Self {
            schema: EVIDENCE_SCHEMA.to_string(),
            name: result.test.name.clone(),
            fixture: result.test.path.display().to_string(),
            fixture_sha256,
            revision: result.revision.clone(),
            mode: result.test.config.mode.dir_name().to_string(),
            outcome: EvidenceOutcome::from(&result.outcome),
            reason: match &result.outcome {
                TestOutcome::Failed { reason } => Some(reason_class(reason)),
                _ => None,
            },
            backend,
            opt,
            features: result.test.config.features.clone(),
            declared_backends: result.test.config.backends.clone(),
            instructions: None,
            rss_kb: None,
        }
    }

    /// Serialises to one compact JSON line (with trailing newline).
    ///
    /// # Panics
    ///
    /// Never: the record is a plain data structure, so serialisation
    /// cannot fail — this is an invariant site, not a user-input path.
    #[must_use]
    pub fn to_jsonl_line(&self) -> String {
        let mut line = serde_json::to_string(self)
            .expect("EvidenceRecord is a plain data structure and cannot fail to serialise");
        line.push('\n');
        line
    }

    /// Parses one JSONL line strictly (`deny_unknown_fields`).
    ///
    /// # Errors
    ///
    /// Returns the serde error text (which names the offending field)
    /// when the line is not a valid `glyim.evidence/1` record.
    pub fn from_jsonl_line(line: &str) -> Result<Self, String> {
        serde_json::from_str(line).map_err(|e| e.to_string())
    }

    /// Checks the invariants serde cannot express: hash format and
    /// declared-backend membership in the ledger set.
    ///
    /// Returns human-readable issues (empty = valid); `write_evidence`
    /// refuses to persist records that fail this check so a schema bug
    /// upstream cannot poison the ledger's evidence store.
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
                "fixture_sha256 must be 64 lowercase hex chars, got {:?} (name: {})",
                self.fixture_sha256, self.name
            ));
        }
        for backend in &self.declared_backends {
            if !LEDGER_BACKENDS.contains(&backend.as_str()) {
                issues.push(format!(
                    "declared backend {backend:?} is outside the ledger set {:?} (name: {})",
                    LEDGER_BACKENDS, self.name
                ));
            }
        }
        issues
    }
}

impl From<&TestOutcome> for EvidenceOutcome {
    fn from(outcome: &TestOutcome) -> Self {
        match outcome {
            TestOutcome::Passed => Self::Passed,
            TestOutcome::Failed { .. } => Self::Failed,
            TestOutcome::Ignored => Self::Ignored,
        }
    }
}

/// Maps a failure to a stable, short reason class.
///
/// The free-form compiler output inside `FailureReason` variants is
/// deliberately dropped: evidence consumers aggregate over classes, and
/// a compiler-message refactor must not change ledger statistics.
#[must_use]
pub fn reason_class(reason: &crate::error::FailureReason) -> String {
    use crate::error::FailureReason as F;
    match reason {
        F::CompilePassUnexpectedErrors { .. } => "compile-pass produced errors".to_string(),
        F::AnnotationParseError { .. } => "annotation parse error".to_string(),
        F::DiagnosticMismatch { .. } => "diagnostic mismatch".to_string(),
        F::ErrorPatternNotFound { .. } => "error pattern not found".to_string(),
        F::UiOutputDiffers { .. } => "ui output differs".to_string(),
        F::UiNoExpectedFile { .. } => "ui expected file missing".to_string(),
        F::TimeoutExceeded { .. } | F::RunTimeout { .. } => "timeout exceeded".to_string(),
        F::CompilationFailed { phase, .. } => format!("compilation failed at {phase}"),
        F::NeedUnsatisfiable { need, .. } => format!("need unsatisfiable: {need}"),
        F::RunFailed { .. } => "run failed".to_string(),
        F::StdoutMismatch { .. } => "stdout mismatch".to_string(),
        F::StderrMismatch { .. } => "stderr mismatch".to_string(),
    }
}

/// Derives the optimisation level from a fixture's compile flags.
///
/// `-O2`/`-O3`… parse directly; bare `-O` means level 2 (the CLI
/// convention); anything else is 0.
#[must_use]
pub fn opt_level_from_flags(flags: &[String]) -> u8 {
    for flag in flags {
        if let Some(rest) = flag.strip_prefix("-O") {
            if rest.is_empty() {
                return 2;
            }
            if let Ok(level) = rest.parse::<u8>() {
                return level;
            }
        }
    }
    0
}

/// A loud, actionable evidence error (the fix for the silent
/// `let _ = std::fs::write` seam this module replaces).
#[derive(Debug)]
pub enum EvidenceError {
    /// The evidence path could not be written.
    Io {
        /// The path that failed.
        path: std::path::PathBuf,
        /// The underlying IO error.
        source: std::io::Error,
    },
    /// Records failed schema validation; nothing was written.
    Schema {
        /// The path evidence would have been written to.
        path: std::path::PathBuf,
        /// The validation issues, one per line.
        issues: String,
    },
}

impl std::fmt::Display for EvidenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "cannot write evidence to {}: {source}", path.display())
            }
            Self::Schema { path, issues } => write!(
                f,
                "evidence records failed schema validation for {}: {issues}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for EvidenceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Schema { .. } => None,
        }
    }
}

impl miette::Diagnostic for EvidenceError {
    fn help<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        match self {
            Self::Io { .. } => Some(Box::new(format!(
                "set {EVIDENCE_ENV} to a writable path (the harness creates the file, \
                 but its parent directory must already exist)"
            ))),
            Self::Schema { .. } => Some(Box::new(
                "records must satisfy the M1.5 schema: 64-hex SHA-256 fixture hashes and \
                 `//@ backends:` values from the ledger set (gam, llvm, cranelift, \
                 bytecode). Re-run `cargo run -p glyim-test --bin glyim-tag-fixtures` \
                 if a fixture carries a stale backend tag."
                    .to_string(),
            )),
        }
    }

    fn severity(&self) -> Option<miette::Severity> {
        Some(miette::Severity::Error)
    }
}

/// Validates `records` and writes them as JSONL (one line per record,
/// trailing newline) to `path`, returning the byte count.
///
/// # Errors
///
/// [`EvidenceError::Schema`] when any record fails [`EvidenceRecord::validate`]
/// (nothing is written); [`EvidenceError::Io`] when the file cannot be
/// created or written.
pub fn write_evidence(path: &Path, records: &[EvidenceRecord]) -> Result<u64, EvidenceError> {
    let _span = info_span!(
        "evidence_write",
        records = records.len(),
        path = %path.display()
    )
    .entered();

    let issues: Vec<String> = records.iter().flat_map(EvidenceRecord::validate).collect();
    if !issues.is_empty() {
        return Err(EvidenceError::Schema {
            path: path.to_path_buf(),
            issues: issues.join("; "),
        });
    }

    let mut body = String::new();
    for record in records {
        body.push_str(&record.to_jsonl_line());
    }

    let file = std::fs::File::create(path).map_err(|source| EvidenceError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut writer = std::io::BufWriter::new(file);
    writer
        .write_all(body.as_bytes())
        .and_then(|()| writer.flush())
        .map_err(|source| EvidenceError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    let bytes = body.len() as u64;
    tracing::info!(records = records.len(), bytes, "evidence written");
    Ok(bytes)
}
