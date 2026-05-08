use crate::fingerprint::Fingerprint;
use std::path::PathBuf;

/// A dependency edge: what a query result depends on.
/// When any dependency changes, the query result is invalidated.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Dependency {
    /// A source file at a specific content hash.
    File {
        path: PathBuf,
        hash: Fingerprint,
    },
    /// Another query's result, identified by its key fingerprint.
    Query {
        key_fingerprint: Fingerprint,
    },
    /// A compiler configuration key-value pair.
    Config {
        key: String,
        value: Fingerprint,
    },
}

impl Dependency {
    /// Create a file dependency.
    pub fn file(path: impl Into<PathBuf>, hash: Fingerprint) -> Self {
        Self::File { path: path.into(), hash }
    }

    /// Create a query dependency.
    pub fn query(key_fingerprint: Fingerprint) -> Self {
        Self::Query { key_fingerprint }
    }

    /// Create a config dependency.
    pub fn config(key: impl Into<String>, value: Fingerprint) -> Self {
        Self::Config { key: key.into(), value }
    }

    /// Return the fingerprint that identifies this dependency,
    /// used for looking up the dependency in the graph.
    pub fn fingerprint(&self) -> Fingerprint {
        match self {
            Self::File { path, hash } => Fingerprint::combine(
                Fingerprint::of_str(&path.to_string_lossy()),
                *hash,
            ),
            Self::Query { key_fingerprint } => *key_fingerprint,
            Self::Config { key, value } => Fingerprint::combine(
                Fingerprint::of_str(key),
                *value,
            ),
        }
    }
}
