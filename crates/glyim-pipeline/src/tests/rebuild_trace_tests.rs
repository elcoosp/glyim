//! M6.9 `--explain-rebuild` wire-up tests.
//!
//! Plan §6 M6.9 `DoD`: "documented output format". The format itself is pinned
//! in `glyim-db::rebuild_trace`; these tests pin the *wire-up* — that each
//! [`BodyQueries`] method records the correct `RebuildOutcome` at each cache
//! decision, so the rendered report reflects what the query DB actually did.

use crate::body_queries::BodyQueries;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_db::{Database, QueryKind, RebuildOutcome};
use glyim_mir::Body;
use glyim_span::Span;
use glyim_type::Ty;
use glyim_typeck::thir;
use std::sync::Arc;

/// A minimal THIR body distinct per `local` (different owner → different
/// `Debug` → different fingerprint).
fn thir_body_for(local: u32) -> thir::Body {
    thir::Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(local)),
        params: Vec::new(),
        return_ty: Ty::UNIT,
        stmts: Vec::new(),
        span: Span::DUMMY,
    }
}

/// `thir_body_query` records `Miss` on a fresh compute and `Backdated` when a
/// re-run's fingerprint matches the cached entry (early cutoff). The trace is
/// cleared between calls because the snapshot is sorted, not insertion-ordered.
#[test]
fn thir_body_records_miss_then_backdated() {
    let db = Database::default();
    let def_id = LocalDefId::from_raw(21);

    // Fresh: no cache → compute + cache + record Miss.
    db.thir_body_query(def_id, || thir_body_for(21));
    let snap = db.rebuild_trace_snapshot();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].kind, QueryKind::ThirBody);
    assert_eq!(snap[0].outcome, RebuildOutcome::Miss);

    // Re-run with a structurally identical body → fingerprint match → Backdated.
    db.clear_rebuild_trace();
    db.thir_body_query(def_id, || thir_body_for(21));
    let snap = db.rebuild_trace_snapshot();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].outcome, RebuildOutcome::Backdated);
}

/// `mir_build_query_with_diags` records `Miss` on a fresh compute and `Hit`
/// when the entry is present (compute closure does not run). The trace is
/// cleared between calls so each snapshot reflects one call — the snapshot
/// is sorted by `(kind, id, outcome)`, so two events for the same query would
/// sort `Hit` before `Miss` rather than in issue order.
#[test]
fn mir_build_records_miss_then_hit() {
    let db = Database::default();
    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(22));
    let ty_ctx = glyim_test::test_ty_ctx().freeze();

    // Fresh: cache miss → compute + validate + cache + record Miss.
    db.mir_build_query_with_diags(owner, &ty_ctx, || Ok((Body::dummy(owner), Vec::new())))
        .expect("compute succeeds");
    let snap = db.rebuild_trace_snapshot();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].kind, QueryKind::MirBuild);
    assert_eq!(snap[0].outcome, RebuildOutcome::Miss);

    // Re-run: cache hit → record Hit, compute closure must not run.
    db.clear_rebuild_trace();
    db.mir_build_query_with_diags(owner, &ty_ctx, || panic!("hit must not recompute"))
        .expect("cache hit returns Ok");
    let snap = db.rebuild_trace_snapshot();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].outcome, RebuildOutcome::Hit);
}

/// `def_map_query` records `Miss` on a fresh compute and `Backdated` when the
/// body-agnostic fingerprint is unchanged across a body-only edit.
#[test]
fn def_map_records_miss_then_backdated() {
    let db = Database::default();
    let src: Arc<str> = Arc::from("fn foo() -> i32 { 1 }\n");
    let file = db
        .vfs()
        .add_file_content(std::path::Path::new("m6_9.g"), src.clone());

    // Fresh: fingerprint computed, no cached entry → compute + cache + Miss.
    db.def_map_query(file, || build_def_map(&db, &src, file));
    let snap = db.rebuild_trace_snapshot();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].kind, QueryKind::DefMap);
    assert_eq!(snap[0].outcome, RebuildOutcome::Miss);

    // Re-run with unchanged source → fingerprint match → Backdated.
    db.clear_rebuild_trace();
    db.def_map_query(file, || panic!("backdate must not recompute"));
    let snap = db.rebuild_trace_snapshot();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].outcome, RebuildOutcome::Backdated);
}

/// Parse + expand + `build_def_map` — the `compute` closure a real consumer
/// would pass to `def_map_query` (mirrors the M6.4 `body_queries_demo`
/// example).
fn build_def_map(db: &Database, src: &str, file: glyim_span::FileId) -> glyim_def_map::CrateDefMap {
    let parse = glyim_frontend::parse_to_syntax(src, file);
    let mut hygiene = glyim_span::HygieneCtx::new();
    let mut expander = glyim_meta::Expander::new(&mut hygiene);
    expander.set_source_file(file);
    expander.set_vfs(db.vfs());
    let (expanded, _) = expander.expand_crate(&parse.root);
    glyim_def_map::build_def_map(&expanded, db.krate(), db.interner().clone()).0
}
