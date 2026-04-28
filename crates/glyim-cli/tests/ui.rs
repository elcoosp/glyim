use std::path::{Path, PathBuf};
use std::fs;

fn ui_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("ui")
}

fn compile_stderr(source: &str, file_path: &str) -> String {
    let parse_out = glyim_parse::parse(source);
    if parse_out.errors.is_empty() {
        match glyim_codegen_llvm::compile_to_ir(source) {
            Ok(_) => String::new(),
            Err(e) => format!("error: {e}"),
        }
    } else {
        let diags: Vec<_> = parse_out.errors.iter().map(|e| {
            let span = match e {
                glyim_parse::ParseError::Expected { span, .. } => Some(glyim_diag::Span::new(span.0, span.1)),
                glyim_parse::ParseError::UnexpectedEof { .. } => None,
                glyim_parse::ParseError::ExpectedExpr { span, .. } => Some(glyim_diag::Span::new(span.0, span.1)),
                glyim_parse::ParseError::Message { span, .. } => Some(glyim_diag::Span::new(span.0, span.1)),
            };
            glyim_diag::Diagnostic::error(e.to_string()).with_span_opt(span)
        }).collect();
        glyim_diag::render_diagnostics(source, file_path, &diags)
    }
}

fn run_ui_test(name: &str) {
    let xyz_path = ui_dir().join(format!("{name}.xyz"));
    let stderr_path = ui_dir().join(format!("{name}.xyz.stderr"));
    let source = fs::read_to_string(&xyz_path)
        .unwrap_or_else(|_| panic!("missing source file {:?}", xyz_path));
    let actual = compile_stderr(&source, &format!("tests/ui/{name}.xyz"));

    if stderr_path.exists() {
        let expected = fs::read_to_string(&stderr_path).unwrap();
        assert_eq!(actual, expected, "stderr mismatch for {}", name);
    } else {
        fs::write(&stderr_path, &actual).unwrap();
        panic!("First run: wrote expected output to {:?}. Run again to compare.", stderr_path);
    }
}

#[test] fn ui_let_missing_eq() { run_ui_test("let_missing_eq"); }
#[test] fn ui_assign_immutable() { run_ui_test("assign_immutable"); }
#[test] fn ui_missing_main() { run_ui_test("missing_main"); }
#[test] fn ui_unterminated_string() { run_ui_test("unterminated_string"); }
#[test] fn ui_missing_closing_brace() { run_ui_test("missing_closing_brace"); }
#[test] fn ui_multiple_errors() { run_ui_test("multiple_errors"); }
#[test] fn ui_if_missing_brace() { run_ui_test("if_missing_brace"); }
#[test] fn ui_unexpected_token() { run_ui_test("unexpected_token"); }
#[test] fn ui_missing_comma_in_params() { run_ui_test("missing_comma_in_params"); }
#[test] fn ui_duplicate_param() { run_ui_test("duplicate_param"); }
#[test] fn ui_nested_error() { run_ui_test("nested_error"); }
#[test] fn ui_empty_source() { run_ui_test("empty_source"); }
