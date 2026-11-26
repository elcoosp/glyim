//! Consumer walkthrough of the signature-help seam (LSP-SIGNATURE-HELP):
//! the `textDocument/signatureHelp` capability behind an editor's
//! parameter hints while typing a call.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lsp --example signature_help_walkthrough
//! ```
//!
//! The example drives the public path end-to-end without a client
//! process: a document runs through the same parse → def-map → HIR
//! pipeline the analysis driver runs, the results are stored in
//! `AnalysisDatabase` exactly like the driver stores them, and
//! `signature_help` — the function the router registers for
//! `SignatureHelpRequest` — turns a cursor inside a call into the
//! callee's rendered signature with the active parameter computed from
//! the cursor's position. A cursor outside every call, and a callee the
//! symbol index does not record, answer `null` honestly. Every offset is
//! derived from the document by token search, so the walkthrough cannot
//! silently drift from `DOC`.

use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::signature_help::signature_help;
use lsp_types::{
    SignatureHelpParams, TextDocumentIdentifier, TextDocumentPositionParams, WorkDoneProgressParams,
};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

/// The walkthrough document: a two-parameter function, a nested call
/// pair, and an unresolved callee.
const DOC: &str = "fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n\
fn outer(v: i32) -> i32 {\n    v\n}\n\
fn inner(x: i32, y: i32) -> i32 {\n    x + y\n}\n\
fn main() {\n    let one = add(1, 2);\n    let two = outer(inner(one, one));\n    ghost(3)\n}\n";

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
    let path = PathBuf::from("/walkthrough/signature_help.g");
    // Both maps allocate ids from 0, so the db-side and caller-side ids
    // for `path` agree (same pattern as the driver's get_or_create).
    let file_id = db.file_map.write().get_or_create(&path);
    let caller_file_id = file_map.get_or_create(&path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, DOC.to_string()));

    // The driver's pipeline. The request path reads the reference graph
    // (call sites) and the symbol index (signatures); the rest mirrors
    // what the driver stores so the session state is complete.
    let crate_id = glyim_core::CrateId::from_usize(0);
    let parse_result = glyim_frontend::parse_to_syntax(DOC, file_id);
    let mut interner = glyim_core::Interner::new();
    let (_def_map, _def_diags) =
        glyim_def_map::build_def_map(&parse_result.root, crate_id, interner.clone());
    let (hir, _hir_diags) =
        glyim_hir::pipeline_api::lower_crate_for_pipeline(&parse_result.root, &mut interner);
    db.reference_graph
        .write()
        .build_from_hir(file_id, &hir, &interner);
    db.symbol_index
        .write()
        .build_from_hir(file_id, &hir, &interner);

    let uri = Url::from_file_path(&path)
        .ok()
        .and_then(|u| lsp_types::Uri::from_str(u.as_str()).ok())
        .expect("walkthrough path converts to a URI");

    let help_at = |name_offset: usize| {
        let (line, character) = line_col_of(name_offset);
        SignatureHelpParams {
            context: None,
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: lsp_types::Position { line, character },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        }
    };

    // 1. The cursor on the FIRST argument of `add(1, 2)`: parameter 0.
    let first_arg = find("add(1") + 4; // the `1`
    let help = signature_help(&db, &file_map, &help_at(first_arg))
        .expect("the cursor sits inside add(1, 2)");
    println!(
        "cursor on `1`  -> {} (active parameter {:?})",
        help.signatures[0].label, help.active_parameter
    );

    // 2. The cursor on the SECOND argument: parameter 1.
    let second_arg = find("2);"); // the `2` of add(1, 2)
    let help = signature_help(&db, &file_map, &help_at(second_arg))
        .expect("the cursor sits inside add(1, 2)");
    println!(
        "cursor on `2`  -> active parameter {:?}",
        help.active_parameter
    );

    // 3. A NESTED call: the cursor on inner's second argument resolves
    //    to `inner`, not the enclosing `outer` — the innermost call the
    //    argument belongs to wins.
    let inner_y = find("one));"); // the second `one` inside inner(...)
    let help = signature_help(&db, &file_map, &help_at(inner_y))
        .expect("the cursor sits inside inner(...)");
    println!(
        "cursor on inner's 2nd arg -> {} (active parameter {:?})",
        help.signatures[0].label, help.active_parameter
    );

    // 4. The honest miss: `ghost` has no symbol-index entry, so the
    //    request answers null instead of fabricating a signature.
    let ghost = find("ghost(");
    assert!(
        signature_help(&db, &file_map, &help_at(ghost)).is_none(),
        "an unrecorded callee has no signature to show"
    );
    println!("cursor on `ghost(` -> null (no recorded signature)");
}
