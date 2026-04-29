use crate::error::PkgError;
use glyim_macro_vfs::{ContentHash, ContentStore, LocalContentStore};
use std::path::Path;

/// Client for the Glyim content-addressable storage system.
///
/// Uses a local store for now; will be extended to talk to the CAS server
/// in a future phase.
pub struct CasClient {
    local: LocalContentStore,
}

impl CasClient {
    pub fn new(base_dir: &Path) -> std::io::Result<Self> {
        let local = LocalContentStore::new(base_dir)?;
        Ok(Self { local })
    }

    /// Store content and return its hash.
    pub fn store(&self, content: &[u8]) -> ContentHash {
        self.local.store(content)
    }

    /// Retrieve content by its hash.
    pub fn retrieve(&self, hash: ContentHash) -> Option<Vec<u8>> {
        self.local.retrieve(hash)
    }

    /// Check if content is available locally.
    pub fn contains(&self, hash: ContentHash) -> bool {
        self.retrieve(hash).is_some()
    }

    /// Register a name for a hash.
    pub fn register_name(&self, name: &str, hash: ContentHash) {
        self.local.register_name(name, hash);
    }

    /// Resolve a name to a hash.
    pub fn resolve_name(&self, name: &str) -> Option<ContentHash> {
        self.local.resolve_name(name)
    }

    /// Download content from a remote CAS server (stub).
    pub fn download_from_remote(
        &self,
        _hash: ContentHash,
        _remote_url: &str,
    ) -> Result<(), PkgError> {
        Err(PkgError::Registry("remote CAS download not yet implemented".into()))
    }
}
