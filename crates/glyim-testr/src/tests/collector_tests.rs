use glyim_testr::collector::collect_tests;
use glyim_parse::parse;

#[test]
fn finds_only_functions_with_test_attribute() {
    let out = parse("#[test]\nfn a() { 0 }\nfn b() { 1 }\n#[test]\nfn c() { 2 }");
    let tests = collect_tests(&out.ast, &out.interner, None, false);
    assert_eq!(tests.len(), 2);
    let names: Vec<&str> = tests.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"a"));
    assert!(names.contains(&"c"));
}

#[test]
fn ignores_functions_when_ignore_not_included() {
    let out = parse("#[test]\n#[ignore]\nfn a() { 0 }");
    let tests = collect_tests(&out.ast, &out.interner, None, false);
    assert!(tests.is_empty());
}

#[test]
fn includes_ignored_when_flag_true() {
    let out = parse("#[test]\n#[ignore]\nfn a() { 0 }");
    let tests = collect_tests(&out.ast, &out.interner, None, true);
    assert_eq!(tests.len(), 1);
    assert!(tests[0].ignored);
}

#[test]
fn filter_excludes_non_matching() {
    let out = parse("#[test]\nfn alpha() { 0 }\n#[test]\nfn beta() { 0 }");
    let tests = collect_tests(&out.ast, &out.interner, Some("alpha"), false);
    assert_eq!(tests.len(), 1);
    assert_eq!(tests[0].name, "alpha");
}

#[test]
fn detects_optimize_check_attribute() {
    let out = parse("#[optimize_check]\nfn vec_add() -> i64 { 0 }");
    let tests = collect_tests(&out.ast, &out.interner, None, false);
    assert_eq!(tests.len(), 1);
    assert!(tests[0].is_optimize_check);
}
