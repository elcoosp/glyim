//! TDD red test for Script 52: a `const` inside a module must have its type
//! registered, so `crate::m::MY_CONST` can be referenced from a sibling
//! module. Reproduces the stdlib's `pub const GLOBAL: Global = Global;`
//! resolution failure.
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
fn const_at_crate_root_resolves() {
    let src = r#"
        const X: i32 = 42;
        fn main() -> i32 { X }
    "#;
    let out = compile(src);
    assert_no_errors(&out.diagnostics);
}

#[test]
fn const_inside_module_resolves() {
    // The bug: `m::X` fails because X's type is registered at the
    // wrong ConstDefId (or not at all).
    let src = r#"
        mod m {
            const X: i32 = 42;
        }
        fn main() -> i32 { m::X }
    "#;
    let out = compile(src);
    assert_no_errors(&out.diagnostics);
}

#[test]
fn const_struct_value_inside_module_resolves() {
    // Closest to GLOBAL: a const of a user struct type inside a module.
    let src = r#"
        mod m {
            pub struct S;
            pub const GLOBAL: S = S;
        }
        fn main() { let _g = m::GLOBAL; }
    "#;
    let out = compile(src);
    assert_no_errors(&out.diagnostics);
}
