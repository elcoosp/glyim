use glyim_testr::harness;

#[test]
fn inject_harness_generates_dispatchers() {
    let source = "fn foo() -> i64 { 42 }\nfn bar() -> i64 { 99 }";
    let tests = vec!["foo".to_string(), "bar".to_string()];
    let modified = harness::inject_harness(source, &tests);
    assert!(modified.contains("fn foo() -> i64 { 42 }"));
    assert!(modified.contains("fn bar() -> i64 { 99 }"));
    assert!(modified.contains("fn main() -> i64"));
    assert!(modified.contains("getenv"));
    assert!(modified.contains("str_eq"));
    // Null-terminated strings for comparison
    assert!(modified.contains("\"foo\\0\""));
    assert!(modified.contains("\"bar\\0\""));
    assert!(modified.contains("PASS "));
    assert!(modified.contains("FAIL "));
}
