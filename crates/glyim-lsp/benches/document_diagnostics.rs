//! Pull-diagnostics bench (LSP-PULL-DIAG).
//!
//! Dependency-free (`harness = false`), mirroring the `server_log` and
//! `type_definition` benches: measures the pull-diagnostics request path
//! end-to-end over a synthetic corpus — the report policy (set assembly
//! through the `DiagnosticSource` seam) and the full wire request (URI →
//! open-file lookup → recorded set → full report) — so CI logs can track
//! the per-request cost as the recorded-diagnostics store grows
//! consumers. Run with:
//!
//! ```text
//! cargo bench -p glyim-lsp --bench document_diagnostics
//! ```

use std::hint::black_box;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Instant;

use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::pull_diagnostics::{RecordedDiagnosticSource, pull_diagnostics, pull_report};
use lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range, TextDocumentIdentifier};
use url::Url;

/// One unit with a recorded problem, so the report carries real payload.
const UNITS: usize = 64;

fn main() {
    // Build the corpus: one document whose recorded set carries `UNITS`
    // diagnostics, stored exactly like the analysis driver stores them.
    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/bench/pull_diag.g");
    let file_id = db.file_map.write().get_or_create(&path);
    let caller_file_id = file_map.get_or_create(&path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");
    db.source_maps.write().insert(
        file_id,
        SourceMap::new(file_id, "fn main() -> i32 {\n    42\n}\n".to_string()),
    );

    let recorded: Vec<Diagnostic> = (0..UNITS)
        .map(|i| Diagnostic {
            range: Range {
                start: Position {
                    line: i as u32,
                    character: 0,
                },
                end: Position {
                    line: i as u32,
                    character: 1,
                },
            },
            severity: Some(DiagnosticSeverity::WARNING),
            source: Some("glyim".to_string()),
            message: format!("synthetic warning {i}"),
            ..Diagnostic::default()
        })
        .collect();
    db.diagnostics.write().insert(file_id, recorded);

    // Policy bench: the report assembly over the recorded set through the
    // seam, no database locks — the unit the policy tests pin. The guard
    // lives in this block only, so the wire bench below starts from a
    // lock-free database.
    {
        let recorded_map = db.diagnostics.read();
        let source_impl = RecordedDiagnosticSource::new(&recorded_map);
        let t0 = Instant::now();
        let mut items = 0usize;
        for _ in 0..1_000 {
            let report = pull_report(&source_impl, file_id, None);
            if let lsp_types::DocumentDiagnosticReportKind::Full(full) = &report {
                items += full.items.len();
            }
            black_box(&report);
        }
        let policy = t0.elapsed();
        println!(
            "policy compute : {:>9.1?}/call (1,000 calls, {items} items total)",
            policy / 1_000
        );
    };

    // Wire bench: the full request path through a real database, exactly
    // as the router handler drives it (URI → open-file lookup → recorded
    // set → full report).
    let params = lsp_types::DocumentDiagnosticParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&path)
                .ok()
                .and_then(|u| lsp_types::Uri::from_str(u.as_str()).ok())
                .expect("bench fixture path converts to a URI"),
        },
        identifier: None,
        previous_result_id: None,
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };
    let t1 = Instant::now();
    let mut wire_hits = 0usize;
    for _ in 0..1_000 {
        let response = pull_diagnostics(&db, &file_map, &params).expect("request hits the file");
        wire_hits += 1;
        black_box(&response);
    }
    let wire = t1.elapsed();
    println!(
        "wire request   : {:>9.1?}/call (1,000 calls, {wire_hits} resolved)",
        wire / 1_000
    );

    black_box(wire_hits);
}
