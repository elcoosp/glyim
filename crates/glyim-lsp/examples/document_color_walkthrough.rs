//! Consumer walkthrough of the document-color seam (LSP-COLOR): the
//! `textDocument/documentColor` listing behind an editor's inline color
//! swatches, and the `textDocument/colorPresentation` echo behind the
//! picker's "replace with this color" action.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lsp --example document_color_walkthrough
//! ```
//!
//! The example drives the public path end-to-end without a client
//! process: a styled document is stored in `AnalysisDatabase` exactly
//! like the analysis driver stores it (file map, source map, and the
//! parse tree's green node — the color engines read the stored tree only,
//! no parse/HIR/typeck pass is on the request path, so swatches answer on
//! broken code too), and the two entry points the router registers for
//! `DocumentColor` / `ColorPresentationRequest` — `document_colors` and
//! `color_presentations` — turn the file into swatch sites and the picked
//! color into the canonical splice edit.

use glyim_frontend::parse_to_syntax;
use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::document_color::{color_presentations, document_colors};
use glyim_span::FileId;
use lsp_types::{
    Color, ColorPresentationParams, DocumentColorParams, PartialResultParams,
    TextDocumentIdentifier, Uri, WorkDoneProgressParams,
};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

/// The walkthrough document: two color sites (one six-digit, one
/// shorthand) around one non-color string — the shape a GSX styling file
/// hands the server.
const DOC: &str = "fn main() -> i32 {\n    let fill = \"#ff00aa\";\n    let name = \"not-a-color\";\n    let theme = \"#f0a\";\n    0\n}\n";

fn main() {
    // Populate the database exactly like the analysis driver does: file
    // map, source map, parse tree.
    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let doc_path = PathBuf::from("/walkthrough/document_color.gly");
    let file_id = file_map.get_or_create(&doc_path);
    db.file_map.write().get_or_create(&doc_path);
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, DOC.to_string()));
    let green = parse_to_syntax(DOC, file_id).green_node;
    db.syntaxes.write().insert(file_id, green);

    let uri = Uri::from_str(Url::from_file_path(&doc_path).unwrap().as_ref()).unwrap();

    // The listing path: the swatch sites the editor renders inline. The
    // range is the payload extent (quotes excluded) — the swatch sits on
    // the color characters.
    let colors = document_colors(
        &db,
        &file_map,
        &DocumentColorParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        },
    )
    .expect("the document carries color sites");
    println!("document_color_walkthrough: {DOC:?}");
    for color in &colors {
        println!(
            "  swatch over {}..{} — r={:.3} g={:.3} b={:.3} a={:.3}",
            color.range.start.character,
            color.range.end.character,
            color.color.red,
            color.color.green,
            color.color.blue,
            color.color.alpha
        );
    }

    // The picker echo: the user recolors the first site; the request
    // carries the new value over the range the listing reported.
    let first = &colors[0];
    let presentations = color_presentations(
        &db,
        &file_map,
        &ColorPresentationParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            color: Color {
                red: 63.0 / 255.0,
                green: 167.0 / 255.0,
                blue: 214.0 / 255.0,
                alpha: 1.0,
            },
            range: first.range,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        },
    )
    .expect("the range maps onto a recorded site");
    let presentation = &presentations[0];
    println!(
        "  picker echo: label {} replaces {}..{}",
        presentation.label, first.range.start.character, first.range.end.character
    );

    // The splice: applying the answered edit re-spells the site in place.
    let edit = presentation.text_edit.as_ref().expect("the splice edit");
    let mut recolored = DOC.to_string();
    let sm = SourceMap::new(FileId::from_raw(0), DOC.to_string());
    let start = sm
        .line_col_to_offset(
            edit.range.start.line as usize,
            edit.range.start.character as usize,
        )
        .expect("the edit start maps");
    let end = sm
        .line_col_to_offset(
            edit.range.end.line as usize,
            edit.range.end.character as usize,
        )
        .expect("the edit end maps");
    recolored.replace_range(start..end, &edit.new_text);
    println!("  recolored: {recolored:?}");
    assert!(
        recolored.contains("\"#3fa7d6\""),
        "the site re-spelled canonically"
    );
    println!("  one notation, one presentation: the hex literal is the whole story");
}
