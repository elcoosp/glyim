use glyim_diag::{FileId, SourceMap};
use glyim_parse::ParseError;
use lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};

pub fn convert_parse_errors(
    _file_id: FileId,
    source_map: &SourceMap,
    errors: &[ParseError],
) -> Vec<Diagnostic> {
    errors
        .iter()
        .filter_map(|error| {
            let (start, end) = match error {
                ParseError::Expected { span, .. }
                | ParseError::ExpectedExpr { span, .. }
                | ParseError::Message { span, .. } => (span.0, span.1),
                ParseError::UnexpectedEof { .. } => return None,
            };
            let (start_lc, _) = source_map.span_to_position(start, end)?;
            Some(Diagnostic {
                range: Range {
                    start: Position {
                        line: start_lc.line as u32,
                        character: start_lc.column as u32,
                    },
                    end: Position {
                        line: start_lc.line as u32,
                        character: (start_lc.column + 1) as u32,
                    },
                },
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("glyim".to_string()),
                message: error.to_string(),
                ..Default::default()
            })
        })
        .collect()
}
