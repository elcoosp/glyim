//! Consumer walkthrough of the related-documents fan-out
//! (LSP-RELATED-DIAG): `textDocument/diagnostic` answering not just the
//! pulled file's recorded set but also a `relatedDocuments` report per
//! file the dependency graph says is *affected by* it — the transitive
//! dependents the driver re-analyses when the pulled file changes.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lsp --example related_diagnostics_walkthrough
//! ```
//!
//! The example drives the public path end-to-end without a client
//! process: two documents run through the same parse pipeline the
//! analysis driver runs, their diagnostics are converted through the
//! same seam the driver's push path uses, and a dependency edge is
//! recorded exactly like the driver's `extract_dependencies` records
//! one (`dependent depends on dependency`). Pulling the *dependency*
//! fans out to the dependent; pulling the dependent answers without a
//! `relatedDocuments` key at all — the exact shape the pull path
//! answered before the fan-out existed. Every offset is derived from
//! the documents by the shared conversion seam, so the walkthrough
//! cannot silently drift from its fixtures.

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
        ("/walkthrough/related_dependency.g", BROKEN_DOC),
        ("/walkthrough/related_dependent.g", CLEAN_DOC),
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

    // The driver's recorded edge shape: `file depends on dep`. The
    // dependent imports the dependency, so a change to the dependency
    // can change the dependent's set — which is what makes the
    // dependent a related document for pulls of the dependency.
    let dependency = PathBuf::from("/walkthrough/related_dependency.g");
    let dependent = PathBuf::from("/walkthrough/related_dependent.g");
    db.dep_graph
        .write()
        .add_dep(dependent.clone(), dependency.clone());

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

    println!("dependency document: {BROKEN_DOC:?}\n");
    let params = params_for(dependency.to_str().expect("utf-8 path"));
    match pull_diagnostics(&db, &file_map, &params) {
        Some(lsp_types::DocumentDiagnosticReportResult::Report(
            lsp_types::DocumentDiagnosticReport::Full(full),
        )) => {
            let items = &full.full_document_diagnostic_report.items;
            println!(
                "pull on the dependency → full report with {} item(s), {} related report(s):",
                items.len(),
                full.related_documents.as_ref().map_or(0, |m| m.len())
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
            for (uri, report) in full.related_documents.unwrap_or_default() {
                match report {
                    lsp_types::DocumentDiagnosticReportKind::Full(related_full) => println!(
                        "  → related {} : {} item(s) (the dependent's own pull answer)",
                        uri.as_str(),
                        related_full.items.len()
                    ),
                    other => println!("  → related {} : unexpected {other:?}", uri.as_str()),
                }
            }
            println!();
        }
        Some(other) => println!("pull on the dependency → unexpected response {other:?}\n"),
        None => println!("pull on the dependency → null (unexpected)\n"),
    }

    println!("dependent document: {CLEAN_DOC:?}\n");
    let params = params_for(dependent.to_str().expect("utf-8 path"));
    match pull_diagnostics(&db, &file_map, &params) {
        Some(lsp_types::DocumentDiagnosticReportResult::Report(
            lsp_types::DocumentDiagnosticReport::Full(full),
        )) => println!(
            "pull on the dependent → full report with {} item(s), {} related report(s) \
             (nothing depends on the dependent, so the answer carries no \
             `relatedDocuments` key at all — the pre-fan-out wire shape)",
            full.full_document_diagnostic_report.items.len(),
            full.related_documents.as_ref().map_or(0, |m| m.len())
        ),
        Some(other) => println!("pull on the dependent → unexpected response {other:?}\n"),
        None => println!("pull on the dependent → null (unexpected)\n"),
    }
}
