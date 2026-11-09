//! M1.5 evidence-record contract tests (plan §6 M1.5: "glyim-test emits
//! evidence JSONL"; `DoD`: evidence schema validated; run is hermetic —
//! no clock/PRNG in records).
//!
//! These tests pin the JSONL seam the readiness ledger's `compute` will
//! consume (plan §3.4): typed outcome/backend enums, SHA-256 fixture
//! hashes, byte-deterministic serialisation, and strict (deny-unknown)
//! reading.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use glyim_diag::GlyimDiagnostic;

use crate::harness::collector::DiscoveredTest;
use crate::harness::config::TestConfig;
use crate::harness::evidence::{
    EVIDENCE_ENV, EvidenceBackend, EvidenceError, EvidenceOutcome, EvidenceRecord,
    opt_level_from_flags, write_evidence,
};
use crate::harness::executor::{TestOutcome, TestResult};

/// A minimal passing result for one in-memory fixture.
fn result_with(name: &str, source: &str, config: TestConfig) -> TestResult {
    let test = Arc::new(DiscoveredTest {
        path: PathBuf::from(format!("tests/run-pass/{name}.g")),
        name: name.to_string(),
        config,
        source: Arc::from(source),
        revisions: vec![String::new()],
    });
    TestResult {
        test,
        revision: String::new(),
        outcome: TestOutcome::Passed,
        duration: Duration::from_millis(123),
        diagnostics: Vec::<GlyimDiagnostic>::new(),
    }
}

fn passing_result(name: &str, source: &str) -> TestResult {
    result_with(name, source, TestConfig::default())
}

#[test]
fn evidence_env_name_is_stable() {
    assert_eq!(EVIDENCE_ENV, "GLYIM_TEST_EVIDENCE");
}

#[test]
fn record_from_result_hashes_fixture_source() {
    let source = "fn main() -> i32 { 42 }\n";
    let config = TestConfig {
        mode: crate::harness::config::TestMode::RunPass,
        features: vec!["STD-ALLOC-VEC".to_string()],
        backends: vec!["gam".to_string(), "llvm".to_string()],
        ..TestConfig::default()
    };
    let result = result_with("baseline_arith", source, config);
    let record = EvidenceRecord::from_result(&result, EvidenceBackend::Gam, 0);
    assert_eq!(record.name, "baseline_arith");
    assert_eq!(record.mode, "run-pass");
    assert_eq!(record.outcome, EvidenceOutcome::Passed);
    assert_eq!(record.backend, EvidenceBackend::Gam);
    assert_eq!(record.opt, 0);
    assert!(record.reason.is_none());
    // SHA-256 of the exact fixture source, hex-encoded.
    assert_eq!(
        record.fixture_sha256,
        "ad80dac19c78782db39060a076cc8b85df1627224548f6dc0e01df74f59ccfef"
    );
    assert_eq!(record.features, vec!["STD-ALLOC-VEC".to_string()]);
    assert_eq!(
        record.declared_backends,
        vec!["gam".to_string(), "llvm".to_string()]
    );
    // M2 instruments instruction counts / RSS; M1.5 reserves the fields.
    assert_eq!(record.instructions, None);
    assert_eq!(record.rss_kb, None);
}

#[test]
fn failed_result_carries_reason_class() {
    let mut result = passing_result("mismatch", "fn main() {}");
    result.outcome = TestOutcome::Failed {
        reason: crate::error::FailureReason::CompilePassUnexpectedErrors {
            errors: vec!["[T0001] mismatched types".to_string()],
        },
    };
    let record = EvidenceRecord::from_result(&result, EvidenceBackend::Gam, 0);
    assert_eq!(record.outcome, EvidenceOutcome::Failed);
    let reason = record.reason.expect("failed records carry a reason");
    assert!(
        reason.contains("compile-pass produced errors"),
        "reason class must be a stable short class, got: {reason}"
    );
    // The free-form compiler output must not leak into the class field.
    assert!(!reason.contains("mismatched types"));
}

#[test]
fn serialisation_is_byte_deterministic_and_hermetic() {
    let source = "fn main() { loop { break; } }\n";
    let mut result = passing_result("hermetic", source);
    // A wall-clock duration is deliberately part of TestResult; it must
    // never reach the evidence record (M1.5 hermeticity DoD).
    result.duration = Duration::from_secs_f64(0.019_283_746);
    let record = EvidenceRecord::from_result(&result, EvidenceBackend::Llvm, 2);
    let first = record.to_jsonl_line();
    let second = record.to_jsonl_line();
    assert_eq!(first, second, "same record must serialise byte-identically");
    for banned in ["duration", "timestamp", "elapsed", "instant"] {
        assert!(
            !first.to_ascii_lowercase().contains(banned),
            "hermetic records must not contain clock-derived fields, saw {banned} in {first}"
        );
    }
}

#[test]
fn jsonl_roundtrip_and_deny_unknown_fields() {
    let source = "fn main() -> i32 { 1 }\n";
    let record = EvidenceRecord::from_result(
        &passing_result("roundtrip", source),
        EvidenceBackend::Gam,
        0,
    );
    let line = record.to_jsonl_line();
    let parsed = EvidenceRecord::from_jsonl_line(&line).expect("a written record must read back");
    assert_eq!(parsed, record);

    let junk = r#"{"schema":"glyim.evidence/1","name":"x","fixture_sha256":"00","wall_ms":42}"#;
    let err = EvidenceRecord::from_jsonl_line(junk)
        .expect_err("unknown fields must be rejected (schema discipline)");
    assert!(
        err.contains("unknown field"),
        "deny-unknown must name the offending field, got: {err}"
    );
}

#[test]
fn validator_rejects_bad_sha256_and_unknown_backend_spelling() {
    let source = "fn main() {}\n";
    let record =
        EvidenceRecord::from_result(&passing_result("sha", source), EvidenceBackend::Gam, 0);
    let mut bad_hash = record.clone();
    bad_hash.fixture_sha256 = "not-hex".to_string();
    let issues = bad_hash.validate();
    assert!(
        issues.iter().any(|i| i.contains("sha256")),
        "validator must flag a malformed hash: {issues:?}"
    );

    let mut bad_backend =
        EvidenceRecord::from_result(&passing_result("bk", source), EvidenceBackend::Gam, 0);
    bad_backend.declared_backends = vec!["wasm".to_string()];
    let issues = bad_backend.validate();
    assert!(
        issues.iter().any(|i| i.contains("wasm")),
        "validator must flag a backend outside the ledger set: {issues:?}"
    );
}

#[test]
fn writer_writes_one_line_per_record_with_trailing_newline() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("evidence.jsonl");
    let source = "fn main() -> i32 { 7 }\n";
    let records = vec![
        EvidenceRecord::from_result(&passing_result("a", source), EvidenceBackend::Gam, 0),
        EvidenceRecord::from_result(&passing_result("b", source), EvidenceBackend::Gam, 0),
    ];
    let bytes = write_evidence(&path, &records).expect("write into a fresh tempdir succeeds");
    let written = std::fs::read(&path).expect("file exists after write");
    assert_eq!(bytes as usize, written.len(), "reported byte count matches");
    let text = String::from_utf8(written).expect("jsonl is utf-8");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "one line per record");
    assert!(text.ends_with('\n'), "jsonl ends with a newline");
    for line in lines {
        EvidenceRecord::from_jsonl_line(line).expect("every line parses");
    }
}

#[test]
fn writer_reports_io_errors_with_remedy_help() {
    // A path whose parent does not exist must fail loudly, not silently
    // (this is the weak-error seam the M0 rails flag): the error must
    // render with a miette `help` remedy.
    let path = PathBuf::from("/nonexistent-dir-for-glyim-evidence/evidence.jsonl");
    let records = vec![EvidenceRecord::from_result(
        &passing_result("a", "fn main() {}\n"),
        EvidenceBackend::Gam,
        0,
    )];
    let err: EvidenceError =
        write_evidence(&path, &records).expect_err("missing parent dir must be an error");
    let report = miette::Report::new(err);
    let rendered = format!("{report:?}");
    // The workspace's miette has no `fancy` feature, so the narratable
    // rendering is what consumers see; it must carry the message, the
    // severity, and the remedy.
    assert!(
        rendered.contains("GLYIM_TEST_EVIDENCE"),
        "help text must name the env var remedy: {rendered}"
    );
    assert!(
        rendered.contains("severity: \"Error\""),
        "diagnostic must render as an error: {rendered}"
    );
    assert!(
        rendered.contains("cannot write evidence"),
        "diagnostic must include the message: {rendered}"
    );
}

#[test]
fn opt_level_reads_compile_flags() {
    assert_eq!(opt_level_from_flags(&[]), 0);
    assert_eq!(opt_level_from_flags(&["--emit=mir".to_string()]), 0);
    assert_eq!(opt_level_from_flags(&["-O2".to_string()]), 2);
    assert_eq!(opt_level_from_flags(&["-O".to_string()]), 2);
    assert_eq!(opt_level_from_flags(&["-O0".to_string()]), 0);
    assert_eq!(
        opt_level_from_flags(&["--emit=mir".to_string(), "-O3".to_string()]),
        3
    );
}

#[test]
fn schema_version_is_pinned() {
    let source = "fn main() {}\n";
    let record =
        EvidenceRecord::from_result(&passing_result("sv", source), EvidenceBackend::Gam, 0);
    assert_eq!(record.schema, "glyim.evidence/1");
    let line = record.to_jsonl_line();
    assert!(line.contains(r#""schema":"glyim.evidence/1""#));
}
