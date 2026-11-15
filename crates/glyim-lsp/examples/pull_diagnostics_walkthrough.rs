//! Consumer walkthrough of the pull-diagnostics seam (LSP-PULL-DIAG): the
//! `textDocument/diagnostic` capability — the same diagnostic set the
//! push path (`publishDiagnostics`) publishes, answered on demand.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lsp --example pull_diagnostics_walkthrough
//! ```
//!
//! The example drives the public path end-to-end without a client
//! process: one document runs through the same parse pipeline the
//! analysis driver runs, its diagnostics are converted through the same
//! seam the driver's push path uses and stored in `AnalysisDatabase`
//! exactly like the driver stores them, and `pull_diagnostics` — the
//! function the router registers for `DocumentDiagnosticRequest` —
//! answers the full report. A clean analysed file answers an *empty*
//! full report (the protocol's "checked, nothing to report"), and a
//! document the server never analysed answers `null`. Every offset is
//! derived from the document by token search, so the walkthrough cannot
//! silently drift from its fixtures.

use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::diagnostics::convert_diagnostics;
use glyim_lsp::pull_diagnostics::pull_diagnostics;
use lsp_types::{DocumentDiagnosticParams, TextDocumentIdentifier};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

/// A source with one unmistakable parse error and a clean one beside it.
const BROKEN_DOC: &str = "fn broken( {\n}\n";
const CLEAN_DOC: &str = "fn main() -> i32 {\n    42\n}\n";

fn main() {
    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();

    // Record both documents exactly like the driver's analysis step:
    // parse through the real frontend, convert through the shared seam,
    // store the LSP set (empty sets are removed — the driver's clearing
    // contract).
    for (name, source) in [
        ("/walkthrough/pull_broken.g", BROKEN_DOC),
        ("/walkthrough/pull_clean.g", CLEAN_DOC),
    ] {
        let path = PathBuf::from(name);
        let file_id = db.file_map.write().get_or_create(&path);
        let caller_file_id = file_map.get_or_create(&path);
        assert_eq!(file_id, caller_file_id, "file ids must agree");
        db.source_maps
            .write()
            .insert(file_id, SourceMap::new(file_id, source.to_string()));
        let parse_result = glyim_frontend::parse_to_syntax(source, file_id);
        let sm = db.source_maps.read().get(&file_id).unwrap().clone();
        let diags = convert_diagnostics(file_id, &sm, &parse_result.diagnostics);
        if diags.is_empty() {
            db.diagnostics.write().remove(&file_id);
        } else {
            db.diagnostics.write().insert(file_id, diags);
        }
    }

    let params_for = |name: &str| DocumentDiagnosticParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(name)
                .ok()
                .and_then(|u| lsp_types::Uri::from_str(u.as_str()).ok())
                .expect("walkthrough path converts to a URI"),
        },
        identifier: None,
        previous_result_id: None,
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };

    println!("broken document: {BROKEN_DOC:?}\n");
    let params = params_for("/walkthrough/pull_broken.g");
    match pull_diagnostics(&db, &file_map, &params) {
        Some(lsp_types::DocumentDiagnosticReportResult::Report(
            lsp_types::DocumentDiagnosticReport::Full(full),
        )) => {
            let items = &full.full_document_diagnostic_report.items;
            println!(
                "pull on the broken document → full report with {} item(s):",
                items.len()
            );
            for item in items {
                println!(
                    "  - {}:{} .. {}:{} ({:?}) {}",
                    item.range.start.line,
                    item.range.start.character,
                    item.range.end.line,
                    item.range.end.character,
                    item.severity,
                    item.message,
                );
            }
            println!();
        }
        Some(other) => println!("pull on the broken document → unexpected response {other:?}\n"),
        None => println!("pull on the broken document → null (unexpected)\n"),
    }

    println!("clean document: {CLEAN_DOC:?}\n");
    let params = params_for("/walkthrough/pull_clean.g");
    match pull_diagnostics(&db, &file_map, &params) {
        Some(lsp_types::DocumentDiagnosticReportResult::Report(
            lsp_types::DocumentDiagnosticReport::Full(full),
        )) => println!(
            "pull on the clean document → full report with {} item(s) \
             (an empty full report is the protocol's \"checked, nothing to report\")\n",
            full.full_document_diagnostic_report.items.len()
        ),
        Some(other) => println!("pull on the clean document → unexpected response {other:?}\n"),
        None => println!("pull on the clean document → null (unexpected)\n"),
    }

    let params = params_for("/walkthrough/never-opened.g");
    match pull_diagnostics(&db, &file_map, &params) {
        Some(_) => println!("pull on a never-analysed document → unexpected non-null"),
        None => println!(
            "pull on a never-analysed document → null (the router maps this to a \
             JSON-RPC error; pull's result is a report, not an optional one)"
        ),
    }
}
