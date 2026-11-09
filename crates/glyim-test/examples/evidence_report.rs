//! Library consumer example: the M1.5 evidence JSONL contract.
//!
//! Shows the two sides of the readiness-ledger evidence seam (plan
//! §3.4 / §6 M1.5):
//!
//! 1. the *producer* side — build `EvidenceRecord`s the way the test
//!    harness does and append them as JSONL (one record per line, no
//!    wall-clock data);
//! 2. the *consumer* side — re-read the file, validate each record
//!    against the schema, and aggregate reason classes the way
//!    `ledger compute` will.
//!
//! Run with:
//!
//! ```text
//! cargo run -p glyim-test --example evidence_report
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use glyim_test::harness::collector::DiscoveredTest;
use glyim_test::harness::config::{TestConfig, TestMode};
use glyim_test::harness::evidence::{
    EVIDENCE_ENV, EVIDENCE_SCHEMA, EvidenceBackend, EvidenceOutcome, EvidenceRecord,
    opt_level_from_flags, write_evidence,
};
use glyim_test::harness::executor::{TestOutcome, TestResult};

/// Builds one synthetic result the way the harness would after running
/// a fixture. `outcome` chooses the record's outcome; the failed case
/// carries the same reason class the real strategies produce.
fn synthetic_result(name: &str, outcome: TestOutcome) -> TestResult {
    let config = TestConfig {
        mode: TestMode::RunPass,
        features: vec![
            "LANG-SYN-OPERATORS".to_string(),
            "STD-ALLOC-VEC".to_string(),
        ],
        backends: vec!["gam".to_string(), "llvm".to_string()],
        ..TestConfig::default()
    };
    TestResult {
        test: Arc::new(DiscoveredTest {
            path: PathBuf::from(format!("tests/run-pass/{name}.g")),
            name: name.to_string(),
            config,
            source: Arc::from(format!("fn main() -> i32 {{ {name} }}\n").as_str()),
            revisions: vec![String::new()],
        }),
        revision: String::new(),
        outcome,
        // Wall-clock durations never enter evidence records.
        duration: std::time::Duration::from_millis(17),
        diagnostics: Vec::new(),
    }
}

fn main() {
    // --- Producer side -------------------------------------------------
    let results = [
        synthetic_result("alpha", TestOutcome::Passed),
        synthetic_result("beta", TestOutcome::Passed),
        synthetic_result(
            "gamma",
            TestOutcome::Failed {
                reason: glyim_test::FailureReason::RunFailed {
                    exit_code: Some(1),
                    expected_exit_code: Some(0),
                },
            },
        ),
        synthetic_result("delta", TestOutcome::Ignored),
    ];

    let backend = EvidenceBackend::Gam;
    let records: Vec<EvidenceRecord> = results
        .iter()
        .map(|result| {
            let opt = opt_level_from_flags(&result.test.config.compile_flags);
            EvidenceRecord::from_result(result, backend, opt)
        })
        .collect();

    let out = std::env::temp_dir().join("glyim-evidence-example.jsonl");
    let bytes = write_evidence(&out, &records)
        .expect("evidence write into the system temp dir must succeed");
    println!(
        "wrote {} records ({} bytes) — the same JSONL {} would emit",
        records.len(),
        bytes,
        EVIDENCE_ENV
    );

    // --- Consumer side -------------------------------------------------
    let text = std::fs::read_to_string(&out).expect("we just wrote this file");
    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for (lineno, line) in text.lines().enumerate() {
        let record = EvidenceRecord::from_jsonl_line(line)
            .unwrap_or_else(|err| panic!("line {} is not a valid record: {err}", lineno + 1));
        let issues = record.validate();
        assert!(
            issues.is_empty(),
            "line {} failed validation: {issues:?}",
            lineno + 1
        );
        let class = match record.outcome {
            EvidenceOutcome::Passed => "passed".to_string(),
            EvidenceOutcome::Failed => record.reason.unwrap_or_else(|| "failed".to_string()),
            EvidenceOutcome::Ignored => "ignored".to_string(),
        };
        *counts.entry(class).or_insert(0) += 1;
    }
    let _ = std::fs::remove_file(&out);

    println!("\nschema {EVIDENCE_SCHEMA} — reason-class aggregate:");
    for (class, count) in &counts {
        println!("  {class}: {count}");
    }
}
