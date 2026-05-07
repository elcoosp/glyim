use crate::diagnostics::convert_parse_errors;
use glyim_diag::{FileId, SourceMap, Span};
use glyim_parse::ParseError;
use std::path::PathBuf;

#[test]
fn parse_error_converts_to_lsp_diagnostic() {
    let file_id = FileId(0);
    let sm = SourceMap::new(PathBuf::from("/test/main.g"), file_id, "let x 42".to_string());
    let errors = vec![ParseError::Message {
        msg: "expected =".into(),
        span: (5, 7),
    }];
    let diags = convert_parse_errors(file_id, &sm, &errors);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].source.as_deref(), Some("glyim"));
    assert!(diags[0].message.contains("expected"));
}
