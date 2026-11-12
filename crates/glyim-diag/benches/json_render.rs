//! Diagnostic-renderer bench (diagnostic-renderer seam).
//!
//! Dependency-free (`harness = false`), mirroring the `glyim-ledger`
//! benches: measures both [`DiagnosticRenderer`] impls over a synthetic
//! 256-diagnostic batch (one primary + two secondary spans each, the
//! shape a non-exhaustive-match error produces), so CI logs can track
//! render cost as the diagnostic surface grows. Run with:
//!
//! ```text
//! cargo bench -p glyim-diag --bench json_render
//! ```

use std::hint::black_box;
use std::time::Instant;

use glyim_diag::renderer::{DiagnosticRenderer, HumanDiagnosticRenderer, JsonDiagnosticRenderer};
use glyim_diag::{DiagSeverity, ErrorCategory, ErrorCode, GlyimDiagnostic};
use glyim_span::{ByteIdx, FileId, MultiSpan, Span};

/// Builds a 256-diagnostic batch: every even index is a type error with
/// two secondary spans (the structured non-exhaustive-match shape), every
/// odd index a warning over a dummy span.
fn synthetic_batch() -> Vec<GlyimDiagnostic> {
    let primary = Span::new(
        FileId::from_raw(1),
        ByteIdx::ZERO,
        ByteIdx::from_raw(64),
        glyim_span::SyntaxContext::ROOT,
    );
    let secondary = Span::new(
        FileId::from_raw(1),
        ByteIdx::from_raw(80),
        ByteIdx::from_raw(144),
        glyim_span::SyntaxContext::ROOT,
    );
    let type_code = ErrorCode {
        category: ErrorCategory::Type,
        number: 50,
    };
    (0..256)
        .map(|i| {
            if i % 2 == 0 {
                GlyimDiagnostic::new(
                    type_code,
                    DiagSeverity::Error,
                    format!("non-exhaustive match: missing variants `V{i}`"),
                    MultiSpan::from_span(primary)
                        .with_secondary(secondary, "first defined here")
                        .with_secondary(primary, "pattern added here"),
                )
            } else {
                GlyimDiagnostic::warning(
                    Span::DUMMY,
                    format!("unused variable `x{i}` in function body"),
                )
            }
        })
        .collect()
}

fn main() {
    let diags = synthetic_batch();
    let json = JsonDiagnosticRenderer.render(black_box(&diags));
    let human = HumanDiagnosticRenderer.render(black_box(&diags));

    // Determinism: two renders are byte-identical (plan C6).
    assert_eq!(json, JsonDiagnosticRenderer.render(&diags));
    assert_eq!(human, HumanDiagnosticRenderer.render(&diags));
    // Contract pins: one JSON record per line, one human line per diag.
    assert_eq!(json.lines().count(), 256);
    assert_eq!(human.lines().count(), 256);
    assert!(json.starts_with("{\"code\":"));

    let started = Instant::now();
    for _ in 0..200 {
        black_box(JsonDiagnosticRenderer.render(black_box(&diags)));
    }
    let json_elapsed = started.elapsed();

    let started = Instant::now();
    for _ in 0..200 {
        black_box(HumanDiagnosticRenderer.render(black_box(&diags)));
    }
    let human_elapsed = started.elapsed();

    println!(
        "diag renderer bench: 256 diags x 200 iters\n  json : {json_elapsed:?} ({:.1} MB rendered)\n  human: {human_elapsed:?} ({:.1} MB rendered)",
        json.len() as f64 * 200.0 / (1024.0 * 1024.0),
        human.len() as f64 * 200.0 / (1024.0 * 1024.0),
    );
}
