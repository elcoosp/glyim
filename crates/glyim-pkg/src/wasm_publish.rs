//! Publish‑time Wasm compilation for Glyim macros.
use crate::PkgError;
use glyim_macro_vfs::{ContentHash, ContentStore};
use std::path::Path;

/// Compile the macro at the given source file to a Wasm blob,
/// store it in the CAS, and return its content hash.
pub fn compile_and_store_macro_wasm(
    source_path: &Path,
    target_triple: &str,
    store: &dyn ContentStore,
) -> Result<ContentHash, PkgError> {
    let source = std::fs::read_to_string(source_path)
        .map_err(PkgError::Io)?;
    let wasm_bytes = glyim_codegen_llvm::compile_to_wasm(&source, target_triple)
        .map_err(|e| PkgError::Registry(format!("Wasm compilation failed: {e}")))?;
    let hash = store.store(&wasm_bytes);
    Ok(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_macro_vfs::LocalContentStore;

    #[test]
    fn compile_and_store_trivial_macro() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("test.g");
        std::fs::write(&src, "fn main() -> i64 { 42 }").unwrap();
        let store = LocalContentStore::new(dir.path()).unwrap();
        let hash = compile_and_store_macro_wasm(&src, "wasm32-wasi", &store)
            .expect("compile");
        assert!(store.retrieve(hash).is_some(), "Wasm blob must be stored");
    }
}
