//! Library consumer example: compile **and execute** a Glyim program
//! in-process through the MIR interpreter.
//!
//! This is the "run without installing anything" walkthrough (M2.4): the
//! same `glyim --emit=run` capability, driven as a library. No LLVM, no
//! system linker, no `glyim-runtime` staticlib — the program is lowered
//! through the shared `prepare_compilation` front half and executed via
//! the canonical [`glyim_mir_interp::run_program`] process contract,
//! which is also what the `glyim` CLI's `--emit=run` and the test
//! harness's `InterpRunner` use.
//!
//! Run with:
//!
//! ```text
//! cargo run -p glyim-pipeline --example run_interp
//! ```

use glyim_db::{CrateConfig, Database};
use glyim_mir_interp::RunKind;

/// The toy program: `main` returns the exit code, the way a real binary
/// would — `--emit=run` propagates it as the process exit code (masked
/// to `0xff`, like the OS does for a linked `--emit=exec` binary).
const PROGRAM: &str = r"
fn fib(n: i32) -> i32 {
    let mut a: i32 = 0;
    let mut b: i32 = 1;
    let mut i: i32 = 0;
    while i < n {
        let next: i32 = a + b;
        a = b;
        b = next;
        i = i + 1;
    }
    a
}

fn main() -> i32 {
    fib(10)
}
";

fn main() {
    // 1. Materialise the program in a scratch dir (the pipeline reads
    //    from the VFS, which the `Database` loads from disk).
    let dir = std::env::temp_dir().join("glyim-run-interp-example");
    std::fs::create_dir_all(&dir).expect("scratch dir must be creatable");
    let source = dir.join("run_interp.g");
    std::fs::write(&source, PROGRAM).expect("scratch source must be writable");

    // 2. Compile to MIR through the registry-threaded entry point — the
    //    same one `--emit=run` uses, so a proc-macro-dependent program
    //    gets identical `LocalDefId` numbering in the entry pre-pass and
    //    the compile. The returned `MirCompilation` carries the same
    //    observability side-channels as every other entry point
    //    (`profile` for `--time-passes`, `warnings` to render).
    let mut db = Database::new(CrateConfig {
        name: "run_interp".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 0,
    });
    let compilation =
        glyim_pipeline::compile_file_to_mir_with_proc_registry(&mut db, &source, None)
            .expect("the toy program must compile");
    for warning in &compilation.warnings {
        eprintln!("warning: {}", warning.message);
    }

    // 3. Resolve `main` through the entry pre-pass (the same raw
    //    `LocalDefId` the `--emit=exec` path hands to the backend for
    //    the C-ABI `main` wrapper).
    let entry_main = glyim_pipeline::Pipeline::entry_main_local_id(&mut db, &source)
        .expect("program has a main");

    // 4. Execute. The process contract is canonical: exit code masked to
    //    `0xff`, panics exit 101 with a `thread 'main' panicked:` stderr
    //    line, and stdout/stderr bytes come back on every path.
    let bodies: Vec<_> = compilation.bodies.values().cloned().collect();
    let outcome =
        glyim_mir_interp::run_program(&bodies, compilation.ty_ctx.as_ref(), Some(entry_main));

    println!("run kind    : {:?}", outcome.kind);
    println!("exit code   : {}", outcome.exit_code);
    println!("stdout bytes: {}", outcome.stdout.len());
    assert_eq!(outcome.kind, RunKind::Completed);
    // fib(10) == 55; the OS masks `main`'s 55 to 55 (still < 256).
    assert_eq!(outcome.exit_code, 55);
    println!("fib(10) executed in-process — exit code 55, no linker involved");
}
