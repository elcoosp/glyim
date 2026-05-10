#[allow(unused_imports, dead_code)]
use crate::common::*;

#[test]
fn e2e_doc_generator_func() {
    let dir = tempfile::tempdir().unwrap();
    let pkg_dir = dir.path().join("docpkg");
    std::fs::create_dir_all(pkg_dir.join("src")).unwrap();
    std::fs::write(
        pkg_dir.join("glyim.toml"),
        "[package]\nname = \"docpkg\"\nversion = \"0.1.0\"\n",
    ).unwrap();
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
    std::fs::write(pkg_dir.join("src/main.g"), src).unwrap();

    let doc_dir = dir.path().join("site");
    let result = pipeline::generate_doc(&pkg_dir, Some(&doc_dir), None);
    assert!(result.is_ok());

    let api_json = doc_dir.join("public/api/api.json");
    assert!(api_json.exists());
    let json_str = std::fs::read_to_string(&api_json).unwrap();
    assert!(json_str.contains("Adds two integers together."));
    assert!(json_str.contains("let result = add(1, 2)"));
    assert!(json_str.contains("assert(result == 3)"));
}

#[test]
fn e2e_doc_impl_method() {
    let dir = tempfile::tempdir().unwrap();
    let pkg_dir = dir.path().join("docpkg2");
    std::fs::create_dir_all(pkg_dir.join("src")).unwrap();
    std::fs::write(
        pkg_dir.join("glyim.toml"),
        "[package]\nname = \"docpkg2\"\nversion = \"0.1.0\"\n",
    ).unwrap();
    let src = "struct Counter { val: i64 }\nimpl Counter {\n    // Increments the counter.\n    fn inc(mut self: Counter) -> Counter { self.val = self.val + 1; self }\n}\nmain = () => 0";
    std::fs::write(pkg_dir.join("src/main.g"), src).unwrap();
    let doc_dir = dir.path().join("site");
    let result = pipeline::generate_doc(&pkg_dir, Some(&doc_dir), None);
    assert!(result.is_ok());
    let api_json = doc_dir.join("public/api/api.json");
    let json_str = std::fs::read_to_string(api_json).unwrap();
    assert!(json_str.contains("Increments the counter."));
}
