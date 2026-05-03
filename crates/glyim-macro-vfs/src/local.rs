use crate::hash::ContentHash;
use crate::store::ContentStore;
use std::fs;
use std::path::PathBuf;

#[allow(dead_code)]
pub struct LocalContentStore {
    base_dir: PathBuf,
    objects_dir: PathBuf,
    names_dir: PathBuf,
}

impl LocalContentStore {
    pub fn new(base_dir: impl Into<PathBuf>) -> std::io::Result<Self> {
        let base_dir = base_dir.into();
        let objects_dir = base_dir.join("objects");
        let names_dir = base_dir.join("names");
        fs::create_dir_all(&objects_dir)?;
        fs::create_dir_all(&names_dir)?;
        Ok(Self {
            base_dir,
            objects_dir,
            names_dir,
        })
    }

    pub fn list_blobs(&self) -> Vec<ContentHash> {
        let mut hashes = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.objects_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir()
                    && let Ok(sub) = std::fs::read_dir(&path) {
                        for sub_entry in sub.flatten() {
                            let fname = sub_entry.file_name().to_string_lossy().to_string();
                            let full_hex = format!("{}{}", path.file_name().unwrap().to_string_lossy(), fname);
                            if let Ok(h) = ContentHash::from_hex(&full_hex) {
                                hashes.push(h);
                            }
                        }
                    }
            }
        }
        hashes
    }

    pub fn list_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.names_dir) {
            for entry in entries.flatten() {
                let fname = entry.file_name().to_string_lossy().to_string();
                names.push(fname);
            }
        }
        names
    }

    pub fn delete_blob(&self, hash: ContentHash) -> std::io::Result<()> {
        let path = self.object_path(hash);
        if path.exists() {
            std::fs::remove_file(path)
        } else {
            Ok(())
        }
    }

    fn object_path(&self, hash: ContentHash) -> PathBuf {
        let hex = hash.to_hex();
        self.objects_dir.join(&hex[0..2]).join(&hex[2..])
    }
}

impl ContentStore for LocalContentStore {
    fn store(&self, content: &[u8]) -> ContentHash {
        let hash = ContentHash::of(content);
        let path = self.object_path(hash);
        if !path.exists() {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(&path, content);
        }
        hash
    }

    fn retrieve(&self, hash: ContentHash) -> Option<Vec<u8>> {
        fs::read(self.object_path(hash)).ok()
    }

    fn register_name(&self, name: &str, hash: ContentHash) {
        let safe = name.replace(['/', '\\'], "_");
        let _ = fs::write(self.names_dir.join(&safe), hash.to_hex());
    }

    fn resolve_name(&self, name: &str) -> Option<ContentHash> {
        let safe = name.replace(['/', '\\'], "_");
        let hex = fs::read_to_string(self.names_dir.join(&safe)).ok()?;
        hex.parse().ok()
    }

    fn store_action_result(
        &self,
        action_hash: ContentHash,
        result: crate::store::ActionResult,
    ) -> Result<(), crate::store::StoreError> {
        let json = serde_json::to_vec(&result)
            .map_err(|e| crate::store::StoreError::Io(format!("serialize action result: {e}")))?;
        let stored_hash = self.store(&json);
        if stored_hash != action_hash {
            self.register_name(&format!("action:{}", action_hash), stored_hash);
        }
        Ok(())
    }

    fn retrieve_action_result(
        &self,
        action_hash: ContentHash,
    ) -> Option<crate::store::ActionResult> {
        let name = format!("action:{}", action_hash);
        let hash = self.resolve_name(&name)?;
        let json = self.retrieve(hash)?;
        serde_json::from_slice(&json).ok()
    }

    fn has_blobs(&self, hashes: &[ContentHash]) -> Vec<ContentHash> {
        hashes
            .iter()
            .filter(|h| self.retrieve(**h).is_some())
            .copied()
            .collect()
    }
}
