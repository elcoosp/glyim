use crate::diagnostics;
use glyim_diag::GlyimDiagnostic;
use glyim_span::Span;
use lsp_types::{DiagnosticSeverity, Range};

#[test]
fn test_convert_diagnostics() {
    let file_id = glyim_span::FileId::from_raw(1);
    let source_map = &crate::SourceMap::new(file_id, "fn main() {}".to_string());

    let glyim_diags = vec![
        GlyimDiagnostic::type_error(Span::DUMMY, "Mismatched types"),
        GlyimDiagnostic::parse_error(Span::DUMMY, "Unexpected token"),
    ];

    let lsp_diags = diagnostics::convert_diagnostics(file_id, source_map, &glyim_diags);

    assert_eq!(lsp_diags.len(), 2);

    assert_eq!(lsp_diags[0].severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(lsp_diags[0].message, "Mismatched types");
    assert_eq!(lsp_diags[0].source, Some("glyim".to_string()));
    assert_eq!(lsp_diags[0].range, Range::default());

    assert_eq!(lsp_diags[1].severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(lsp_diags[1].message, "Unexpected token");
}
