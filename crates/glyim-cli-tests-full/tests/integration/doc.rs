#[allow(unused_imports, dead_code)]
use crate::common::*;

#[test]
fn e2e_doc_generator_func() {
    let src = r#"
// Adds two integers together.
//
// # Examples
//
// ```glyim
// let result = add(1, 2)
// assert(result == 3)
// ```
fn add(a: i64, b: i64) -> i64 { a + b }
main = () => add(1, 2)
"#;
    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("test.g");
    std::fs::write(&source_path, src).unwrap();

    let doc_dir = dir.path().join("doc");
    let result = pipeline::generate_doc(&source_path, Some(&doc_dir));
    assert!(result.is_ok());

    let index_html = doc_dir.join("index.html");
    assert!(index_html.exists());
    let html = std::fs::read_to_string(&index_html).unwrap();
    assert!(html.contains("Adds two integers together."));
    assert!(html.contains("let result = add(1, 2)"));
    assert!(html.contains("assert(result == 3)"));
}

#[test]
fn e2e_doc_impl_method() {
    let src = "struct Counter { val: i64 }\nimpl Counter {\n    // Increments the counter.\n    fn inc(mut self: Counter) -> Counter { self.val = self.val + 1; self }\n}\nmain = () => 0";
    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("test.g");
    std::fs::write(&source_path, src).unwrap();
    let doc_dir = dir.path().join("doc");
    let result = pipeline::generate_doc(&source_path, Some(&doc_dir));
    assert!(result.is_ok());
    let html = std::fs::read_to_string(doc_dir.join("index.html")).unwrap();
    assert!(html.contains("Increments the counter."));
}

