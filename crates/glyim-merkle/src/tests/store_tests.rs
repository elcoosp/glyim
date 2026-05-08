use crate::store::MerkleStore;
use crate::node::{MerkleNode, MerkleNodeData, MerkleNodeHeader};
use glyim_macro_vfs::ContentHash;
use std::sync::Arc;

struct InMemoryStore {
    blobs: std::sync::Mutex<std::collections::HashMap<ContentHash, Vec<u8>>>,
}
impl InMemoryStore {
    fn new() -> Self { Self { blobs: std::sync::Mutex::new(std::collections::HashMap::new()) } }
}
impl glyim_macro_vfs::ContentStore for InMemoryStore {
    fn store(&self, content: &[u8]) -> ContentHash {
        let hash = ContentHash::of(content);
        self.blobs.lock().unwrap().insert(hash, content.to_vec());
        hash
    }
    fn retrieve(&self, hash: ContentHash) -> Option<Vec<u8>> {
        self.blobs.lock().unwrap().get(&hash).cloned()
    }
    fn register_name(&self, _: &str, _: ContentHash) {}
    fn resolve_name(&self, _: &str) -> Option<ContentHash> { None }
    fn store_action_result(&self, _: ContentHash, _: glyim_macro_vfs::ActionResult) -> Result<(), glyim_macro_vfs::StoreError> { Ok(()) }
    fn retrieve_action_result(&self, _: ContentHash) -> Option<glyim_macro_vfs::ActionResult> { None }
    fn has_blobs(&self, _: &[ContentHash]) -> Vec<ContentHash> { vec![] }
}

#[test]
fn store_and_retrieve() {
    let cas = Arc::new(InMemoryStore::new());
    let store = MerkleStore::new(cas);
    let data = MerkleNodeData::HirFn { name: "test".into(), serialized: vec![7] };
    let header = MerkleNodeHeader { data_type_tag: data.data_type_tag(), child_count: 0 };
    let node = MerkleNode { hash: ContentHash::ZERO, children: vec![], data, header };
    let hash = store.put(node);
    let retrieved = store.get(&hash).unwrap();
    assert_eq!(retrieved.children.len(), 0);
}

#[test]
fn missing_node_returns_none() {
    let cas = Arc::new(InMemoryStore::new());
    let store = MerkleStore::new(cas);
    assert!(store.get(&ContentHash::of(b"nope")).is_none());
}
