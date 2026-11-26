//! `M5.1` `Phase` 3 driver-extraction parity tests (`M1` `DoD` c).
//!
//! The driver monolith (`prepare_compilation`, 464 lines) is collapsed into a
//! ≤60-line chain of extracted `CompilerPhase`-implementing structs
//! (`LexParsePhase` → `DefMapPhase` → `HirPhase` → `TypeckPhase` →
//! `LowerPhase` → `MonoPhase` → `EmitPhase`). The extraction is a pure
//! behavior-preserving move: every diagnostic and every MIR byte the monolith
//! produced must survive the refactor unchanged.
//!
//! These tests pin that contract: run `compile_file_to_mir` (the public entry
//! that routes through `prepare_compilation`) on two fixtures, normalize the
//! output (MIR bodies sorted by owner `DefId`, diagnostics sorted by their
//! `Debug` rendering), and hash the normalized form with the `M6.1`
//! [`glyim_core::StableHasher`]. The hash is compared to a golden captured
//! from the pre-refactor monolith — a single mismatched byte moves the hash
//! and fails the test.
//!
//! Fixtures: `simple_main` (arithmetic `main`, exercises the full
//! `LexParse` → `DefMap` → `Hir` → `Typeck` → `Lower` → `Mono` → `Emit` chain) and `struct_impl` (a `struct Point` + `impl` block +
//! method call, exercises `TypeckPhase`'s ADT/impl-header registration and the
//! annotation-needed path in `EmitPhase`).

use crate::compile_file_to_mir;
use glyim_core::StableHasher;
use glyim_db::{CrateConfig, Database};
use std::path::PathBuf;

/// Unique temp path so parallel test runs never collide.
fn fixture_path(name: &str, src: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "glyim_mir_parity_{name}_{}_{}.g",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    ));
    std::fs::write(&path, src).expect("write fixture");
    path
}

/// Normalize a `MirCompilation` into a deterministic byte stream and hash it
/// with the `M6.1` `StableHasher`. Bodies are sorted by owner `DefId` (raw krate
/// then raw local) so declaration-order wobble does not move the hash;
/// diagnostics are sorted by their `Debug` rendering so the set is
/// order-independent (the pipeline's `DiagSink::into_diagnostics` already
/// sorts + dedups on a total key, but the sort here is the test's own
/// canonicalization, independent of that contract).
fn normalized_hash(
    result: &Result<crate::MirCompilation, Vec<glyim_diag::GlyimDiagnostic>>,
) -> [u8; 16] {
    let mut hasher = StableHasher::new();
    match result {
        Ok(mir) => {
            let mut entries: Vec<_> = mir.bodies.iter().collect();
            entries.sort_by_key(|(def_id, _)| (def_id.krate.to_raw(), def_id.local_id.to_raw()));
            for (def_id, body) in entries {
                hasher.write_str(&format!("BODY:{def_id:?}"));
                hasher.write_str(&format!("{body:#?}"));
            }
            let mut diags = mir.warnings.clone();
            diags.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
            for d in &diags {
                hasher.write_str("DIAG:");
                hasher.write_str(&format!("{d:#?}"));
            }
        }
        Err(diags) => {
            let mut diags = diags.clone();
            diags.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
            for d in &diags {
                hasher.write_str("DIAG:");
                hasher.write_str(&format!("{d:#?}"));
            }
        }
    }
    hasher.finish()
}

fn assert_parity(name: &str, src: &str, golden_hex: &str) {
    let path = fixture_path(name, src);
    let mut db = Database::new(CrateConfig {
        name: "parity".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 0,
    });
    let result = compile_file_to_mir(&mut db, &path, 0);
    let hash = normalized_hash(&result);
    use std::fmt::Write;
    let mut actual_hex = String::with_capacity(hash.len() * 2);
    for &b in hash.iter() {
        write!(actual_hex, "{b:02x}").expect("hex write into String is infallible");
    }
    assert_eq!(
        actual_hex, golden_hex,
        "M1 parity drift on {name}: the extracted-phase chain produced a different normalized MIR+diagnostic hash than the pre-refactor monolith golden.\n\
         expected (monolith golden): {golden_hex}\n\
         actual   (extracted chain): {actual_hex}\n\
         A byte-identical diagnostics + MIR contract is the M5.1 Phase 3 DoD."
    );
}

/// `simple_main`: arithmetic `main` exercises the full
/// `LexParse` → `DefMap` → `Hir` → `Typeck` → `Lower` → `Mono` → `Emit` chain end-to-end.
/// Golden re-captured after the intentional `Span` `(lo, len)` storage
/// migration (`f6506cad`, plan M3.2/CT-3) changed the `Debug` rendering the
/// normalization hashes; the M5.2 typed-THIR boundary (`TypedBody`) is
/// MIR-neutral (the chain's bytes are unchanged by it).
#[test]
fn mir_parity_simple_main_matches_monolith_golden() {
    assert_parity(
        "simple_main",
        "fn main() -> i32 {\n    let a = 3;\n    a * 2\n}\n",
        "ed2eeafb2870dd2323f1a0cb296c87b9",
    );
}

/// `struct_impl`: a `struct Point` + `impl Point { fn new }` + method call,
/// exercising `TypeckPhase`'s ADT forward-reference registration, type-alias /
/// impl-header resolution, and `EmitPhase`'s annotation-needed path. Golden
/// captured from the pre-refactor monolith.
#[test]
fn mir_parity_struct_impl_matches_monolith_golden() {
    assert_parity(
        "struct_impl",
        "struct Point { x: i32, y: i32 }\nimpl Point {\n    fn new(x: i32, y: i32) -> Point {\n        Point { x, y }\n    }\n}\nfn main() -> i32 {\n    let p = Point::new(1, 2);\n    p.x + p.y\n}\n",
        "61a9ba56c0fed3f110707ac3de30d017",
    );
}
