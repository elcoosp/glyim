use sha2::{Sha256, Digest};
use glyim_macro_vfs::ContentHash;

/// Compute a deterministic cache key for macro expansion.
///
/// Returns raw SHA-256 bytes; the caller can wrap this in a [`ContentHash`]
/// by hashing it (e.g. `ContentHash::of(&key)`) or using a future
/// `ContentHash::from_bytes` constructor.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic() {
        let cver = "0.5.0";
        let target = "x86_64-unknown-linux-gnu";
        let wasm_hash = ContentHash::of_str("abc");
        let ast_hash = ContentHash::of_str("def");
        let key1 = compute_cache_key(cver, target, &wasm_hash, &ast_hash, &[]);
        let key2 = compute_cache_key(cver, target, &wasm_hash, &ast_hash, &[]);
        assert_eq!(key1, key2);
    }

    #[test]
    fn differs_on_input() {
        let cver = "0.5.0";
        let target = "x86_64-unknown-linux-gnu";
        let wasm_hash = ContentHash::of_str("abc");
        let ast1 = ContentHash::of_str("42");
        let ast2 = ContentHash::of_str("43");
        let key1 = compute_cache_key(cver, target, &wasm_hash, &ast1, &[]);
        let key2 = compute_cache_key(cver, target, &wasm_hash, &ast2, &[]);
        assert_ne!(key1, key2);
    }

    #[test]
    fn differs_on_impure_file() {
        let cver = "0.5.0";
        let target = "x86_64-unknown-linux-gnu";
        let wasm_hash = ContentHash::of_str("abc");
        let ast = ContentHash::of_str("42");
        let file1 = ContentHash::of_str("file1");
        let file2 = ContentHash::of_str("file2");
        let key1 = compute_cache_key(cver, target, &wasm_hash, &ast, &[file1]);
        let key2 = compute_cache_key(cver, target, &wasm_hash, &ast, &[file2]);
        assert_ne!(key1, key2);
    }
}
