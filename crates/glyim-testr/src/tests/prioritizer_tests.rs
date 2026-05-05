use crate::prioritizer::sort_tests;
use crate::types::TestDef;
use crate::config::PriorityMode;

#[test]
fn declaration_order_keeps_input() {
    let mut tests = vec![
        TestDef { name: "c".into(), source_file: "".into(), ignored: false, should_panic: false, is_optimize_check: false, tags: vec![] },
        TestDef { name: "a".into(), source_file: "".into(), ignored: false, should_panic: false, is_optimize_check: false, tags: vec![] },
    ];
    sort_tests(&mut tests, PriorityMode::DeclarationOrder);
    assert_eq!(tests[0].name, "c");
    assert_eq!(tests[1].name, "a");
}

#[test]
fn fast_first_sorts_by_name() {
    let mut tests = vec![
        TestDef { name: "b".into(), source_file: "".into(), ignored: false, should_panic: false, is_optimize_check: false, tags: vec![] },
        TestDef { name: "a".into(), source_file: "".into(), ignored: false, should_panic: false, is_optimize_check: false, tags: vec![] },
    ];
    sort_tests(&mut tests, PriorityMode::FastFirst);
    assert_eq!(tests[0].name, "a");
    assert_eq!(tests[1].name, "b");
}
