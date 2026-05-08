use crate::*;

fn assert_idempotent(source: &str) {
    let config = FormatConfig::default();
    let first = format_source(source, &config).expect("format failed");
    let second = format_source(&first, &config).expect("reformat failed");
    assert_eq!(
        first, second,
        "Not idempotent!\n--- source ---\n{}\n--- first ---\n{}\n--- second ---\n{}",
        source, first, second
    );
}

#[test]
fn idempotent_empty() {
    assert_idempotent("");
}

#[test]
fn idempotent_simple_main() {
    assert_idempotent("main = () => 42");
}

#[test]
fn idempotent_if_else() {
    assert_idempotent("fn main() -> i64 {\n    if true { 1 } else { 0 }\n}\n");
}

#[test]
fn idempotent_struct_def() {
    assert_idempotent("struct Point {\n    x: i64,\n    y: i64,\n}\n");
}

#[test]
fn idempotent_comments() {
    assert_idempotent("// this is a comment\nfn main() {}\n");
}

#[test]
fn idempotent_nested_blocks() {
    assert_idempotent("fn outer() {\n    fn inner() {\n        let x = 1;\n    }\n}\n");
}
