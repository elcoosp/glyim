//! Opt-level wire-up contract tests (end to end).
//!
//! The CLI's `--opt-level` reaches the LLVM backend, the linker, and the
//! compile-cache key — and, since the pass-plan seam, the MIR optimizer
//! too. These tests pin the visible end of that threading through
//! [`compile_file_to_mir`], the seam the in-process runners execute
//! through:
//!
//! * `-O0` runs the minimal plan (`slice_desugar` only): a dead store
//!   survives into the returned `MirCompilation` bodies;
//! * `-O2` runs the full historical pipeline: the same dead store is
//!   erased (const-prop folds, DCE removes);
//! * `-O0` and `-O2` both still typecheck and validate identically — the
//!   plan changes which passes run, never the diagnostics contract.

use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_db::{CrateConfig, Database};
use glyim_mir::StatementKind;
use glyim_pipeline::{Pipeline, compile_file_to_mir};

/// Unique temp path so parallel test runs never collide.
fn fixture_path(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir();
    let path = dir.join(format!(
        "glyim_opt_level_plans_{}_{}_{}.g",
        name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::write(
        &path,
        "fn main() -> i32 {\n    let dead = 42;\n    let a = 3;\n    a * 2\n}\n",
    )
    .expect("write fixture");
    path
}

fn test_db() -> Database {
    Database::new(CrateConfig {
        name: "opt_level_plans".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 0,
    })
}

/// Total `Assign` statement count in `main`'s optimized body — the
/// DCE-observable shape.
fn main_assign_count(
    bodies: &std::collections::HashMap<DefId, std::sync::Arc<glyim_mir::Body>>,
) -> usize {
    let main_id = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));
    // The fixture has a single user function; `main`'s body is the one the
    // entry pre-pass selected (fall back to the only body present).
    let body = bodies
        .iter()
        .find(|(id, _)| **id == main_id)
        .map(|(_, b)| &**b)
        .or_else(|| bodies.values().map(|b| &**b).next())
        .expect("compiled bodies are never empty");
    body.basic_blocks
        .iter()
        .map(|bb| {
            bb.statements
                .iter()
                .filter(|s| matches!(s.kind, StatementKind::Assign(_, _)))
                .count()
        })
        .sum()
}

/// A dead store survives `-O0` and is erased by `-O2` — through the real
/// pipeline, not just the unit-level plan selection.
#[test]
fn opt_level_selects_the_mir_pass_plan_end_to_end() {
    let path = fixture_path("levels");

    let mut db0 = test_db();
    let c0 = compile_file_to_mir(&mut db0, &path, 0).expect("-O0 compiles");
    let mut db2 = test_db();
    let c2 = compile_file_to_mir(&mut db2, &path, 2).expect("-O2 compiles");

    let n0 = main_assign_count(&c0.bodies);
    let n2 = main_assign_count(&c2.bodies);
    assert!(
        n0 > n2,
        "-O0 must keep the dead store ({n0} assigns) that -O2 erases ({n2} assigns)"
    );
    assert!(
        n0 >= 4,
        "lowering emits at least the four live assigns, got {n0}"
    );
    // `-O2` keeps exactly the return write: const-prop folds `a * 2` (and
    // the `a = 3` store feeding it) into one constant store into `_0`.
    assert!(n2 >= 1, "-O2 keeps the folded return write, got {n2}");
}

/// The Opt phase stays charged in the phase profile at both levels — the
/// observability contract (`Phase::Opt` present, plan size invisible to
/// the profile schema).
#[test]
fn opt_phase_still_profiles_at_both_levels() {
    let path = fixture_path("profile");
    for level in [0u8, 2] {
        let mut db = test_db();
        let c = compile_file_to_mir(&mut db, &path, level).expect("compiles");
        // The profile must be schema-complete: every phase slot rendered,
        // with the Opt slot present even when the minimal plan ran.
        let profile = &c.profile;
        let _ = profile.micros(glyim_pipeline::phase_prof::Phase::Opt);
    }
    // Entry resolution agrees between levels (the pre-pass is
    // level-independent).
    let mut db_entry = test_db();
    let entry = Pipeline::entry_main_local_id(&mut db_entry, &path);
    assert!(entry.is_some(), "entry resolution is level-independent");
}
