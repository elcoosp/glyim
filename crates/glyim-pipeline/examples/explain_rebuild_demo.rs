//! M6.9 `--explain-rebuild` consumer example.
//!
//! Demonstrates [`Database::render_rebuild_report`] (the documented output
//! format `--explain-rebuild` prints) by issuing the same `def_map` query
//! twice: the first call is a cache miss (compute runs, recorded `miss`); the
//! second is a backdate (the body-agnostic `item_tree_fingerprint` is
//! unchanged, so compute is skipped and the cached value is returned,
//! recorded `backdated`). The rendered report is the deterministic text the
//! CLI flag emits to stderr.
//!
//! # Run
//!
//! ```sh
//! cargo run -p glyim-pipeline --no-default-features --example explain_rebuild_demo
//! ```
//!
//! Expected output: a two-row `--explain-rebuild` table — `def_map … miss`
//! then `def_map … backdated` — proving the trace recorded both outcomes and
//! the render is the documented format.
//!
//! # Note
//!
//! The main compile driver does not yet route its phases through the
//! `BodyQueries` API (the M6.4 driver wire-up is the remaining follow-up), so
//! a `glyim-cli --explain-rebuild` run on a real program currently reports
//! `0 queries recorded` — the query DB is not exercised by the driver. This
//! example drives the query API directly, which is the path the driver will
//! take once M6.4's wire-up lands; the trace API and render format are stable
//! either way.

use glyim_db::{CrateConfig, Database};
use glyim_pipeline::body_queries::BodyQueries;
use glyim_span::{FileId, HygieneCtx};
use std::path::Path;
use std::sync::Arc;

/// Parse + expand + `build_def_map` — the `compute` closure a real consumer
/// would pass to `def_map_query`.
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
    let src: Arc<str> = Arc::from("fn foo() -> i32 { 1 }\nfn bar() -> i32 { 2 }\n");
    let file = db.vfs().add_file_content(path, src.clone());

    // First query: cache miss → compute runs → recorded `miss`.
    db.clear_rebuild_trace();
    let arc1 = db.def_map_query(file, || build_def_map(&db, &src, file));
    println!("initial def_map cached ({} modules)", arc1.modules.len());
    print!("{}", db.render_rebuild_report());

    // Second query, unchanged source: the body-agnostic fingerprint is
    // unchanged → backdate → compute skipped → recorded `backdated`.
    db.clear_rebuild_trace();
    let arc2 = db.def_map_query(file, || panic!("backdate must not recompute"));
    println!(
        "after unchanged re-compile: cache hit (Arc::ptr_eq = {})",
        Arc::ptr_eq(&arc1, &arc2),
    );
    print!("{}", db.render_rebuild_report());

    // Signature change: rename `bar` → `baz`. The fingerprint moves → miss.
    db.clear_rebuild_trace();
    let src2: Arc<str> = Arc::from("fn foo() -> i32 { 1 }\nfn baz() -> i32 { 2 }\n");
    db.vfs().set_file_content(file, src2.clone()).expect("set");
    db.def_map_query(file, || build_def_map(&db, &src2, file));
    print!("{}", db.render_rebuild_report());
}
