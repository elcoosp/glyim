//! Consumer walkthrough: construct diagnostics and render them through the
//! [`DiagnosticRenderer`] seam (plan §3.4).
//!
//! Run with:
//!
//! ```text
//! cargo run -p glyim-diag --example render_diagnostics
//! ```
//!
//! The example is the library counterpart of the CLI's `--error-format`
//! flag: the same batch rendered as human text (stderr, as the CLI does)
//! and as JSON diagnostics schema v1 records (stdout, one object per
//! line). Stream routing stays with the caller — renderers only format.

use std::fmt::Write as _;
use std::sync::Arc;

use glyim_diag::renderer::{DiagnosticRenderer, HumanDiagnosticRenderer, JsonDiagnosticRenderer};
use glyim_diag::{DiagSeverity, ErrorCategory, ErrorCode, GlyimDiagnostic, SubDiagnostic};
use glyim_span::{ByteIdx, FileId, MultiSpan, Span};

fn main() {
    let span = Span::new(
        FileId::from_raw(0),
        ByteIdx::from_raw(4),
        ByteIdx::from_raw(12),
        glyim_span::SyntaxContext::ROOT,
    );
    let diags = vec![
        GlyimDiagnostic::new(
            ErrorCode {
                category: ErrorCategory::Type,
                number: 1,
            },
            DiagSeverity::Error,
            "mismatched types: expected `i32`, found `&str`",
            MultiSpan::from_span(span),
        )
        .with_sub(SubDiagnostic {
            severity: DiagSeverity::Note,
            message: "expected because of the annotated type".into(),
            span: Some(MultiSpan::from_span(span)),
        })
        .with_source_code(Arc::from("let x: i32 = \"oops\";")),
        GlyimDiagnostic::warning(Span::DUMMY, "unused variable `y`"),
    ];

    // Runtime dispatch — the shape a `--error-format` flag desugars to.
    let human: &dyn DiagnosticRenderer = &HumanDiagnosticRenderer;
    let json: &dyn DiagnosticRenderer = &JsonDiagnosticRenderer;

    eprint!("{}", human.render(&diags));

    let mut out = String::new();
    let _ = writeln!(out, "{}", json.render(&diags).trim_end());
    print!("{out}");
}
