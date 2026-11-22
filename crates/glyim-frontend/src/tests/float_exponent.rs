use crate::lex;
use glyim_diag::GlyimDiagnostic;
use glyim_span::FileId;

#[test]
fn test_incomplete_float_exponent() {
    let source = "1e";
    let file_id = FileId::from_usize(1);
    let result = lex(source, file_id);
    assert!(!result.diagnostics.is_empty());
    let has_exp_error = result
        .diagnostics
        .iter()
        .any(|d: &GlyimDiagnostic| d.message.contains("incomplete float exponent"));
    assert!(has_exp_error);
    assert!(!result.tokens.is_empty());
}
