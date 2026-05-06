use crate::dependency::Dependency;
use crate::fingerprint::Fingerprint;
use std::any::Any;
use std::sync::Arc;

/// Whether a cached query result is still valid (Green) or needs recomputation (Red).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum QueryStatus {
    /// The result is up-to-date and can be reused.
    Green,
    /// The result is stale and must be recomputed.
    Red,
}

impl QueryStatus {
    pub fn is_valid(self) -> bool {
        matches!(self, Self::Green)
    }
}

/// A stored query result, including its value, fingerprint, dependencies, and validity.
pub struct QueryResult {
    /// The computed value (type-erased).
    pub value: Arc<dyn Any + Send + Sync>,
    /// Fingerprint of the value (used for dependency tracking).
    pub fingerprint: Fingerprint,
    /// What inputs this result depends on.
    pub dependencies: Vec<Dependency>,
    /// Whether this result is still valid.
    pub status: QueryStatus,
}

impl QueryResult {
    /// Create a new query result.
    pub fn new(
        value: Arc<dyn Any + Send + Sync>,
        fingerprint: Fingerprint,
        dependencies: Vec<Dependency>,
        status: QueryStatus,
    ) -> Self {
        Self { value, fingerprint, dependencies, status }
    }

    /// Mark this result as invalid (Red).
    pub fn invalidate(&mut self) {
        self.status = QueryStatus::Red;
    }

    /// Check if this result is still valid.
    pub fn is_valid(&self) -> bool {
        self.status.is_valid()
    }
}
