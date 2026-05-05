use glyim_testr::harness;

#[test]
fn inject_harness_generates_dispatchers_without_null_bytes() {
    let source = "fn foo() -> i64 { 42 }\nfn bar() -> i64 { 99 }";
    let tests = vec!["foo".to_string(), "bar".to_string()];
    let modified = harness::inject_harness(source, &tests);
    assert!(modified.contains("fn foo() -> i64 { 42 }"));
    assert!(modified.contains("fn bar() -> i64 { 99 }"));
    assert!(modified.contains("fn main() -> i64"));
    assert!(modified.contains("getenv"));
    assert!(modified.contains("str_eq"));
    assert!(modified.contains("\"foo\""));
    assert!(modified.contains("\"bar\""));
    assert!(modified.contains("PASS "));
    assert!(modified.contains("FAIL "));
    assert!(!modified.contains("\\0"), "harness must not inject literal backslash-zero");
}
