pub mod fingerprint;
pub mod query_key;
pub mod dependency;
pub mod result;
pub mod context;
pub mod dep_graph;
pub mod invalidation;
pub mod persistence;
pub mod incremental;

pub use fingerprint::Fingerprint;
pub use query_key::QueryKey;
pub use dependency::Dependency;
pub use result::{QueryResult, QueryStatus};
pub use context::QueryContext;
pub use dep_graph::DependencyGraph;
pub use invalidation::InvalidationReport;
pub use incremental::IncrementalState;

#[cfg(test)]
mod tests;
