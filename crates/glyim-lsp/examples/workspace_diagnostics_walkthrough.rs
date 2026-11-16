//! Consumer walkthrough of the workspace-diagnostics seam
//! (LSP-WORKSPACE-DIAG): the `workspace/diagnostic` capability — one
//! full report per analysed file, composed from the same recorded sets
//! the push path (`publishDiagnostics`) publishes and the per-file pull
//! (`textDocument/diagnostic`) answers.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lsp --example workspace_diagnostics_walkthrough
//! ```
//!
//! The example drives the public path end-to-end without a client
//! process: two documents run through the same parse pipeline the
//! analysis driver runs, their diagnostics are converted through the
//! same seam the driver's push path uses and stored in
//! `AnalysisDatabase` exactly like the driver stores them, and their
//! `didChange` versions are recorded exactly like the driver records
//! them. `workspace_diagnostics` — the function the router registers
//! for `WorkspaceDiagnosticRequest` — answers one full report per
//! analysed file, ascending file id. A clean analysed file answers an
//! *empty* full report inside the workspace answer (the protocol's
//! "checked, nothing to report"), and an empty session answers an
//! empty `items` array — the workspace pull is always answerable,
//! unlike the per-file pull whose unknown-document case is a JSON-RPC
//! error.

use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::diagnostics::convert_diagnostics;
use glyim_lsp::workspace_diagnostics::workspace_diagnostics;
use lsp_types::WorkspaceDiagnosticParams;
use std::path::PathBuf;

/// A source with one unmistakable parse error and a clean one beside it.
const BROKEN_DOC: &str = "fn broken( {\n}\n";
const CLEAN_DOC: &str = "fn main() -> i32 {\n    42\n}\n";

fn main() {
    // An empty session: the workspace pull is answerable even with
    // nothing analysed.
    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let params = WorkspaceDiagnosticParams {
        identifier: None,
        previous_result_ids: Vec::new(),
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };
    match workspace_diagnostics(&db, &file_map, &params) {
        lsp_types::WorkspaceDiagnosticReportResult::Report(report) => println!(
            "empty session → {} report item(s) (the workspace pull is always answerable)\n",
            report.items.len()
        ),
        other => println!("empty session → unexpected response {other:?}\n"),
    }

    // Record both documents exactly like the driver's analysis step:
    // parse through the real frontend, convert through the shared seam,
    // store the LSP set (empty sets are removed — the driver's clearing
    // contract), record the client version exactly like the driver
    // records it.
    for (name, source, version) in [
        ("/walkthrough/ws_broken.g", BROKEN_DOC, 7_i64),
        ("/walkthrough/ws_clean.g", CLEAN_DOC, 8_i64),
    ] {
        let path = PathBuf::from(name);
        let file_id = db.file_map.write().get_or_create(&path);
        let caller_file_id = file_map.get_or_create(&path);
        assert_eq!(file_id, caller_file_id, "file ids must agree");
        db.source_maps
            .write()
            .insert(file_id, SourceMap::new(file_id, source.to_string()));
        db.versions.write().insert(file_id, version);
        let parse_result = glyim_frontend::parse_to_syntax(source, file_id);
        let sm = db.source_maps.read().get(&file_id).unwrap().clone();
        let diags = convert_diagnostics(file_id, &sm, &parse_result.diagnostics);
        if diags.is_empty() {
            db.diagnostics.write().remove(&file_id);
        } else {
            db.diagnostics.write().insert(file_id, diags);
        }
    }

    let result = workspace_diagnostics(&db, &file_map, &params);
    match result {
        lsp_types::WorkspaceDiagnosticReportResult::Report(report) => {
            println!("populated session → {} report item(s):", report.items.len());
            for item in &report.items {
                let lsp_types::WorkspaceDocumentDiagnosticReport::Full(full) = item else {
                    println!(
                        "  - an unchanged report (unexpected: this server never issues result ids)"
                    );
                    continue;
                };
                let items = &full.full_document_diagnostic_report.items;
                println!(
                    "  - {} @ version {:?}: {} item(s)",
                    full.uri.as_str(),
                    full.version,
                    items.len()
                );
                for entry in items {
                    println!(
                        "      {}:{} .. {}:{} ({:?}) {}",
                        entry.range.start.line,
                        entry.range.start.character,
                        entry.range.end.line,
                        entry.range.end.character,
                        entry.severity,
                        entry.message,
                    );
                }
            }
            println!(
                "\n(the reports carry the same sets and coordinates the per-file pull and \
                 the push path publish — reports arrive in ascending file id, so repeated \
                 polls are mutually comparable)"
            );
        }
        other => println!("populated session → unexpected response {other:?}"),
    }
}
