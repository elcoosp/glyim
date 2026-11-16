//! Consumer walkthrough of the range-formatting seam (LSP-RANGE-FORMAT):
//! the `textDocument/rangeFormatting` request behind an editor's
//! "format selection" command.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lsp --example range_formatting_walkthrough
//! ```
//!
//! The example drives the public path end-to-end without a client
//! process: a document runs through the analysis driver's store (the
//! range-formatting engine reads the stored document text only — no HIR
//! or typeck pass is on its path), and `range_formatting` — the function
//! the router registers for `RangeFormatting` — turns the requested
//! selection into the wire `TextEdit`. Every offset is derived from the
//! document by line arithmetic, so the walkthrough cannot silently drift
//! from `DOC`, and the edits are applied to a working copy the way an
//! editor would.

use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::range_formatting::range_formatting;
use lsp_types::{DocumentRangeFormattingParams, Position, Range, TextDocumentIdentifier, Uri};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

/// The walkthrough document: one top-level statement, one compressed
/// function (its body and brace all on one line), one already-formatted
/// line — the shapes a selection spans.
const DOC: &str = "let a=1;\nfn main() {\n    {let b=2;let c=3;}\n    let d = 4;\n}\n";

fn main() {
    // Populate the database exactly like the analysis driver does: file
    // map and source map (the engine reads the stored text only).
    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let doc_path = PathBuf::from("/walkthrough/range_formatting.gly");
    let file_id = file_map.get_or_create(&doc_path);
    db.file_map.write().get_or_create(&doc_path);
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, DOC.to_string()));

    let uri = Uri::from_str(Url::from_file_path(&doc_path).unwrap().as_ref()).unwrap();
    let params_at = |start_line: u32, end_line: u32| DocumentRangeFormattingParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        range: Range {
            start: Position {
                line: start_line,
                character: 0,
            },
            end: Position {
                line: end_line,
                character: 0,
            },
        },
        options: lsp_types::FormattingOptions::default(),
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
    };

    // The selection: line 2 only — the compressed nested block. The edit
    // claims exactly that line span and re-indents it at the depth the
    // unselected prefix leaves.
    let selection =
        range_formatting(&db, &file_map, &params_at(2, 3)).expect("line 2 is compressed");
    println!("textDocument/rangeFormatting over line 2:");
    let edit = &selection[0];
    println!(
        "  edit claims lines {}..{} ({} bytes replaced)",
        edit.range.start.line,
        edit.range.end.line,
        edit.new_text.len(),
    );
    println!("  new text:");
    for line in edit.new_text.lines() {
        println!("    {line:?}");
    }

    // Apply the edit the way an editor does: replace the claimed span.
    let lo: usize = DOC
        .split_inclusive('\n')
        .take(edit.range.start.line as usize)
        .map(str::len)
        .sum();
    let hi = lo
        + DOC[lo..]
            .split_inclusive('\n')
            .take(1)
            .map(str::len)
            .sum::<usize>();
    let mut worked = DOC.to_string();
    worked.replace_range(lo..hi, &edit.new_text);
    println!("document after applying the edit:");
    for line in worked.lines() {
        println!("  {line}");
    }

    // Honesty pins: the unselected lines survive byte-for-byte, the
    // already-formatted line answers `null` (no edit churn), and the
    // whole-document request returns the same text the full-document
    // formatter would emit — one formatter, two wire shapes.
    assert!(
        worked.starts_with("let a=1;\nfn main() {\n") && worked.ends_with("    let d = 4;\n}\n"),
        "the unselected lines survive byte-for-byte"
    );
    assert!(
        range_formatting(&db, &file_map, &params_at(3, 4)).is_none(),
        "an already-formatted selection answers null"
    );
    let whole = range_formatting(&db, &file_map, &params_at(0, DOC.lines().count() as u32))
        .expect("the document has compressed lines");
    let full = glyim_lsp::formatting::format_document(
        &db,
        &lsp_types::DocumentFormattingParams {
            text_document: TextDocumentIdentifier { uri },
            options: lsp_types::FormattingOptions::default(),
            work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        },
    )
    .expect("the full formatter formats the document");
    assert_eq!(
        whole[0].new_text, full[0].new_text,
        "the range path and the full path are one formatter"
    );
    println!(
        "\nwalkthrough complete: the selection claimed its whole lines, composed with the prefix depth, and answered null where no edit was needed."
    );
}
