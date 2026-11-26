//! Crate root.
pub mod body_queries;
pub mod cache;
pub mod cache_metrics;
pub mod incr_verify;
pub mod incremental;
pub mod query;
pub mod rebuild_trace;

pub use body_queries::{ErasedQueryEntry, QueryCache, QueryKind};
pub use incr_verify::{
    GLYIM_INCR_VERIFY, IncrVerifyEvent, IncrVerifyOutcome, IncrVerifyTrace, incr_verify_enabled,
    incr_verify_enabled_for,
};
pub use incremental::{ConfigFingerprint, Durability, SourceFile};
pub use query::QueryContext;
pub use rebuild_trace::{QueryRebuildTrace, RebuildEvent, RebuildOutcome};

use glyim_core::def_id::CrateId;
use glyim_core::interner::Interner;
use glyim_span::FileId;
use glyim_vfs::Vfs;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

/// Handle to share the `TyCtx` between the Database and `CodegenBackend`.
pub type TyCtxHandle = Arc<std::sync::RwLock<Option<Arc<glyim_type::TyCtx>>>>;

/// `Database`.
pub struct Database {
    interner: Interner,
    vfs: Vfs,
    ty_ctx: TyCtxHandle,
    krate: CrateId,
    /// Cache of previously computed mono item symbols.
    mono_cache: RwLock<Option<Vec<String>>>,
    config: CrateConfig,
    /// §3.5.30: per-lint levels.
    lints: glyim_core::lint::LintMap,
    /// M6.4 type-erased body-query cache (`thir_body` / `mir_build` /
    /// `def_map`). See [`body_queries`].
    query_cache: QueryCache,
    /// M6.9 query-rebuild trace: one `RebuildEvent` per body/signature query
    /// call, read by `--explain-rebuild`. See [`rebuild_trace`].
    rebuild_trace: QueryRebuildTrace,
    /// M6.8 incremental ≡ clean oracle trace: one `IncrVerifyEvent` per
    /// cache-hit query call when `GLYIM_INCR_VERIFY=1`, read by the nightly
    /// 10k-sequence harness. See [`incr_verify`].
    incr_verify_trace: IncrVerifyTrace,
    /// M6.8 verify-seam enable flag. Read from `GLYIM_INCR_VERIFY` once at
    /// `Database::new` (so the hot path is a single `AtomicBool::load`, not a
    /// `getenv` per query) and overridable per-`Database` via
    /// [`Database::set_incr_verify_for_test`] so the behavior tests do not
    /// race on process-global env mutation (edition 2024 made `set_var`
    /// unsafe; the parallel test runner also makes env mutation flaky).
    incr_verify_enabled: AtomicBool,
}

#[derive(Clone, Debug)]
/// `CrateConfig`.
pub struct CrateConfig {
    /// `Struct`.
    pub name: String,
    /// `Struct`.
    pub target_triple: String,
    /// `Struct`.
    pub opt_level: u8,
}

impl Database {
    /// `intern_mut`.
    pub fn intern_mut(&mut self) -> &mut Interner {
        &mut self.interner
    }

    /// new.
    #[must_use]
    pub fn new(config: CrateConfig) -> Self {
        Self {
            interner: Interner::new(),
            vfs: Vfs::new(),
            ty_ctx: Arc::new(std::sync::RwLock::new(None)),
            krate: CrateId::from_usize(0),
            mono_cache: RwLock::new(None),
            config,
            lints: glyim_core::lint::LintMap::new(),
            query_cache: parking_lot::RwLock::new(HashMap::new()),
            rebuild_trace: parking_lot::RwLock::new(Vec::new()),
            incr_verify_trace: parking_lot::RwLock::new(Vec::new()),
            incr_verify_enabled: AtomicBool::new(incr_verify::incr_verify_enabled()),
        }
    }

    /// interner.
    pub fn interner(&self) -> &Interner {
        &self.interner
    }

    /// vfs.
    pub fn vfs(&self) -> &Vfs {
        &self.vfs
    }

    /// krate.
    pub fn krate(&self) -> CrateId {
        self.krate
    }

    /// `set_ty_ctx`.
    ///
    /// A poisoned lock (a panic while another thread held the write guard)
    /// recovers through `PoisonError::into_inner` instead of panicking the
    /// caller: the `Option<TyCtx>` inside is a plain assignment, not an
    /// invariant a panic could have left half-updated.
    pub fn set_ty_ctx(&self, ctx: glyim_type::TyCtx) {
        let mut guard = self
            .ty_ctx
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = Some(Arc::new(ctx));
    }

    /// `get_ty_ctx`.
    ///
    /// Poison recovery mirrors [`Database::set_ty_ctx`]: a panicking reader
    /// cannot have damaged the shared `Option`, so the guard's payload is
    /// still the source of truth.
    pub fn get_ty_ctx(&self) -> Option<Arc<glyim_type::TyCtx>> {
        self.ty_ctx
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// `ty_ctx_handle`.
    pub fn ty_ctx_handle(&self) -> TyCtxHandle {
        self.ty_ctx.clone()
    }

    /// `set_mono_cache`.
    pub fn set_mono_cache(&self, items: Vec<String>) {
        // Plan §3.1: the mono cache must not silently accumulate duplicate items
        // when the same monomorphization is requested more than once. De-duplicate
        // while preserving first-seen order so lookups via `mono_cache()` remain
        // stable and O(n) insertion cost stays bounded.
        let mut seen = std::collections::HashSet::new();
        let deduped: Vec<String> = items
            .into_iter()
            .filter(|item| seen.insert(item.clone()))
            .collect();
        *self.mono_cache.write() = Some(deduped);
    }

    /// `mono_cache`.
    pub fn mono_cache(&self) -> parking_lot::RwLockReadGuard<'_, Option<Vec<String>>> {
        self.mono_cache.read()
    }

    /// config.
    pub fn config(&self) -> &CrateConfig {
        &self.config
    }

    /// Per-lint levels (§3.5.30).
    pub fn lints(&self) -> &glyim_core::lint::LintMap {
        &self.lints
    }

    /// Replace the per-lint map (§3.5.30).
    pub fn set_lints(&mut self, lints: glyim_core::lint::LintMap) {
        self.lints = lints;
    }
}

impl Default for Database {
    fn default() -> Self {
        Self::new(CrateConfig {
            name: "main".to_string(),
            target_triple: "x86_64-unknown-linux-gnu".to_string(),
            opt_level: 0,
        })
    }
}

impl query::QueryContext for Database {
    fn interner(&self) -> &Interner {
        Database::interner(self)
    }

    fn vfs(&self) -> &Vfs {
        Database::vfs(self)
    }

    fn krate(&self) -> CrateId {
        Database::krate(self)
    }

    fn file_text(&self, file: FileId) -> Option<Arc<str>> {
        self.vfs.file_content(file)
    }

    fn item_tree_fingerprint(&self, file: FileId) -> glyim_core::Fingerprint {
        // Compute from the file's body-agnostic signature summary (M6.3). A
        // body-only edit leaves this unchanged; `item_tree_fingerprint_of`
        // cuts each signature at the first `{` so the body never enters the
        // hash. Reading through `file_text` (the trait query) keeps this impl
        // independent of the VFS struct.
        match self.file_text(file) {
            Some(content) => incremental::item_tree_fingerprint_of(&content),
            None => glyim_core::Fingerprint::from_bytes([0; 16]),
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod database_tests {
    use super::*;

    #[test]
    fn test_database_stores_config() {
        let config = CrateConfig {
            name: "test_crate".to_string(),
            target_triple: "aarch64-unknown-linux-gnu".to_string(),
            opt_level: 2,
        };
        let db = Database::new(config);
        assert_eq!(db.config().name, "test_crate");
        assert_eq!(db.config().target_triple, "aarch64-unknown-linux-gnu");
        assert_eq!(db.config().opt_level, 2);
    }

    #[test]
    fn test_mono_cache_dedups_preserving_order() {
        // Plan §3.1: set_mono_cache must not accumulate duplicates when the same
        // monomorphization is supplied more than once; first-seen order is kept.
        let db = Database::default();
        db.set_mono_cache(vec![
            "foo".to_string(),
            "bar".to_string(),
            "foo".to_string(),
            "baz".to_string(),
            "bar".to_string(),
        ]);
        let cache = db.mono_cache();
        let items = cache.as_ref().expect("cache should be populated");
        assert_eq!(
            items,
            &vec!["foo".to_string(), "bar".to_string(), "baz".to_string(),]
        );
    }
}
