use crate::dep_graph::DependencyGraph;
use crate::dependency::Dependency;
use crate::fingerprint::Fingerprint;
use crate::invalidation::invalidate;
use crate::result::{QueryResult, QueryStatus};
use dashmap::DashMap;
use std::any::Any;
use std::sync::Arc;

/// A snapshot of a cached query entry (for persistence).
pub struct CacheEntry {
    pub key: Fingerprint,
    pub fingerprint: Fingerprint,
    pub dependencies: Vec<Dependency>,
    pub is_green: bool,
}

pub struct QueryContext {
    cache: DashMap<Fingerprint, QueryResult>,
    dep_graph: std::sync::RwLock<DependencyGraph>,
}

impl QueryContext {
    pub fn new() -> Self {
        Self {
            cache: DashMap::new(),
            dep_graph: std::sync::RwLock::new(DependencyGraph::new()),
        }
    }

    pub fn insert(
        &self,
        key: Fingerprint,
        value: Arc<dyn Any + Send + Sync>,
        value_fingerprint: Fingerprint,
        dependencies: Vec<Dependency>,
    ) {
        {
            let mut graph = self.dep_graph.write().unwrap();
            graph.add_node(key);
            for dep in &dependencies {
                graph.add_edge(key, dep.fingerprint());
            }
        }
        self.cache.insert(
            key,
            QueryResult::new(value, value_fingerprint, dependencies, QueryStatus::Green),
        );
    }

    /// Insert a query result with an explicit status (used by persistence).
    pub fn insert_with_status(
        &self,
        key: Fingerprint,
        value: Arc<dyn Any + Send + Sync>,
        value_fingerprint: Fingerprint,
        dependencies: Vec<Dependency>,
        status: QueryStatus,
    ) {
        {
            let mut graph = self.dep_graph.write().unwrap();
            graph.add_node(key);
            for dep in &dependencies {
                graph.add_edge(key, dep.fingerprint());
            }
        }
        self.cache.insert(
            key,
            QueryResult::new(value, value_fingerprint, dependencies, status),
        );
    }

    pub fn get(&self, key: &Fingerprint) -> Option<QueryResult> {
        self.cache.get(key).map(|r| QueryResult {
            value: r.value.clone(),
            fingerprint: r.fingerprint,
            dependencies: r.dependencies.clone(),
            status: r.status,
        })
    }

    pub fn is_green(&self, key: &Fingerprint) -> bool {
        self.cache.get(key).map(|r| r.is_valid()).unwrap_or(false)
    }

    pub fn invalidate_key(&self, key: Fingerprint) {
        if let Some(mut result) = self.cache.get_mut(&key) {
            result.invalidate();
        }
    }

    pub fn record_dependency(&self, query_key: Fingerprint, dep: Dependency) {
        let mut graph = self.dep_graph.write().unwrap();
        graph.add_edge(query_key, dep.fingerprint());
    }

    pub fn invalidate_fingerprints(
        &self,
        changed: &[Fingerprint],
    ) -> crate::invalidation::InvalidationReport {
        let graph = self.dep_graph.read().unwrap();
        let report = invalidate(&graph, changed);
        drop(graph);
        for red_fp in &report.red {
            self.invalidate_key(*red_fp);
        }
        report
    }

    pub fn invalidate_dependencies(&self, changed_deps: &[Dependency]) {
        let changed_fps: Vec<Fingerprint> =
            changed_deps.iter().map(|d| d.fingerprint()).collect();
        self.invalidate_fingerprints(&changed_fps);
    }

    pub fn clear(&self) {
        self.cache.clear();
        let mut graph = self.dep_graph.write().unwrap();
        *graph = DependencyGraph::new();
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    pub fn dep_graph(&self) -> &std::sync::RwLock<DependencyGraph> {
        &self.dep_graph
    }

    /// Iterate over all cached entries (for persistence).
    pub fn cache_iter(&self) -> Vec<CacheEntry> {
        self.cache
            .iter()
            .map(|item| CacheEntry {
                key: *item.key(),
                fingerprint: item.fingerprint,
                dependencies: item.dependencies.clone(),
                is_green: item.is_valid(),
            })
            .collect()
    }

    /// Execute a query: return cached result if Green, otherwise call `compute`.
    pub fn query<V: 'static + Send + Sync + Clone>(
        &self,
        key: Fingerprint,
        compute: impl FnOnce() -> Arc<dyn Any + Send + Sync>,
        value_fingerprint: Fingerprint,
        dependencies: Vec<Dependency>,
    ) -> V {
        if let Some(cached) = self.cache.get(&key) {
            if cached.is_valid() {
                if let Some(val) = cached.value.downcast_ref::<V>() {
                    return val.clone();
                }
            }
        }
        drop(self.cache.get(&key));
        let value = compute();
        let result_value = value
            .downcast_ref::<V>()
            .expect("query compute returned wrong type")
            .clone();
        self.insert(key, value, value_fingerprint, dependencies);
        result_value
    }
}

impl Default for QueryContext {
    fn default() -> Self {
        Self::new()
    }
}
