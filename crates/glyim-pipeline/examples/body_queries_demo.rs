//! M6.4 body-query consumer example.
//!
//! Demonstrates the [`BodyQueries`] trait on [`Database`]: the `def_map`
//! query returns a cached `Arc<CrateDefMap>` on a body-only edit (early
//! cutoff / backdating — the body-agnostic `item_tree_fingerprint` key is
//! unchanged), and recomputes only on a signature change.
//!
//! # Run
//!
//! ```sh
//! cargo run -p glyim-pipeline --no-default-features --example body_queries_demo
//! ```
//!
//! Expected output: the initial query caches the def map; a body-only edit is
//! a cache hit (`Arc::ptr_eq` = true, `compute` not called); a signature change
//! (renaming `bar` → `baz`) is a cache miss (`Arc::ptr_eq` = false, `compute`
//! called, fingerprint moved).

use glyim_db::{CrateConfig, Database, QueryContext};
use glyim_pipeline::body_queries::BodyQueries;
use glyim_span::{FileId, HygieneCtx};
use std::path::Path;
use std::sync::Arc;

/// Parse + expand + `build_def_map` — the `compute` closure a real consumer
/// would pass to `def_map_query`. The query caches the result; this only runs
/// on a cache miss.
fn build_def_map(db: &Database, src: &str, file: FileId) -> glyim_def_map::CrateDefMap {
    let parse = glyim_frontend::parse_to_syntax(src, file);
    let mut hygiene = HygieneCtx::new();
    let mut expander = glyim_meta::Expander::new(&mut hygiene);
    expander.set_source_file(file);
    expander.set_vfs(db.vfs());
    let (expanded, _) = expander.expand_crate(&parse.root);
    glyim_def_map::build_def_map(&expanded, db.krate(), db.interner().clone()).0
}

fn main() {
    let db = Database::new(CrateConfig {
        name: "demo".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 0,
    });
    let path = Path::new("demo.g");
    let v1: Arc<str> = Arc::from("fn foo() -> i32 { 1 }\nfn bar() -> i32 { 2 }\n");
    let file = db.vfs().add_file_content(path, v1.clone());

    // Initial query: cache miss → compute → cache.
    let arc1 = db.def_map_query(file, || build_def_map(&db, &v1, file));
    let fp1 = QueryContext::item_tree_fingerprint(&db, file);
    println!("initial def_map cached ({} modules)", arc1.modules.len());

    // Body-only edit: `bar`'s body changes, signature unchanged. The
    // `item_tree_fingerprint` is body-agnostic, so the query cache-hits and
    // returns the SAME Arc — the compute closure (which would panic) never runs.
    let v2: Arc<str> = Arc::from("fn foo() -> i32 { 1 }\nfn bar() -> i32 { 999 }\n");
    db.vfs().set_file_content(file, v2).expect("set content");
    let arc2 = db.def_map_query(file, || panic!("body-only edit must cache-hit"));
    let fp2 = QueryContext::item_tree_fingerprint(&db, file);
    println!(
        "after body-only edit: cache hit (Arc::ptr_eq = {}, fingerprint unchanged = {})",
        Arc::ptr_eq(&arc1, &arc2),
        fp1 == fp2,
    );

    // Signature change: rename `bar` → `baz`. The fingerprint moves → cache miss
    // → compute runs → fresh Arc.
    let v3: Arc<str> = Arc::from("fn foo() -> i32 { 1 }\nfn baz() -> i32 { 999 }\n");
    db.vfs()
        .set_file_content(file, v3.clone())
        .expect("set content");
    let arc3 = db.def_map_query(file, || build_def_map(&db, &v3, file));
    let fp3 = QueryContext::item_tree_fingerprint(&db, file);
    println!(
        "after signature change: cache miss (Arc::ptr_eq = {}, fingerprint moved = {})",
        Arc::ptr_eq(&arc1, &arc3),
        fp1 != fp3,
    );
}
