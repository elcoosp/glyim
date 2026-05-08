use crate::dep_graph::DependencyGraph;
use crate::fingerprint::Fingerprint;

#[test]
fn empty_graph_has_no_nodes() {
    let g = DependencyGraph::new();
    assert_eq!(g.node_count(), 0);
}

#[test]
fn add_node() {
    let mut g = DependencyGraph::new();
    let fp = Fingerprint::of(b"query_1");
    g.add_node(fp);
    assert_eq!(g.node_count(), 1);
}

#[test]
fn add_duplicate_node_is_idempotent() {
    let mut g = DependencyGraph::new();
    let fp = Fingerprint::of(b"query_1");
    g.add_node(fp);
    g.add_node(fp);
    assert_eq!(g.node_count(), 1);
}

#[test]
fn add_edge_between_nodes() {
    let mut g = DependencyGraph::new();
    let a = Fingerprint::of(b"a");
    let b = Fingerprint::of(b"b");
    g.add_node(a);
    g.add_node(b);
    g.add_edge(a, b);
    assert_eq!(g.edge_count(), 1);
}

#[test]
fn transitive_dependents_single_hop() {
    let mut g = DependencyGraph::new();
    let file = Fingerprint::of(b"file");
    let query1 = Fingerprint::of(b"query1");
    let query2 = Fingerprint::of(b"query2");
    g.add_node(file);
    g.add_node(query1);
    g.add_node(query2);
    g.add_edge(query1, file);
    g.add_edge(query2, query1);
    let affected = g.transitive_dependents(&[file]);
    assert!(affected.contains(&query1));
    assert!(affected.contains(&query2));
}

#[test]
fn transitive_dependents_diamond() {
    let mut g = DependencyGraph::new();
    let file = Fingerprint::of(b"file");
    let q1 = Fingerprint::of(b"q1");
    let q2 = Fingerprint::of(b"q2");
    let q3 = Fingerprint::of(b"q3");
    g.add_node(file);
    g.add_node(q1);
    g.add_node(q2);
    g.add_node(q3);
    g.add_edge(q1, file);
    g.add_edge(q2, file);
    g.add_edge(q3, q1);
    g.add_edge(q3, q2);
    let affected = g.transitive_dependents(&[file]);
    assert!(affected.contains(&q1));
    assert!(affected.contains(&q2));
    assert!(affected.contains(&q3));
    assert_eq!(affected.len(), 3);
}

#[test]
fn transitive_dependents_unrelated_node_not_affected() {
    let mut g = DependencyGraph::new();
    let file = Fingerprint::of(b"file");
    let q1 = Fingerprint::of(b"q1");
    let unrelated = Fingerprint::of(b"unrelated");
    g.add_node(file);
    g.add_node(q1);
    g.add_node(unrelated);
    g.add_edge(q1, file);
    let affected = g.transitive_dependents(&[file]);
    assert!(!affected.contains(&unrelated));
}

#[test]
fn contains_node() {
    let mut g = DependencyGraph::new();
    let fp = Fingerprint::of(b"exists");
    assert!(!g.contains(fp));
    g.add_node(fp);
    assert!(g.contains(fp));
}

#[test]
fn direct_dependents() {
    let mut g = DependencyGraph::new();
    let file = Fingerprint::of(b"file");
    let q1 = Fingerprint::of(b"q1");
    let q2 = Fingerprint::of(b"q2");
    g.add_node(file);
    g.add_node(q1);
    g.add_node(q2);
    g.add_edge(q1, file);
    g.add_edge(q2, file);
    let deps = g.direct_dependents(file);
    assert_eq!(deps.len(), 2);
    assert!(deps.contains(&q1));
    assert!(deps.contains(&q2));
}

#[test]
fn remove_node_cascades() {
    let mut g = DependencyGraph::new();
    let a = Fingerprint::of(b"a");
    let b = Fingerprint::of(b"b");
    g.add_node(a);
    g.add_node(b);
    g.add_edge(b, a);
    g.remove_node(a);
    assert!(!g.contains(a));
    assert_eq!(g.edge_count(), 0);
}
