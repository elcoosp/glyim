use crate::renderer::{DiagnosticRenderer, HumanDiagnosticRenderer, JsonDiagnosticRenderer};
use crate::*;
use glyim_span::{ByteIdx, FileId, Span};

use std::sync::Arc;

/// A type-error diagnostic over `src.g` offsets 0..5, plus a secondary
/// span 10..20 — the smallest diagnostic that exercises every JSON field.
fn sample_error() -> GlyimDiagnostic {
    let primary = Span::new(
        FileId::from_raw(1),
        ByteIdx::ZERO,
        ByteIdx::from_raw(5),
        glyim_span::SyntaxContext::ROOT,
    );
    let secondary = Span::new(
        FileId::from_raw(1),
        ByteIdx::from_raw(10),
        ByteIdx::from_raw(20),
        glyim_span::SyntaxContext::ROOT,
    );
    GlyimDiagnostic::new(
        ErrorCode {
            category: ErrorCategory::Type,
            number: 1,
        },
        DiagSeverity::Error,
        "mismatched types",
        MultiSpan::from_span(primary).with_secondary(secondary, "first defined here"),
    )
    .with_source_code(Arc::from("let x: i32 = \"s\";"))
}

/// T0001: one JSON object per line, in batch order.
#[test]
fn json_renders_one_object_per_line() {
    let diags = vec![sample_error(), sample_error()];
    let out = JsonDiagnosticRenderer.render(&diags);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2);
    for line in &lines {
        let parsed: serde_json::Value =
            serde_json::from_str(line).expect("each line is valid JSON");
        assert_eq!(parsed["version"], serde_json::json!(1));
        assert_eq!(parsed["code"], serde_json::json!("T0001"));
        assert_eq!(parsed["severity"], serde_json::json!("error"));
        assert_eq!(parsed["message"], serde_json::json!("mismatched types"));
    }
}

/// T0001: the exact wire shape — one record per diagnostic, spans as
/// `{file, lo, hi}` objects with the primary span first. Pinned literally
/// so an accidental schema change is a loud test failure, not a silent
/// editor-facing drift.
#[test]
fn json_matches_schema_v1_golden() {
    let out = JsonDiagnosticRenderer.render(&[sample_error()]);
    assert_eq!(
        out,
        "{\"code\":\"T0001\",\"message\":\"mismatched types\",\"severity\":\"error\",\
         \"spans\":[{\"file\":1,\"hi\":5,\"lo\":0},{\"file\":1,\"hi\":20,\"lo\":10}],\
         \"version\":1}\n"
    );
}

/// T0001: spans carry the primary span first, then secondaries in
/// declaration order (the CLI's `print_diagnostics` contract).
#[test]
fn json_spans_include_secondary_in_order() {
    let out = JsonDiagnosticRenderer.render(&[sample_error()]);
    let parsed: serde_json::Value = serde_json::from_str(out.trim_end()).expect("valid JSON");
    let spans = parsed["spans"].as_array().expect("spans array");
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0]["lo"], serde_json::json!(0));
    assert_eq!(spans[0]["hi"], serde_json::json!(5));
    assert_eq!(spans[1]["lo"], serde_json::json!(10));
    assert_eq!(spans[1]["hi"], serde_json::json!(20));
}

/// T0001: severity serialises lowercase (error/warning/note/help), not
/// Rust's `Debug` spelling.
#[test]
fn json_severity_is_lowercase() {
    let span = Span::DUMMY;
    let batches = [
        (DiagSeverity::Error, "error"),
        (DiagSeverity::Warning, "warning"),
        (DiagSeverity::Note, "note"),
        (DiagSeverity::Help, "help"),
    ];
    for (severity, expected) in batches {
        let diag = GlyimDiagnostic::new(
            ErrorCode {
                category: ErrorCategory::Type,
                number: 1,
            },
            severity,
            "m",
            MultiSpan::from_span(span),
        );
        let out = JsonDiagnosticRenderer.render(std::slice::from_ref(&diag));
        let parsed: serde_json::Value = serde_json::from_str(out.trim_end()).expect("valid JSON");
        assert_eq!(parsed["severity"], serde_json::json!(expected));
    }
}

/// T0001: an empty batch renders to an empty string (no trailing blank
/// line), so piping stdout yields zero records.
#[test]
fn json_empty_batch_is_empty() {
    assert_eq!(JsonDiagnosticRenderer.render(&[]), "");
}

/// Human rendering keeps the `Display` line shape: `[CODE] message
/// @lo..hi`, one line per diagnostic.
#[test]
fn human_matches_display_lines() {
    let out = HumanDiagnosticRenderer.render(&[sample_error()]);
    assert_eq!(out, "[T0001] mismatched types @0..5\n");
}

/// The seam is object-safe: a caller can pick the renderer at runtime
/// (`--error-format=json` style) through `&dyn DiagnosticRenderer`.
#[test]
fn renders_dispatch_through_trait_object() {
    let diags = vec![sample_error()];
    let human: &dyn DiagnosticRenderer = &HumanDiagnosticRenderer;
    let json: &dyn DiagnosticRenderer = &JsonDiagnosticRenderer;
    assert_eq!(human.render(&diags), format!("{}\n", diags[0]));
    assert!(json.render(&diags).starts_with("{\"code\":"));
}

/// Rendering is a pure function of the batch: two calls are byte-identical
/// (plan C6 — deterministic by construction).
#[test]
fn renders_are_deterministic() {
    let diags = vec![sample_error(), sample_error()];
    assert_eq!(
        JsonDiagnosticRenderer.render(&diags),
        JsonDiagnosticRenderer.render(&diags)
    );
    assert_eq!(
        HumanDiagnosticRenderer.render(&diags),
        HumanDiagnosticRenderer.render(&diags)
    );
}
