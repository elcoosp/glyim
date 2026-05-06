use crate::node::MerkleNode;
use dashmap::DashMap;
use glyim_macro_vfs::{ContentHash, ContentStore};
use std::sync::Arc;

pub struct MerkleStore {
    cas: Arc<dyn ContentStore>,
    cache: DashMap<ContentHash, MerkleNode>,
}

impl MerkleStore {
    pub fn new(cas: Arc<dyn ContentStore>) -> Self {
        Self { cas, cache: DashMap::new() }
    }

    pub fn put(&self, node: MerkleNode) -> ContentHash {
        let hash = node.compute_hash();
        let serialized = node.serialize();
        self.cas.store(&serialized);
        self.cache.insert(hash, node);
        hash
    }

    pub fn get(&self, hash: &ContentHash) -> Option<MerkleNode> {
        if let Some(cached) = self.cache.get(hash) {
            return Some(cached.clone());
        }
        let data = self.cas.retrieve(*hash)?;
        let node = MerkleNode::deserialize(&data).ok()?;
        self.cache.insert(*hash, node.clone());
        Some(node)
    }

    pub fn contains(&self, hash: &ContentHash) -> bool {
        self.cache.contains_key(hash) || self.cas.retrieve(*hash).is_some()
    }

    pub fn clear_cache(&self) { self.cache.clear(); }
    pub fn cache_size(&self) -> usize { self.cache.len() }
}
