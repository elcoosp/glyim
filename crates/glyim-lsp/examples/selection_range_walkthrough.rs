//! Consumer walkthrough of the selection-range seam (LSP-SELECTION): the
//! `textDocument/selectionRange` capability behind an editor's
//! expand-selection.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lsp --example selection_range_walkthrough
//! ```
//!
//! The example drives the public path end-to-end without a client
//! process: a document is parsed with the same `parse_to_syntax` call the
//! analysis driver makes, the immutable green tree is stored in
//! `AnalysisDatabase::syntaxes` under the `FileMap`-allocated id exactly
//! like the driver stores it, and `selection_ranges` — the function the
//! router registers for `SelectionRangeRequest` — turns cursor positions
//! into the parent-linked chain the client walks outward on each expand:
//! token → statement → block → item → whole file. Consecutive duplicate
//! extents (rowan wrapper nodes that span exactly their only child) are
//! collapsed by the policy, so every step visibly widens. Every offset is
//! derived from the document by token search, so the walkthrough cannot
//! silently drift from `DOC`.

use glyim_frontend::parse_to_syntax;
use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::selection_range::selection_ranges;
use lsp_types::{Position, SelectionRangeParams, TextDocumentIdentifier};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

/// The walkthrough document: one function, a let statement with an
/// initializer, a call inside the initializer.
const DOC: &str = "fn main() {\n    let total = add(2, 3);\n    total\n}\n";

/// Byte offset of the first `needle` in DOC.
fn find(needle: &str) -> usize {
    DOC.find(needle)
        .unwrap_or_else(|| panic!("walkthrough fixture drifted: `{needle}` not found"))
}

/// Byte offset -> (line, utf16 character); the inverse of
/// `SourceMap::line_col_to_offset`, derived from `DOC` so the requests
/// cannot lie about columns.
fn line_col_of(offset: usize) -> (u32, u32) {
    let mut line = 0u32;
    let mut last_nl = 0usize;
    for (i, ch) in DOC.char_indices() {
        if i == offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            last_nl = i + 1;
        }
    }
    let character = DOC[last_nl..offset]
        .chars()
        .map(char::len_utf16)
        .sum::<usize>();
    (line, character as u32)
}

fn main() {
    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/walkthrough/selection.gly");
    // Both maps allocate ids from 0, so the db-side and caller-side ids
    // for `path` agree (same pattern as `test_utils::setup_test_db` and
    // the driver's own get_or_create).
    let file_id = db.file_map.write().get_or_create(&path);
    let caller_file_id = file_map.get_or_create(&path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");

    let parse_result = parse_to_syntax(DOC, file_id);
    assert!(
        parse_result.diagnostics.is_empty(),
        "walkthrough fixture must parse cleanly"
    );
    db.syntaxes.write().insert(file_id, parse_result.green_node);
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, DOC.to_string()));

    // Cursor on the `add` call inside the initializer, and on the binding
    // name `total`.
    let call_offset = find("add");
    let binding_offset = find("total");
    let params = SelectionRangeParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&path)
                .ok()
                .and_then(|u| lsp_types::Uri::from_str(u.as_str()).ok())
                .expect("walkthrough path converts to a URI"),
        },
        positions: [call_offset, binding_offset]
            .iter()
            .map(|&offset| {
                let (line, character) = line_col_of(offset);
                Position { line, character }
            })
            .collect(),
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };

    let ranges = selection_ranges(&db, &file_map, &params)
        .expect("both cursors sit on tokens of an open document");

    println!("document: {DOC:?}");
    println!("cursors : `add` @ byte {call_offset}, `total` @ byte {binding_offset}\n");
    for (label, range) in [
        ("expand-selection from `add`", &ranges[0]),
        ("expand-selection from `total` (binding)", &ranges[1]),
    ] {
        println!("{label}:");
        let mut current = Some(range);
        let mut step = 0;
        while let Some(selection) = current {
            let r = &selection.range;
            println!(
                "  {step:>2}. {}:{} .. {}:{}",
                r.start.line, r.start.character, r.end.line, r.end.character,
            );
            current = selection.parent.as_deref();
            step += 1;
        }
        println!();
    }
    println!(
        "Each step is one press of expand-selection: the chain walks the real \
         parse tree (token → statement → block → item → file), never guesses."
    );
}
