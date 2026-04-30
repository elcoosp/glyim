use crate::hash::ContentHash;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileArtifact {
    pub logical_path: String,
    pub content_hash: ContentHash,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ActionResult {
    pub output_files: Vec<FileArtifact>,
    pub exit_code: i32,
    pub stdout_hash: Option<ContentHash>,
    pub stderr_hash: Option<ContentHash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    Network(String),
    Auth(String),
    Io(String),
    HashMismatch { expected: ContentHash, actual: ContentHash },
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network(msg) => write!(f, "network error: {msg}"),
            Self::Auth(msg) => write!(f, "auth error: {msg}"),
            Self::Io(msg) => write!(f, "io error: {msg}"),
            Self::HashMismatch { expected, actual } => {
                write!(f, "hash mismatch: expected {expected}, got {actual}")
            }
        }
    }
}

impl std::error::Error for StoreError {}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e.to_string())
    }
}

pub trait ContentStore {
    fn store(&self, content: &[u8]) -> ContentHash;
    fn retrieve(&self, hash: ContentHash) -> Option<Vec<u8>>;
    fn register_name(&self, name: &str, hash: ContentHash);
    fn resolve_name(&self, name: &str) -> Option<ContentHash>;

    fn store_action_result(
        &self,
        action_hash: ContentHash,
        result: ActionResult,
    ) -> Result<(), StoreError>;
    fn retrieve_action_result(&self, action_hash: ContentHash) -> Option<ActionResult>;
    fn has_blobs(&self, hashes: &[ContentHash]) -> Vec<ContentHash>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockStore;

    impl ContentStore for MockStore {
        fn store(&self, _content: &[u8]) -> ContentHash {
            ContentHash::of_str("test")
        }
        fn retrieve(&self, _hash: ContentHash) -> Option<Vec<u8>> {
            Some(vec![])
        }
        fn register_name(&self, _name: &str, _hash: ContentHash) {}
        fn resolve_name(&self, _name: &str) -> Option<ContentHash> {
            None
        }
        fn store_action_result(&self, _action_hash: ContentHash, _result: ActionResult) -> Result<(), StoreError> {
            Ok(())
        }
        fn retrieve_action_result(&self, _action_hash: ContentHash) -> Option<ActionResult> {
            None
        }
        fn has_blobs(&self, hashes: &[ContentHash]) -> Vec<ContentHash> {
            hashes.to_vec()
        }
    }

    #[test]
    fn mock_store_implements_trait() {
        let store = MockStore;
        let _ = store.store(b"hello");
        let _ = store.retrieve(ContentHash::of_str("test"));
        let _ = store.store_action_result(ContentHash::of_str("action"), ActionResult {
            output_files: vec![],
            exit_code: 0,
            stdout_hash: None,
            stderr_hash: None,
        });
    }
}
