use super::executor::{TestOutcome, TestResult};
use super::plan::TestSummary;
use std::io::Write;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
/// `TestReporter`.
pub struct TestReporter {
    verbose: bool,
}

impl TestReporter {
    /// new.
    #[must_use]
    pub fn new(verbose: bool) -> Self {
        Self { verbose }
    }

    /// report.
    #[must_use]
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
                        let _ = writeln!(stderr, "\n{reason}");
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
            // Loud failure: a dropped JSON report is a real defect (the
            // consumer may gate on it), so it prints a miette-rendered
            // diagnostic instead of vanishing into a `let _ =`.
            if let Err(err) = self.write_json(results, &summary) {
                let report =
                    miette::Report::msg(format!("JSON test report could not be written: {err}"));
                eprintln!("{report:?}");
                eprintln!("help: ensure the working directory is writable and `target/` exists");
            }
        }

        summary
    }

    fn write_status(&self, stream: &mut StandardStream, status: &str, color: Color) {
        let _ = stream.set_color(ColorSpec::new().set_fg(Some(color)).set_bold(true));
        let _ = write!(stream, "{status:>8} ");
        let _ = stream.reset();
    }

    /// Serialises the run to `target/test-results.json` when
    /// `GLYIM_TEST_JSON` is set.
    ///
    /// Errors are returned, not swallowed: a run whose JSON report was
    /// silently dropped used to look identical to one that wrote it —
    /// the M0 weak-error rail flags exactly that shape. Console write
    /// failures (broken pipe) still degrade gracefully via `let _ =`.
    fn write_json(&self, results: &[TestResult], summary: &TestSummary) -> std::io::Result<()> {
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

        let text = serde_json::to_string_pretty(&out).map_err(std::io::Error::other)?;
        std::fs::write("target/test-results.json", text)?;
        eprintln!("JSON results written to target/test-results.json");
        Ok(())
    }
}
