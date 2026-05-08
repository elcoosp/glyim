use crate::dep_graph::DependencyGraph;
use crate::fingerprint::Fingerprint;
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct InvalidationReport {
    pub red: HashSet<Fingerprint>,
    pub green: HashSet<Fingerprint>,
}

impl InvalidationReport {
    pub fn new(red: HashSet<Fingerprint>, green: HashSet<Fingerprint>) -> Self {
        Self { red, green }
    }

    pub fn red_count(&self) -> usize {
        self.red.len()
    }

    pub fn green_count(&self) -> usize {
        self.green.len()
    }

    pub fn is_green(&self, fp: &Fingerprint) -> bool {
        self.green.contains(fp)
    }
}

pub fn invalidate(graph: &DependencyGraph, changed: &[Fingerprint]) -> InvalidationReport {
    let transitive = graph.transitive_dependents(changed);
    let mut red: HashSet<Fingerprint> = changed.iter().copied().collect();
    red.extend(transitive);
    let green: HashSet<Fingerprint> = graph
        .nodes()
        .into_iter()
        .filter(|fp| !red.contains(fp))
        .collect();
    InvalidationReport::new(red, green)
}
