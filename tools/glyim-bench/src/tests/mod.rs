//! Integration tests for the M2.2 harness: runner → report → CLI round
//! trips, and the determinism/strictness contract that `glyim.bench/1`
//! inherits from the M1.5 evidence seam. The M2.3 corpus substrate carries
//! its own determinism/verify contract (and, in `corpus_dir_tests`, the
//! committed-corpora directory contract), the M2.4 scaling analyzer its
//! log-log fit and wiring contracts, the M2.5 diff its regression gate, and
//! the M2.6 budget gate its absolute per-program budgets.

mod budget_tests;
mod corpus_determinism;
mod corpus_dir_tests;
mod diff_tests;
mod harness_round_trip;
mod history_tests;
mod matrix_tests;
mod scaling_tests;
mod trend_tests;
