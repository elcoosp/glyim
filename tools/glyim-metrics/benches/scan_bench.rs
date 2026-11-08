//! Scan-throughput bench for `scan_tree` (M0.6).
//!
//! Dependency-free (`harness = false`): measures repeated full-tree scans
//! of the workspace and prints ns/scan plus per-file and per-line cost so
//! CI logs can track the metrics job's runtime headroom. Run with:
//!
//! ```text
//! cargo bench -p glyim-metrics
//! ```
//!
//! The scanned root is `$GLYIM_METRICS_BENCH_ROOT`, defaulting to the
//! workspace root (two levels above this crate).

use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

fn main() {
    const RUNS: usize = 5;

    let default_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = match std::env::var_os("GLYIM_METRICS_BENCH_ROOT") {
        Some(v) => PathBuf::from(v),
        None => default_root,
    };

    // Warmup (also caches the tree in the page cache).
    let warm = glyim_metrics::scan_tree(&root);
    println!(
        "root: {} ({} files, {} code lines)",
        root.display(),
        warm.rust_files,
        warm.code_lines
    );
    let mut elapsed_ns: Vec<u128> = Vec::with_capacity(RUNS);
    for i in 0..RUNS {
        let start = Instant::now();
        let m = glyim_metrics::scan_tree(black_box(&root));
        let d = start.elapsed();
        black_box(m);
        elapsed_ns.push(d.as_nanos());
        println!("run {i}: {:>10} ns", d.as_nanos());
    }

    let min = *elapsed_ns.iter().min().unwrap_or(&0);
    let mean: u128 = elapsed_ns.iter().sum::<u128>() / RUNS.max(1) as u128;
    println!("min:  {min:>10} ns/scan");
    println!("mean: {mean:>10} ns/scan");
    if warm.rust_files > 0 {
        println!("min per file: {:>8} ns", min / warm.rust_files as u128);
    }
    if warm.code_lines > 0 {
        println!("min per code line: {:>6} ns", min / warm.code_lines as u128);
    }
}
