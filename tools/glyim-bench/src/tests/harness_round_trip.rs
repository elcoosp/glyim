//! End-to-end `glyim.bench/1` contract tests: a real (trivial) child
//! process, the full runner path, strict deserialisation, and byte-determinism
//! across two independent runs of the same plan.

use crate::error::BenchError;
use crate::report::{BENCH_SCHEMA, BenchReport};
use crate::runner::RunPlan;
use crate::source::MeasurementSource;

fn trivial_plan() -> RunPlan {
    RunPlan::new("sh")
        .with_iterations(3)
        .with_source(MeasurementSource::WallOnly)
        // The wall clock of a shell spawn is intentionally noisy; these tests
        // pin structure and determinism, not wall stability. The budget is a
        // large *finite* value on purpose: `f64::INFINITY` is not
        // JSON-representable (serde_json renders non-finite floats as
        // `null`, which then fails the report's own deserialisation), and
        // the round-trip tests below exercise that contract.
        .with_max_cov_pct(1_000.0)
        .push_arg("-c")
        .push_arg("exit 0")
}

#[test]
#[cfg(unix)]
fn runner_to_json_to_report_round_trips() {
    let report = crate::runner::run_iterated(&trivial_plan()).expect("runs");
    assert_eq!(report.schema, BENCH_SCHEMA);
    let json = report.to_json_pretty().expect("serialises");
    let back: BenchReport = serde_json::from_str(&json).expect("deserialises");
    assert_eq!(back, report);
}

#[test]
#[cfg(unix)]
fn two_runs_of_the_same_plan_serialise_identically_for_equal_samples() {
    // Determinism contract: the *renderer* is byte-deterministic for a given
    // profile of samples (the samples themselves differ between physical
    // runs — that is what the CoV gate is for). Feed both documents the same
    // samples by rendering one report twice and one deserialised copy.
    let report = crate::runner::run_iterated(&trivial_plan()).expect("runs");
    let json_a = report.to_json_pretty().expect("serialises");
    let copy: BenchReport = serde_json::from_str(&json_a).expect("deserialises");
    let json_b = copy.to_json_pretty().expect("serialises");
    assert_eq!(json_a, json_b);
    assert_eq!(report.render_text(), copy.render_text());
}

#[test]
#[cfg(unix)]
fn gate_cli_rejects_tampered_reports() {
    let report = crate::runner::run_iterated(&trivial_plan()).expect("runs");
    let mut tampered = report.clone();
    // Push the gate over any finite budget by faking a huge spread.
    tampered.gate.cov_pct = 99.0;
    tampered.gate.passed = false;
    let json = tampered.to_json_pretty().expect("serialises");
    let dir = std::env::temp_dir().join(format!("glyim-bench-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("tampered.json");
    std::fs::write(&path, &json).expect("write");
    let err = crate::cli::execute_gate(0.1, &path).expect_err("gate must fail");
    assert!(matches!(err, BenchError::GateFailed { .. }));
    let help = miette::Diagnostic::help(&err).expect("help");
    assert!(help.to_string().contains("--iterations"));
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir(&dir);
}

#[test]
#[cfg(unix)]
fn rss_column_is_collected_on_linux() {
    let report = crate::runner::run_iterated(&trivial_plan()).expect("runs");
    if cfg!(target_os = "linux") {
        assert!(
            report.samples.iter().all(|s| s.rss_kb.is_some()),
            "getrusage(RUSAGE_CHILDREN) must populate rss_kb on Linux"
        );
    }
}
