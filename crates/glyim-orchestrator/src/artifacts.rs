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

    pub fn store_package_artifact(&self, artifact: &PackageArtifact) -> ContentHash {
        let serialized = postcard::to_allocvec(artifact).unwrap_or_default();
        self.cas.store(&serialized)
    }

    pub fn retrieve_package_artifact(&self, hash: ContentHash) -> Option<PackageArtifact> {
        let data = self.cas.retrieve(hash)?;
        postcard::from_bytes(&data).ok()
    }

    pub fn has_package_artifact(&self, hash: ContentHash) -> bool {
        self.cas.retrieve(hash).is_some()
    }

    pub fn store_object_code(&self, bytes: &[u8]) -> ContentHash {
        self.cas.store(bytes)
    }

    pub fn retrieve_object_code(&self, hash: ContentHash) -> Option<Vec<u8>> {
        self.cas.retrieve(hash)
    }

    pub fn store_symbol_table(&self, table: &super::symbols::PackageSymbolTable) -> ContentHash {
        let serialized = postcard::to_allocvec(table).unwrap_or_default();
        self.cas.store(&serialized)
    }

    pub fn retrieve_symbol_table(&self, hash: ContentHash) -> Option<super::symbols::PackageSymbolTable> {
        let data = self.cas.retrieve(hash)?;
        postcard::from_bytes(&data).ok()
    }

    pub fn extract_object_code(&self, artifact: &PackageArtifact) -> Result<PathBuf, String> {
        let obj_data = self
            .retrieve_object_code(artifact.object_code_hash)
            .ok_or_else(|| format!("object code not found for {}", artifact.package_name))?;
        let tmp_dir = tempfile::tempdir().map_err(|e| e.to_string())?;
        let obj_path = tmp_dir.path().join(format!("{}.o", artifact.package_name));
        std::fs::write(&obj_path, &obj_data).map_err(|e| e.to_string())?;
        let _ = tmp_dir.keep();
        Ok(obj_path)
    }

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
