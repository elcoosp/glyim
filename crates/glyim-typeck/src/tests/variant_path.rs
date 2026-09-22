//! Pipeline-level proof that an enum variant defined in the value namespace is
//! referenceable through a value-namespace path (`Color::Red`,
//! `mod::Color::Red`, and data-carrying constructors `Some(5)` /
//! `palette::Color::Green(x)`).
//!
//! This exercises: def-map registration of enum variants in the value
//! namespace, `check_path` resolving unit variants to a
//! `thir::ExprKind::VariantRef` carrying the enclosing enum's type and
//! data-carrying variants to a `thir::ExprKind::VariantCtor` of function type,
//! and MIR lowering (unit → `Aggregate` of the ADT; ctor call → `Aggregate`
//! with the field operands).
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
fn unit_variant_referenced_by_single_segment_path() {
    let src = r#"
        enum Color { Red, Green, Blue }
        fn main() {
            let _ = Color::Red;
            let _ = Color::Green;
        }
    "#;
    let output = compile(src);
    assert_no_errors(&output.diagnostics);
}

#[test]
fn unit_variant_referenced_by_multi_segment_path() {
    let src = r#"
        mod palette {
            enum Color { Red, Green }
        }
        fn main() {
            let _ = palette::Color::Red;
        }
    "#;
    let output = compile(src);
    assert_no_errors(&output.diagnostics);
}

#[test]
fn data_variant_constructor_called_by_single_segment_path() {
    let src = r#"
        enum OptionI32 { None, Some(i32) }
        fn main() {
            let _ = OptionI32::Some(5);
        }
    "#;
    let output = compile(src);
    assert_no_errors(&output.diagnostics);
}

#[test]
fn data_variant_constructor_called_by_multi_segment_path() {
    let src = r#"
        mod palette {
            enum Color { Red, Green(i32) }
        }
        fn main() {
            let _ = palette::Color::Green(7);
        }
    "#;
    let output = compile(src);
    assert_no_errors(&output.diagnostics);
}
