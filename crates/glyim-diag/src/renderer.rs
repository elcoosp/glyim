//! Pluggable diagnostic rendering (plan §3.4 / P1-O5).
//!
//! Rendering used to live at the edges with no shared contract: the CLI
//! carried a hand-rolled `--error-format=json` emitter inline (its schema
//! documented in a comment block), and every other consumer — the LSP's
//! severity/range mapping, `glyip`, out-of-tree harnesses — had to
//! re-derive the wire shape from that comment. This module is the seam:
//! a [`DiagnosticRenderer`] trait with the two formats the compiler ships
//! today ([`HumanDiagnosticRenderer`], [`JsonDiagnosticRenderer`]), so
//! the schema has exactly one definition and one test suite.
//!
//! A renderer owns *formatting only*. Stream routing stays with the
//! caller: the CLI writes JSON records to stdout (Rust's
//! `--message-format=json` convention) and human text to stderr, exactly
//! as before — the `render` output is byte-identical to what each site
//! produced inline.

use std::fmt::Write as _;

use crate::GlyimDiagnostic;

/// Formats a batch of diagnostics into a string, one record per line.
///
/// Implementations are pure functions of the batch (plan C6): the same
/// diagnostics render byte-identically on every call, in batch order.
/// The trait is object-safe so callers can pick the format at runtime
/// (`--error-format=json`-style flags) via `&dyn DiagnosticRenderer`.
pub trait DiagnosticRenderer {
    /// Render `diags` in batch order. Each record ends with `\n`; an
    /// empty batch renders to an empty string, so piping the output
    /// yields exactly one record per diagnostic.
    ///
    /// # Errors
    ///
    /// Infallible by contract: implementations must not fail on any
    /// well-formed [`GlyimDiagnostic`]. (Construction paths in
    /// `glyim-diag` already guarantee well-formed diagnostics; a renderer
    /// that cannot represent one must produce a best-effort record rather
    /// than an error.)
    #[must_use]
    fn render(&self, diags: &[GlyimDiagnostic]) -> String;
}

/// The `--error-format=human` renderer: one [`Display`](fmt::Display)
/// line per diagnostic (`[CODE] message @lo..hi`), matching the
/// compiler's human output byte-for-byte.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HumanDiagnosticRenderer;

impl DiagnosticRenderer for HumanDiagnosticRenderer {
    fn render(&self, diags: &[GlyimDiagnostic]) -> String {
        let mut out = String::new();
        for d in diags {
            let _ = writeln!(out, "{d}");
        }
        out
    }
}

/// The `--error-format=json` renderer: **JSON diagnostics schema v1**
/// (P1-O5, `KNOWN_GAPS` §5) — one JSON object per line on the caller's
/// stream of choice, suitable for editor/LSP consumption.
///
/// Wire shape (one object per diagnostic):
///
/// ```json
/// {"code":"T0001","message":"mismatched types","severity":"error",
///  "spans":[{"file":1,"hi":5,"lo":0}],"version":1}
/// ```
///
/// - `code` is [`ErrorCode`]'s `Display` spelling (unchanged).
/// - `severity` is lowercase (`error`/`warning`/`note`/`help`).
/// - `spans` lists the primary span first, then secondaries in
///   declaration order; `file` is the raw [`FileId`](glyim_span::FileId)
///   and `lo`/`hi` are byte offsets. Secondary-span labels are not part
///   of schema v1 (offsets only) — bump `version` on an incompatible
///   change.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JsonDiagnosticRenderer;

impl DiagnosticRenderer for JsonDiagnosticRenderer {
    fn render(&self, diags: &[GlyimDiagnostic]) -> String {
        let mut out = String::new();
        for d in diags {
            let spans: Vec<serde_json::Value> = std::iter::once(&d.span.primary)
                .chain(d.span.secondary.iter().map(|(s, _)| s))
                .map(|s| {
                    let r = s.range();
                    serde_json::json!({
                        "file": s.file.to_raw(),
                        "lo": r.start,
                        "hi": r.end,
                    })
                })
                .collect();
            let severity_str = match d.severity {
                crate::DiagSeverity::Error => "error",
                crate::DiagSeverity::Warning => "warning",
                crate::DiagSeverity::Note => "note",
                crate::DiagSeverity::Help => "help",
            };
            let obj = serde_json::json!({
                "version": 1,
                "code": d.code.to_string(),
                "severity": severity_str,
                "message": d.message,
                "spans": spans,
            });
            let _ = writeln!(out, "{obj}");
        }
        out
    }
}
