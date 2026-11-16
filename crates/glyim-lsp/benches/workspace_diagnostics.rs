//! Workspace-diagnostics bench (LSP-WORKSPACE-DIAG).
//!
//! Dependency-free (`harness = false`), mirroring the `document_diagnostics`
//! bench: measures the workspace-diagnostics request path end-to-end over a
//! synthetic corpus — the composition policy (one report per analysed file
//! through the `AnalysedSetSource` seam) and the full wire request (recorded
//! stores → per-file reports) — so CI logs can track the per-request cost as
//! the analysed-file set grows. Run with:
//!
//! ```text
//! cargo bench -p glyim-lsp --bench workspace_diagnostics
//! ```

use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::workspace_diagnostics::{
    RecordedAnalysedSource, workspace_diagnostics, workspace_report,
};
use lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};

/// Half the corpus carries a recorded problem, so the answer has payload.
const FILES: usize = 32;
const DIAGS_PER_BROKEN_FILE: usize = 4;

fn main() {
    // Build the corpus: `FILES` analysed documents, every second one with
    // `DIAGS_PER_BROKEN_FILE` recorded diagnostics, stored exactly like the
    // analysis driver stores them (broken sets inserted, clean sets removed
    // — the driver's clearing contract).
    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    for i in 0..FILES {
        let broken = i % 2 == 0;
        let path = PathBuf::from(format!("/bench/ws_file_{i}.g"));
        let file_id = db.file_map.write().get_or_create(&path);
        let caller_file_id = file_map.get_or_create(&path);
        assert_eq!(file_id, caller_file_id, "file ids must agree");
        db.source_maps.write().insert(
            file_id,
            SourceMap::new(file_id, "fn main() -> i32 {\n    42\n}\n".to_string()),
        );
        db.versions.write().insert(file_id, i as i64);
        if broken {
            let recorded: Vec<Diagnostic> = (0..DIAGS_PER_BROKEN_FILE)
                .map(|d| Diagnostic {
                    range: Range {
                        start: Position {
                            line: d as u32,
                            character: 0,
                        },
                        end: Position {
                            line: d as u32,
                            character: 1,
                        },
                    },
                    severity: Some(DiagnosticSeverity::WARNING),
                    source: Some("glyim".to_string()),
                    message: format!("synthetic warning {i}/{d}"),
                    ..Diagnostic::default()
                })
                .collect();
            db.diagnostics.write().insert(file_id, recorded);
        }
    }

    // Policy bench: the workspace composition over the recorded stores
    // through the seam, no re-sorting surprises — the unit the policy
    // tests pin. The guards live in this block only, so the wire bench
    // below starts from a lock-free database.
    {
        let analysed = db.source_maps.read();
        let versions = db.versions.read();
        let recorded = db.diagnostics.read();
        let source = RecordedAnalysedSource::new(&file_map, &analysed, &versions, &recorded);
        let t0 = Instant::now();
        let mut reports = 0usize;
        for _ in 0..1_000 {
            let report = workspace_report(&source);
            reports += report.items.len();
            black_box(&report);
        }
        let policy = t0.elapsed();
        println!(
            "policy compute : {:>9.1?}/call (1,000 calls, {reports} reports total)",
            policy / 1_000
        );
    };

    // Wire bench: the full request path through a real database, exactly
    // as the router handler drives it (recorded stores → per-file full
    // reports, ascending file id). The params are ignored by design
    // (`previous_result_ids` never honored); a default set is the honest
    // wire shape.
    let params = lsp_types::WorkspaceDiagnosticParams {
        identifier: None,
        previous_result_ids: Vec::new(),
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };
    let t1 = Instant::now();
    let mut wire_files = 0usize;
    for _ in 0..1_000 {
        let response = workspace_diagnostics(&db, &file_map, &params);
        if let lsp_types::WorkspaceDiagnosticReportResult::Report(report) = &response {
            wire_files += report.items.len();
        }
        black_box(&response);
    }
    let wire = t1.elapsed();
    println!(
        "wire request   : {:>9.1?}/call (1,000 calls, {wire_files} file reports total)",
        wire / 1_000
    );

    black_box(wire_files);
}
