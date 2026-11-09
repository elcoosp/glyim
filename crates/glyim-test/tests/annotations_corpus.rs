//! `M1.4` `DoD` gate: every `.g` fixture is annotated, every expectation explicit.
//!
//! Walks the whole `tests/` fixture tree (run-pass, run-fail, compile-pass,
//! compile-fail, ui, no-ice, typeck, runtime, codegen) and enforces the
//! fixture annotation contract introduced by the readiness ledger plan
//! (`docs/roadmaps/glyim-master-plan.md` M1.4):
//!
//! 1. every fixture carries at least one `//@` directive (unknown keys are
//!    parse errors — typos must not become silent no-op tags);
//! 2. every fixture declares at least one `//@ feature:` registry ID and a
//!    non-empty `//@ backends:` list, so `ledger compute` (M1.8) can join
//!    fixtures to registry features without guessing;
//! 3. outcome expectations are explicit: run-pass / run-fail fixtures must
//!    declare an exit code or stdout/stderr pattern, compile-fail fixtures
//!    must carry at least one `//~` inline annotation or `//@ expect-error`;
//! 4. `ui` fixtures declare `//@ mode: ui` and have a `.expected` snapshot;
//! 5. `runtime/` fixtures gate themselves with `//@ only-target:`.

use std::path::{Path, PathBuf};

use glyim_test::annotations::directives::{KNOWN_BACKENDS, is_valid_feature_id, parse_directives};
use glyim_test::harness::config::{TestConfig, TestMode, parse_test_config};

fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")
}

/// The `.g` fixture roots the corpora own. Every `.g` under `tests/` must
/// live in one of these (keeps the gate honest about new directories).
const FIXTURE_DIRS: &[&str] = &[
    "run-pass",
    "run-fail",
    "compile-pass",
    "compile-fail",
    "ui",
    "no-ice",
    "typeck",
    "runtime",
    "codegen",
];

fn collect_g_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().and_then(|s| s.to_str()) == Some("g") {
                out.push(p);
            }
        }
    }
    walk(root, &mut out);
    out.sort();
    out
}

/// Returns the fixture's directory key (`run-pass`, `runtime`, …) relative
/// to the corpus root, or `None` when it lives outside every known fixture
/// directory.
fn fixture_dir_key(path: &Path) -> Option<String> {
    let rel = path.strip_prefix(corpus_root()).ok()?;
    let first = rel.components().next()?.as_os_str().to_str()?;
    FIXTURE_DIRS
        .iter()
        .find(|d| **d == first)
        .map(|d| (*d).to_string())
}

#[test]
fn every_fixture_is_annotated_and_carries_explicit_expectations() {
    let root = corpus_root();
    let fixtures = collect_g_files(&root);
    assert!(
        fixtures.len() >= 375,
        "fixture count regressed below the M1.4 tagging baseline ({} found)",
        fixtures.len()
    );

    let mut problems = Vec::new();

    for path in &fixtures {
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let Ok(source) = std::fs::read_to_string(path) else {
            problems.push(format!("{rel}: unreadable"));
            continue;
        };

        if fixture_dir_key(path).is_none() {
            problems.push(format!(
                "{rel}: .g file outside the known fixture directories"
            ));
            continue;
        }

        let annotations = match parse_directives(&source) {
            Ok(a) => a,
            Err((line, msg)) => {
                problems.push(format!("{rel}: line {line}: {msg}"));
                continue;
            }
        };

        if !annotations.present {
            problems.push(format!("{rel}: no `//@` annotation header (M1.4 DoD)"));
            continue;
        }

        if annotations.features.is_empty() {
            problems.push(format!("{rel}: missing `//@ feature:` tag"));
        }
        for f in &annotations.features {
            if !is_valid_feature_id(f) {
                problems.push(format!("{rel}: malformed feature ID `{f}`"));
            }
        }

        match &annotations.backends {
            None => problems.push(format!("{rel}: missing `//@ backends:` tag")),
            Some(bs) => {
                if bs.is_empty() {
                    problems.push(format!("{rel}: empty `//@ backends:` list"));
                }
                for b in bs {
                    if !KNOWN_BACKENDS.contains(&b.as_str()) {
                        problems.push(format!("{rel}: unknown backend `{b}`"));
                    }
                }
            }
        }

        // Outcome expectations, per mode. Parse the merged legacy+`//@`
        // view so the gate keeps holding while legacy headers still exist.
        let config: TestConfig = match parse_test_config(&source) {
            Ok(parsed) => parsed.config,
            Err(msg) => {
                problems.push(format!("{rel}: {msg}"));
                continue;
            }
        };
        let mode = config.mode;
        match mode {
            TestMode::RunPass | TestMode::RunFail => {
                let has_outcome = config.expected_exit_code.is_some()
                    || config.check_stdout.is_some()
                    || config.check_stderr.is_some()
                    || annotations.has_outcome_expectation();
                if !has_outcome {
                    problems.push(format!(
                        "{rel}: {mode:?} fixture lacks an outcome expectation (declare `//@ expect-exit:`)"
                    ));
                }
            }
            TestMode::CompileFail => {
                let has_inline = !glyim_test::annotations::Annotation::parse_all(&source)
                    .unwrap_or_default()
                    .is_empty();
                if !has_inline
                    && config.error_patterns.is_empty()
                    && annotations.expect_errors.is_empty()
                {
                    problems.push(format!(
                        "{rel}: compile-fail fixture lacks an error expectation (inline `//~ ERROR` or `//@ expect-error:`)"
                    ));
                }
            }
            TestMode::Ui => {
                if annotations.mode != Some(TestMode::Ui) {
                    problems.push(format!("{rel}: ui fixture must declare `//@ mode: ui`"));
                }
                let expected = path.with_extension("expected");
                if !expected.exists() {
                    problems.push(format!("{rel}: ui fixture missing .expected snapshot"));
                }
            }
            TestMode::CompilePass => {}
        }

        // Only the native-link async proof corpus needs the target gate;
        // other runtime fixtures (e.g. the bytecode-backend drop-order
        // fixture) run on any host.
        if rel.starts_with("runtime/m5") && annotations.only_target.is_none() {
            problems.push(format!(
                "{rel}: runtime/m5 fixture must gate itself with `//@ only-target:`"
            ));
        }
    }

    // Other crates drive the same `TestCollector` over their own fixture
    // corpora (`glyim-lower/tests/mir`, `glyim-typeck/tests/compile-*`,
    // `glyim-lang-{alloc,core,std}/tests/compile-pass`,
    // `glyim-pipeline/tests/compile-pass`). The M1.4 contract is
    // workspace-wide: every one of those fixtures must be annotated and
    // carry the ledger tags too.
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root must exist");
    let other = collect_g_files(&workspace_root.join("crates"))
        .into_iter()
        .filter(|p| {
            // Fixture corpora live under `tests/`; other `.g` files (the
            // stdlib sources shipped in `crates/glyim-lang-*/lib/`) are
            // program text, not fixtures.
            p.components().any(|c| c.as_os_str() == "tests")
                && !p.components().any(|c| c.as_os_str() == "glyim-test")
        });
    for path in other {
        let rel = path
            .strip_prefix(&workspace_root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let Ok(source) = std::fs::read_to_string(path) else {
            problems.push(format!("{rel}: unreadable"));
            continue;
        };
        let annotations = match parse_directives(&source) {
            Ok(a) => a,
            Err((line, msg)) => {
                problems.push(format!("{rel}: line {line}: {msg}"));
                continue;
            }
        };
        if !annotations.present {
            problems.push(format!("{rel}: no `//@` annotation header (M1.4 DoD)"));
        }
        if annotations.features.is_empty() {
            problems.push(format!("{rel}: missing `//@ feature:` tag"));
        }
        if annotations.backends.is_none() {
            problems.push(format!("{rel}: missing `//@ backends:` tag"));
        }
    }

    assert!(
        problems.is_empty(),
        "{} fixture annotation problem(s):\n{}",
        problems.len(),
        problems.join("\n")
    );
}
