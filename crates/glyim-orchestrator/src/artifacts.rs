use glyim_macro_vfs::{ContentHash, ContentStore};
use glyim_merkle::MerkleStore;
use std::path::PathBuf;
use std::sync::Arc;
use serde::{Serialize, Deserialize};

/// Manages compilation artifacts in the CAS.
pub struct ArtifactManager {
    cas: Arc<dyn ContentStore>,
    #[allow(dead_code)]
    merkle: Arc<MerkleStore>,
}

/// A package's complete compilation output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageArtifact {
    pub package_name: String,
    pub version: String,
    pub merkle_root: ContentHash,
    pub symbol_table_hash: ContentHash,
    pub object_code_hash: ContentHash,
    pub per_fn_objects: Vec<(String, ContentHash)>,
    pub metadata_hash: ContentHash,
    pub target_triple: Option<String>,
    pub opt_level: String,
    pub compiler_version: String,
}

impl ArtifactManager {
    pub fn new(cas: Arc<dyn ContentStore>, merkle: Arc<MerkleStore>) -> Self {
        Self { cas, merkle }
    }

    /// Store a package's compilation output in the CAS.
    /// Returns the content hash of the serialized PackageArtifact.
    pub fn store_package_artifact(&self, artifact: &PackageArtifact) -> ContentHash {
        let serialized = postcard::to_allocvec(artifact).unwrap_or_default();
        self.cas.store(&serialized)
    }

    /// Retrieve a package's compilation output from the CAS.
    pub fn retrieve_package_artifact(&self, hash: ContentHash) -> Option<PackageArtifact> {
        let data = self.cas.retrieve(hash)?;
        postcard::from_bytes(&data).ok()
    }

    /// Check if a package artifact exists in the CAS.
    pub fn has_package_artifact(&self, hash: ContentHash) -> bool {
        self.cas.retrieve(hash).is_some()
    }

    /// Store object code in the CAS and return its hash.
    pub fn store_object_code(&self, bytes: &[u8]) -> ContentHash {
        self.cas.store(bytes)
    }

    /// Retrieve object code from the CAS.
    pub fn retrieve_object_code(&self, hash: ContentHash) -> Option<Vec<u8>> {
        self.cas.retrieve(hash)
    }

    /// Store a symbol table in the CAS.
    pub fn store_symbol_table(&self, table: &super::symbols::PackageSymbolTable) -> ContentHash {
        let serialized = postcard::to_allocvec(table).unwrap_or_default();
        self.cas.store(&serialized)
    }

    /// Retrieve a symbol table from the CAS.
    pub fn retrieve_symbol_table(&self, hash: ContentHash) -> Option<super::symbols::PackageSymbolTable> {
        let data = self.cas.retrieve(hash)?;
        postcard::from_bytes(&data).ok()
    }

    /// Extract the object code for a package from the CAS and write to a temp file.
    /// Returns the path to the temporary .o file.
    pub fn extract_object_code(&self, artifact: &PackageArtifact) -> Result<PathBuf, String> {
        let obj_data = self
            .retrieve_object_code(artifact.object_code_hash)
            .ok_or_else(|| format!("object code not found for {}", artifact.package_name))?;
        let tmp_dir = tempfile::tempdir().map_err(|e| e.to_string())?;
        let obj_path = tmp_dir.path().join(format!("{}.o", artifact.package_name));
        std::fs::write(&obj_path, &obj_data).map_err(|e| e.to_string())?;
        // Leak the tempdir to keep the file alive until linking
        let _ = tmp_dir.into_path();
        Ok(obj_path)
    }

    /// Verify that an artifact is compatible with the current build configuration.
    pub fn verify_artifact(
        artifact: &PackageArtifact,
        target_triple: Option<&str>,
        compiler_version: &str,
    ) -> Result<(), String> {
        if artifact.target_triple.as_deref() != target_triple {
            return Err(format!(
                "target triple mismatch: expected {:?}, got {:?}",
                target_triple, artifact.target_triple
            ));
        }
        if artifact.compiler_version != compiler_version {
            return Err(format!(
                "compiler version mismatch: expected {}, got {}",
                compiler_version, artifact.compiler_version
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_macro_vfs::LocalContentStore;
    use crate::symbols::PackageSymbolTable;
    use glyim_interner::Interner;

    fn create_test_artifact_manager() -> (ArtifactManager, Arc<dyn ContentStore>) {
        let dir = tempfile::tempdir().unwrap();
        let local_store = LocalContentStore::new(dir.path()).unwrap();
        let store: Arc<dyn ContentStore> = Arc::new(local_store);
        let merkle_store = Arc::new(MerkleStore::new(store.clone()));
        let manager = ArtifactManager::new(store.clone(), merkle_store);
        (manager, store)
    }

    #[test]
    fn store_and_retrieve_package_artifact() {
        let (manager, _store) = create_test_artifact_manager();
        let artifact = PackageArtifact {
            package_name: "test-pkg".into(),
            version: "0.1.0".into(),
            merkle_root: ContentHash::of(b"root"),
            symbol_table_hash: ContentHash::of(b"symbols"),
            object_code_hash: ContentHash::of(b"obj"),
            per_fn_objects: vec![],
            metadata_hash: ContentHash::of(b"meta"),
            target_triple: None,
            opt_level: "debug".into(),
            compiler_version: "0.5.0".into(),
        };

        let hash = manager.store_package_artifact(&artifact);
        let retrieved = manager.retrieve_package_artifact(hash).unwrap();
        assert_eq!(retrieved.package_name, "test-pkg");
        assert_eq!(retrieved.merkle_root, artifact.merkle_root);
    }

    #[test]
    fn missing_artifact_returns_none() {
        let (manager, _store) = create_test_artifact_manager();
        let hash = ContentHash::of(b"nonexistent");
        assert!(manager.retrieve_package_artifact(hash).is_none());
    }

    #[test]
    fn store_and_retrieve_object_code() {
        let (manager, _store) = create_test_artifact_manager();
        let code = b"fake object code";
        let hash = manager.store_object_code(code);
        let retrieved = manager.retrieve_object_code(hash).unwrap();
        assert_eq!(retrieved, code);
    }

    #[test]
    fn store_and_retrieve_symbol_table() {
        let (manager, _store) = create_test_artifact_manager();
        let mut table = PackageSymbolTable::new();
        let mut interner = Interner::new();
        let sym = interner.intern("add");
        table.register_export("math-lib", sym, ContentHash::of(b"obj"));

        let hash = manager.store_symbol_table(&table);
        let retrieved = manager.retrieve_symbol_table(hash).unwrap();
        let (pkg, _) = retrieved.resolve(sym).unwrap();
        assert_eq!(pkg, "math-lib");
    }

    #[test]
    fn verify_artifact_matching() {
        let artifact = PackageArtifact {
            package_name: "test".into(),
            version: "1.0".into(),
            merkle_root: ContentHash::ZERO,
            symbol_table_hash: ContentHash::ZERO,
            object_code_hash: ContentHash::ZERO,
            per_fn_objects: vec![],
            metadata_hash: ContentHash::ZERO,
            target_triple: Some("x86_64-unknown-linux-gnu".into()),
            opt_level: "release".into(),
            compiler_version: "0.5.0".into(),
        };
        assert!(ArtifactManager::verify_artifact(
            &artifact,
            Some("x86_64-unknown-linux-gnu"),
            "0.5.0"
        ).is_ok());
    }

    #[test]
    fn verify_artifact_target_mismatch() {
        let artifact = PackageArtifact {
            package_name: "test".into(),
            version: "1.0".into(),
            merkle_root: ContentHash::ZERO,
            symbol_table_hash: ContentHash::ZERO,
            object_code_hash: ContentHash::ZERO,
            per_fn_objects: vec![],
            metadata_hash: ContentHash::ZERO,
            target_triple: Some("aarch64-apple-darwin".into()),
            opt_level: "release".into(),
            compiler_version: "0.5.0".into(),
        };
        assert!(ArtifactManager::verify_artifact(
            &artifact,
            Some("x86_64-unknown-linux-gnu"),
            "0.5.0"
        ).is_err());
    }

    #[test]
    fn verify_artifact_version_mismatch() {
        let artifact = PackageArtifact {
            package_name: "test".into(),
            version: "1.0".into(),
            merkle_root: ContentHash::ZERO,
            symbol_table_hash: ContentHash::ZERO,
            object_code_hash: ContentHash::ZERO,
            per_fn_objects: vec![],
            metadata_hash: ContentHash::ZERO,
            target_triple: None,
            opt_level: "release".into(),
            compiler_version: "0.4.0".into(),
        };
        assert!(ArtifactManager::verify_artifact(
            &artifact,
            None,
            "0.5.0"
        ).is_err());
    }

    #[test]
    fn extract_object_code_to_temp_file() {
        let (manager, _store) = create_test_artifact_manager();
        let code = b"fake object code for linking";
        let obj_hash = manager.store_object_code(code);

        let artifact = PackageArtifact {
            package_name: "link-test".into(),
            version: "0.1.0".into(),
            merkle_root: ContentHash::ZERO,
            symbol_table_hash: ContentHash::ZERO,
            object_code_hash: obj_hash,
            per_fn_objects: vec![],
            metadata_hash: ContentHash::ZERO,
            target_triple: None,
            opt_level: "debug".into(),
            compiler_version: "0.5.0".into(),
        };

        let path = manager.extract_object_code(&artifact).unwrap();
        let contents = std::fs::read(&path).unwrap();
        assert_eq!(contents, code);
    }
}
