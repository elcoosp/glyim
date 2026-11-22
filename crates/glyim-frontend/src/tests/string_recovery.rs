use crate::lex;
use glyim_diag::GlyimDiagnostic;
use glyim_span::FileId;
use glyim_syntax::SyntaxKind;

#[test]
fn test_unterminated_string() {
    let source = "\"unclosed";
    let file_id = FileId::from_usize(1);
    let result = lex(source, file_id);
    assert!(!result.diagnostics.is_empty());
    let has_unterm_error = result
        .diagnostics
        .iter()
        .any(|d: &GlyimDiagnostic| d.message.contains("unterminated string"));
    assert!(has_unterm_error);
    let str_token = result
        .tokens
        .iter()
        .find(|t| t.kind == SyntaxKind::StringLit);
    assert!(str_token.is_some());
    assert_eq!(str_token.unwrap().text.as_str(), "\"unclosed");
}
