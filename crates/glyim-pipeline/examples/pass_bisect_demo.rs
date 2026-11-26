//! Library consumer example: drive the MIR pass manager's bisect seam
//! (`--opt-bisect-limit=N`) as a library.
//!
//! This is the M5.11 walkthrough: the pass manager already validates after
//! every pass and attributes violations to the offending pass; this example
//! shows the consumer-facing slice — compile the same program with no
//! limit (the plain `optimize_at_level` path, no stats collected) and with
//! `opt_bisect_limit = Some(2)` (only the first two passes of the `-O2`
//! plan run, one [`glyim_opt::PassStats`] row per planned pass), then
//! print both the per-pass table and the statement-count difference that
//! proves the limit stopped before DCE.
//!
//! The CLI equivalent is `glyim --emit=run --opt-bisect-limit=2
//! --time-passes prog.g`, which prints the same aggregated table after the
//! phase profile.
//!
//! Run with:
//!
//! ```text
//! cargo run -p glyim-pipeline --example pass_bisect_demo
//! ```

use glyim_db::{CrateConfig, Database};
use glyim_pipeline::compile_file_to_mir_with_proc_registry;

/// The toy program: `40 + 2` is folded by `constant_prop` (plan pass 2)
/// and the dead store it feeds is removed by `dce` (plan pass 3) — so a
/// bisect limit of 2 keeps the dead store and the full run removes it.
const PROGRAM: &str = r"
fn main() -> i32 {
    let dead: i32 = 40 + 2;
    return 0;
}
";

fn compile(db: &mut Database, path: &std::path::Path, limit: Option<usize>) {
    let mir = compile_file_to_mir_with_proc_registry(db, path, None, 2, limit)
        .expect("the demo program must compile");
    let limit_note = match limit {
        Some(n) => format!("--opt-bisect-limit={n}"),
        None => "no limit".to_string(),
    };
    let stmts: usize = mir
        .bodies
        .values()
        .map(|b| {
            b.basic_blocks
                .iter()
                .map(|bb| bb.statements.len())
                .sum::<usize>()
        })
        .sum();
    println!("compile ({limit_note}): {stmts} statements");
    if mir.pass_stats.is_empty() {
        println!("  (un-bisected path: no per-pass rows collected)");
        return;
    }
    println!(
        "  {:<16} {:>5} {:>8} {:>8} {:>8}",
        "pass", "ran", "changed", "stmtsΔ", "blocksΔ"
    );
    for (owner, s) in &mir.pass_stats {
        println!(
            "  {:<16} {:>5} {:>8} {:>8} {:>8}  ({owner:?})",
            s.name,
            if s.ran { "yes" } else { "no" },
            if s.body_changed { "yes" } else { "no" },
            s.stmt_delta,
            s.block_delta,
        );
    }
}

fn main() {
    // 1. Materialise the program in a scratch dir (the pipeline reads from
    //    the VFS, which the `Database` loads from disk).
    let dir = std::env::temp_dir().join("glyim-pass-bisect-example");
    std::fs::create_dir_all(&dir).expect("scratch dir must be creatable");
    let source = dir.join("pass_bisect_demo.g");
    std::fs::write(&source, PROGRAM).expect("scratch source must be writable");

    // 2. Compile the same program twice through the registry-threaded MIR
    //    entry (`--emit=run`'s entry point): once un-bisected, once at
    //    `--opt-bisect-limit=2`.
    let mut db = Database::new(CrateConfig {
        name: "pass_bisect_demo".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 2,
    });
    compile(&mut db, &source, None);
    compile(&mut db, &source, Some(2));
}
