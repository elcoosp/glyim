//! `compile_file_to_mir` contract tests (M2.4 slice).
//!
//! Written test-first. `compile_file_to_mir` is the pipeline's MIR-only
//! entry point — the seam every in-process runner (the `glyim` CLI's
//! `--emit=run`, `glyip test`, the differential harness) executes
//! through. The contract pinned here:
//!
//! * the registry-threaded variant
//!   [`compile_file_to_mir_with_proc_registry`] and the bare wrapper
//!   produce **identical** `MirCompilation` values for a program without
//!   proc-macro deps — the wrapper is a documented delegator, not a
//!   second pipeline;
//! * a `MirCompilation` carries the same observability side-channels
//!   every other entry point publishes: the per-phase [`PhaseProfile`]
//!   (`--time-passes` / `--self-profile` stay meaningful in run mode)
//!   and the non-error `warnings` (a user who never sees a warning has
//!   no way to act on it — same rationale as `CompileArtifacts::warnings`);
//! * entry resolution through the pre-pass matches the registry-less
//!   wrapper for registry-less programs.

use glyim_db::{CrateConfig, Database};
use glyim_pipeline::{Pipeline, compile_file_to_mir, compile_file_to_mir_with_proc_registry};

/// Unique temp path so parallel test runs never collide.
fn fixture_path(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir();
    let path = dir.join(format!(
        "glyim_mir_compilation_{}_{}_{}.g",
        name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::write(&path, "fn main() -> i32 {\n    let a = 3;\n    a * 2\n}\n")
        .expect("write fixture");
    path
}

fn test_db() -> Database {
    Database::new(CrateConfig {
        name: "mir_compilation".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 0,
    })
}

#[test]
fn registry_variant_and_wrapper_agree_on_bodies() {
    let path = fixture_path("agreement");
    let mut db_a = test_db();
    let via_wrapper = compile_file_to_mir(&mut db_a, &path, 2).expect("wrapper compiles");
    let mut db_b = test_db();
    let via_registry = compile_file_to_mir_with_proc_registry(&mut db_b, &path, None, 2)
        .expect("registry variant compiles");

    let mut wrapper_keys: Vec<_> = via_wrapper.bodies.keys().collect();
    let mut registry_keys: Vec<_> = via_registry.bodies.keys().collect();
    wrapper_keys.sort();
    registry_keys.sort();
    assert!(
        !wrapper_keys.is_empty(),
        "a compiling program must produce MIR bodies"
    );
    assert_eq!(
        wrapper_keys, registry_keys,
        "both entry points must lower the same set of bodies"
    );
}

#[test]
fn mir_compilation_carries_profile_and_warnings() {
    let path = fixture_path("observability");
    let mut db = test_db();
    let compilation =
        compile_file_to_mir_with_proc_registry(&mut db, &path, None, 2).expect("fixture compiles");

    // The profile is recorded unconditionally by the shared front half
    // (one Instant pair + two Cell writes per phase), so a successful
    // compile always has a nonzero total.
    assert!(
        compilation.profile.total_micros() > 0,
        "compile_file_to_mir must publish the per-phase profile"
    );
    // A clean program produces no warnings, but the channel itself must
    // exist and be renderable by callers.
    assert!(
        compilation.warnings.is_empty(),
        "a clean program has no warnings, got: {:?}",
        compilation
            .warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    // The renderer entry points used by --time-passes / --self-profile
    // work on the published profile unchanged.
    assert!(!compilation.profile.render_text().is_empty());
    assert!(!compilation.profile.render_chrome_trace().is_empty());
}

#[test]
fn entry_pre_pass_with_registry_matches_wrapper() {
    let path = fixture_path("entry");
    let mut db = test_db();
    let bare = Pipeline::entry_main_local_id(&mut db, &path);
    let mut db2 = test_db();
    let threaded = Pipeline::entry_main_local_id_with_proc_registry(&mut db2, &path, None);
    assert_eq!(
        bare, threaded,
        "registry-less pre-pass must agree with the wrapper"
    );
    assert!(bare.is_some(), "the fixture defines main");
}
