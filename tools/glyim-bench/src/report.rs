//! The `glyim.bench/1` report document.
//!
//! Same discipline as the M1.5 evidence records: strict
//! (`deny_unknown_fields`) deserialisation, typed enums, and **byte-
//! deterministic** rendering for a given run — no wall-clock timestamps, no
//! PRNG data. Two runs of the same command with the same counters serialise
//! to identical JSON, which is what the M2.5 perf-history store diffs.

use crate::source::MeasurementSource;
use crate::stats::SampleStats;

/// Schema tag pinned for the report shape.
pub const BENCH_SCHEMA: &str = "glyim.bench/1";

/// One iteration's measurements. Counter columns are `None` when the
/// selected source does not provide them (and for allocation counters, which
/// only exist for in-process runs).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchSample {
    /// Wall-clock duration of the iteration, in nanoseconds.
    pub wall_ns: u64,
    /// Retired instructions (`perf stat` / callgrind).
    pub instructions: Option<u64>,
    /// CPU cycles (`perf stat` only).
    pub cycles: Option<u64>,
    /// Child peak RSS, in KiB (`getrusage(RUSAGE_CHILDREN)`, Linux only;
    /// the running maximum over reaped children).
    pub rss_kb: Option<u64>,
    /// Gross allocation events (in-process runs with
    /// [`CountingAllocator`](crate::alloc::CountingAllocator) only).
    pub alloc_count: Option<u64>,
    /// Gross allocation bytes (in-process runs only).
    pub alloc_bytes: Option<u64>,
}

/// The stability gate's outcome over the primary counter.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateOutcome {
    /// Which metric the gate was applied to (`instructions` or `wall_ns`).
    pub metric: String,
    /// Measured coefficient of variation, in percent.
    pub cov_pct: f64,
    /// Allowed coefficient of variation, in percent.
    pub max_cov_pct: f64,
    /// Whether the gate passed.
    pub passed: bool,
}

/// A complete bench run: identity, samples, summaries, and the gate.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchReport {
    /// Schema tag, always [`BENCH_SCHEMA`].
    pub schema: String,
    /// The measured program (the wrapper is recorded via `source`).
    pub program: String,
    /// Arguments passed to the measured program.
    pub args: Vec<String>,
    /// Counter source used.
    pub source: MeasurementSource,
    /// Number of executed iterations.
    pub iterations: usize,
    /// Per-iteration samples, in execution order.
    pub samples: Vec<BenchSample>,
    /// Summary over the instruction counts, when the source provides them.
    pub instructions: Option<SampleStats>,
    /// Summary over the wall-clock samples.
    pub wall: SampleStats,
    /// Stability gate outcome over the primary counter.
    pub gate: GateOutcome,
}

impl BenchReport {
    /// Build the envelope around a finished run.
    #[must_use]
    pub fn new(
        program: impl Into<String>,
        args: Vec<String>,
        source: MeasurementSource,
        iterations: usize,
        samples: Vec<BenchSample>,
        instructions: Option<SampleStats>,
        wall: SampleStats,
        gate: GateOutcome,
    ) -> Self {
        Self {
            schema: BENCH_SCHEMA.to_string(),
            program: program.into(),
            args,
            source,
            iterations,
            samples,
            instructions,
            wall,
            gate,
        }
    }

    /// Pretty-printed JSON (deterministic field order via the struct shape).
    ///
    /// # Errors
    ///
    /// [`BenchError::ReportParse`] if serialisation fails (cannot happen for
    /// in-memory values, but kept for the `Result`-shaped seam).
    pub fn to_json_pretty(&self) -> Result<String, crate::error::BenchError> {
        serde_json::to_string_pretty(self)
            .map(|mut text| {
                text.push('\n');
                text
            })
            .map_err(|err| crate::error::BenchError::ReportParse {
                detail: err.to_string(),
            })
    }

    /// The samples of the gate's primary metric.
    #[must_use]
    pub fn gate_samples(&self) -> Vec<u64> {
        if self.instructions.is_some() {
            self.samples
                .iter()
                .filter_map(|s| s.instructions)
                .collect::<Vec<_>>()
        } else {
            self.samples.iter().map(|s| s.wall_ns).collect()
        }
    }

    /// Fixed-width human-readable table (deterministic for a given report).
    #[must_use]
    pub fn render_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "glyim-bench report ({})\n  program: {} {}\n  source:  {}\n  runs:    {}\n",
            self.schema,
            self.program,
            self.args.join(" "),
            self.source.as_str(),
            self.iterations,
        ));
        out.push_str("  #    wall_ns      instructions\n");
        for (index, sample) in self.samples.iter().enumerate() {
            let instructions = sample
                .instructions
                .map_or_else(|| "-".to_string(), |v| v.to_string());
            out.push_str(&format!(
                "  {:<3} {:>12} {:>16}\n",
                index, sample.wall_ns, instructions
            ));
        }
        out.push_str(&format!(
            "  wall: min {} ns, median {:.0} ns, mean {:.0} ns\n",
            self.wall.min, self.wall.median, self.wall.mean,
        ));
        if let Some(stats) = &self.instructions {
            out.push_str(&format!(
                "  instructions: min {}, median {:.0}, mean {:.0}\n",
                stats.min, stats.median, stats.mean,
            ));
        }
        out.push_str(&format!(
            "  gate: {} CoV {:.3}% (budget {:.3}%) -> {}\n",
            self.gate.metric,
            self.gate.cov_pct,
            self.gate.max_cov_pct,
            if self.gate.passed { "PASS" } else { "FAIL" },
        ));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{BENCH_SCHEMA, BenchReport, BenchSample, GateOutcome};
    use crate::source::MeasurementSource;
    use crate::stats::{SampleStats, summarize};

    fn sample(wall_ns: u64, instructions: Option<u64>) -> BenchSample {
        BenchSample {
            wall_ns,
            instructions,
            cycles: None,
            rss_kb: None,
            alloc_count: None,
            alloc_bytes: None,
        }
    }

    fn report(instructions: Vec<Option<u64>>) -> BenchReport {
        let walls: Vec<u64> = (0..instructions.len()).map(|i| 1_000 + i as u64).collect();
        let samples: Vec<BenchSample> = instructions
            .iter()
            .zip(walls.iter())
            .map(|(ins, wall)| sample(*wall, *ins))
            .collect();
        let wall_stats = summarize(&walls).expect("non-empty");
        let ins_values: Vec<u64> = instructions.iter().filter_map(|v| *v).collect();
        let ins_stats = summarize(&ins_values);
        let cov = ins_stats.as_ref().map_or(wall_stats.cov_pct, |s| s.cov_pct);
        BenchReport::new(
            "prog",
            vec!["a".into()],
            if ins_stats.is_some() {
                MeasurementSource::PerfStat
            } else {
                MeasurementSource::WallOnly
            },
            samples.len(),
            samples,
            ins_stats,
            wall_stats,
            GateOutcome {
                metric: "instructions".into(),
                cov_pct: cov,
                max_cov_pct: 0.1,
                passed: cov <= 0.1,
            },
        )
    }

    #[test]
    fn schema_tag_is_pinned_and_serialised() {
        let doc = report(vec![Some(100), Some(100)]);
        assert_eq!(doc.schema, BENCH_SCHEMA);
        let json = doc.to_json_pretty().expect("serialises");
        assert!(json.contains("\"glyim.bench/1\""), "schema tag in JSON");
    }

    #[test]
    fn rendering_is_byte_deterministic() {
        let doc = report(vec![Some(500), Some(501), Some(502)]);
        let first = doc.render_text();
        let second = doc.render_text();
        assert_eq!(first, second);
        let first_json = doc.to_json_pretty().expect("serialises");
        let second_json = doc.to_json_pretty().expect("serialises");
        assert_eq!(first_json, second_json);
    }

    #[test]
    fn round_trip_preserves_the_report() {
        let doc = report(vec![Some(500), Some(502)]);
        let json = doc.to_json_pretty().expect("serialises");
        let back: BenchReport = serde_json::from_str(&json).expect("deserialises");
        assert_eq!(back, doc);
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let json = r#"{
            "schema": "glyim.bench/1",
            "program": "p",
            "args": [],
            "source": "wall",
            "iterations": 1,
            "samples": [],
            "instructions": null,
            "wall": {"min": 1, "max": 1, "mean": 1.0, "median": 1.0, "cov_pct": 0.0},
            "gate": {"metric": "wall_ns", "cov_pct": 0.0, "max_cov_pct": 0.1, "passed": true},
            "sneaky_extra": true
        }"#;
        let err = serde_json::from_str::<BenchReport>(json).expect_err("unknown field");
        let help = miette::Diagnostic::help(&crate::error::BenchError::ReportParse {
            detail: err.to_string(),
        })
        .expect("help");
        assert!(help.to_string().contains("glyim.bench/1"));
    }

    #[test]
    fn gate_samples_prefer_instructions() {
        let doc = report(vec![Some(100), Some(100)]);
        assert_eq!(doc.gate_samples(), vec![100, 100]);
        let wall_only = report(vec![None, None]);
        assert_eq!(wall_only.gate_samples(), vec![1_000, 1_001]);
    }
}
