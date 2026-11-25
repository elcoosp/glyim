//! Metamorphic suite v1 over fixtures + generated programs (plan M4.7).
//!
//! Two input classes, per the plan's `DoD` line "relations run nightly over
//! fixtures + generated programs":
//!
//! 1. **Generated** — `glyim-smith` programs (the M4.3 crate), one slice
//!    per profile across seeds and sizes.
//! 2. **Fixtures** — a deterministic sample of the committed `run-pass`
//!    corpus, filtered to programs the in-process interpreter can run
//!    (no `--with-stdlib`, no `needs-llvm`).
//!
//! Every relation must end `Invariant`; `Skipped` is allowed only where
//! the relation itself declares it does not apply (renaming vs string
//! literals, reordering vs single-item sources). Any `Diverged`/`Invalid`
//! fails the suite — that is a compiler bug under the relation.

use glyim_smith::{GeneratorProfile, generate};
use glyim_test::metamorphic::{MetamorphicRunner, RelationOutcome, all_relations};
use std::fs;
use std::path::{Path, PathBuf};

/// PR-tier sample: profiles × seeds × sizes for generated programs.
const GENERATED_SEEDS: u64 = 8;
const GENERATED_SIZES: [usize; 3] = [1, 9, 32];
/// PR-tier fixture sample size.
const FIXTURE_SAMPLE: usize = 12;

/// Collects the fixture sample: deterministic order (`walkdir`-sorted),
/// filtered to interpreter-runnable headers.
fn fixture_sample() -> Vec<(String, PathBuf)> {
    let run_pass = Path::new("tests/run-pass");
    let mut files: Vec<PathBuf> = walkdir::WalkDir::new(run_pass)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| entry.path().extension().is_some_and(|e| e == "g"))
        .map(|entry| entry.into_path())
        .collect();
    files.sort();
    files
        .into_iter()
        .filter(|path| {
            let Ok(source) = fs::read_to_string(path) else {
                return false;
            };
            // Interpreter-runnable: no stdlib prepend, no real-llvm gate,
            // and a run-pass mode the header pins.
            !source.contains("needs-llvm")
                && !source.contains("compile-flags: --with-stdlib")
                && source.contains("mode: run-pass")
        })
        .take(FIXTURE_SAMPLE)
        .map(|path| {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("fixture")
                .to_string();
            (name, path)
        })
        .collect()
}

#[test]
fn relations_hold_over_generated_programs() {
    let runner_dir = tempfile::tempdir().expect("tempdir");
    let runner = MetamorphicRunner::new(runner_dir.path().to_path_buf());
    let relations = all_relations();
    let mut violations: Vec<String> = Vec::new();
    for profile in GeneratorProfile::ALL {
        for seed in 0..GENERATED_SEEDS {
            for size in GENERATED_SIZES {
                let program = generate(*profile, seed, size).expect("in-bounds");
                for relation in &relations {
                    let outcome = relation.check(
                        &program.source,
                        &mut glyim_smith::SmithRng::new(seed),
                        &runner,
                    );
                    report(
                        &mut violations,
                        relation.name(),
                        &format!("{} seed={seed} size={size}", profile.as_str()),
                        outcome,
                    );
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "metamorphic relations violated (plan M4.7):\n{}",
        violations.join("\n=====\n")
    );
}

#[test]
fn relations_hold_over_fixtures() {
    let runner_dir = tempfile::tempdir().expect("tempdir");
    let runner = MetamorphicRunner::new(runner_dir.path().to_path_buf());
    let relations = all_relations();
    let mut violations: Vec<String> = Vec::new();
    let mut checked = 0usize;
    for (name, path) in fixture_sample() {
        let Ok(source) = fs::read_to_string(&path) else {
            continue;
        };
        checked += 1;
        for relation in &relations {
            let outcome = relation.check(&source, &mut glyim_smith::SmithRng::new(0), &runner);
            report(&mut violations, relation.name(), &name, outcome);
        }
    }
    assert!(
        checked >= 8,
        "fixture sample too small: {checked} collected"
    );
    assert!(
        violations.is_empty(),
        "metamorphic relations violated over fixtures (plan M4.7):\n{}",
        violations.join("\n=====\n")
    );
}

/// Fails the suite through the collected `violations` list.
fn report(violations: &mut Vec<String>, relation: &str, label: &str, outcome: RelationOutcome) {
    match outcome {
        RelationOutcome::Invariant => {}
        RelationOutcome::Skipped { reason } => {
            eprintln!("{relation} on {label}: skipped ({reason})");
        }
        RelationOutcome::Diverged {
            baseline_exit,
            variant_exit,
            variant_source,
        } => violations.push(format!(
            "{relation} DIVERGED on {label}: baseline={baseline_exit} variant={variant_exit}\n--- variant ---\n{variant_source}"
        )),
        RelationOutcome::Invalid {
            diagnostics,
            variant_source,
        } => violations.push(format!(
            "{relation} INVALID on {label}:\n{}\n--- variant ---\n{variant_source}",
            diagnostics.join("\n")
        )),
    }
}
