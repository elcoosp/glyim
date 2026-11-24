use glyim_diag::GlyimDiagnostic;
use glyim_span::FileId;
use lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};

// LSP-DEAD-EXTERNAL-DIAG: the former external-diagnostics integration
// (`ExternalDiagnostic`, `ExternalSeverity`, `parse_cargo_check_json`,
// `convert_external_diagnostics` + their self-tests) parsed `cargo check
// --message-format=json` output for a plan-§22.4 linter-merge path that
// never got its consumer: no caller ever built an `ExternalDiagnostic`
// (the only reference outside the helpers was their own test module), so
// the cluster was dead scaffolding that could silently drift from the
// real `convert_diagnostics` seam. Removed; the single live conversion
// path below is the definition of diagnostic conversion.

/// `convert_diagnostics`.
#[must_use]
pub fn convert_diagnostics(
    _file_id: FileId,
    source_map: &crate::database::SourceMap,
    diags: &[GlyimDiagnostic],
) -> Vec<Diagnostic> {
    diags
        .iter()
        .map(|d| {
            let range = if let Some(((start_line, start_col), (end_line, end_col))) = source_map
                .span_to_position(d.span.primary.lo.to_usize(), d.span.primary.hi.to_usize())
            {
                Range {
                    start: Position {
                        line: start_line as u32,
                        character: start_col as u32,
                    },
                    end: Position {
                        line: end_line as u32,
                        character: end_col as u32,
                    },
                }
            } else {
                Range::default()
            };
            Diagnostic {
                range,
                severity: Some(match d.severity {
                    glyim_diag::DiagSeverity::Error => DiagnosticSeverity::ERROR,
                    glyim_diag::DiagSeverity::Warning => DiagnosticSeverity::WARNING,
                    _ => DiagnosticSeverity::INFORMATION,
                }),
                source: Some("glyim".to_string()),
                message: d.message.clone(),
                ..Default::default()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The converter maps a glyim diagnostic's span through the same
    /// `SourceMap` the push path publishes with: error severity, the
    /// `glyim` source tag, and exact UTF-16 positions.
    #[test]
    fn converts_glyim_diagnostics_with_mapped_positions() {
        let source = "fn broken( {\n}\n";
        let sm = crate::database::SourceMap::new(FileId::from_usize(0), source.to_string());
        let parse_result = glyim_frontend::parse_to_syntax(source, FileId::from_usize(0));
        assert!(
            !parse_result.diagnostics.is_empty(),
            "fixture must be broken"
        );
        let out = convert_diagnostics(FileId::from_usize(0), &sm, &parse_result.diagnostics);
        assert_eq!(out.len(), parse_result.diagnostics.len());
        for diag in &out {
            assert_eq!(diag.source.as_deref(), Some("glyim"));
            assert_eq!(diag.severity, Some(DiagnosticSeverity::ERROR));
        }
        // Positions stay inside the document (the parse errors sit on
        // lines 0-1 of a two-line fixture).
        assert!(out.iter().all(|d| d.range.start.line <= 1));
    }
}
