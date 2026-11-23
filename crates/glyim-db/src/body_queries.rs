//! M6.4 body-query cache storage (type-erased).
//!
//! The plan (§6 M6.4) lands `thir_body`, `mir_build`, and `def_map` as queries
//! on the `Database` with early cutoff (backdating): a re-run that produces a
//! structurally identical result returns the cached `Arc` and does NOT
//! invalidate downstream queries. The typed query methods live in
//! `glyim-pipeline` (which already depends on `glyim-typeck`/`glyim-mir`/
//! `glyim-def-map`); this module holds the **storage** on `Database` so the
//! cache survives across query re-runs without pulling those crates into
//! `glyim-db`'s dep set (the task rails forbid new dep edges).
//!
//! # Why type-erased
//!
//! `glyim-db` cannot name `thir::Body` / `glyim_mir::Body` /
//! `CrateDefMap` without adding them as dependencies. Instead, the cache
//! stores `Arc<dyn Any + Send + Sync>` keyed by `(QueryKind, u64)`. The typed
//! accessors in `glyim-pipeline` downcast via `Arc::downcast::<T>()` at the
//! call site. The `Fingerprint` is stored alongside the value so the
//! backdating equality check (recomputed hash == cached hash) is a plain
//! `Fingerprint` comparison — no downcast needed for the hot path.
//!
//! # Backdating contract
//!
//! A query re-run computes its content `Fingerprint` (via `StableHasher` for
//! body queries, `item_tree_fingerprint` for `def_map`). If the recomputed
//! hash matches the cached entry's hash, the cached `Arc<T>` is returned
//! (backdated) and downstream caches are NOT invalidated. If the hash
//! differs, the new value is cached and the downstream cache for that id is
//! invalidated (the typed accessor handles the downstream invalidation, since
//! the dependency graph — `thir_body` → `mir_build` — is known only to the
//! typed layer).

use glyim_core::Fingerprint;
use parking_lot::RwLock;
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

/// The kind of M6.4 body/signature query a cache entry holds. The cache is
/// keyed by `(QueryKind, u64)` so each query family has its own id space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum QueryKind {
    /// A THIR body query (`thir_body`). Downstream of `def_map`; upstream of
    /// `mir_build`.
    ThirBody,
    /// A MIR body query (`mir_build`). Downstream of `thir_body`; the unified
    /// MIR validator (M5.5) runs before a `mir_build` result is cached.
    MirBuild,
    /// A crate def-map query (`def_map`). Keyed on the body-agnostic
    /// `item_tree_fingerprint`, so a body-only edit is a cache hit.
    DefMap,
}

/// A type-erased cached query value paired with its content `Fingerprint`
/// (M6.4). The fingerprint is the backdating equality check; the value is
/// downcast at the typed call site in `glyim-pipeline`.
pub struct ErasedQueryEntry {
    /// The `StableHasher` / `item_tree_fingerprint` content hash. Two entries
    /// with the same hash are structurally identical — the backdating
    /// short-circuit.
    pub hash: Fingerprint,
    /// The type-erased cached value (`Arc<thir::Body>`, `Arc<Body>`, or
    /// `Arc<CrateDefMap>`; the concrete type is recovered via
    /// `Arc::downcast::<T>()` at the typed call site).
    pub value: Arc<dyn Any + Send + Sync>,
}

/// The M6.4 query cache: `(QueryKind, u64)` → `ErasedQueryEntry`. Stored on
/// `Database` as a `RwLock` so concurrent readers don't block (the salsa-backed
/// `Database` that replaces this later keeps the same storage shape).
pub type QueryCache = RwLock<HashMap<(QueryKind, u64), ErasedQueryEntry>>;

/// `Database` M6.4 query-cache storage accessors.
///
/// These are the type-erased primitives the typed `BodyQueries` trait in
/// `glyim-pipeline` builds on. Callers should prefer the typed accessors
/// (`thir_body_query`, `mir_build_query`, `def_map_query`) over these
/// primitives — the typed layer handles the backdating equality check and
/// downstream invalidation.
impl crate::Database {
    /// Read a cached entry's fingerprint (for the backdating equality check)
    /// without downcasting the value. Returns `None` on a cache miss.
    #[must_use]
    pub fn query_cache_hash(&self, kind: QueryKind, id: u64) -> Option<Fingerprint> {
        self.query_cache.read().get(&(kind, id)).map(|e| e.hash)
    }

    /// Read a cached entry's value (the typed accessor downcasts it via
    /// `Arc::downcast::<T>()`). Returns `None` on a cache miss.
    #[must_use]
    pub fn query_cache_value(
        &self,
        kind: QueryKind,
        id: u64,
    ) -> Option<Arc<dyn Any + Send + Sync>> {
        self.query_cache
            .read()
            .get(&(kind, id))
            .map(|e| e.value.clone())
    }

    /// Insert (or replace) a cached entry. The caller computes the
    /// `Fingerprint` (via `StableHasher` for body queries, or
    /// `item_tree_fingerprint` for the `def_map` query) and the type-erased
    /// `Arc<T>` value.
    pub fn query_cache_insert(
        &self,
        kind: QueryKind,
        id: u64,
        hash: Fingerprint,
        value: Arc<dyn Any + Send + Sync>,
    ) {
        self.query_cache
            .write()
            .insert((kind, id), ErasedQueryEntry { hash, value });
    }

    /// Invalidate a cached entry. Called when an upstream query's content
    /// changed (NOT on a backdated re-run — a backdated re-run is a cache hit
    /// and skips this). The typed `thir_body_query` accessor calls this on the
    /// `MirBuild` cache for a def whose THIR body changed.
    pub fn query_cache_invalidate(&self, kind: QueryKind, id: u64) {
        self.query_cache.write().remove(&(kind, id));
    }

    /// The number of cached entries (test/diagnostic helper; `--explain-rebuild`
    /// will surface this).
    #[must_use]
    pub fn query_cache_len(&self) -> usize {
        self.query_cache.read().len()
    }
}

/// A sentinel fingerprint for cache entries whose backdating is keyed on a
/// downstream query's hash rather than their own (e.g. `mir_build` is
/// invalidated by `thir_body`'s hash change, not its own). The zero fingerprint
/// is never produced by `StableHasher` (SHA-256 of any input is non-zero in the
/// first 128 bits for realistic inputs), so it cannot collide with a real
/// content hash.
#[must_use]
pub fn sentinel_fingerprint() -> Fingerprint {
    Fingerprint::from_bytes([0; 16])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CrateConfig, Database};

    /// The type-erased storage round-trips an `Arc<String>`: insert, read the
    /// hash, read the value, downcast.
    #[test]
    fn erased_storage_round_trips_arc_string() {
        let db = Database::new(CrateConfig {
            name: "t".to_string(),
            target_triple: "x86_64-unknown-linux-gnu".to_string(),
            opt_level: 0,
        });
        let hash = Fingerprint::from_bytes([1; 16]);
        let value: Arc<dyn Any + Send + Sync> = Arc::new(String::from("body"));
        db.query_cache_insert(QueryKind::ThirBody, 7, hash, value);
        assert_eq!(db.query_cache_hash(QueryKind::ThirBody, 7), Some(hash));
        let cached = db
            .query_cache_value(QueryKind::ThirBody, 7)
            .expect("present");
        let arc = cached.downcast::<String>().expect("downcast to String");
        assert_eq!(&*arc, "body");
    }

    /// `query_cache_invalidate` removes the entry; a subsequent read misses.
    #[test]
    fn invalidate_removes_the_entry() {
        let db = Database::new(CrateConfig {
            name: "t".to_string(),
            target_triple: "x86_64-unknown-linux-gnu".to_string(),
            opt_level: 0,
        });
        db.query_cache_insert(
            QueryKind::MirBuild,
            1,
            sentinel_fingerprint(),
            Arc::new(42_u32),
        );
        assert_eq!(db.query_cache_len(), 1);
        db.query_cache_invalidate(QueryKind::MirBuild, 1);
        assert_eq!(db.query_cache_len(), 0);
        assert!(db.query_cache_hash(QueryKind::MirBuild, 1).is_none());
    }

    /// The sentinel fingerprint is all-zero and distinct from a real hash.
    #[test]
    fn sentinel_is_all_zero() {
        let s = sentinel_fingerprint();
        assert_eq!(*s.as_bytes(), [0; 16]);
    }
}
