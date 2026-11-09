//! Evidence-record timing harness (M1.5).
//!
//! Dependency-free (`harness = false`), mirroring the M1.4
//! `annotations` bench: measures the per-fixture evidence cost the
//! readiness ledger adds to a run so CI logs can track headroom.
//! `EvidenceRecord::from_result` + serialisation runs once per fixture
//! at the end of a harness pass, and `write_evidence` re-validates the
//! whole slice before writing. Run with:
//!
//! ```text
//! cargo bench -p glyim-test --bench evidence
//! ```

use std::hint::black_box;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use glyim_test::harness::collector::DiscoveredTest;
use glyim_test::harness::config::TestConfig;
use glyim_test::harness::evidence::{
    EvidenceBackend, EvidenceRecord, opt_level_from_flags, write_evidence,
};

/// Mirrors the real corpus shape (375 fixtures) so the totals read as
/// "cost of one full run's evidence pass".
const FIXTURES: usize = 375;

/// Builds a synthetic discovered fixture with the same rich `//@`
/// header shape the M1.4 codemod produces (a few features + backends).
fn synthetic_test(index: usize) -> Arc<DiscoveredTest> {
    let name = format!("bench_fixture_{index}");
    let config = TestConfig {
        features: vec![
            format!("LANG-SYN-OPERATORS-{index}"),
            "STD-ALLOC-VEC".to_string(),
        ],
        backends: vec!["gam".to_string(), "llvm".to_string()],
        compile_flags: vec!["--with-stdlib".to_string(), "-O2".to_string()],
        ..TestConfig::default()
    };
    Arc::new(DiscoveredTest {
        path: PathBuf::from(format!("tests/run-pass/{name}.g")),
        name,
        config,
        source: Arc::from(format!("fn main() -> i32 {{ {index} }}\n").as_str()),
        revisions: vec![String::new()],
    })
}

fn main() {
    let tests: Vec<_> = (0..FIXTURES).map(synthetic_test).collect();
    let backend = EvidenceBackend::Gam;

    // Phase 1: record construction (sha256 + field mapping), the cost
    // every executed fixture pays once.
    let t = Instant::now();
    let mut hashed = 0usize;
    for (i, test) in tests.iter().enumerate() {
        let mut result = build_result(test.clone());
        // One failed record per 8th fixture so reason-class mapping is
        // exercised on the hot path too.
        if i % 8 == 0 {
            result.outcome = glyim_test::harness::executor::TestOutcome::Ignored;
        }
        let opt = opt_level_from_flags(&test.config.compile_flags);
        let record = EvidenceRecord::from_result(black_box(&result), backend, opt);
        hashed += black_box(record.name).len();
        drop(result);
    }
    let build = t.elapsed();

    // Phase 2: serialisation to JSONL (per-record, includes validation
    // input for the write path below).
    let records: Vec<EvidenceRecord> = tests
        .iter()
        .map(|test| {
            let opt = opt_level_from_flags(&test.config.compile_flags);
            EvidenceRecord::from_result(&build_result(test.clone()), backend, opt)
        })
        .collect();
    let t = Instant::now();
    let mut bytes = 0usize;
    for record in &records {
        let line = record.to_jsonl_line();
        bytes += black_box(line.len());
    }
    let serialise = t.elapsed();

    // Phase 3: full write (validate + JSONL to a scratch file), the
    // once-per-run tail cost.
    let out = std::env::temp_dir().join("glyim-evidence-bench.jsonl");
    let t = Instant::now();
    let written = write_evidence(black_box(&out), black_box(&records))
        .expect("bench write into the system temp dir must succeed");
    let write = t.elapsed();
    let _ = std::fs::remove_file(&out);

    println!("evidence bench over {FIXTURES} fixtures");
    println!("  from_result : {build:?}");
    println!("  to_jsonl    : {serialise:?} ({bytes} bytes)");
    println!("  write_evidence: {write:?} ({written} bytes)");
    let _ = hashed;
}

/// Wraps a discovered fixture in a passing `TestResult` (the bench does
/// not execute the pipeline; the record shape is what is measured).
fn build_result(test: Arc<DiscoveredTest>) -> glyim_test::harness::executor::TestResult {
    use glyim_test::harness::executor::{TestOutcome, TestResult};
    TestResult {
        test,
        revision: String::new(),
        outcome: TestOutcome::Passed,
        duration: std::time::Duration::from_millis(1),
        diagnostics: Vec::new(),
    }
}
