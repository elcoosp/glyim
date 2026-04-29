use crate::{Diagnostic, Severity};
use ariadne::{Label, Report, ReportKind, Source};

pub fn render_diagnostics(source: &str, file_path: &str, diagnostics: &[Diagnostic]) -> String {
    let mut out = Vec::new();
    let cache = Source::from(source);
    for diag in diagnostics {
        let kind = match diag.severity {
            Severity::Error => ReportKind::Error,
            Severity::Warning => ReportKind::Warning,
            Severity::Note => ReportKind::Advice,
        };
        let span = diag.span.map(|s| s.start..s.end).unwrap_or(0..0);
        let mut report = Report::build(kind, (file_path, span)).with_message(&diag.message);
        if let Some(span) = diag.span {
            if span.start != span.end {
                report = report.with_label(
                    Label::new((file_path, span.start..span.end)).with_message(&diag.message),
                );
            }
        }
        report
            .finish()
            .write((file_path, cache.clone()), &mut out)
            .unwrap();
    }
    String::from_utf8(out).unwrap_or_default()
}

pub fn render_single(source: &str, file_path: &str, diag: &Diagnostic) -> String {
    render_diagnostics(source, file_path, &[diag.clone()])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Span;

    #[test]
    fn render_error_with_span() {
        let diag = Diagnostic::error("unexpected token").with_span(Span::new(5, 8));
        let rendered = render_single("hello abc world", "test.g", &diag);
        assert!(rendered.contains("unexpected token"));
    }
    #[test]
    fn render_warning() {
        let diag = Diagnostic::warning("unused variable").with_span(Span::new(0, 1));
        let rendered = render_single("x", "test.g", &diag);
        assert!(rendered.contains("unused variable"));
    }
    #[test]
    fn render_note() {
        let diag = Diagnostic::note("see also").with_span(Span::new(0, 1));
        let rendered = render_single("x", "test.g", &diag);
        assert!(rendered.contains("see also"));
    }
    #[test]
    fn render_error_without_span() {
        let diag = Diagnostic::error("no main");
        let rendered = render_single("", "test.g", &diag);
        assert!(rendered.contains("no main"));
    }
    #[test]
    fn render_multiple_diagnostics() {
        let diags = vec![
            Diagnostic::error("first").with_span(Span::new(0, 1)),
            Diagnostic::error("second").with_span(Span::new(5, 6)),
        ];
        let rendered = render_diagnostics("a bcde f", "test.g", &diags);
        assert!(rendered.contains("first"));
        assert!(rendered.contains("second"));
    }
    #[test]
    fn render_empty() {
        assert!(render_diagnostics("", "test.g", &[]).is_empty());
    }
    #[test]
    fn render_zero_width_span() {
        let diag = Diagnostic::error("expected").with_span(Span::new(3, 3));
        let rendered = render_single("abc", "test.g", &diag);
        assert!(rendered.contains("expected"));
    }
}
