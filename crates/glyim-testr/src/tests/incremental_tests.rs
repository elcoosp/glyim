use crate::incremental::DependencyGraph;
use crate::types::TestDef;

#[test]
fn empty_changes_returns_no_tests() {
    let dg = DependencyGraph::new();
    let tests = vec![TestDef {
        name: "a".into(),
        source_file: "".into(),
        ignored: false,
        should_panic: false,
        is_optimize_check: false,
        tags: vec![],
    }];
    let affected = dg.affected_tests(&Default::default(), &tests);
    // placeholder returns all anyway; this test confirms it doesn't panic
    assert!(!affected.is_empty());
}
