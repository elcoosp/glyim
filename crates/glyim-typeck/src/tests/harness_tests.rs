use glyim_test::harness::{TestMode, TestRunner};
use std::path::PathBuf;

#[test]
fn compile_pass_tests() {
    // Use absolute path from CARGO_MANIFEST_DIR
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/compile-pass");
    TestRunner::new(path.to_str().unwrap())
        .mode(TestMode::CompilePass)
        .parallel(false)
        .build()
        .expect("failed to discover tests")
        .run();
}

#[test]
fn compile_fail_tests() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/compile-fail");
    TestRunner::new(path.to_str().unwrap())
        .mode(TestMode::CompileFail)
        .parallel(false)
        .build()
        .expect("failed to discover tests")
        .run();
}
