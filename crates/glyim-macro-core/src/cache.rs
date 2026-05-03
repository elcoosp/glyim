use sha2::{Sha256, Digest};
use std::sync::Arc;
use glyim_macro_vfs::{ContentHash, ContentStore, StoreError};

/// Compute a deterministic cache key for macro expansion.
pub fn compute_cache_key(
    compiler_version: &str,
    target_triple: &str,
    macro_wasm_hash: &ContentHash,
    input_ast_hash: &ContentHash,
    impure_file_hashes: &[ContentHash],
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(compiler_version.as_bytes());
    hasher.update(target_triple.as_bytes());
    hasher.update(macro_wasm_hash.as_bytes());
    hasher.update(input_ast_hash.as_bytes());
    for fh in impure_file_hashes {
        hasher.update(fh.as_bytes());
    }
    let digest = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&digest);
    key
}

/// Caching layer that stores macro output bytes in a CAS.
///
/// Output bytes are stored under a named key derived from the cache key.
pub struct MacroExpansionCache {
    pub store: Arc<dyn ContentStore>,
}

impl MacroExpansionCache {
    pub fn new(store: Arc<dyn ContentStore>) -> Self {
        Self { store }
    }

    pub fn lookup(&self, cache_key: &[u8; 32]) -> Option<Vec<u8>> {
        let name = format!("macro-{}", hex::encode(cache_key));
        let output_hash = self.store.resolve_name(&name)?;
        self.store.retrieve(output_hash)
    }

    pub fn store(&self, cache_key: &[u8; 32], output: &[u8]) -> Result<(), StoreError> {
        let output_hash = self.store.store(output);
        let name = format!("macro-{}", hex::encode(cache_key));
        self.store.register_name(&name, output_hash);
        Ok(())
    }
}

// ─── In-memory ContentStore for tests ──────────────────────────
use std::collections::HashMap;
use std::sync::Mutex;
use glyim_macro_vfs::ActionResult;

pub struct InMemoryStore {
    blobs: Mutex<HashMap<ContentHash, Vec<u8>>>,
    names: Mutex<HashMap<String, ContentHash>>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self {
            blobs: Mutex::new(HashMap::new()),
            names: Mutex::new(HashMap::new()),
        }
    }
}

impl ContentStore for InMemoryStore {
    fn store(&self, content: &[u8]) -> ContentHash {
        let hash = ContentHash::of(content);
        self.blobs.lock().unwrap().insert(hash, content.to_vec());
        hash
    }
    fn retrieve(&self, hash: ContentHash) -> Option<Vec<u8>> {
        self.blobs.lock().unwrap().get(&hash).cloned()
    }
    fn register_name(&self, name: &str, hash: ContentHash) {
        self.names.lock().unwrap().insert(name.to_string(), hash);
    }
    fn resolve_name(&self, name: &str) -> Option<ContentHash> {
        self.names.lock().unwrap().get(name).copied()
    }
    fn store_action_result(&self, _h: ContentHash, _r: ActionResult) -> Result<(), StoreError> { Ok(()) }
    fn retrieve_action_result(&self, _h: ContentHash) -> Option<ActionResult> { None }
    fn has_blobs(&self, _hashes: &[ContentHash]) -> Vec<ContentHash> { vec![] }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn deterministic_cache_key() {
        let cver = "0.5.0";
        let target = "x86_64-unknown-linux-gnu";
        let wasm_hash = ContentHash::of_str("abc");
        let ast_hash = ContentHash::of_str("def");
        let key1 = compute_cache_key(cver, target, &wasm_hash, &ast_hash, &[]);
        let key2 = compute_cache_key(cver, target, &wasm_hash, &ast_hash, &[]);
        assert_eq!(key1, key2);
    }

    #[test]
    fn store_and_lookup() {
        let cache = MacroExpansionCache::new(Arc::new(InMemoryStore::new()));
        let key = [1u8; 32];
        let output = b"hello";
        cache.store(&key, output).expect("store");
        let retrieved = cache.lookup(&key).expect("lookup");
        assert_eq!(retrieved, output);
    }
}
