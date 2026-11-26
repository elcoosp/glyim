//! Consumer walkthrough of the moniker seam (LSP-MONIKER): the
//! `textDocument/moniker` request behind an external indexer's symbol
//! identities.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lsp --example moniker_walkthrough
//! ```
//!
//! The example drives the public path end-to-end without a client
//! process: a document runs through the same parse → def-map → HIR →
//! symbol-index pipeline the analysis driver runs (the moniker engine
//! reads the recorded index facts and the stored parse tree only — no
//! typeck pass is on its path), the stores are filled in
//! `AnalysisDatabase` exactly like the driver fills them, and `monikers`
//! — the function the router registers for `MonikerRequest` — turns the
//! cursor's symbol into the wire identities. Every offset is derived
//! from the document by token search, so the walkthrough cannot silently
//! drift from `DOC`.

use glyim_core::{CrateId, Interner};
use glyim_def_map::build_def_map;
use glyim_frontend::parse_to_syntax;
use glyim_hir::pipeline_api::lower_crate_for_pipeline;
use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::moniker::monikers;
use lsp_types::{
    MonikerParams, Position, TextDocumentIdentifier, TextDocumentPositionParams, Uri,
    WorkDoneProgressParams,
};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

/// The walkthrough document: one aliased import, one locally-declared
/// struct, one free function, and a body using all of them — the shape
/// an indexer crawls. The alias renames the binding (`Pt`), but the
/// moniker identity stays the path the source spells.
const DOC: &str = "use crate::shapes::Point as Pt;\n\
                   \n\
                   struct Widget { n: i32 }\n\
                   \n\
                   fn helper() -> i32 { 1 }\n\
                   \n\
                   fn main() -> i32 {\n\
                   \x20   let w = Widget { n: Pt { x: 1 }.n };\n\
                   \x20   helper()\n\
                   }\n";

fn main() {
    // Populate the database exactly like the analysis driver does: file
    // map, source map, symbol index, parse trees.
    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let doc_path = PathBuf::from("/walkthrough/moniker.gly");
    let file_id = file_map.get_or_create(&doc_path);
    db.file_map.write().get_or_create(&doc_path);
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, DOC.to_string()));

    let crate_id = CrateId::from_usize(0);
    let parse_result = parse_to_syntax(DOC, file_id);
    let mut interner = Interner::new();
    let (_def_map, _def_diags) = build_def_map(&parse_result.root, crate_id, interner.clone());
    let (hir, _hir_diags) = lower_crate_for_pipeline(&parse_result.root, &mut interner);
    db.symbol_index
        .write()
        .build_from_hir(file_id, &hir, &interner);
    db.hirs.write().insert(file_id, hir);
    db.syntaxes.write().insert(file_id, parse_result.green_node);

    let uri = Uri::from_str(Url::from_file_path(&doc_path).unwrap().as_ref()).unwrap();
    let params_at = |offset: usize| {
        let (line, character) = offset_to_line_col(DOC, offset);
        MonikerParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position { line, character },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: lsp_types::PartialResultParams::default(),
        }
    };
    let label_of = |kind: Option<lsp_types::MonikerKind>| match kind {
        Some(lsp_types::MonikerKind::Import) => "import",
        Some(lsp_types::MonikerKind::Export) => "export",
        _ => "unknown",
    };

    // A cursor on the alias inside the `use` statement: the identity is
    // the spelled path, not the alias.
    let on_use = monikers(&db, &file_map, &params_at(DOC.find("as Pt").unwrap() + 4))
        .expect("the import binding answers");
    println!("textDocument/moniker on the `Pt` binding:");
    for m in &on_use {
        println!(
            "  {} {} under scheme `{}` at {} uniqueness",
            label_of(m.kind),
            m.identifier,
            m.scheme,
            serde_name(m.unique)
        );
    }

    // A cursor on the struct's name: the recorded export identity.
    let on_struct = monikers(
        &db,
        &file_map,
        &params_at(DOC.find("struct Widget").unwrap() + 7),
    )
    .expect("the struct is recorded");
    println!("textDocument/moniker on the `Widget` definition:");
    for m in &on_struct {
        println!(
            "  {} {} under scheme `{}` at {} uniqueness",
            label_of(m.kind),
            m.identifier,
            m.scheme,
            serde_name(m.unique)
        );
    }

    // A bare usage of the imported name in the body: the usage refers to
    // the imported symbol, so the import identity answers.
    let on_usage = monikers(
        &db,
        &file_map,
        &params_at(DOC.find("Pt { x: 1 }").unwrap() + 2),
    )
    .expect("the usage resolves to the import");
    println!("textDocument/moniker on the `Pt` usage:");
    for m in &on_usage {
        println!(
            "  {} {} under scheme `{}` at {} uniqueness",
            label_of(m.kind),
            m.identifier,
            m.scheme,
            serde_name(m.unique)
        );
    }

    // Honesty pins: the import identity is the spelled path (not the
    // alias), the export identity is the recorded qualified path, and a
    // name no fact identifies (the local binding `w`) answers null —
    // no identity is guessed.
    assert_eq!(on_use[0].identifier, "crate::shapes::Point");
    assert_eq!(on_use[0].kind, Some(lsp_types::MonikerKind::Import));
    assert_eq!(on_struct[0].identifier, "Widget");
    assert_eq!(on_struct[0].kind, Some(lsp_types::MonikerKind::Export));
    assert_eq!(on_usage[0].identifier, "crate::shapes::Point");
    let local_binding = monikers(&db, &file_map, &params_at(DOC.find("let w =").unwrap() + 4));
    assert!(
        local_binding.is_none(),
        "a zero-width local recording answers no identity"
    );
    println!("\nwalkthrough complete: recorded facts answer the identities, never a guess.");
}

/// The uniqueness level's wire spelling, for the walkthrough's printout.
fn serde_name(unique: lsp_types::UniquenessLevel) -> &'static str {
    match unique {
        lsp_types::UniquenessLevel::Document => "document",
        lsp_types::UniquenessLevel::Project => "project",
        lsp_types::UniquenessLevel::Group => "group",
        lsp_types::UniquenessLevel::Scheme => "scheme",
        lsp_types::UniquenessLevel::Global => "global",
    }
}

/// Byte offset → the `(line, utf16_col)` wire position (the same mapping
/// `SourceMap` performs on the request path).
fn offset_to_line_col(source: &str, offset: usize) -> (u32, u32) {
    let mut line = 0u32;
    let mut last_nl = 0usize;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            last_nl = i + 1;
        }
    }
    (
        line,
        source[last_nl..offset]
            .chars()
            .map(char::len_utf16)
            .sum::<usize>() as u32,
    )
}
