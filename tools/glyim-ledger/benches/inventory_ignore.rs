//! Inventory-ignore bench (plan §3.5 remainder faces).
//!
//! Dependency-free (`harness = false`), mirroring the `waivers` bench:
//! measures the `inventory ignore` filing path (extractor walk + registry
//! load + the duplicate/claimed scans over a synthetic 500-entry ignore
//! set, canonical fragment) and the `inventory unignore` retirement path
//! (registry load + exact-key scan + canonical rewrite), with the write
//! captured by the in-memory seams so the measured cost is the
//! validation work, not the fs write. Run with:
//!
//! ```text
//! cargo bench -p glyim-ledger --bench inventory_ignore
//! ```

use std::cell::RefCell;
use std::fmt::Write as _;
use std::hint::black_box;
use std::time::Instant;

use glyim_ledger::{
    InMemoryIgnoreRewriter, InMemoryIgnoreSink, InventoryIgnoreRequest, InventoryUnignoreRequest,
    run_inventory_ignore, run_inventory_unignore,
};
use ledger_schema::InventoryIgnore;

/// Builds the 500-entry ignore text seeded into the committed registry
/// (the worst case the duplicate scan sees; reasons are unique so the
/// canonical rewrite output is distinguishable per entry).
fn seeded_ignore_text(count: usize) -> String {
    let mut text = String::new();
    for i in 0..count {
        let _ = write!(
            text,
            "[[ignore]]\n\
             item = \"mir.StatementKind.Bench{i}\"\n\
             reason = \"bench exemption {i}: synthetic parity-matrix row\"\n\n"
        );
    }
    text
}

fn main() {
    let dir = tempfile::tempdir().expect("bench tmp");
    let root = dir.path();
    // The registry layout the load pass requires, and one extractable
    // runtime FFI symbol (the extractor walk cost is the fixture's, not
    // the ignore set's).
    std::fs::create_dir_all(root.join("ledger/features")).expect("mkdir features");
    std::fs::create_dir_all(root.join("ledger/gaps")).expect("mkdir gaps");
    std::fs::create_dir_all(root.join("crates/glyim-runtime/src")).expect("mkdir src");
    std::fs::write(
        root.join("crates/glyim-runtime/src/lib.rs"),
        "#[unsafe(no_mangle)]\npub extern \"C\" fn glyim_bench(size: usize) -> usize { size }\n",
    )
    .expect("write fixture symbol");
    std::fs::write(
        root.join("ledger/inventory-ignore.toml"),
        seeded_ignore_text(500),
    )
    .expect("seed ignore registry");

    // The add face: the full validation path (extractor walk, registry
    // load, exact-key membership over the extraction, claim scan over the
    // registry, duplicate scan over the seeded 500-entry set, canonical
    // fragment) with the append captured by the in-memory sink.
    let add = InventoryIgnoreRequest {
        item: "runtime.ffi.glyim_bench".to_string(),
        reason: "bench exemption: validation-path measurement".to_string(),
    };
    let sink = InMemoryIgnoreSink::default();
    let started = Instant::now();
    let added = black_box(run_inventory_ignore(
        black_box(root),
        black_box(&add),
        black_box(&sink),
    ))
    .expect("the bench add must pass");
    let add_elapsed = started.elapsed();
    assert_eq!(added.remaining, 501);
    assert_eq!(sink.written.borrow().len(), 1);
    assert!(added.fragment.contains("runtime.ffi.glyim_bench"));

    // The unignore face: the full retirement path (registry load over the
    // seeded 500-entry set, exact-key scan, canonical rewrite of the
    // remaining entries) with the rewrite captured by the in-memory
    // rewriter.
    let rewriter = InMemoryIgnoreRewriter {
        doc: RefCell::new(
            (0..500)
                .map(|i| InventoryIgnore {
                    item: format!("mir.StatementKind.Bench{i}"),
                    reason: format!("bench exemption {i}: synthetic parity-matrix row"),
                })
                .collect::<Vec<_>>(),
        ),
        removed: RefCell::new(Vec::new()),
    };
    let unignore = InventoryUnignoreRequest {
        item: "mir.StatementKind.Bench250".to_string(),
    };
    let started = Instant::now();
    let removed = black_box(run_inventory_unignore(
        black_box(root),
        black_box(&unignore),
        black_box(&rewriter),
    ))
    .expect("the bench unignore must pass");
    let unignore_elapsed = started.elapsed();
    assert_eq!(removed.remaining, 499);
    assert_eq!(
        removed.reason,
        "bench exemption 250: synthetic parity-matrix row"
    );
    assert_eq!(rewriter.removed.borrow().len(), 1);

    println!(
        "inventory-ignore bench: 500-entry ignore set (1 extractable symbol)\n  add     (extractor walk + registry + claim + dup scan, in-memory sink): {add_elapsed:.1?}\n  unignore (registry load + exact-key scan, in-memory rewrite): {unignore_elapsed:.1?}"
    );
}
