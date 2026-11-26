//! Consumer walkthrough of the type-hierarchy seam (LSP-TYPE-HIERARCHY):
//! the `textDocument/prepareTypeHierarchy` + `typeHierarchy/supertypes` +
//! `typeHierarchy/subtypes` triple behind an editor's type-hierarchy view.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lsp --example type_hierarchy_walkthrough
//! ```
//!
//! The example drives the public path end-to-end without a client
//! process: a document runs through the same parse → def-map → HIR
//! pipeline the analysis driver runs (the hierarchy engine reads the
//! recorded HIR facts only — no typeck pass is on its path), the HIRs are
//! stored in `AnalysisDatabase` exactly like the driver stores them, and
//! `prepare_type_hierarchy` — the function the router registers for
//! `TypeHierarchyPrepare` — turns the cursor's item into the wire item.
//! That item is echoed back through `type_supertypes` and
//! `type_subtypes` — the functions the router registers for
//! `TypeHierarchySupertypes` / `TypeHierarchySubtypes` — exactly the
//! echo the client sends, re-joined against the recorded facts. Every
//! offset is derived from the document by token search, so the
//! walkthrough cannot silently drift from `DOC`.

use glyim_core::{CrateId, Interner};
use glyim_def_map::build_def_map;
use glyim_frontend::parse_to_syntax;
use glyim_hir::pipeline_api::lower_crate_for_pipeline;
use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::type_hierarchy::{prepare_type_hierarchy, type_subtypes, type_supertypes};
use lsp_types::{
    Position, TextDocumentIdentifier, TextDocumentPositionParams, TypeHierarchyPrepareParams,
    TypeHierarchySubtypesParams, TypeHierarchySupertypesParams, Uri, WorkDoneProgressParams,
};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

/// The walkthrough document: one trait, two implementors (a struct and an
/// enum), one inherent impl, and one unimplemented struct — the shape an
/// editor user would open the hierarchy on. The inherent impl (`impl
/// Meter`) records no trait and must not appear as a supertype; `Solo` is
/// never implemented and its supertypes walk answers nothing.
const DOC: &str = "trait Meter {\n\
                   fn reading(&self) -> i32;\n\
                   }\n\
                   struct Gauge { v: i32 }\n\
                   enum Mode { On }\n\
                   impl Meter for Gauge {\n\
                   fn reading(&self) -> i32 { self.v }\n\
                   }\n\
                   impl Meter for Mode {\n\
                   fn reading(&self) -> i32 { 0 }\n\
                   }\n\
                   impl Gauge {\n\
                   fn zero(&self) -> i32 { 0 }\n\
                   }\n\
                   struct Solo { z: i32 }\n";

fn main() {
    // Populate the database exactly like the analysis driver does: file
    // map, source map, stored HIRs.
    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let doc_path = PathBuf::from("/walkthrough/type_hierarchy.gly");
    let file_id = file_map.get_or_create(&doc_path);
    db.file_map.write().get_or_create(&doc_path);
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, DOC.to_string()));

    let parse_result = parse_to_syntax(DOC, file_id);
    let mut interner = Interner::new();
    let (_def_map, _def_diags) =
        build_def_map(&parse_result.root, CrateId::from_usize(0), interner.clone());
    let (hir, _hir_diags) = lower_crate_for_pipeline(&parse_result.root, &mut interner);
    db.hirs.write().insert(file_id, hir);

    let uri = Uri::from_str(Url::from_file_path(&doc_path).unwrap().as_ref()).unwrap();

    // A cursor on the trait name: exactly the params the router
    // registers for `TypeHierarchyPrepare`.
    let (line, character) = offset_to_line_col(DOC, DOC.find("trait Meter").unwrap() + 6);
    let trait_params = TypeHierarchyPrepareParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
    };
    let trait_item = prepare_type_hierarchy(&db, &file_map, &trait_params)
        .expect("the trait item prepares")
        .into_iter()
        .next()
        .unwrap();
    println!("prepareTypeHierarchy on `Meter`:");
    println!(
        "  {} item at {}:{}..{}:{}",
        if trait_item.kind == lsp_types::SymbolKind::INTERFACE {
            "interface"
        } else {
            "other"
        },
        trait_item.range.start.line,
        trait_item.range.start.character,
        trait_item.range.end.line,
        trait_item.range.end.character,
    );

    // The echo round-trips: subtypes of the trait are its impl self
    // types (Gauge, Mode), each anchored at its impl block.
    let subtypes_params = TypeHierarchySubtypesParams {
        item: trait_item.clone(),
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };
    let subs = type_subtypes(&db, &file_map, &subtypes_params).expect("Meter has implementors");
    println!("typeHierarchy/subtypes on the echoed `Meter` item:");
    for item in &subs {
        println!(
            "  {} at {}:{}",
            item.name, item.range.start.line, item.range.start.character
        );
    }

    // A cursor on the struct name: supertypes of `Gauge` are the traits
    // implemented for it — the trait impl anchors, not the inherent impl.
    let (line, character) = offset_to_line_col(DOC, DOC.find("struct Gauge").unwrap() + 7);
    let struct_params = TypeHierarchyPrepareParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
    };
    let gauge_item = prepare_type_hierarchy(&db, &file_map, &struct_params)
        .expect("the struct item prepares")
        .into_iter()
        .next()
        .unwrap();
    let supertypes_params = TypeHierarchySupertypesParams {
        item: gauge_item.clone(),
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };
    let supers =
        type_supertypes(&db, &file_map, &supertypes_params).expect("Gauge has a trait impl");
    println!("typeHierarchy/supertypes on the echoed `Gauge` item:");
    for item in &supers {
        println!(
            "  {} at {}:{}",
            item.name, item.range.start.line, item.range.start.character
        );
    }

    // Honesty pins: the trait prepares as an interface; its subtypes are
    // exactly the two impl self types in source order; the struct's
    // supertypes name the trait exactly once (the inherent `impl Gauge`
    // records no trait); supertypes of the trait and subtypes of the
    // struct answer nothing — the recorded facts are never guessed
    // around.
    assert_eq!(trait_item.kind, lsp_types::SymbolKind::INTERFACE);
    let sub_names: Vec<&str> = subs.iter().map(|item| item.name.as_str()).collect();
    assert_eq!(
        sub_names,
        vec!["Gauge", "Mode"],
        "one member per impl self type"
    );
    let sup_names: Vec<&str> = supers.iter().map(|item| item.name.as_str()).collect();
    assert_eq!(
        sup_names,
        vec!["Meter"],
        "one member per trait impl, inherent impls excluded"
    );
    let trait_supertypes_params = TypeHierarchySupertypesParams {
        item: trait_item,
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };
    assert!(
        type_supertypes(&db, &file_map, &trait_supertypes_params).is_none(),
        "no supertrait facts are recorded, so a trait's supertypes answer null"
    );
    let struct_subtypes_params = TypeHierarchySubtypesParams {
        item: gauge_item,
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };
    assert!(
        type_subtypes(&db, &file_map, &struct_subtypes_params).is_none(),
        "a struct has no recorded sub-hierarchy, so its subtypes answer null"
    );
    println!("\nwalkthrough complete: recorded items prepare, impl facts answer the walks.");
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
