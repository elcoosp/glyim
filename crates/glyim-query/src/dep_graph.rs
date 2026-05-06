use crate::fingerprint::Fingerprint;
use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::{HashMap, HashSet};

/// A directed acyclic graph that records dependencies between queries and their inputs.
///
/// Edges point from *dependent* → *dependency*: if query Q depends on file F,
/// there is an edge Q → F. To find what's affected when F changes, we find all
/// nodes that have a path to F (i.e., reverse traversal).
pub struct DependencyGraph {
    graph: DiGraph<Fingerprint, ()>,
    index_map: HashMap<Fingerprint, NodeIndex>,
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            index_map: HashMap::new(),
        }
    }

    pub fn add_node(&mut self, fp: Fingerprint) {
        if !self.index_map.contains_key(&fp) {
            let idx = self.graph.add_node(fp);
            self.index_map.insert(fp, idx);
        }
    }

    pub fn contains(&self, fp: Fingerprint) -> bool {
        self.index_map.contains_key(&fp)
    }

    pub fn add_edge(&mut self, dependent: Fingerprint, dependency: Fingerprint) {
        self.add_node(dependent);
        self.add_node(dependency);
        let from = self.index_map[&dependent];
        let to = self.index_map[&dependency];
        if !self.graph.contains_edge(from, to) {
            self.graph.add_edge(from, to, ());
        }
    }

    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    pub fn direct_dependents(&self, fp: Fingerprint) -> Vec<Fingerprint> {
        let Some(&target_idx) = self.index_map.get(&fp) else {
            return vec![];
        };
        self.graph
            .neighbors_directed(target_idx, petgraph::Direction::Incoming)
            .map(|idx| self.graph[idx])
            .collect()
    }

    /// BFS from each root following incoming edges (reverse direction).
    pub fn transitive_dependents(&self, roots: &[Fingerprint]) -> HashSet<Fingerprint> {
        let mut affected = HashSet::new();
        let mut queue: Vec<NodeIndex> = Vec::new();
        for root_fp in roots {
            if let Some(&idx) = self.index_map.get(root_fp) {
                queue.push(idx);
            }
        }
        while let Some(current) = queue.pop() {
            for neighbor in
                self.graph.neighbors_directed(current, petgraph::Direction::Incoming)
            {
                let fp = self.graph[neighbor];
                if affected.insert(fp) {
                    queue.push(neighbor);
                }
            }
        }
        affected
    }

    pub fn remove_node(&mut self, fp: Fingerprint) {
        if let Some(idx) = self.index_map.remove(&fp) {
            let _ = self.graph.remove_node(idx);
        }
    }

    pub fn nodes(&self) -> Vec<Fingerprint> {
        self.graph.node_indices().map(|idx| self.graph[idx]).collect()
    }
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}
