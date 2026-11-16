//! Consumer walkthrough of the document-link seam (LSP-DOC-LINK): the
//! `textDocument/documentLink` capability behind an editor's ctrl-click
//! on an include-macro path.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lsp --example document_links_walkthrough
//! ```
//!
//! The example drives the public path end-to-end without a client
//! process: a document runs through the same parse step the analysis
//! driver runs (the link engine reads recorded parse trees only — no
//! lowering or typeck pass is on its path), the results are stored in
//! `AnalysisDatabase` exactly like the driver stores them, and
//! `document_links` — the function the router registers for
//! `DocumentLinkRequest` — turns every include-macro literal into a
//! clickable link to the resolved file, while non-file macros (`env!`)
//! and non-literal calls (`include!(concat!(…))`) answer nothing. Every
//! offset is derived from the document by token search, so the
//! walkthrough cannot silently drift from `DOC`.

use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::document_link::document_links;
use lsp_types::{DocumentLinkParams, PartialResultParams, TextDocumentIdentifier};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

/// The walkthrough document: two include-macro calls, a non-file macro,
/// and a composed (non-literal) call — the shape an editor user would
/// hover over.
const DOC: &str = "include_str!(\"inc/header.g\");\n\
                   include_bytes!(\"inc/data.bin\");\n\
                   fn main() -> i32 {\n\
                   env!(\"PATH\");\n\
                   include!(concat!(\"a\", \"b\"));\n\
                   0\n\
                   }\n";

fn main() {
    // The document lives in a throwaway directory, so the resolved
    // targets are real absolute paths the same way an editor session's
    // are. The included files do not have to exist: a link to a
    // not-yet-created file stays clickable (the editor reports the
    // missing file on click, which is the honest answer).
    let dir = tempfile::tempdir().expect("walkthrough temp dir");
    let doc_path: PathBuf = dir.path().join("doc.g");

    // Populate the database exactly like the analysis driver does: file
    // map, source map, and the parse tree (green node).
    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let file_id = file_map.get_or_create(&doc_path);
    db.file_map.write().get_or_create(&doc_path);
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, DOC.to_string()));
    let green = glyim_frontend::parse_to_syntax(DOC, file_id).green_node;
    db.syntaxes.write().insert(file_id, green);

    // The request: exactly the params the router registers for
    // `DocumentLinkRequest`.
    let uri = Url::from_file_path(&doc_path)
        .ok()
        .and_then(|u| lsp_types::Uri::from_str(u.as_str()).ok())
        .expect("walkthrough path converts to a URI");
    let params = DocumentLinkParams {
        text_document: TextDocumentIdentifier { uri },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let links = document_links(&db, &file_map, &params).expect("walkthrough doc carries links");

    // The literals in source order — links answer in the same order, so
    // pairing them by index is exact.
    let literal_texts = ["\"inc/header.g\"", "\"inc/data.bin\""];

    println!("document: {}", doc_path.display());
    println!("resolved {} document link(s):", links.len());
    for (link, literal) in links.iter().zip(literal_texts) {
        // The target maps back to a real path through the same URI
        // machinery the request path used to build it.
        let target = link
            .target
            .as_ref()
            .and_then(|uri| Url::parse(uri.as_str()).ok())
            .and_then(|url| url.to_file_path().ok());
        println!(
            "  literal {literal} (line {}), target: {} [{}]",
            link.range.start.line,
            target
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "(unmapped)".to_string()),
            link.tooltip.as_deref().unwrap_or("(no tooltip)"),
        );
    }

    // Honesty pins: the non-file macro and the composed call answered
    // nothing — guessing their targets would be wrong, so they are not
    // links at all — and the two real targets resolve under the
    // document's directory.
    assert_eq!(links.len(), 2, "env!/include!(concat!) must not link");
    assert_eq!(
        links
            .iter()
            .filter_map(|l| l
                .target
                .as_ref()
                .and_then(|uri| Url::parse(uri.as_str()).ok())
                .and_then(|url| url.to_file_path().ok()))
            .collect::<Vec<PathBuf>>(),
        vec![
            dir.path().join("inc/header.g"),
            dir.path().join("inc/data.bin")
        ],
        "targets resolve under the document's directory"
    );
    println!("\nwalkthrough complete: include literals link, non-file macros stay inert.");
}
