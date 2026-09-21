use super::executor::{TestOutcome, TestResult};
use super::plan::TestSummary;
use std::io::Write;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

/// TestReporter.
pub struct TestReporter {
    verbose: bool,
}

impl TestReporter {
    /// new.
    pub fn new(verbose: bool) -> Self {
        Self { verbose }
    }

    /// report.
    pub fn report(&self, results: &[TestResult]) -> TestSummary {
        let mut stderr = StandardStream::stderr(ColorChoice::Always);
        let mut summary = TestSummary::default();

        for result in results {
            summary.total += 1;
            match &result.outcome {
                TestOutcome::Passed => {
                    summary.passed += 1;
                    self.write_status(&mut stderr, "ok", Color::Green);
                }
                TestOutcome::Failed { reason } => {
                    summary.failed += 1;
                    self.write_status(&mut stderr, "FAILED", Color::Red);
                    if self.verbose {
                        let _ = writeln!(stderr, "\n{}", reason);
                    }
                }
                TestOutcome::Ignored => {
                    summary.ignored += 1;
                    self.write_status(&mut stderr, "IGNORED", Color::Yellow);
                }
            }
            let _ = writeln!(stderr, "{} [{}]", result.test.name, result.revision);
        }

        let _ = writeln!(stderr, "\n---");
        let _ = writeln!(
            stderr,
            "{} run, {} passed, {} failed, {} ignored",
            summary.total, summary.passed, summary.failed, summary.ignored,
        );

        if std::env::var("GLYIM_TEST_JSON").is_ok() {
            self.write_json(results, &summary);
        }

        summary
    }

    fn write_status(&self, stream: &mut StandardStream, status: &str, color: Color) {
        let _ = stream.set_color(ColorSpec::new().set_fg(Some(color)).set_bold(true));
        let _ = write!(stream, "{:>8} ", status);
        let _ = stream.reset();
    }

    #[cfg(feature = "json-output")]
    fn write_json(&self, results: &[TestResult], summary: &TestSummary) {
        use serde::Serialize;
        #[derive(Serialize)]
        struct Out {
            total: usize,
            passed: usize,
            failed: usize,
            ignored: usize,
            tests: Vec<Test>,
        }
        #[derive(Serialize)]
        struct Test {
            name: String,
            revision: String,
            outcome: String,
            duration_ms: u64,
            reason: Option<String>,
        }

        let tests: Vec<Test> = results
            .iter()
            .map(|r| {
                let (outcome, reason) = match &r.outcome {
                    TestOutcome::Passed => ("passed".into(), None),
                    TestOutcome::Failed { reason: fr } => ("failed".into(), Some(fr.to_string())),
                    TestOutcome::Ignored => ("ignored".into(), None),
                };
                Test {
                    name: r.test.name.clone(),
                    revision: r.revision.clone(),
                    outcome,
                    duration_ms: r.duration.as_millis() as u64,
                    reason,
                }
            })
            .collect();

        let out = Out {
            total: summary.total,
            passed: summary.passed,
            failed: summary.failed,
            ignored: summary.ignored,
            tests,
        };
        if let Ok(s) = serde_json::to_string_pretty(&out) {
            let _ = std::fs::write("target/test-results.json", s);
            eprintln!("JSON results written to target/test-results.json");
        }
    }

    #[cfg(not(feature = "json-output"))]
    fn write_json(&self, _results: &[TestResult], _summary: &TestSummary) {
        eprintln!("Note: Enable 'json-output' feature for JSON output");
    }
}
