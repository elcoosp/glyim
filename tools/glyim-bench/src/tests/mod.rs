//! Integration tests for the M2.2 harness: runner → report → CLI round
//! trips, and the determinism/strictness contract that `glyim.bench/1`
//! inherits from the M1.5 evidence seam. The M2.3 corpus substrate carries
//! its own determinism/verify contract alongside, and the M2.4 scaling
//! analyzer its log-log fit and wiring contracts.

mod corpus_determinism;
mod diff_tests;
mod harness_round_trip;
mod scaling_tests;
