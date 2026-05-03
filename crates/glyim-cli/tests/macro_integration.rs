use glyim_cli::pipeline;

#[test]
fn identity_macro_in_pipeline() {
    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("test.g");
    std::fs::write(&source_path, "@identity(main = () => 42)").unwrap();

    let result = pipeline::run(&source_path, None);
    match result {
        Ok(code) => assert_eq!(code, 42, "expected exit code 42, got {code}"),
        Err(e) => {
            eprintln!("EXPANDED SOURCE:\n{}\n", std::fs::read_to_string(&source_path).unwrap());
            panic!("pipeline failed: {e}");
        }
    }
}
