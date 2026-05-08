use crate::node::MerkleNode;
use glyim_macro_vfs::{ContentHash, ContentStore};
use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

pub struct MerkleStore {
    /// Backing CAS storage.
    cas: Arc<dyn ContentStore>,
    /// In-memory LRU cache for frequently accessed artifacts.
    cache: Mutex<LruCache<ContentHash, MerkleNode>>,
    /// Write buffer for batching CAS writes.
    write_buffer: Mutex<Vec<(ContentHash, Vec<u8>)>>,
    /// Maximum write buffer size before flushing.
    write_buffer_capacity: usize,
}

impl MerkleStore {
    pub fn new(cas: Arc<dyn ContentStore>) -> Self {
        Self {
            cas,
            cache: Mutex::new(LruCache::new(
                NonZeroUsize::new(256).unwrap()
            )),
            write_buffer: Mutex::new(Vec::new()),
            write_buffer_capacity: 64,
        }
    }

    /// Store a MerkleNode and return its hash. The node is serialized
    /// and written to the CAS via a buffered write.
    pub fn put(&self, node: MerkleNode) -> ContentHash {
        let hash = node.compute_hash();
        let serialized = node.serialize();

        // Buffer the write
        {
            let mut buffer = self.write_buffer.lock().unwrap();
            buffer.push((hash, serialized.clone()));
            if buffer.len() >= self.write_buffer_capacity {
                self.flush_buffer(&mut buffer);
            }
        }

        // Populate the in-memory cache
        self.cache.lock().unwrap().put(hash, node);

        hash
    }

    /// Retrieve a MerkleNode by its hash. Checks the LRU cache first,
    /// then falls back to the CAS.
    pub fn get(&self, hash: &ContentHash) -> Option<MerkleNode> {
        // Check LRU cache
        if let Some(cached) = self.cache.lock().unwrap().get(hash) {
            return Some(cached.clone());
        }

        // Fall back to CAS
        let data = self.cas.retrieve(*hash)?;
        let node = MerkleNode::deserialize(&data).ok()?;

        // Populate cache
        self.cache.lock().unwrap().put(*hash, node.clone());

        Some(node)
    }

    /// Check if a node is present (cache or CAS).
    pub fn contains(&self, hash: &ContentHash) -> bool {
        if self.cache.lock().unwrap().contains(hash) {
            return true;
        }
        self.cas.retrieve(*hash).is_some()
    }

    /// Clear the in-memory cache (does not affect CAS).
    pub fn clear_cache(&self) {
        self.cache.lock().unwrap().clear();
    }

    /// Number of cached entries.
    pub fn cache_size(&self) -> usize {
        self.cache.lock().unwrap().len()
    }

    /// Flush any buffered writes to the CAS. Must be called at the
    /// end of compilation to ensure all artifacts are persisted.
    pub fn flush(&self) {
        let mut buffer = self.write_buffer.lock().unwrap();
        self.flush_buffer(&mut buffer);
    }

    /// Register a human-readable name for a content hash.
    pub fn register_name(&self, name: &str, hash: ContentHash) {
        self.cas.register_name(name, hash);
    }

    /// Resolve a human-readable name to a content hash.
    pub fn resolve_name(&self, name: &str) -> Option<ContentHash> {
        self.cas.resolve_name(name)
    }

    fn flush_buffer(&self, buffer: &mut Vec<(ContentHash, Vec<u8>)>) {
        for (_hash, data) in buffer.drain(..) {
            // Store in CAS; hash is already known but we don't need the
            // return value here because we already computed it.
            let _ = self.cas.store(&data);
        }
    }
}
