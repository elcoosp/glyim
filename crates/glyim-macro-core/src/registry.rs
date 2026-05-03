use glyim_macro_vfs::ContentStore;
use std::collections::HashMap;
use std::sync::Arc;

/// A registry that maps macro names to their Wasm bytecode.
pub struct MacroRegistry {
    macros: HashMap<String, Vec<u8>>,
    store: Arc<dyn ContentStore>,
}

impl MacroRegistry {
    /// Create a new empty registry backed by the given content store.
    pub fn new(store: Arc<dyn ContentStore>) -> Self {
        Self {
            macros: HashMap::new(),
            store,
        }
    }

    /// Register a macro with its Wasm blob.
    pub fn register(&mut self, name: &str, wasm: Vec<u8>) {
        self.macros.insert(name.to_string(), wasm);
    }

    /// Look up a macro's Wasm blob.
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.macros.get(name).map(|v| v.as_slice())
    }

    /// Load a macro from the content store by its name.
    ///
    /// The name is resolved through the store's `resolve_name` and then
    /// the blob is retrieved. Returns `true` if successful.
    pub fn load_from_store(&mut self, name: &str) -> bool {
        if let Some(hash) = self.store.resolve_name(name)
            && let Some(wasm) = self.store.retrieve(hash)
        {
            self.macros.insert(name.to_string(), wasm);
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::InMemoryStore;
    use std::sync::Arc;

    #[test]
    fn register_and_get() {
        let mut reg = MacroRegistry::new(Arc::new(InMemoryStore::new()));
        let wasm = vec![0, 1, 2, 3];
        reg.register("test_macro", wasm.clone());
        assert_eq!(reg.get("test_macro"), Some(wasm.as_slice()));
    }

    #[test]
    fn load_from_store() {
        let store = Arc::new(InMemoryStore::new());
        let mut reg = MacroRegistry::new(store.clone());

        // Store a blob under the name "my_macro"
        let wasm = b"fake_wasm".to_vec();
        let hash = store.store(&wasm);
        store.register_name("my_macro", hash);

        // Load it into the registry
        assert!(reg.load_from_store("my_macro"));
        assert_eq!(reg.get("my_macro"), Some(wasm.as_slice()));
    }
}
