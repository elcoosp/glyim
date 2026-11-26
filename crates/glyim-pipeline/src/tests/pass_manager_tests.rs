//! M5.11 `--opt-bisect-limit` wire-up tests (plan §6 M5.11 / §4.4 PF-14).
//!
//! The pass manager's in-crate seam ([`glyim_opt::MirPass`] registry,
//! verify-after-each-pass attribution, per-pass [`glyim_opt::PassStats`],
//! the injected-`CorruptingPass` bisect test) already existed; this module
//! pins the *consumer-facing* slice the plan's acceptance line requires:
//!
//! > "Pass manager skeleton (PF-14) with `REQUIRES`/`ENSURES`,
//! > verify-after-each-pass, per-pass stats, `--opt-bisect-limit=N`"
//! > — `docs/roadmaps/glyim-master-plan.md` §6 M5.11
//!
//! i.e. the flag must be drivable from the CLI through the pipeline: a
//! compile with `opt_bisect_limit = Some(n)` runs only the first `n` passes
//! of [`glyim_opt::PassPlan::for_opt_level`], returns one
//! [`glyim_opt::PassStats`] row per planned pass (skipped passes report
//! `ran: false`), and the partially-optimized body is observably different
//! from the fully-optimized one.

use crate::{MirCompilation, compile_file_to_mir_with_proc_registry};
use glyim_db::{CrateConfig, Database};
use std::path::PathBuf;

/// Unique temp path so parallel test runs never collide (mirrors
/// `mir_parity_tests::fixture_path`).
fn fixture_path(name: &str, src: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "glyim_pass_manager_{name}_{}_{}.g",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    ));
    std::fs::write(&path, src).expect("write fixture");
    path
}

fn test_db() -> Database {
    Database::new(CrateConfig {
        name: "bisect".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 2,
    })
}

/// A program whose `40 + 2` fold and dead store are exactly the work the
/// `-O2` plan passes (`const_prop`, then `dce`) perform — so a bisect limit
/// below the `dce` index leaves the dead store in the MIR and the full run
/// removes it.
const DEAD_STORE_SRC: &str = r"fn main() -> i32 {
    let dead = 40 + 2;
    return 0;
}
";

/// M5.11 acceptance: `--opt-bisect-limit=N` runs only the first `n` passes
/// of the plan and reports one `PassStats` row per planned pass.
#[test]
fn opt_bisect_limit_runs_only_the_first_n_passes_and_returns_per_pass_stats() {
    let path = fixture_path("dead_store", DEAD_STORE_SRC);
    let mut db = test_db();

    // Full run (no limit): the plain `optimize_at_level` path runs and
    // collects no per-pass rows (that's the zero-cost default contract).
    let full = compile_file_to_mir_with_proc_registry(&mut db, &path, None, 2, None)
        .expect("full compile must succeed");
    assert!(
        full.pass_stats.is_empty(),
        "the un-bisected compile must not pay for stats collection"
    );

    // Bisected run at limit 2: only the first two `-O2` plan passes run
    // (`slice_desugar`, `const_prop`); the rest report `ran: false`. One row
    // per planned pass, in plan order.
    let plan_len = glyim_opt::PassPlan::for_opt_level(2).passes().len();
    let bisected = compile_file_to_mir_with_proc_registry(&mut db, &path, None, 2, Some(2))
        .expect("bisected compile must succeed");
    assert_eq!(
        bisected.pass_stats.len(),
        plan_len,
        "one PassStats row per planned pass, in plan order"
    );
    let (ran, skipped): (Vec<_>, Vec<_>) = bisected.pass_stats.iter().partition(|(_, s)| s.ran);
    assert_eq!(ran.len(), 2, "exactly the first two plan passes ran");
    assert_eq!(
        skipped.len(),
        bisected.pass_stats.len() - 2,
        "every pass beyond the limit reports ran: false"
    );
    // Plan order: the ran rows are the *first* two rows of the stat list.
    assert!(
        bisected
            .pass_stats
            .iter()
            .rev()
            .take(skipped.len())
            .all(|(_, s)| !s.ran),
        "skipped rows must be the plan tail, in run order"
    );
    // Pass ids are stable and distinct (the plan's registry contract).
    let names: Vec<_> = bisected.pass_stats.iter().map(|(_, s)| s.name).collect();
    let unique = names.iter().collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        unique.len(),
        names.len(),
        "pass ids must be unique per plan"
    );

    // The bisect limit is behaviourally respected: a limit below the DCE
    // index leaves a body the full run shrinks (the dead store survives).
    let stmt_count = |mir: &MirCompilation| -> usize {
        mir.bodies
            .values()
            .map(|b| {
                b.basic_blocks
                    .iter()
                    .map(|bb| bb.statements.len())
                    .sum::<usize>()
            })
            .sum()
    };
    assert!(
        stmt_count(&bisected) > stmt_count(&full),
        "limit=2 must stop before DCE: the bisected body keeps the dead store \
         the full run removes"
    );
}

/// M5.11 acceptance: `limit = 0` runs no pass at all — every row reports
/// `ran: false` and the body is the raw lowering (nothing was optimized).
#[test]
fn opt_bisect_limit_zero_runs_no_pass() {
    let path = fixture_path("limit_zero", DEAD_STORE_SRC);
    let mut db = test_db();
    let zero = compile_file_to_mir_with_proc_registry(&mut db, &path, None, 2, Some(0))
        .expect("limit=0 compile must succeed");
    assert!(!zero.pass_stats.is_empty());
    assert!(
        zero.pass_stats.iter().all(|(_, s)| !s.ran),
        "limit=0 must skip every planned pass"
    );

    // Determinism: the same compile twice produces identical stats.
    let again = compile_file_to_mir_with_proc_registry(&mut db, &path, None, 2, Some(0))
        .expect("repeat compile must succeed");
    let render = |mir: &MirCompilation| -> Vec<(&'static str, bool, bool, i64, i64)> {
        mir.pass_stats
            .iter()
            .map(|(_, s)| (s.name, s.ran, s.body_changed, s.stmt_delta, s.block_delta))
            .collect()
    };
    assert_eq!(
        render(&zero),
        render(&again),
        "bisect stats must be deterministic"
    );
}
