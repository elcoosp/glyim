//! TDD test for Script 76: `&str::len()` should not be ambiguous between
//! `str::len` and `<[T]>::len`. The receiver is a str, so the slice impl
//! doesn't apply.
use glyim_span::FileId;
use glyim_test::assert_no_errors;
use glyim_test::harness::compiler::{CompileOutput, PipelineCompiler, TestCompiler};
use std::sync::Arc;
use glyim_test::mock::MockCodegen;

fn compile(src: &str) -> CompileOutput {
    let backend = Arc::new(MockCodegen::new());
    let compiler = PipelineCompiler::new(backend);
    compiler.compile(src, std::path::Path::new("test.g"), FileId::from_raw(1), &[])
}

#[test]
fn str_len_not_ambiguous_with_slice() {
    let src = r#"
        pub mod m {
            impl str {
                fn len(&self) -> usize { 0 }
            }
            impl<T> [T] {
                fn len(&self) -> usize { 0 }
            }
        }
        fn main() {
            let s = "hello";
            let _n = s.len();
        }
    "#;
    let out = compile(src);
    assert_no_errors(&out.diagnostics);
}
