//! Interpreter-agreement tests: the checksum contract end-to-end.
//!
//! Plan M4.3: programs are generated "ending in a checksum print" and
//! §3.11 requires fixtures that *assert*. These tests run each generated
//! program through the canonical `glyim_mir_interp::run_program` process
//! contract (the same seam `glyim --emit=run` drives) and require the
//! observed exit to equal the recorded checksum through the OS mask.

use glyim_db::{CrateConfig, Database};
use glyim_mir_interp::RunKind;
use glyim_pipeline::{Pipeline, compile_file_to_mir};
use glyim_smith::{GeneratorProfile, MAX_SIZE, generate, masked_exit};
use std::fs;
use std::path::PathBuf;

/// The harness-standard host configuration (the interpreter is the only
/// backend this test drives; `target_triple` is the host's own).
fn test_config() -> CrateConfig {
    CrateConfig {
        name: "glyim_smith_interpreter".to_string(),
        target_triple: format!(
            "{}-unknown-{}",
            std::env::consts::ARCH,
            std::env::consts::OS
        ),
        opt_level: 0,
    }
}

/// Compiles and runs one program through the canonical seam, returning
/// the interpreter's outcome.
fn run(source: &str, dir: &std::path::Path, index: usize) -> glyim_mir_interp::RunOutcome {
    let path: PathBuf = dir.join(format!("smith_run_{index}.g"));
    fs::write(&path, source).expect("write generated program");
    let mut db = Database::new(test_config());
    let entry_main = Pipeline::entry_main_local_id(&mut db, &path)
        .expect("every generated program defines `fn main`");
    let compilation = compile_file_to_mir(&mut db, &path, 0).expect("generated program compiles");
    assert!(
        compilation.warnings.is_empty(),
        "generated program must compile warning-free"
    );
    let bodies: Vec<_> = compilation.bodies.values().cloned().collect();
    glyim_mir_interp::run_program(&bodies, compilation.ty_ctx.as_ref(), Some(entry_main))
}

#[test]
fn every_program_exits_with_its_checksum() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut index = 0usize;
    for profile in GeneratorProfile::ALL {
        for seed in 0..40u64 {
            for size in [1usize, 6, 24, MAX_SIZE] {
                let program = generate(*profile, seed, size).expect("in-bounds");
                let outcome = run(&program.source, dir.path(), index);
                index += 1;
                assert_eq!(
                    outcome.kind,
                    RunKind::Completed,
                    "{profile:?} seed={seed} size={size} panicked:\n{}",
                    String::from_utf8_lossy(&outcome.stderr)
                );
                assert_eq!(
                    outcome.exit_code,
                    masked_exit(program.expected_exit),
                    "{profile:?} seed={seed} size={size} checksum mismatch (expected {} masked)\n--- source ---\n{}",
                    program.expected_exit,
                    program.source
                );
            }
        }
    }
}
