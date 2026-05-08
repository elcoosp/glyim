pub mod context;
pub mod dep_graph;
pub mod dependency;
pub mod fingerprint;
pub mod incremental;
pub mod invalidation;
pub mod persistence;
pub mod query_key;
pub mod result;

pub use context::QueryContext;
pub use dep_graph::DependencyGraph;
pub use dependency::Dependency;
pub use fingerprint::Fingerprint;
pub use incremental::IncrementalState;
pub use invalidation::InvalidationReport;
pub use query_key::QueryKey;
pub use result::{QueryResult, QueryStatus};
pub mod granularity;
pub use granularity::{CacheGranularity, EditHistory, GranularityMonitor};

#[cfg(test)]
mod tests;
