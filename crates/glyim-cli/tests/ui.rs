use glyim_diag::miette;
use glyim_hir::lower;
use glyim_parse::parse;
use glyim_typeck::TypeChecker;
use std::fs;
use std::path::PathBuf;

fn ui_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("ui")
}

fn compile_stderr(source: &str, file_path: &str) -> String {
    let parse_out = parse(source);
    let mut errors = String::new();
    if !parse_out.errors.is_empty() {
        for e in &parse_out.errors {
            use std::fmt::Write;
            let report = glyim_diag::Report::new(e.clone())
                .with_source_code(miette::NamedSource::new(file_path, source.to_string()));
            let _ = writeln!(errors, "{:?}", report);
        }
        return errors;
    }
    let mut interner = parse_out.interner;
    let hir = lower(&parse_out.ast, &mut interner);
    let mut tc = TypeChecker::new(interner);
    if let Err(type_errors) = tc.check(&hir) {
        for e in &type_errors {
            use std::fmt::Write;
            let _ = writeln!(errors, "error: {e}");
        }
        return errors;
    }
    match glyim_codegen_llvm::compile_to_ir(source) {
        Ok(_) => {}
        Err(e) => {
            use std::fmt::Write;
            let _ = writeln!(errors, "error: {e}");
        }
    }
    errors
}

fn run_ui_test(name: &str) {
    let source_path = ui_dir().join(format!("{name}.g"));
    let source = fs::read_to_string(&source_path)
        .unwrap_or_else(|_| panic!("missing source file {:?}", source_path));
    let actual = compile_stderr(&source, &format!("tests/ui/{name}.g"));
    insta::assert_snapshot!(name, actual);
}

#[test]
fn ui_let_missing_eq() {
    run_ui_test("let_missing_eq");
}
#[test]
fn ui_assign_immutable() {
    run_ui_test("assign_immutable");
}
#[test]
fn ui_missing_main() {
    run_ui_test("missing_main");
}
#[test]
fn ui_unterminated_string() {
    run_ui_test("unterminated_string");
}
#[test]
fn ui_missing_closing_brace() {
    run_ui_test("missing_closing_brace");
}
#[test]
fn ui_multiple_errors() {
    run_ui_test("multiple_errors");
}
#[test]
fn ui_if_missing_brace() {
    run_ui_test("if_missing_brace");
}
#[test]
fn ui_unexpected_token() {
    run_ui_test("unexpected_token");
}
#[test]
fn ui_missing_comma_in_params() {
    run_ui_test("missing_comma_in_params");
}
#[test]
fn ui_duplicate_param() {
    run_ui_test("duplicate_param");
}
#[test]
fn ui_nested_error() {
    run_ui_test("nested_error");
}
#[test]
fn ui_empty_source() {
    run_ui_test("empty_source");
}
#[test]
fn ui_bool_mismatch() {
    run_ui_test("bool_mismatch");
}
#[test]
fn ui_type_mismatch() {
    run_ui_test("type_mismatch");
}
