use glyim_span::FileId;
use glyim_test::assert_diag_contains;
use glyim_test::assert_has_errors;
use glyim_test::assert_no_errors;
use glyim_test::harness::compiler::{CompileOutput, PipelineCompiler, TestCompiler};
use glyim_test::mock::MockCodegen;
use std::sync::Arc;

fn compile(src: &str) -> CompileOutput {
    let backend = Arc::new(MockCodegen::new());
    let compiler = PipelineCompiler::new(backend);
    compiler.compile(src, std::path::Path::new("test.g"), FileId::from_raw(1), &[])
}

// These tests exercise the real compile pipeline for `let x =` named
// bindings, array literals, struct literals with spread, and data-variant /
// or / range / slice pattern matching in `match` expressions. All pass through
// the real pipeline (`PipelineCompiler` + `MockCodegen`).

#[test]
fn match_guard_uses_binding() {
    let output = compile(
        r#"
        enum OptionI32 { None, Some(i32) }
        fn main() {
            let x = OptionI32::Some(5);
            match x {
                OptionI32::Some(y) if y > 0 => {},
                _ => {}
            }
        }
        "#,
    );
    assert_no_errors(&output.diagnostics);
}

#[test]
fn or_pattern_same_types() {
    let output = compile(
        r#"
        fn main() {
            match 1 {
                1 | 2 => {},
                _ => {}
            }
        }
        "#,
    );
    assert_no_errors(&output.diagnostics);
}

#[test]
fn range_pattern_integer() {
    let output = compile(
        r#"
        fn main() {
            match 5 {
                1..=10 => {},
                _ => {}
            }
        }
        "#,
    );
    assert_no_errors(&output.diagnostics);
}

#[test]
fn slice_pattern_array() {
    let output = compile(
        r#"
        fn main() {
            let arr = [1, 2, 3];
            match arr {
                [a, b, ..rest] => {},
                _ => {}
            }
        }
        "#,
    );
    assert_no_errors(&output.diagnostics);
}

#[test]
fn index_expression_array() {
    let output = compile(
        r#"
        fn main() {
            let arr = [1, 2, 3];
            let x = arr[0];
        }
        "#,
    );
    assert_no_errors(&output.diagnostics);
}

#[test]
fn struct_literal_with_spread() {
    let output = compile(
        r#"
        struct S { x: i32, y: i32 }
        fn main() {
            let a = S { x: 1, y: 2 };
            let b = S { x: 3, ..a };
        }
        "#,
    );
    assert_no_errors(&output.diagnostics);
}

#[test]
fn or_pattern_mismatched_types_fails() {
    let output = compile(
        r#"
        fn main() {
            match 1 {
                true | "hello" => {},
                _ => {}
            }
        }
        "#,
    );
    assert_has_errors(&output.diagnostics);
    assert_diag_contains(&output.diagnostics, "mismatched types");
}
