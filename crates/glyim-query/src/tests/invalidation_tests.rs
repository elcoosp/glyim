use glyim_query::invalidation::{InvalidationReport, invalidate};
use glyim_query::dep_graph::DependencyGraph;
use glyim_query::fingerprint::Fingerprint;

#[test]
fn invalidate_nothing_when_no_changes() {
    let mut g = DependencyGraph::new();
    let q = Fingerprint::of(b"query");
    g.add_node(q);
    let report = invalidate(&g, &[]);
    assert!(report.red.is_empty());
    assert!(report.green.contains(&q));
}

#[test]
fn invalidate_single_query_when_input_changes() {
    let mut g = DependencyGraph::new();
    let file = Fingerprint::of(b"file");
    let query = Fingerprint::of(b"query");
    g.add_node(file);
    g.add_node(query);
    g.add_edge(query, file);
    let report = invalidate(&g, &[file]);
    assert!(report.red.contains(&query));
    assert!(!report.green.contains(&query));
}

#[test]
fn invalidate_cascades_transitively() {
    let mut g = DependencyGraph::new();
    let file = Fingerprint::of(b"file");
    let q1 = Fingerprint::of(b"q1");
    let q2 = Fingerprint::of(b"q2");
    g.add_node(file);
    g.add_node(q1);
    g.add_node(q2);
    g.add_edge(q1, file);
    g.add_edge(q2, q1);
    let report = invalidate(&g, &[file]);
    assert!(report.red.contains(&q1));
    assert!(report.red.contains(&q2));
}

#[test]
fn unrelated_queries_stay_green() {
    let mut g = DependencyGraph::new();
    let file_a = Fingerprint::of(b"file_a");
    let file_b = Fingerprint::of(b"file_b");
    let q_a = Fingerprint::of(b"q_a");
    let q_b = Fingerprint::of(b"q_b");
    g.add_node(file_a);
    g.add_node(file_b);
    g.add_node(q_a);
    g.add_node(q_b);
    g.add_edge(q_a, file_a);
    g.add_edge(q_b, file_b);
    let report = invalidate(&g, &[file_a]);
    assert!(report.red.contains(&q_a));
    assert!(!report.red.contains(&q_b));
    assert!(report.green.contains(&q_b));
}

#[test]
fn invalidation_report_counts() {
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
    let file_b = Fingerprint::of(b"file_b");
    g.add_node(file_b);
    g.add_edge(q3, file_b);
    let report = invalidate(&g, &[file]);
    assert_eq!(report.red.len(), 2);
    assert_eq!(report.green.len(), 2);
}

#[test]
fn changed_inputs_also_marked_red() {
    let mut g = DependencyGraph::new();
    let file = Fingerprint::of(b"file");
    let q = Fingerprint::of(b"query");
    g.add_node(file);
    g.add_node(q);
    g.add_edge(q, file);
    let report = invalidate(&g, &[file]);
    assert!(report.red.contains(&file));
}
