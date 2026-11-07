//! One-fixture probe: run the MIR interpreter against a single run-pass
//! fixture with GLYIM_INTERP_TRACE=1 to capture a clean (un-interleaved)
//! trace. Not part of the regular suite — meant for debugging.
//!
//! Usage:
//!   GLYIM_INTERP_TRACE=1 cargo test -p glyim-test --features real-llvm \
//!     --test probe_single -- --nocapture
#![cfg(feature = "real-llvm")]

use glyim_test::harness::compiler::{PipelineCompiler, TestCompiler};
use glyim_test::harness::interpreter_runner::InterpRunner;
use glyim_test::mock::MockCodegen;
use std::sync::Arc;
use std::time::Duration;

fn probe(fixture: &str, flags: &[&str]) {
    eprintln!("\n========== PROBE: {} ==========", fixture);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(fixture);
    let src = std::fs::read_to_string(&path).expect("read fixture");
    let flag_vec: Vec<String> = flags.iter().map(|s| s.to_string()).collect();
    let compiler = PipelineCompiler::new(Arc::new(MockCodegen::new())).with_real_llvm();
    let out = compiler.compile(&src, &path, glyim_span::FileId::from_raw(0), &flag_vec);
    for d in &out.diagnostics {
        if d.is_error() {
            eprintln!("COMPILE ERROR: {}", d.message);
        }
    }
    let Some(ty_ctx) = out.ty_ctx.clone() else {
        eprintln!("NO ty_ctx — cannot run interp");
        return;
    };
    eprintln!(
        "entry_main={:?}, mir_bodies.len()={}",
        out.entry_main,
        out.mir_bodies.len()
    );
    let runner = InterpRunner::new(out.mir_bodies.clone(), ty_ctx, out.entry_main);
    let result = runner.run(Duration::from_secs(30));
    eprintln!("=== INTERP RESULT ===");
    eprintln!("  exit_code:  {:?}", result.exit_code);
    eprintln!("  stdout:     {:?}", result.stdout);
    eprintln!("  stderr:     {:?}", result.stderr);
    eprintln!("  timed_out:  {}", result.timed_out);
}

#[test]
fn probe_slice_iter_next() {
    probe("run-pass/behavior/slice_iter_next.g", &["--with-stdlib"]);
}
