//! Integration tests for the M2.2 harness: runner → report → CLI round
//! trips, and the determinism/strictness contract that `glyim.bench/1`
//! inherits from the M1.5 evidence seam. The M2.3 corpus substrate carries
//! its own determinism/verify contract alongside.

mod corpus_determinism;
mod harness_round_trip;
