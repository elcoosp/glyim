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
}
