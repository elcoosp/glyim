#[allow(unused_imports, dead_code)]
use crate::common::*;
use glyim_cli::test_runner::*;

#[test]
fn test_run_summary_all_passed() {
    let summary = TestRunSummary {
        results: vec![
            ("a".into(), TestResult::Passed),
            ("b".into(), TestResult::Passed),
        ],
    };
    assert_eq!(summary.passed(), 2);
    assert_eq!(summary.failed(), 0);
    assert_eq!(summary.total(), 2);
    assert_eq!(summary.exit_code(), 0);
}

#[test]
fn test_run_summary_with_failure() {
    let summary = TestRunSummary {
        results: vec![
            ("a".into(), TestResult::Passed),
            ("b".into(), TestResult::Failed),
            ("c".into(), TestResult::Ignored),
        ],
    };
    assert_eq!(summary.passed(), 1);
    assert_eq!(summary.failed(), 1);
    assert_eq!(summary.ignored(), 1);
    assert_eq!(summary.total(), 3);
    assert_eq!(summary.exit_code(), 1);
}

#[test]
fn test_collect_test_functions_with_filter() {
    let parse_out = glyim_parse::parse("#[test]\nfn a() { 0 }\n#[test]\nfn b() { 1 }");
    let tests = collect_test_functions(&parse_out.ast, &parse_out.interner, Some("a"), false);
    assert_eq!(tests.len(), 1);
    assert_eq!(tests[0].name, "a");
}

#[test]
fn test_collect_test_functions_ignored_filtered_out() {
    let parse_out = glyim_parse::parse("#[test]\n#[ignore]\nfn a() { 0 }");
    let tests = collect_test_functions(&parse_out.ast, &parse_out.interner, None, false);
    assert!(tests.is_empty());
}

#[test]
fn test_collect_test_functions_ignored_included() {
    let parse_out = glyim_parse::parse("#[test]\n#[ignore]\nfn a() { 0 }");
    let tests = collect_test_functions(&parse_out.ast, &parse_out.interner, None, true);
    assert_eq!(tests.len(), 1);
    assert!(tests[0].ignored);
}
