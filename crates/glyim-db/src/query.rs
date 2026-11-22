//! M6 query-engine scaffolding (M6.0/M6.2 lead-in): the [`QueryContext`]
//! trait abstracts the core interner + VFS lookups the frontend queries
//! against.
//!
//! # Why
//!
//! The master plan (§6 M6) moves the compiler onto a salsa-style query
//! database so a body-only edit invalidates only the queries that transitively
//! depend on it (`--explain-rebuild` shows the minimal recompute set, M6.4).
//! That transition touches every frontend call site that today reaches
//! directly into [`Database`]'s private fields (`db.vfs().file_content(id)`,
//! `db.interner()`, `db.krate()`). Doing the migration in one shot is a
//! cross-lane break the plan forbids (§7.2 rule 4 wants codemods + an
//! interface PR first); instead this trait is the interface PR.
//!
//! # What
//!
//! [`QueryContext`] collects the read-only lookups a frontend query needs —
//! the name interner, the virtual file system, and the crate id — plus one
//! simple query, [`QueryContext::file_text`], migrated off direct
//! `db.vfs().file_content(id)` struct access onto a trait method. Today the
//! only implementation is [`Database`](crate::Database); when M6.4 lands the
//! salsa-backed `Database` keeps the same trait surface, so callers that
//! already depend on `QueryContext` (rather than the concrete struct) need no
//! further change. The trait is intentionally minimal: signature/body queries
//! (`def_map`, `typeck`, `mir_build`) arrive in later M6 work items once the
//! durability model (M6.1/M6.2) settles.
use glyim_core::def_id::CrateId;
use glyim_core::interner::Interner;
use glyim_span::FileId;
use glyim_vfs::Vfs;
use std::sync::Arc;

/// The query context: the read-only interface every frontend query resolves
/// against. Encapsulates the name interner, the virtual file system, the
/// crate id, and a simple file-text query migrated off direct struct access.
///
/// Callers should depend on `impl QueryContext` (or `&dyn QueryContext`) rather
/// than `&Database` so the later salsa swap is local to [`Database`]'s impl
/// block.
pub trait QueryContext {
    /// The crate's name interner (symbols are stable across edits, M6.1).
    fn interner(&self) -> &Interner;
    /// The virtual file system backing source lookups.
    fn vfs(&self) -> &Vfs;
    /// The crate id under compilation.
    fn krate(&self) -> CrateId;
    /// Query: the source text of a file. Today a straight VFS read; M6.4 will
    /// memoize this on a salsa query keyed by `SourceFile` content hash
    /// (M6.2) so an unchanged file is a cache hit. Returns `None` for a
    /// `FileId` the VFS has no content for, mirroring
    /// [`Vfs::file_content`](glyim_vfs::Vfs::file_content).
    ///
    /// Replaces ad-hoc `db.vfs().file_content(id)` call sites: the query
    /// boundary (not the VFS struct) is now the thing callers name.
    fn file_text(&self, file: FileId) -> Option<Arc<str>>;
}
