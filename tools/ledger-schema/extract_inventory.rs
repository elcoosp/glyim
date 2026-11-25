//! Inventory extraction timing harness for the Readiness Ledger (M1.3).
//!
//! Dependency-free (`harness = false`): synthesizes Rust sources at the
//! scale the tree actually has (five tracked MIR enums with ~30 variants,
//! ~100 `no_mangle` FFI symbols, the 207-variant `SyntaxKind` enum, ~25
//! type-form variants, ~20 CLI flags, and ~40 stdlib `.g` items), then
//! measures repeated extraction runs over every §3.5 family so CI logs
//! can track the `ledger-check` job's extraction headroom. Run with:
//!
//! ```text
//! cargo bench -p ledger-schema
//! ```
//!
//! Parsing dominates the cost (`syn` full grammar for the Rust families,
//! the Glyim frontend for the stdlib family), so the harness keeps one
//! warmup run before measuring; the numbers are compile-time-only (no
//! file IO — the walk is trivially linear in file count).

use std::fmt::Write as _;
use std::hint::black_box;
use std::time::Instant;

use ledger_schema::{
    extract_cli_flags, extract_error_codes, extract_lints, extract_mir_variants,
    extract_runtime_ffi, extract_stdlib_api, extract_syntax_kinds, extract_ty_kinds,
};

/// Variants per synthetic MIR enum (the real tree has 29 across 5 enums).
const VARIANTS_PER_ENUM: usize = 30;

/// Synthetic FFI symbols (the real tree has 99 across 5 files).
const FFI_SYMBOLS: usize = 100;

/// Synthetic `SyntaxKind` variants (the real tree has 207).
const SYNTAX_KINDS: usize = 207;

/// Synthetic type-form variants per tracked enum (the real tree has 24
/// `TyKind` + 3 `AdtKind` + 17 `BuiltinTraitDef` variants).
const TY_VARIANTS: usize = 25;

/// Synthetic CLI flags (the real tree has 18).
const CLI_FLAGS: usize = 18;

/// Synthetic stdlib items in one module (the real tree has up to 30).
const STDLIB_ITEMS: usize = 30;

/// Synthetic registered error codes (the real tree has 12).
const ERROR_CODES: usize = 12;

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

fn syntax_source() -> String {
    let mut source = String::from("pub enum SyntaxKind {\n");
    for kind in 0..SYNTAX_KINDS {
        let _ = writeln!(source, "    /// kind {kind}");
        let _ = writeln!(source, "    Kind{kind},");
    }
    source.push_str("}\n");
    source
}

fn ty_source() -> String {
    let mut source = String::new();
    for name in ["TyKind", "AdtKind", "BuiltinTraitDef"] {
        let _ = writeln!(source, "pub enum {name} {{");
        for variant in 0..TY_VARIANTS {
            let _ = writeln!(source, "    /// form {variant}");
            let _ = writeln!(source, "    Form{variant}(u64),");
        }
        source.push_str("}\n\n");
    }
    source
}

fn cli_source() -> String {
    let mut source = String::from("#[derive(Parser, Debug)]\npub struct CliArgs {\n");
    for flag in 0..CLI_FLAGS {
        let _ = writeln!(source, "    /// flag {flag}");
        let _ = writeln!(
            source,
            "    #[arg(long = \"flag-{flag}\", default_value = \"off\")] pub flag_{flag}: bool,"
        );
    }
    source.push_str("}\n");
    source
}

fn stdlib_source() -> String {
    let mut source = String::new();
    for item in 0..STDLIB_ITEMS {
        let _ = writeln!(source, "/// item {item}");
        let _ = writeln!(source, "fn synth_{item}(x: i32) -> i32 {{ x }}");
    }
    source
}

fn error_code_source() -> String {
    let mut source = String::new();
    for code in 0..ERROR_CODES {
        let _ = writeln!(
            source,
            "[[code]]\ncategory = \"Type\"\nnumber = {code}\ntitle = \"t{code}\"\nexplanation = \"synthetic\"\n"
        );
    }
    source
}

fn main() {
    const RUNS: usize = 5;

    let mir = mir_source();
    let ffi = ffi_source();
    let syntax = syntax_source();
    let ty = ty_source();
    let cli = cli_source();
    let stdlib = stdlib_source();
    let codes = error_code_source();

    // Warmup (also asserts the synthetic corpora extract cleanly).
    let warm_mir = extract_mir_variants(black_box(&mir));
    let warm_ffi = extract_runtime_ffi(black_box(&ffi));
    let warm_syntax = extract_syntax_kinds(black_box(&syntax));
    let warm_ty = extract_ty_kinds(black_box(&ty));
    let warm_cli = extract_cli_flags(black_box(&cli));
    let warm_stdlib = extract_stdlib_api(black_box(&stdlib), "core", "synth");
    let warm_codes = extract_error_codes(black_box(&codes));
    let warm_lints = extract_lints(black_box(&ty));
    println!(
        "corpus: {} MIR + {} FFI + {} syntax + {} ty + {} cli + {} stdlib + {} codes (lints reuses the ty corpus)",
        5 * VARIANTS_PER_ENUM,
        FFI_SYMBOLS,
        SYNTAX_KINDS,
        3 * TY_VARIANTS,
        CLI_FLAGS,
        STDLIB_ITEMS,
        ERROR_CODES,
    );
    assert_eq!(
        warm_mir.len(),
        5 * VARIANTS_PER_ENUM,
        "synthetic MIR corpus"
    );
    assert_eq!(warm_ffi.len(), FFI_SYMBOLS, "synthetic FFI corpus");
    assert_eq!(
        warm_syntax.len(),
        SYNTAX_KINDS,
        "synthetic SyntaxKind corpus"
    );
    assert_eq!(warm_ty.len(), 3 * TY_VARIANTS, "synthetic ty corpus");
    assert_eq!(warm_cli.len(), CLI_FLAGS, "synthetic CLI corpus");
    assert_eq!(warm_stdlib.len(), STDLIB_ITEMS, "synthetic stdlib corpus");
    assert_eq!(warm_codes.len(), ERROR_CODES, "synthetic error-code corpus");
    assert!(warm_lints.is_empty(), "ty corpus carries no lints");

    let mut elapsed_ns: Vec<u128> = Vec::with_capacity(RUNS);
    for run in 0..RUNS {
        let start = Instant::now();
        let mir_items = extract_mir_variants(black_box(&mir));
        let ffi_items = extract_runtime_ffi(black_box(&ffi));
        let syntax_items = extract_syntax_kinds(black_box(&syntax));
        let ty_items = extract_ty_kinds(black_box(&ty));
        let cli_items = extract_cli_flags(black_box(&cli));
        let stdlib_items = extract_stdlib_api(black_box(&stdlib), "core", "synth");
        let code_items = extract_error_codes(black_box(&codes));
        let lint_items = extract_lints(black_box(&ty));
        black_box((
            mir_items,
            ffi_items,
            syntax_items,
            ty_items,
            cli_items,
            stdlib_items,
            code_items,
            lint_items,
        ));
        let d = start.elapsed();
        elapsed_ns.push(d.as_nanos());
        println!("run {run}: {:>10} ns/extract+all-families", d.as_nanos());
    }

    let min = elapsed_ns.iter().min().copied().unwrap_or(0);
    let mean: u128 = elapsed_ns.iter().sum::<u128>() / RUNS.max(1) as u128;
    println!("min:  {min:>10} ns/extract");
    println!("mean: {mean:>10} ns/extract");
}
