use crate::context::QueryContext;
use crate::dependency::Dependency;
use crate::fingerprint::Fingerprint;
use crate::invalidation::InvalidationReport;
use crate::persistence::PersistenceLayer;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// High-level state for incremental compilation.
///
/// Tracks source file hashes and the query context for memoized results.
/// Persisted via postcard binary serialization.
/// Current version of the incremental state format.
/// Must be bumped when the layout of cached data changes.
const CURRENT_VERSION: u32 = 1;

pub struct IncrementalState {
    /// Directory for persistent storage.
    cache_dir: PathBuf,
    /// Hashes of source files from the previous build.
    source_hashes: HashMap<String, Fingerprint>,
    /// The query context (memoized results + dependency graph).
    ctx: QueryContext,
}

impl IncrementalState {
    /// Load an existing incremental state, or create a fresh one.
    pub fn load_or_create(cache_dir: &Path) -> Self {
        let state_dir = cache_dir.join("incremental");
        let source_hashes_path = state_dir.join("source-hashes.bin");
        let version_path = state_dir.join("version.bin");

        // Check version; if mismatch, start fresh.
        let saved_version: Option<u32> = if version_path.exists() {
            std::fs::read(&version_path)
                .ok()
                .and_then(|bytes| postcard::from_bytes(&bytes).ok())
        } else {
            None
        };
        let should_reset = saved_version != Some(CURRENT_VERSION);
        if should_reset {
            tracing::warn!(
                "Incremental state version mismatch (expected {}, got {:?}); clearing cache.",
                CURRENT_VERSION,
                saved_version,
            );
            // Remove old state
            let _ = std::fs::remove_dir_all(&state_dir);
            let _ = std::fs::create_dir_all(&state_dir);
        }

        let source_hashes: HashMap<String, Fingerprint> =
            if !should_reset && source_hashes_path.exists() {
                let data = std::fs::read(&source_hashes_path).unwrap_or_default();
                postcard::from_bytes(&data).unwrap_or_default()
            } else {
                HashMap::new()
            };

        let ctx = if should_reset {
            QueryContext::new()
        } else {
            PersistenceLayer::load(&state_dir).unwrap_or_default()
        };

        Self {
            cache_dir: state_dir,
            source_hashes,
            ctx,
        }
    }

    /// Record the hash of a source file.
    pub fn record_source(&mut self, path: &str, hash: Fingerprint) {
        self.source_hashes.insert(path.to_string(), hash);
    }

    /// Get the hash of a source file from the previous build.
    pub fn source_hash(&self, path: &str) -> Option<Fingerprint> {
        self.source_hashes.get(path).copied()
    }

    /// Get all recorded source hashes.
    pub fn source_hashes(&self) -> &HashMap<String, Fingerprint> {
        &self.source_hashes
    }

    /// Compute which files changed compared to the previous build.
    pub fn compute_changed_files(&self, current: &[(&str, Fingerprint)]) -> Vec<String> {
        current
            .iter()
            .filter(|(path, hash)| self.source_hashes.get(*path) != Some(hash))
            .map(|(path, _)| path.to_string())
            .collect()
    }

    /// Compute which files were deleted.
    pub fn compute_deleted_files(&self, current_paths: &[&str]) -> Vec<String> {
        let current_set: std::collections::HashSet<&str> = current_paths.iter().copied().collect();
        self.source_hashes
            .keys()
            .filter(|path| !current_set.contains(path.as_str()))
            .cloned()
            .collect()
    }

    /// Apply source file changes: update hashes, invalidate affected queries.
    pub fn apply_changes(&mut self, changes: &[(&str, Fingerprint)]) -> InvalidationReport {
        let mut changed_deps = Vec::new();
        for (path, hash) in changes {
            let old_hash = self.source_hashes.get(*path).copied();
            if old_hash != Some(*hash) {
                changed_deps.push(Dependency::file(*path, *hash));
                self.source_hashes.insert(path.to_string(), *hash);
            }
        }

        if changed_deps.is_empty() {
            return InvalidationReport::new(
                std::collections::HashSet::new(),
                self.ctx
                    .dep_graph()
                    .read()
                    .unwrap()
                    .nodes()
                    .into_iter()
                    .collect(),
            );
        }

        // Convert dependencies to fingerprints and invalidate
        let changed_fps: Vec<Fingerprint> = changed_deps.iter().map(|d| d.fingerprint()).collect();
        self.ctx.invalidate_fingerprints(&changed_fps)
    }

    /// Access the query context (immutable).
    pub fn ctx(&self) -> &QueryContext {
        &self.ctx
    }

    /// Access the query context (mutable).
    pub fn ctx_mut(&mut self) -> &mut QueryContext {
        &mut self.ctx
    }

    /// Save the incremental state to disk.
    pub fn save(&self) -> Result<(), String> {
        std::fs::create_dir_all(&self.cache_dir).map_err(|e| format!("create dir: {e}"))?;

        // Save version
        let version_path = self.cache_dir.join("version.bin");
        let version_bytes = postcard::to_allocvec(&CURRENT_VERSION)
            .map_err(|e| format!("serialize version: {e}"))?;
        std::fs::write(&version_path, version_bytes).map_err(|e| format!("write version: {e}"))?;

        // Save source hashes
        let source_hashes_path = self.cache_dir.join("source-hashes.bin");
        let data = postcard::to_allocvec(&self.source_hashes)
            .map_err(|e| format!("serialize source hashes: {e}"))?;
        std::fs::write(&source_hashes_path, data)
            .map_err(|e| format!("write source hashes: {e}"))?;

        // Save query context
        PersistenceLayer::save(&self.ctx, &self.cache_dir)?;

        Ok(())
    }
}
