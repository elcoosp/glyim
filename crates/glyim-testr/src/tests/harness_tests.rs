use glyim_testr::harness;

#[test]
fn inject_harness_uses_glyim_str_eq_and_getenv() {
    let source = "fn foo() -> i64 { 42 }\nfn bar() -> i64 { 99 }";
    let tests = vec!["foo".to_string(), "bar".to_string()];
    let modified = harness::inject_harness(source, &tests);
    // Verify the original functions are preserved
    assert!(modified.contains("fn foo() -> i64 { 42 }"));
    assert!(modified.contains("fn bar() -> i64 { 99 }"));
    // Verify it uses __glyim_getenv, not getenv
    assert!(
        modified.contains("__glyim_getenv("),
        "harness should use __glyim_getenv, got: {}",
        modified
    );
    // Verify it uses __glyim_str_eq, not str_eq
    assert!(
        modified.contains("__glyim_str_eq(name_ptr"),
        "harness should use __glyim_str_eq, got: {}",
        modified
    );
    // Verify null-terminated comparison strings
    assert!(modified.contains("\"foo\\0\""));
    assert!(modified.contains("\"bar\\0\""));
    // Verify PASS/FAIL write calls
    assert!(modified.contains("PASS foo"));
    assert!(modified.contains("FAIL bar"));
    // Verify unknown test fallback
    assert!(modified.contains("error: unknown test"));
}

#[test]
fn inject_harness_empty_tests() {
    let source = "fn main() -> i64 { 0 }";
    let tests: Vec<String> = vec![];
    let modified = harness::inject_harness(source, &tests);
    // Should still contain the main function and fallback
    assert!(modified.contains("fn main() -> i64 { 0 }"));
    assert!(modified.contains("error: unknown test"));
    assert!(modified.contains("__glyim_getenv"));
}
