//! Glyim measurement harness (plan M2.2, `docs/roadmaps/glyim-master-plan.md`
//! §6 M2.2).
//!
//! M2.1 gave every pipeline phase a `tracing` span and three renderers; what
//! the tree still lacked is the tool that turns a compile (or any command)
//! into *numbers with a stability guarantee*. Constitution C7: "Measure,
//! then optimize; budgets are tests" — every perf claim from M2.3 corpora to
//! the M2.7 quick-win campaign is an instruction-count delta from a committed
//! benchmark, and a claim without a stability gate is noise.
//!
//! This crate is that harness:
//!
//! - [`runner`] executes a command N times and collects a [`report::BenchSample`]
//!   per iteration: wall-clock nanoseconds, instruction and cycle counts
//!   (via `perf stat`, or `valgrind --tool=callgrind` when explicitly
//!   requested), child peak RSS (`getrusage(RUSAGE_CHILDREN)`, Linux), and —
//!   for in-process consumers — gross allocation count/bytes from
//!   [`alloc::CountingAllocator`].
//! - [`stats`] computes min/mean/median/max and the coefficient of variation,
//!   and applies the M2.2 definition-of-done gate: **`CoV` of the instruction
//!   counts < 0.1 % over the run**.
//! - [`report`] serialises a run as a `glyim.bench/1` JSON document with the
//!   same discipline as the M1.5 evidence records: `deny_unknown_fields`,
//!   typed enums, no clock or PRNG data, byte-deterministic rendering.
//! - [`corpus`] is the M2.3 slice 1 substrate: seven deterministic synthetic
//!   program generator families (wide/deep/generic/trait/match/macro/closure)
//!   plus the `glyim.corpus/1` manifest whose identity column makes
//!   "corpora committed with hashes" machine-checked.
//! - [`error`] gives every failure seam a `miette` diagnostic with an
//!   actionable `help` line, mirroring the `glyim-test` evidence seam.
//!
//! The M1.5 evidence schema reserved `instructions` / `rss_kb` columns for
//! exactly this instrumentation; wiring those columns into `glyim-test` is
//! the M2.3 corpora slice, after the harness's stability gate has been
//! validated on real hosts.
//!
//! # Examples
//!
//! Run a command 10 times and require stable instruction counts:
//!
//! ```text
//! glyim-bench run --iterations 10 -- ./target/release/glyim --emit=obj fib.g
//! ```
//!
//! Measure a library workload in-process (allocation counters + wall time):
//!
//! ```text
//! cargo run -p glyim-bench --example measure_alloc
//! ```
//!
//! Build and verify a corpus manifest (M2.3):
//!
//! ```text
//! cargo run -p glyim-bench --example corpus_walk
//! ```
//!
//! Fit a committed report history into a scaling exponent (M2.4):
//!
//! ```text
//! cargo run -p glyim-bench --example scaling_fit
//! ```
//!
//! Enforce committed absolute budgets against written reports (M2.6):
//!
//! ```text
//! cargo run -p glyim-bench --example budget_check
//! ```
//!
//! Append runs to the perf history and diff against the latest baseline
//! (plan §6 M2.5):
//!
//! ```text
//! cargo run -p glyim-bench --example history_baseline
//! ```
//!
//! Gate a whole benchmark matrix against the recorded history and catch a
//! uniform sub-budget drift with the geo-mean budget (plan §6 M2.5
//! remainder, §325 "fail >2% per benchmark or >0.5% geo-mean"):
//!
//! ```text
//! cargo run -p glyim-bench --example matrix_gate
//! ```
//!
//! Measure the total drift across a program's committed history window —
//! the accumulated ratchet that per-run diffs never see (plan M2.5
//! complement):
//!
//! ```text
//! cargo run -p glyim-bench --example history_trend
//! ```

/// Gross-allocation counting allocator (`counting_allocator!` macro).
pub mod alloc;
/// Absolute per-program budget enforcement and the `BudgetStore` seam
/// (M2.6 budgets gate).
pub mod budget;
/// clap-derived CLI surface, kept in the library for unit tests.
pub mod cli;
/// Deterministic synthetic program corpus + `glyim.corpus/1` manifest (M2.3).
pub mod corpus;
/// The committed corpora directory: the `CorpusSink` write seam,
/// `DirCorpusSink`, and the byte-level check gate (M2.3 remainder:
/// "corpora committed with hashes").
pub mod corpus_dir;
/// Report-to-report comparison and the `ReportStore` seam (M2.5 perf history).
pub mod diff;
/// Typed errors with `miette` help text for every seam.
pub mod error;
/// The append-only perf-history store and the latest-baseline seam
/// (plan §6 M2.5 `perf/history/*.jsonl`).
pub mod history;
/// The multi-report diff matrix and the geo-mean aggregate gate
/// (plan §6 M2.5 remainder: "fail >2% per benchmark or >0.5% geo-mean").
pub mod matrix;
/// The `glyim.bench/1` report document and its renderers.
pub mod report;
/// The iteration runner: N executions of one command.
pub mod runner;
/// Input-size scaling analysis and the log-log fit (M2.4 scaling analyzer).
pub mod scaling;
/// Counter sources and the `perf stat` / callgrind output parsers.
pub mod source;
/// Sample statistics and the CoV stability gate.
pub mod stats;
/// The history trend analyzer: total drift across the committed window
/// (plan M2.5 complement — the accumulated-drift ratchet detector).
pub mod trend;

#[cfg(test)]
mod tests;
