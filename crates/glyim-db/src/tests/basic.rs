use std::sync::Arc;

use crate::query::QueryContext;
use crate::{CrateConfig, Database};

#[test]
fn test_database_tyctx_roundtrip() {
    // S23-T01: Database stores and retrieves TyCtx correctly
    let db = Database::new(CrateConfig {
        name: "test".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 0,
    });
    // Create a TyCtx (frozen) from a mutable builder
    let ctx_mut = glyim_test::test_ty_ctx();
    let bool_ty = ctx_mut.bool_ty();
    let ctx = ctx_mut.freeze();
    // Store it (move, no clone)
    db.set_ty_ctx(ctx);
    // Retrieve and verify
    let guard = db.get_ty_ctx();
    assert!(guard.is_some());
    let retrieved = guard.as_deref().unwrap();
    // Compare a simple property: bool_ty should be Ty::BOOL
    assert_eq!(retrieved.bool_ty(), bool_ty);
}

#[test]
fn test_database_interner_mut() {
    // S23-T02: Database stores and retrieves Interner mutably
    let mut db = Database::new(CrateConfig {
        name: "test".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 0,
    });
    let name = db.intern_mut().intern("hello");
    // Retrieve via shared reference
    assert_eq!(db.interner().resolve(name), "hello");
}

#[test]
fn test_database_vfs_content() {
    // S23-T03: Database Vfs returns correct file contents
    let db = Database::new(CrateConfig {
        name: "test".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 0,
    });
    let content: Arc<str> = Arc::from("fn main() {}");
    let file_id = db
        .vfs()
        .add_file_content(&std::path::PathBuf::from("main.g"), content.clone())
        .expect("fixture sources are far below the VFS length gate");
    // M6: read through the QueryContext::file_text query rather than reaching
    // into the VFS struct directly.
    let retrieved = db.file_text(file_id).unwrap();
    assert_eq!(&*retrieved, "fn main() {}");
}

#[test]
fn test_query_context_file_text_dispatches_to_vfs() {
    // M6 scaffolding: `Database: QueryContext` must resolve `file_text` to the
    // same content `Vfs::file_content` holds, and report `None` for an id the
    // VFS has no content for (so the later salsa-backed impl keeps the same
    // trait contract).
    let db = Database::new(CrateConfig {
        name: "test".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 0,
    });
    let file_id = db
        .vfs()
        .add_file_content(
            &std::path::PathBuf::from("main.g"),
            Arc::from("fn main() -> i32 { 0 }"),
        )
        .expect("fixture sources are far below the VFS length gate");
    assert_eq!(&*db.file_text(file_id).unwrap(), "fn main() -> i32 { 0 }");
    // A FileId the VFS never registered content for yields `None`, not a panic.
    let unknown = glyim_span::FileId::from_usize(9_999);
    assert!(db.file_text(unknown).is_none());
}
