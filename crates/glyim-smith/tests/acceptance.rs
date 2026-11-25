//! M4.3 acceptance tests — the plan-fidelity seam.
//!
//! Pins the master-plan `DoD` (`docs/roadmaps/glyim-master-plan.md` §6 M4.3):
//!
//! > "**glyim-smith** v1 (scalars, control flow, structs/enums/match,
//! > functions, generics) with checksum printing; seeds recorded"
//! > `DoD`: "**0 invalid (non-compiling) programs in a 100k sample**"
//!
//! The CI-tier test below samples every profile across sizes and seeds and
//! requires **zero invalid programs** through the shared front half
//! (`compile_file_to_mir` — the same "every mode derives from one prepared
//! compilation" rule the other harnesses follow). The full 100k-sample `DoD`
//! run is the `do_d_100k_sample` test, `#[ignore]`d so PR CI stays in
//! budget; it is executed with:
//!
//! ```text
//! cargo test -p glyim-smith --release --test acceptance -- --ignored
//! ```

use glyim_db::CrateConfig;
use glyim_db::Database;
use glyim_smith::{GeneratorProfile, MAX_SIZE, generate};
use std::fs;
use std::path::PathBuf;

/// The harness-standard host configuration (the interpreter is the only
/// backend this test drives; `target_triple` is the host's own).
fn test_config() -> CrateConfig {
    CrateConfig {
        name: "glyim_smith_acceptance".to_string(),
        target_triple: format!(
            "{}-unknown-{}",
            std::env::consts::ARCH,
            std::env::consts::OS
        ),
        opt_level: 0,
    }
}

/// Sample plan: every profile, a stride of seeds, several sizes.
fn sample() -> Vec<(GeneratorProfile, u64, usize)> {
    let mut cases = Vec::new();
    for profile in GeneratorProfile::ALL {
        for seed in 0..24u64 {
            for size in [1usize, 4, 16, MAX_SIZE] {
                cases.push((*profile, seed, size));
            }
        }
    }
    cases
}

/// Compile one generated program through the shared front half; the
/// returned `Err` carries the diagnostics that made the program invalid.
fn compile(program_source: &str, dir: &std::path::Path, index: usize) -> Result<(), String> {
    let path: PathBuf = dir.join(format!("smith_case_{index}.g"));
    fs::write(&path, program_source).expect("write generated program");
    let mut db = Database::new(test_config());
    match glyim_pipeline::compile_file_to_mir(&mut db, &path, 0) {
        Ok(compilation) => {
            let diags = compilation.warnings.len();
            assert_eq!(
                diags, 0,
                "generated program must compile without diagnostics"
            );
            Ok(())
        }
        Err(diagnostics) => {
            let rendered: Vec<String> = diagnostics.iter().map(|d| format!("{d}")).collect();
            Err(rendered.join("\n"))
        }
    }
}

/// CI-tier slice of the 100k `DoD`: 5 profiles × 24 seeds × 4 sizes = 480
/// programs, all required to compile. Any diagnostic fails the test with
/// the seed/size that produced it, so the bug is reproducible from the
/// failure message alone.
#[test]
fn every_profile_compiles_across_seeds_and_sizes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut invalid = Vec::new();
    for (index, (profile, seed, size)) in sample().into_iter().enumerate() {
        let program = generate(profile, seed, size).expect("generation is infallible in-bounds");
        if let Err(diagnostics) = compile(&program.source, dir.path(), index) {
            invalid.push(format!(
                "profile={} seed={seed} size={size}:\n{diagnostics}\n--- source ---\n{}",
                profile.as_str(),
                program.source
            ));
        }
    }
    assert!(
        invalid.is_empty(),
        "{} generated programs did not compile (plan M4.3 `DoD`: 0 invalid):\n{}",
        invalid.len(),
        invalid.join("\n=====\n")
    );
}

/// The plan's literal `DoD`: a 100,000-program sample with 0 invalid
/// programs. `#[ignore]`d in CI (a release-mode run takes minutes, not
/// seconds); executed with `cargo test -p glyim-smith --release -- --ignored`.
#[test]
#[ignore = "plan M4.3 `DoD` sample — run in release: cargo test -p glyim-smith --release -- --ignored"]
fn do_d_100k_sample_zero_invalid_programs() {
    const SAMPLE: usize = 100_000;
    let dir = tempfile::tempdir().expect("tempdir");
    let mut invalid = 0usize;
    for index in 0..SAMPLE {
        // Deterministic walk over the profile × seed × size space: the
        // sample is itself reproducible from the constant alone.
        let profile = GeneratorProfile::ALL[index % GeneratorProfile::ALL.len()];
        let seed = (index / GeneratorProfile::ALL.len()) as u64;
        let size = 1 + (index % 12) * 8; // 1, 9, 17, … 89 ≤ MAX_SIZE
        let program = generate(profile, seed, size).expect("generation is infallible in-bounds");
        if compile(&program.source, dir.path(), index % 64).is_err() {
            invalid += 1;
        }
    }
    assert_eq!(
        invalid, 0,
        "plan M4.3 `DoD` violated: {invalid} invalid programs in a {SAMPLE}-program sample"
    );
}
