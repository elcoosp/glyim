use glyim_merkle::{MerkleNode, MerkleNodeData, MerkleNodeHeader, MerkleStore};
use glyim_macro_vfs::{ContentHash, LocalContentStore};
use std::path::PathBuf;
use std::sync::Arc;

pub struct IncrementalTestCache {
    store: MerkleStore,
    #[allow(dead_code)]
    dir: PathBuf,
}

impl IncrementalTestCache {
    pub fn new(cache_dir: &std::path::Path) -> Option<Self> {
        let store = LocalContentStore::new(cache_dir).ok()?;
        Some(Self {
            store: MerkleStore::new(Arc::new(store)),
            dir: cache_dir.to_path_buf(),
        })
    }

    pub fn load_result(&self, test_name: &str, source_hash: &ContentHash) -> Option<crate::types::TestResult> {
        let key = format!("test-{}", test_name);
        let hash = self.store.resolve_name(&key)?;
        let node = self.store.get(&hash)?;
        if let MerkleNodeData::ObjectCode { symbol_name, bytes } = &node.data {
            if symbol_name == &source_hash.to_hex() {
                if let Ok(result) = postcard::from_bytes::<crate::types::TestResult>(bytes) {
                    return Some(result);
                }
            }
        }
        None
    }

    pub fn store_result(&self, result: &crate::types::TestResult, source_hash: &ContentHash) {
        let bytes = postcard::to_allocvec(result).unwrap_or_default();
        let node = MerkleNode {
            hash: ContentHash::ZERO,
            children: vec![],
            data: MerkleNodeData::ObjectCode {
                symbol_name: source_hash.to_hex(),
                bytes,
            },
            header: MerkleNodeHeader {
                data_type_tag: 0x04,
                child_count: 0,
            },
        };
        let hash = self.store.put(node);
        self.store.register_name(&format!("test-{}", result.name), hash);
        self.store.flush();
    }
}
