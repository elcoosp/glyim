use crate::hash::ContentHash;

#[derive(Debug,Clone,PartialEq,Eq)]
pub struct FileArtifact {
    pub logical_path: String,
    pub content: Vec<u8>,
}

pub trait ContentStore {
    fn store(&self, content: &[u8]) -> ContentHash;
    fn retrieve(&self, hash: ContentHash) -> Option<Vec<u8>>;
    fn register_name(&self, name: &str, hash: ContentHash);
    fn resolve_name(&self, name: &str) -> Option<ContentHash>;
}
