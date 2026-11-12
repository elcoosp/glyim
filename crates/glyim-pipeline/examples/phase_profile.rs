//! Library consumer example: measure a compile with the M2.1 phase
//! profiler.
//!
//! `glyim --time-passes` / `--self-profile` / `--emit=stats` are thin CLI
//! wrappers over [`glyim_pipeline::compile_stats`] and the
//! [`glyim_pipeline::PhaseProfile`] renderers. This example drives the
//! same seams as a library consumer — no CLI, no backend — by staging a
//! small `.g` program and printing every renderer's output.
//!
//! Run with:
//!
//! ```text
//! cargo run -p glyim-pipeline --example phase_profile
//! ```

use glyim_db::{CrateConfig, Database};

/// The toy program: three small functions so the mono/collect phase has
/// real work and the profile is not a single dominating `typeck` row.
const PROGRAM: &str = r"
fn add(a: i32, b: i32) -> i32 {
    a + b
}

fn fib(n: i32) -> i32 {
    let mut a: i32 = 0;
    let mut b: i32 = 1;
    let mut i: i32 = 0;
    while i < n {
        let next: i32 = add(a, b);
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
    // 1. Stage the program in a scratch directory (the pipeline reads
    //    from the VFS/disk like every real compile).
    let dir = std::env::temp_dir().join("glyim-phase-profile-example");
    std::fs::create_dir_all(&dir).expect("scratch dir must be creatable");
    let source = dir.join("measured.g");
    std::fs::write(&source, PROGRAM).expect("scratch source must be writable");

    // 2. The compiler session.
    let mut db = Database::new(CrateConfig {
        name: "measured".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 0,
    });

    // 3. Measure. `compile_stats` is exactly what `--emit=stats` calls:
    //    the analysis-only front half, no backend, no object file.
    let report = match glyim_pipeline::compile_stats(&mut db, &source, None, 2) {
        Ok(report) => report,
        Err(diags) => {
            for diag in diags {
                eprintln!("{:?}", glyim_diag::Report::new(diag));
            }
            std::process::exit(1);
        }
    };

    // 4. Renderer 1 — the `--time-passes` table (stderr in the CLI).
    println!("== --time-passes table ==");
    print!("{}", report.profile.render_text());

    // 5. Renderer 2 — the `--emit=stats` JSON (schema version 1).
    println!("== --emit=stats JSON ==");
    println!("{}", report.render_json());
    println!(
        "(mono bodies: {}, advisory warnings: {})\n",
        report.mono_bodies, report.warnings
    );

    // 6. Renderer 3 — the `--self-profile` Chrome Trace, pretty-echoed so
    //    the example's output shows the event shape Perfetto would load.
    //    The CLI writes this string straight to `--self-profile=FILE`.
    println!("== --self-profile Chrome trace (head) ==");
    let trace = report.profile.render_chrome_trace();
    for line in trace.split("\"displayTimeUnit\"").take(1) {
        println!("{line}...\"displayTimeUnit\":\"ms\"}}");
    }

    // 7. Clean the scratch files so repeated runs stay tidy.
    let _ = std::fs::remove_dir_all(&dir);
}
