//! Crate root.
pub mod cache;

use glyim_core::def_id::CrateId;
use glyim_core::interner::Interner;
use glyim_vfs::Vfs;
use parking_lot::RwLock;
use std::sync::Arc;

/// Handle to share the TyCtx between the Database and CodegenBackend.
pub type TyCtxHandle = Arc<std::sync::RwLock<Option<Arc<glyim_type::TyCtx>>>>;

/// Database.
pub struct Database {
    interner: Interner,
    vfs: Vfs,
    ty_ctx: TyCtxHandle,
    krate: CrateId,
    /// Cache of previously computed mono item symbols.
    mono_cache: RwLock<Option<Vec<String>>>,
    config: CrateConfig,
}

#[derive(Clone, Debug)]
/// CrateConfig.
pub struct CrateConfig {
    /// Struct.
    pub name: String,
    /// Struct.
    pub target_triple: String,
    /// Struct.
    pub opt_level: u8,
}

impl Database {
    /// intern_mut.
    pub fn intern_mut(&mut self) -> &mut Interner {
        &mut self.interner
    }

    /// new.
    pub fn new(config: CrateConfig) -> Self {
        Self {
            interner: Interner::new(),
            vfs: Vfs::new(),
            ty_ctx: Arc::new(std::sync::RwLock::new(None)),
            krate: CrateId::from_raw(0),
            mono_cache: RwLock::new(None),
            config,
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

    /// set_ty_ctx.
    pub fn set_ty_ctx(&self, ctx: glyim_type::TyCtx) {
        *self.ty_ctx.write().unwrap() = Some(Arc::new(ctx));
    }

    /// get_ty_ctx.
    pub fn get_ty_ctx(&self) -> Option<Arc<glyim_type::TyCtx>> {
        self.ty_ctx.read().unwrap().clone()
    }

    /// ty_ctx_handle.
    pub fn ty_ctx_handle(&self) -> TyCtxHandle {
        self.ty_ctx.clone()
    }

    /// set_mono_cache.
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

    /// mono_cache.
    pub fn mono_cache(&self) -> parking_lot::RwLockReadGuard<'_, Option<Vec<String>>> {
        self.mono_cache.read()
    }

    /// config.
    pub fn config(&self) -> &CrateConfig {
        &self.config
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

#[cfg(test)]
mod tests {
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
