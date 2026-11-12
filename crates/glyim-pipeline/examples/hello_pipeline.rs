//! Library consumer example: compile a Glyim program to MIR.
//!
//! This is the "embed the compiler" walkthrough the README promises:
//! drive `glyim-pipeline` as a library — no `glyim` CLI, no LLVM, no
//! backend — by writing a small `.g` program to a scratch directory and
//! lowering it all the way to MIR through the same
//! `prepare_compilation` front half every real emit mode uses.
//!
//! Run with:
//!
//! ```text
//! cargo run -p glyim-pipeline --example hello_pipeline
//! ```

use glyim_db::{CrateConfig, Database};

/// The toy program: a loop accumulating the Fibonacci sequence, small
/// enough to read the whole MIR dump in one screen.
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
    // 1. Materialise the program. `emit_mir` reads from disk, so the
    //    example stages the source in the system temp dir.
    let dir = std::env::temp_dir().join("glyim-hello-pipeline-example");
    std::fs::create_dir_all(&dir).expect("scratch dir must be creatable");
    let source = dir.join("hello.g");
    std::fs::write(&source, PROGRAM).expect("scratch source must be writable");

    // 2. A `Database` is the compiler's session: interners, VFS, target
    //    configuration. Consumers create one per compilation.
    let mut db = Database::new(CrateConfig {
        name: "hello".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 0,
    });

    // 3. Lower to MIR. Errors come back as `Vec<GlyimDiagnostic>`,
    //    rendered with miette — the same diagnostics the CLI prints.
    //    Success also carries the per-phase profile (M2.1): the same
    //    data `glyim --time-passes` prints, free for library consumers.
    let out = dir.join("hello.mir");
    match glyim_pipeline::emit_mir(&mut db, &source, &out, None, 2) {
        Ok(profile) => {
            let text = std::fs::read_to_string(&out).unwrap_or_default();
            println!("MIR written to {} ({} bytes)\n", out.display(), text.len());
            // Show the head so the example's output is self-describing.
            for line in text.lines().take(24) {
                println!("{line}");
            }
            // The phase table: deterministic schema, wall-clock numbers.
            println!("\nPhase profile for this compile:");
            print!("{}", profile.render_text());
        }
        Err(diags) => {
            for diag in diags {
                eprintln!("{:?}", glyim_diag::Report::new(diag));
            }
            std::process::exit(1);
        }
    }

    // 4. Clean the scratch files so repeated runs stay tidy.
    let _ = std::fs::remove_dir_all(&dir);

    // A driver-only variant for library consumers that want the lowered
    // bodies *in memory* (e.g. to feed the MIR interpreter directly):
    //
    //   let mir = glyim_pipeline::compile_file_to_mir(&mut db, &source)?;
    //   for (_def_id, body) in &mir.bodies { /* inspect body */ }
}
