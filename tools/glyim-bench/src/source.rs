//! Counter sources: where instruction/cycle numbers come from.
//!
//! Two machine sources are supported (plus wall-clock-only):
//!
//! - [`MeasurementSource::PerfStat`] — wrap the command in
//!   `perf stat -x, -e instructions,cycles -o <file> -- <command>` and parse
//!   the CSV rows out of the output file. CPU-time-accurate, no
//!   instrumentation slowdown beyond kernel counters.
//! - [`MeasurementSource::Callgrind`] — wrap the command in
//!   `valgrind --tool=callgrind --callgrind-out-file=<file> <command>` and
//!   read the final `summary: <Ir>` line. Deterministic instruction counts
//!   independent of the host CPU, at a larger runtime cost; the right choice
//!   for cross-host comparisons.
//! - [`MeasurementSource::WallOnly`] — no counter program; wall clock and
//!   RSS only. The fallback when neither tool is installed.

use std::fmt;
use std::str::FromStr;

use clap::ValueEnum;

use crate::error::BenchError;

/// The counter source used for a bench run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MeasurementSource {
    /// `perf stat` kernel counters (instructions + cycles).
    PerfStat,
    /// `valgrind --tool=callgrind` instruction counts.
    Callgrind,
    /// Wall clock (+ RSS) only; no counter program.
    WallOnly,
}

impl MeasurementSource {
    /// Canonical lowercase name, as it appears in reports and CLI flags.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PerfStat => "perf",
            Self::Callgrind => "callgrind",
            Self::WallOnly => "wall",
        }
    }

    /// The wrapper program this source shells out to (`None` for wall-only).
    #[must_use]
    pub const fn wrapper_program(self) -> Option<&'static str> {
        match self {
            Self::PerfStat => Some("perf"),
            Self::Callgrind => Some("valgrind"),
            Self::WallOnly => None,
        }
    }
}

impl fmt::Display for MeasurementSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for MeasurementSource {
    type Err = BenchError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "perf" => Ok(Self::PerfStat),
            "callgrind" => Ok(Self::Callgrind),
            "wall" => Ok(Self::WallOnly),
            other => Err(BenchError::PerfStatParse {
                detail: format!(
                    "unknown source `{other}` (expected `perf`, `callgrind`, or `wall`)"
                ),
            }),
        }
    }
}

impl ValueEnum for MeasurementSource {
    fn value_variants<'a>() -> &'a [Self] {
        &[Self::PerfStat, Self::Callgrind, Self::WallOnly]
    }

    fn to_possible_value(&self) -> Option<clap::builder::PossibleValue> {
        Some(clap::builder::PossibleValue::new(self.as_str()))
    }
}

/// Instruction and cycle counters parsed from one `perf stat` output file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct PerfCounters {
    /// Total retired instructions, when the event was counted.
    pub instructions: Option<u64>,
    /// Total CPU cycles, when the event was counted.
    pub cycles: Option<u64>,
}

/// Parse `perf stat -x,` CSV output (the whole file text).
///
/// Rows look like `<count>,<unit>,<event>,<run>,<run-pct>,..`; the parser
/// keys on the *event* field (position 2, perf ≥ 5.x CSV layout) and skips
/// `<not counted>` / `<not supported>` rows. Thousands separators inside the
/// count field are tolerated.
///
/// # Errors
///
/// [`BenchError::PerfStatParse`] when no `instructions` event row is found.
pub fn parse_perf_stat(output: &str) -> Result<PerfCounters, BenchError> {
    let mut counters = PerfCounters::default();
    // A `<not counted>` instructions row is *present but disabled* — that
    // is different from the row being absent, and only the latter is a
    // parse error (see the error branch below).
    let mut saw_instructions_row = false;
    for line in output.lines() {
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() < 3 {
            continue;
        }
        // `perf stat -x,` emits `<count>,,<unit>,<event>,…`, but callers
        // (and the test fixtures) also produce unit-less rows where the
        // event lands one column earlier, and counts may carry thousands
        // separators (`125,340`). Locate the event by name instead of a
        // fixed index, then treat everything before it as the count.
        let Some(event_idx) = fields.iter().position(|f| {
            let f = f.trim();
            f.starts_with("instructions") || f.starts_with("cycles")
        }) else {
            continue;
        };
        let is_instructions = fields[event_idx].trim().starts_with("instructions");
        if is_instructions {
            saw_instructions_row = true;
        }
        if fields[0].trim().starts_with('<') {
            // `<not counted>` / `<not supported>` — the event was disabled.
            continue;
        }
        let count_text: String = fields[..event_idx]
            .iter()
            .map(|f| f.trim())
            .collect::<Vec<&str>>()
            .join("");
        let Ok(count) = count_text.parse::<u64>() else {
            continue;
        };
        if is_instructions {
            counters.instructions = Some(count);
        } else {
            counters.cycles = Some(count);
        }
    }
    // Output with no instructions row at all is garbage (an empty or
    // truncated `perf stat` capture): the runner cannot gate on anything.
    // A `<not counted>` row keeps the output parseable — callers fall back
    // to the wall-clock metric when `instructions` is `None`.
    if !saw_instructions_row {
        return Err(BenchError::PerfStatParse {
            detail: "no `instructions` event row in the output".to_string(),
        });
    }
    Ok(counters)
}

/// Parse the final `summary: <Ir>` line of a callgrind output file.
///
/// # Errors
///
/// [`BenchError::CallgrindParse`] when no summary line (or no count) exists.
pub fn parse_callgrind_summary(output: &str) -> Result<u64, BenchError> {
    let mut summary = None;
    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("summary:") {
            match rest.trim().replace(',', "").parse::<u64>() {
                Ok(value) => summary = Some(value),
                Err(_) => continue,
            }
        }
    }
    summary.ok_or_else(|| BenchError::CallgrindParse {
        detail: "no `summary:` line found (was the child killed before completion?)".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::{PerfCounters, parse_callgrind_summary, parse_perf_stat};
    use crate::error::BenchError;
    use crate::source::MeasurementSource;

    #[test]
    fn perf_stat_csv_rows_parse() {
        let raw = "\
# started on Thu Jan  1 00:00:00 2026\n\
125,340,,instructions,1,100.00,,\n\
77,001,,cycles,1,100.00,,\n";
        let counters = parse_perf_stat(raw).expect("parses");
        assert_eq!(
            counters,
            PerfCounters {
                instructions: Some(125_340),
                cycles: Some(77_001),
            }
        );
    }

    #[test]
    fn perf_stat_not_counted_rows_are_skipped() {
        let raw = "\
<not counted>,,instructions,0,0.00,,\n\
55,,cycles,1,100.00,,\n";
        let counters = parse_perf_stat(raw).expect("cycles row still present");
        assert_eq!(counters.instructions, None);
        assert_eq!(counters.cycles, Some(55));
    }

    #[test]
    fn perf_stat_event_suffixes_match() {
        let raw = "10,,instructions:u,1,100.00,,\n20,,cycles:k,1,100.00,,\n";
        let counters = parse_perf_stat(raw).expect("parses");
        assert_eq!(counters.instructions, Some(10));
        assert_eq!(counters.cycles, Some(20));
    }

    #[test]
    fn perf_stat_without_instructions_row_is_an_error_with_help() {
        let err = parse_perf_stat("1,,cycles,1,100.00,,\n").expect_err("no instructions");
        assert!(matches!(err, BenchError::PerfStatParse { .. }));
        let help = miette::Diagnostic::help(&err).expect("help");
        assert!(help.to_string().contains("perf stat -x,"));
    }

    #[test]
    fn callgrind_summary_line_parses() {
        let raw = "\
events: Ir\n\
creator: valgrind-3.24\n\
summary: 152,340\n";
        assert_eq!(parse_callgrind_summary(raw).expect("parses"), 152_340);
    }

    #[test]
    fn callgrind_takes_the_last_summary() {
        let raw = "summary: 1\nsummary: 2\nsummary: 3\n";
        assert_eq!(parse_callgrind_summary(raw).expect("parses"), 3);
    }

    #[test]
    fn callgrind_without_summary_is_an_error_with_help() {
        let err = parse_callgrind_summary("events: Ir\n").expect_err("no summary");
        assert!(matches!(err, BenchError::CallgrindParse { .. }));
        let help = miette::Diagnostic::help(&err).expect("help");
        assert!(help.to_string().contains("--callgrind-out-file"));
    }

    #[test]
    fn source_round_trips_through_str_and_json() {
        for source in [
            MeasurementSource::PerfStat,
            MeasurementSource::Callgrind,
            MeasurementSource::WallOnly,
        ] {
            let parsed: MeasurementSource = source.as_str().parse().expect("parses");
            assert_eq!(parsed, source);
            let json = serde_json::to_string(&source).expect("serialise");
            let back: MeasurementSource = serde_json::from_str(&json).expect("deserialise");
            assert_eq!(back, source);
        }
    }
}
