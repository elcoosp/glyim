//! Corpus-generation throughput bench (M2.3 slice 1).
//!
//! Dependency-free (`harness = false`), mirroring the `glyim-test`
//! `annotations` and `glyim-metrics` `scan_bench` benches: the M2.3 corpora
//! work (committing programs with hashes, feeding the M2.4 scaling analyzer)
//! regenerates whole manifests on every run, so CI logs should track the
//! generator's headroom. The workload is the full seven-family manifest at
//! the M2.3 default size, plus a single-family re-generation loop that shows
//! the per-program marginal cost. Run with:
//!
//! ```text
//! cargo bench -p glyim-bench --bench corpus_gen
//! ```
//!
//! The generated size is `$GLYIM_CORPUS_BENCH_SIZE` when set (the same
//! environment-override pattern `scan_bench` uses for its scanned root).

use std::hint::black_box;
use std::time::Instant;

use glyim_bench::corpus::{CorpusFamily, CorpusManifest, MAX_SIZE, generate};

/// Seeds per manifest build (the committed corpora slice regenerates the
/// whole family × seed grid).
const SEEDS: usize = 16;

/// Programs generated in the marginal-cost loop.
const REGEN_ITERATIONS: usize = 512;

fn bench_size() -> usize {
    std::env::var("GLYIM_CORPUS_BENCH_SIZE")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| (1..=MAX_SIZE).contains(v))
        .unwrap_or(24)
}

fn main() {
    let size = bench_size();
    let seeds: Vec<u64> = (0..SEEDS as u64).collect();

    // Component 1: full seven-family manifest build (the M2.3 commit path).
    let t = Instant::now();
    let manifest = black_box(CorpusManifest::build(
        black_box(&CorpusFamily::ALL),
        black_box(&seeds),
        black_box(size),
    ))
    .expect("corpus plan is valid");
    let build_ns = t.elapsed().as_nanos() as f64;

    // Component 2: verification (regenerate + hash every row).
    let t = Instant::now();
    black_box(manifest.verify().is_ok());
    let verify_ns = t.elapsed().as_nanos() as f64;

    // Component 3: marginal cost of one program.
    let t = Instant::now();
    for i in 0..REGEN_ITERATIONS {
        let family = CorpusFamily::ALL[i % CorpusFamily::ALL.len()];
        black_box(generate(
            black_box(family),
            black_box(i as u64),
            black_box(size),
        ));
    }
    let per_program_ns = t.elapsed().as_nanos() as f64 / REGEN_ITERATIONS as f64;

    let programs = manifest.programs.len();
    println!("corpus_gen: {programs} programs (7 families x {SEEDS} seeds, size {size})");
    println!(
        "  manifest build: {build_ns:.0} ns total ({:.0} ns/program)",
        build_ns / programs as f64,
    );
    println!("  manifest verify: {verify_ns:.0} ns total");
    println!("  per-program generate: {per_program_ns:.0} ns");
}
