//! TDD test for module-scoped generic struct arity (Script 37).
//!
//! Repro of the stdlib probe failure that Script 32-35 fixed:
//! `pub mod m { pub struct Foo<T> { x: T } }` used to lose `<T>` from
//! `AdtDef.generic_params` (or get an empty substitution), so
//! `m::Foo { x: 42 }` reported "mismatched types: expected integer, found T".
//!
//! Uses the same `PipelineCompiler` harness as `module_call.rs` so the test
//! exercises the full parse → HIR → typeck path.
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
fn module_wrapped_generic_struct_typechecks() {
    // The specific bug: struct with `<T>` inside `pub mod m`.
    let src = r#"
        pub mod m {
            pub struct Foo<T> { x: T }
        }
        fn main() {
            let f = m::Foo { x: 42 };
            let _ = f.x;
        }
    "#;
    let output = compile(src);
    assert_no_errors(&output.diagnostics);
}

#[test]
fn module_wrapped_generic_fn_returns_wrapped_struct() {
    // Closer to Once/Repeat: a generic fn in a module returns a
    // module-scoped generic struct.
    let src = r#"
        pub mod m {
            pub struct Foo<T> { x: T }
            pub fn make<T>(v: T) -> Foo<T> { Foo { x: v } }
        }
        fn main() {
            let f = m::make(42);
            let _ = f.x;
        }
    "#;
    let output = compile(src);
    assert_no_errors(&output.diagnostics);
}

#[test]
fn crate_root_generic_struct_still_works() {
    // Sanity: this has always worked; ensure we didn't regress.
    let src = r#"
        struct Foo<T> { x: T }
        fn main() {
            let f = Foo { x: 42 };
            let _ = f.x;
        }
    "#;
    let output = compile(src);
    assert_no_errors(&output.diagnostics);
}
