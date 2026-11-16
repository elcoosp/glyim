//! Consumer walkthrough of the on-type-formatting seam
//! (LSP-ON-TYPE-FORMAT): the `textDocument/onTypeFormatting` capability
//! behind an editor's closing-bracket keystroke.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lsp --example on_type_formatting_walkthrough
//! ```
//!
//! The example drives the public path end-to-end without a client
//! process: a document with misindented closers is parsed (the engine
//! reads recorded parse trees only — no lowering or typeck pass is on
//! its path), the parse tree is stored in `AnalysisDatabase` exactly
//! like the driver stores it, and `on_type_formatting` — the function
//! the router registers for `OnTypeFormatting` — turns each closing
//! bracket keystroke into one whitespace-run edit that places the line
//! at the matching opener's indentation. Openers, unbalanced shapes,
//! family mismatches, and already-correct indents answer nothing. Every
//! offset is derived from the document by token search, so the
//! walkthrough cannot silently drift from `DOC`.

use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::on_type_formatting::on_type_formatting;
use lsp_types::{FormattingOptions, TextDocumentIdentifier, TextDocumentPositionParams};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

/// The walkthrough document: a nested block whose closers the editor's
/// auto-indent placed wrong (12 spaces for the inner one, 4 for the
/// outer one). The engine must dedent each to its opener line's indent.
///
/// ```text
/// fn main() -> i32 {      <- opener, indent ""
///     if flag {           <- opener, indent "    "
///         helper(0)
///             }           <- typed: 12-space run, opener says "    "
///     }                   <- typed: 4-space run, opener says ""
/// ```
const DOC: &str = "fn main() -> i32 {\n    if flag {\n        helper(0)\n            }\n    }\n";

/// Byte offset of the first `needle` in `src`.
fn offset_of(src: &str, needle: &str) -> usize {
    src.find(needle)
        .unwrap_or_else(|| panic!("fixture drifted: `{needle}` not in DOC"))
}

fn main() {
    // Populate the database exactly like the analysis driver does: file
    // map, source map, and the parse tree (green node).
    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let doc_path = PathBuf::from("/walkthrough/on_type.gly");
    let file_id = file_map.get_or_create(&doc_path);
    db.file_map.write().get_or_create(&doc_path);
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, DOC.to_string()));
    let green = glyim_frontend::parse_to_syntax(DOC, file_id).green_node;
    db.syntaxes.write().insert(file_id, green);

    // The request: exactly the params the router registers for
    // `OnTypeFormatting` — the position is *after* the typed character.
    let uri = Url::from_file_path(&doc_path)
        .ok()
        .and_then(|u| lsp_types::Uri::from_str(u.as_str()).ok())
        .expect("walkthrough path converts to a URI");
    let sm = SourceMap::new(file_id, DOC.to_string());
    let params_after = |typed_char: usize| {
        let ((line, col), _) = sm
            .span_to_position(typed_char, typed_char)
            .expect("fixture offset is char-boundary safe");
        lsp_types::DocumentOnTypeFormattingParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: lsp_types::Position {
                    line: line as u32,
                    character: col as u32 + 1,
                },
            },
            ch: "}".to_string(),
            options: FormattingOptions::default(),
        }
    };

    println!("document: {}", doc_path.display());

    // Keystroke 1: the inner `}` (12-space run). The matching opener is
    // the `if` line ("    "), so the edit dedents 12 → 4 spaces.
    let inner = offset_of(DOC, "            }") + 12;
    let edits =
        on_type_formatting(&db, &file_map, &params_after(inner)).expect("the inner closer dedents");
    println!(
        "inner `}}` keystroke: replace ({}:{})..({}:{}) with {:?}",
        edits[0].range.start.line,
        edits[0].range.start.character,
        edits[0].range.end.line,
        edits[0].range.end.character,
        edits[0].new_text,
    );

    // Keystroke 2: the outer `}` (4-space run). The matching opener is
    // the `fn` line (""), so the edit dedents 4 → 0.
    let outer = offset_of(DOC, "\n    }") + 5;
    let edits =
        on_type_formatting(&db, &file_map, &params_after(outer)).expect("the outer closer dedents");
    println!(
        "outer `}}` keystroke: replace ({}:{})..({}:{}) with {:?}",
        edits[0].range.start.line,
        edits[0].range.start.character,
        edits[0].range.end.line,
        edits[0].range.end.character,
        edits[0].new_text,
    );

    // Honesty pins: both edits spell the opener indents exactly, and the
    // opener keystroke (`{`) answers nothing — the capability fixes
    // closers only.
    assert_eq!(
        edits[0].new_text, "",
        "the fn body's closer sits at column 0"
    );
    let opener_params = lsp_types::DocumentOnTypeFormattingParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: lsp_types::Position {
                line: 0,
                character: 18,
            },
        },
        ch: "{".to_string(),
        options: FormattingOptions::default(),
    };
    assert!(
        on_type_formatting(&db, &file_map, &opener_params).is_none(),
        "openers carry no on-type edit"
    );
    println!("\nwalkthrough complete: closers align to their openers, openers stay inert.");
}
