use glyim_diag::GlyimDiagnostic;
use glyim_span::FileId;
use lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};

/// A diagnostic produced by an *external* linter or build driver (e.g.
/// `cargo check --message-format=json`, `clippy`), as opposed to glyim's own
/// in-process type-checker. Carried separately from `GlyimDiagnostic` because
/// external tools report positions as `(file, line, column)` rather than glyim
/// `Span`s, and are merged into the LSP diagnostic stream by
/// [`convert_diagnostics`] (plan §22.4).
#[derive(Debug, Clone)]
pub struct ExternalDiagnostic {
    /// Struct.
    pub file_id: FileId,
    /// 0-based line of the primary span.
    pub line: u32,
    /// 0-based column of the primary span.
    pub column: u32,
    /// Struct.
    pub message: String,
    /// Struct.
    pub severity: ExternalSeverity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// ExternalSeverity.
pub enum ExternalSeverity {
    /// Variant.
    Error,
    /// Variant.
    Warning,
    /// Variant.
    Note,
}

impl ExternalDiagnostic {
    /// Build an `ExternalDiagnostic`. `file_id` lets the LSP filter external
    /// diagnostics to the currently-open file.
    pub fn new(
        file_id: FileId,
        line: u32,
        column: u32,
        message: String,
        severity: ExternalSeverity,
    ) -> Self {
        Self {
            file_id,
            line,
            column,
            message,
            severity,
        }
    }
}

/// Parse the JSON output of `cargo check --message-format=json` (or any
/// `cargo` driver using the same schema) into external diagnostics.
///
/// Cargo emits one JSON object per line; each `compiler-message` reason carries
/// a `message` object with `level`, `message`, and `spans`. We extract the
/// primary span's `line`/`column` (0-based) and map the `level` to a severity.
/// Lines that are not compiler messages (e.g. `compiler-artifact`,
/// `build-script-executed`) are ignored. Malformed lines are skipped.
pub fn parse_cargo_check_json(json: &str) -> Vec<ExternalDiagnostic> {
    let mut out = Vec::new();
    for line in json.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        // Cargo message envelope: { "reason": "compiler-message", "message": {...}, "target": {...} }
        if value.get("reason").and_then(|r| r.as_str()) != Some("compiler-message") {
            continue;
        }
        let msg = match value.get("message") {
            Some(m) => m,
            None => continue,
        };
        let level = msg.get("level").and_then(|l| l.as_str()).unwrap_or("");
        let severity = match level {
            "error" => ExternalSeverity::Error,
            "warning" => ExternalSeverity::Warning,
            _ => ExternalSeverity::Note,
        };
        let message = msg
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string();
        // Primary span: first span in the `spans` array (cargo orders them with
        // the primary span first; `is_primary` is also honoured when present).
        let primary = msg
            .get("spans")
            .and_then(|s| s.as_array())
            .and_then(|spans| {
                spans
                    .iter()
                    .find(|s| s.get("is_primary").and_then(|p| p.as_bool()) == Some(true))
                    .or_else(|| spans.first())
            });
        let (line, column, file_id) = match primary {
            Some(span) => {
                let line = span.get("line").and_then(|l| l.as_u64()).unwrap_or(0) as u32;
                let column = span.get("column").and_then(|c| c.as_u64()).unwrap_or(0) as u32;
                // Cargo reports 1-based line/column; convert to 0-based.
                let line = line.saturating_sub(1);
                let column = column.saturating_sub(1);
                let file_id = span
                    .get("file_name")
                    .and_then(|f| f.as_str())
                    .map(|_name| FileId::from_raw(0))
                    .unwrap_or(FileId::from_raw(0));
                (line, column, file_id)
            }
            None => (0, 0, FileId::from_raw(0)),
        };
        out.push(ExternalDiagnostic {
            file_id,
            line,
            column,
            message,
            severity,
        });
    }
    out
}

/// convert_diagnostics.
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

/// Convert external (linter/build-driver) diagnostics into LSP diagnostics.
///
/// Unlike [`convert_diagnostics`], external diagnostics carry `(file, line,
/// column)` positions rather than glyim `Span`s, so they are mapped directly to
/// LSP `Position`s. This is the `cargo check` / external-linter integration
/// path (plan §22.4).
pub fn convert_external_diagnostics(diags: &[ExternalDiagnostic]) -> Vec<Diagnostic> {
    diags
        .iter()
        .map(|d| {
            let range = Range {
                start: Position {
                    line: d.line,
                    character: d.column,
                },
                end: Position {
                    line: d.line,
                    character: d.column,
                },
            };
            Diagnostic {
                range,
                severity: Some(match d.severity {
                    ExternalSeverity::Error => DiagnosticSeverity::ERROR,
                    ExternalSeverity::Warning => DiagnosticSeverity::WARNING,
                    ExternalSeverity::Note => DiagnosticSeverity::INFORMATION,
                }),
                source: Some("external".to_string()),
                message: d.message.clone(),
                ..Default::default()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"reason":"compiler-message","package_id":"glyim@0.1.0","target":{"name":"x","src_path":"/p/src/main.g"},"message":{"message":"unused variable: `y`","level":"warning","spans":[{"file_name":"/p/src/main.g","line":3,"column":9,"is_primary":true}],"rendered":"warning: unused variable"},"emitted_at":0}
{"reason":"compiler-artifact","package_id":"glyim@0.1.0","target":{"name":"x","src_path":"/p/src/main.g"},"artifact":null}
{"reason":"compiler-message","package_id":"glyim@0.1.0","target":{"name":"x","src_path":"/p/src/main.g"},"message":{"message":"type mismatch","level":"error","spans":[{"file_name":"/p/src/main.g","line":7,"column":1,"is_primary":true}],"rendered":"error: type mismatch"},"emitted_at":1}
"#;

    #[test]
    fn parses_cargo_check_json_messages() {
        let diags = parse_cargo_check_json(SAMPLE);
        // The `compiler-artifact` line must be skipped; only 2 messages remain.
        assert_eq!(diags.len(), 2);

        let warning = diags
            .iter()
            .find(|d| d.severity == ExternalSeverity::Warning)
            .expect("warning present");
        assert_eq!(warning.message, "unused variable: `y`");
        // Cargo reports 1-based; parser converts to 0-based.
        assert_eq!((warning.line, warning.column), (2, 8));

        let error = diags
            .iter()
            .find(|d| d.severity == ExternalSeverity::Error)
            .expect("error present");
        assert_eq!(error.message, "type mismatch");
        assert_eq!((error.line, error.column), (6, 0));
    }

    #[test]
    fn converts_external_to_lsp_diagnostics() {
        let diags = vec![ExternalDiagnostic::new(
            FileId::from_raw(0),
            2,
            8,
            "unused variable: `y`".to_string(),
            ExternalSeverity::Warning,
        )];
        let out = convert_external_diagnostics(&diags);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(out[0].message, "unused variable: `y`");
        assert_eq!(out[0].range.start.line, 2);
        assert_eq!(out[0].range.start.character, 8);
        assert_eq!(out[0].source.as_deref(), Some("external"));
    }

    #[test]
    fn skips_malformed_lines() {
        let diags = parse_cargo_check_json("not json\n{\"reason\":\"compiler-message\"}\n");
        assert_eq!(diags.len(), 0);
    }
}
