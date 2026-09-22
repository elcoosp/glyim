//! Pipeline-level proof that a function defined inside an inline `mod` block
//! is callable through a multi-segment value-namespace path (`mod::fn`).
//!
//! This exercises the whole chain: `lower_crate` now lowers `Mod` nodes into
//! `ModItem`s, `typeck_crate` registers module functions under the def-map's
//! `LocalDefId` (via `check_fn_items_in_module`), and `check_path` resolves
//! `math::square` through the def-map to the same `FnDefId`.
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
fn module_function_called_via_multi_segment_value_path() {
    let src = r#"
        mod math {
            fn square(x: i32) -> i32 { x * x }
        }
        fn main() {
            let _ = math::square(5);
        }
    "#;
    let output = compile(src);
    assert_no_errors(&output.diagnostics);
}
