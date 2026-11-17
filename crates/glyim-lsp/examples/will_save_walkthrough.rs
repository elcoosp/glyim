//! Consumer walkthrough of the save-time edit seam (LSP-WILL-SAVE):
//! the `textDocument/willSaveWaitUntil` request behind a client's
//! format-on-save toggle.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lsp --example will_save_walkthrough
//! ```
//!
//! The example drives the public path end-to-end without a client
//! process: a document is stored in `AnalysisDatabase` exactly like the
//! analysis driver stores it (the save engine reads the stored text
//! only — no parse, HIR, or typeck pass is on its path), and
//! `will_save_edits` — the function the router registers for
//! `WillSaveWaitUntil` — turns the save into the wire `TextEdit`s.
//! Applying the answered edit to the stored text reproduces byte-for-byte
//! what `textDocument/formatting` would answer for the whole document:
//! one formatter, three wire shapes.

use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::will_save::will_save_edits;
use lsp_types::{TextDocumentIdentifier, TextDocumentSaveReason, Uri, WillSaveTextDocumentParams};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

/// The walkthrough document: a formatted head and tail around one
/// mis-indented middle line — the shape an editor hands the server on
/// save after a small edit left the document partially formatted.
const DOC: &str = "fn main() -> i32 {\n    let head = 1;\nlet broken = 2;\n    let tail = 3;\n}\n";

fn main() {
    // Populate the database exactly like the analysis driver does: file
    // map, source map.
    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let doc_path = PathBuf::from("/walkthrough/will_save.gly");
    let file_id = file_map.get_or_create(&doc_path);
    db.file_map.write().get_or_create(&doc_path);
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, DOC.to_string()));

    let uri = Uri::from_str(Url::from_file_path(&doc_path).unwrap().as_ref()).unwrap();
    let params = WillSaveTextDocumentParams {
        text_document: TextDocumentIdentifier { uri },
        reason: TextDocumentSaveReason::MANUAL,
    };

    // The request path: the edits the client applies before the file
    // hits the disk.
    let edits = will_save_edits(&db, &file_map, &params).expect("the middle line drifts");
    println!("will_save_walkthrough: {DOC:?}");
    for edit in &edits {
        println!(
            "  edit over line {}..{} → {:?}",
            edit.range.start.line, edit.range.end.line, edit.new_text
        );
    }

    // The save contract: applying the answered edit reproduces the
    // shared formatter core's whole-document answer byte-for-byte. The
    // edit claims whole lines, so the byte offsets are the summed line
    // lengths before the claimed span.
    let edit = &edits[0];
    let start: usize = DOC
        .lines()
        .take(edit.range.start.line as usize)
        .map(|line| line.len() + 1)
        .sum();
    let end: usize = DOC
        .lines()
        .take(edit.range.end.line as usize)
        .map(|line| line.len() + 1)
        .sum();
    let mut saved = String::with_capacity(DOC.len());
    saved.push_str(&DOC[..start]);
    saved.push_str(&edit.new_text);
    saved.push_str(&DOC[end.min(DOC.len())..]);
    let formatted = format_source_copy(DOC);
    println!("  saved:  {saved:?}");
    println!("  formatted by the shared core: {formatted:?}");
    assert_eq!(
        saved, formatted,
        "the save edits compose to the formatted document"
    );
    println!("  the save shape and the formatting shape are one formatter");
}

/// The whole-document answer of the shared formatter core (the same
/// policy `textDocument/formatting` serves, spelled out here so the
/// walkthrough stays dependency-free of crate-private items).
fn format_source_copy(source: &str) -> String {
    // `format_source` is `pub(crate)`; the walkthrough re-derives the
    // whole-document answer through the public range engine instead —
    // an empty claim answers the whole document at base depth 0.
    let mock = glyim_lsp::StoredRangeFormatSource::new(source);
    glyim_lsp::compute_range_edits(&mock, 0, source.len())
        .map(|edit| edit.new_text)
        .unwrap_or_else(|| source.to_string())
}
