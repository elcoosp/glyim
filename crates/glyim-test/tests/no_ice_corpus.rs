//! No-ICE corpus: the compiler must never panic on any input.
//!
//! Each fixture under `tests/no-ice/` is fed through the full pipeline
//! inside a `catch_unwind`. A panic is a test failure — a language toolchain
//! that crashes on malformed input is not production-ready, whatever the
//! "correct" diagnostic for that input turns out to be. The corpus is
//! deliberately hostile: unterminated strings, unclosed delimiters, recursive
//! types, cyclic modules, empty items, keyword-as-identifier, huge literals,
//! deep nesting, invalid escapes, and so on.
//!
//! It asserts *only* that no panic occurs. What diagnostic (if any) is
//! produced is out of scope here — that belongs in the compile-fail / UI
//! corpora.

use glyim_test::harness::compiler::{CompileOutput, PipelineCompiler, TestCompiler};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::Arc;

fn corpus_root() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("no-ice")
}

/// Collect every `.g` under `tests/no-ice/`, recursing into subdirectories
/// (so a fixture's external modules are discovered as their own tests too —
/// each is a valid input the compiler must not crash on).
fn collect_g_files(root: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
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

#[test]
fn compiler_never_panics_on_no_ice_corpus() {
    let root = corpus_root();
    let files = collect_g_files(&root);
    assert!(
        !files.is_empty(),
        "the no-ice corpus must not be empty (fixtures missing?)"
    );

    let mut panics: Vec<String> = Vec::new();

    for file in &files {
        let Ok(source) = std::fs::read_to_string(file) else {
            continue;
        };
        let rel = file.strip_prefix(&root).unwrap_or(file);

        // Build a fresh backend per call: `LlvmBackend` owns an `inkwell::
        // Context` which is `!Sync`, and the mock keeps the test hermetic
        // (no LLVM toolchain required). The pipeline still runs parse ->
        // def-map -> HIR -> typeck -> MIR; the mock only skips codegen, and
        // every panic this corpus is designed to catch happens before then.
        let backend: Arc<dyn glyim_codegen::CodegenBackend + Send + Sync> =
            Arc::new(glyim_test::mock::MockCodegen::new());
        let compiler = PipelineCompiler::new(backend);

        let file_clone = file.clone();
        let source_clone = source.clone();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _out: CompileOutput =
                compiler.compile(&source_clone, &file_clone, glyim_span::FileId::from_raw(1), &[]);
        }));

        if let Err(payload) = result {
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "<non-string panic payload>".to_string());
            panics.push(format!("{}: {}", rel.display(), msg));
        }
    }

    assert!(
        panics.is_empty(),
        "{} fixture(s) panicked — the compiler must never ICE on user input:\n  {}",
        panics.len(),
        panics.join("\n  "),
    );
}
