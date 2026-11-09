//! Inventory extraction timing harness for the Readiness Ledger (M1.3).
//!
//! Dependency-free (`harness = false`): synthesizes Rust sources at the
//! scale the tree actually has (five tracked MIR enums with ~30 variants
//! plus ~100 `no_mangle` FFI symbols), then measures repeated
//! `extract_mir_variants` + `extract_runtime_ffi` runs so CI logs can
//! track the `ledger-check` job's extraction headroom. Run with:
//!
//! ```text
//! cargo bench -p ledger-schema
//! ```
//!
//! Parsing dominates the cost (`syn` full grammar), so the harness keeps
//! one warmup run before measuring; the numbers are compile-time-only
//! (no file IO — the walk is trivially linear in file count).

use std::fmt::Write as _;
use std::hint::black_box;
use std::time::Instant;

use ledger_schema::{extract_mir_variants, extract_runtime_ffi};

/// Variants per synthetic MIR enum (the real tree has 29 across 5 enums).
const VARIANTS_PER_ENUM: usize = 30;

/// Synthetic FFI symbols (the real tree has 99 across 5 files).
const FFI_SYMBOLS: usize = 100;

fn mir_source() -> String {
    // The tracked enum names (plan §3.5 row 3); the extractor ignores
    // anything else, so synthetic variants must live inside them.
    let enums = [
        "StatementKind",
        "Rvalue",
        "Operand",
        "ProjectionElem",
        "TerminatorKind",
    ];
    let mut source = String::new();
    for name in enums {
        let _ = writeln!(source, "pub enum {name} {{");
        for variant in 0..VARIANTS_PER_ENUM {
            let _ = writeln!(source, "    /// variant {variant}");
            let _ = writeln!(source, "    Variant{variant}(u64),");
        }
        source.push_str("}\n\n");
    }
    source
}

fn ffi_source() -> String {
    let mut source = String::new();
    for symbol in 0..FFI_SYMBOLS {
        let _ = writeln!(source, "/// symbol {symbol}");
        source.push_str("#[unsafe(no_mangle)]\n");
        let _ = writeln!(
            source,
            "pub unsafe extern \"C\" fn glyim_synth_{symbol}(a: usize, b: usize) -> usize {{ a.wrapping_add(b) }}"
        );
        source.push('\n');
    }
    source
}

fn main() {
    const RUNS: usize = 5;

    let mir = mir_source();
    let ffi = ffi_source();

    // Warmup (also asserts the synthetic corpus extracts cleanly).
    let warm_mir = extract_mir_variants(black_box(&mir));
    let warm_ffi = extract_runtime_ffi(black_box(&ffi));
    let expected_mir = 5 * VARIANTS_PER_ENUM;
    let expected_ffi = FFI_SYMBOLS;
    println!(
        "corpus: {expected_mir} MIR variants + {expected_ffi} FFI symbols (extracted {}/{})",
        warm_mir.len(),
        warm_ffi.len()
    );
    assert_eq!(warm_mir.len(), expected_mir, "synthetic MIR corpus");
    assert_eq!(warm_ffi.len(), expected_ffi, "synthetic FFI corpus");

    let mut elapsed_ns: Vec<u128> = Vec::with_capacity(RUNS);
    for run in 0..RUNS {
        let start = Instant::now();
        let mir_items = extract_mir_variants(black_box(&mir));
        let ffi_items = extract_runtime_ffi(black_box(&ffi));
        black_box((mir_items, ffi_items));
        let d = start.elapsed();
        elapsed_ns.push(d.as_nanos());
        println!("run {run}: {:>10} ns/extract+both-sources", d.as_nanos());
    }

    let min = elapsed_ns.iter().min().copied().unwrap_or(0);
    let mean: u128 = elapsed_ns.iter().sum::<u128>() / RUNS.max(1) as u128;
    println!("min:  {min:>10} ns/extract");
    println!("mean: {mean:>10} ns/extract");
}
