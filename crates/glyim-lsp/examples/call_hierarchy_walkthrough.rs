//! Consumer walkthrough of the call-hierarchy seam (LSP-CALL-HIERARCHY):
//! the `textDocument/prepareCallHierarchy`,
//! `callHierarchy/incomingCalls`, and `callHierarchy/outgoingCalls`
//! capabilities behind an editor's "Show Call Hierarchy".
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lsp --example call_hierarchy_walkthrough
//! ```
//!
//! The example drives the public path end-to-end without a client
//! process: a document runs through the same parse → def-map → HIR
//! pipeline the analysis driver runs, the results are stored in
//! `AnalysisDatabase` exactly like the driver stores them, and the three
//! functions the router registers turn a cursor on a function (or on a
//! call to it) into the hierarchy the editor renders. A cursor on a
//! struct or a local answers `null` honestly: a call hierarchy is
//! function-shaped. Every offset is derived from the document by token
//! search, so the walkthrough cannot silently drift from `DOC`.

use glyim_lsp::call_hierarchy::{incoming_calls, outgoing_calls, prepare_call_hierarchy};
use glyim_lsp::database::{FileMap, SourceMap};
use lsp_types::{
    CallHierarchyIncomingCallsParams, CallHierarchyOutgoingCallsParams, CallHierarchyPrepareParams,
    PartialResultParams, TextDocumentIdentifier, TextDocumentPositionParams,
    WorkDoneProgressParams,
};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

/// The walkthrough document: a leaf function, a wrapper that calls it
/// twice, and an entry point that calls both (with a local binding and a
/// struct to prove the honest misses).
const DOC: &str = "fn leaf(x: i32) -> i32 {\n    x + 1\n}\n\
fn wrap(v: i32) -> i32 {\n    leaf(v) + leaf(v + 1)\n}\n\
fn entry() -> i32 {\n    let three = 3;\n    wrap(three) + leaf(4)\n}\n\
struct Config { tag: i32 }\n";

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
    let path = PathBuf::from("/walkthrough/call_hierarchy.g");
    // Both maps allocate ids from 0, so the db-side and caller-side ids
    // for `path` agree (same pattern as the driver's get_or_create).
    let file_id = db.file_map.write().get_or_create(&path);
    let caller_file_id = file_map.get_or_create(&path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, DOC.to_string()));

    // The driver's pipeline. The call-hierarchy path reads the reference
    // graph (built from the real HIR — the same store rename and
    // find-references read); the rest mirrors what the driver stores so
    // the session state is complete.
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

    let uri = Url::from_file_path(&path)
        .ok()
        .and_then(|u| lsp_types::Uri::from_str(u.as_str()).ok())
        .expect("walkthrough path converts to a URI");

    let prepare_at = |name_offset: usize| {
        let (line, character) = line_col_of(name_offset);
        CallHierarchyPrepareParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: lsp_types::Position { line, character },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        }
    };

    // 1. Prepare on the DEFINITION site: the cursor sits on `leaf` in
    //    `fn leaf(...)`; the server answers the one-item list the client
    //    will render.
    let leaf_offset = find("fn leaf") + 3; // the `leaf` token
    let items = prepare_call_hierarchy(&db, &file_map, &prepare_at(leaf_offset))
        .expect("leaf is a function item");
    let leaf_item = items.into_iter().next().expect("one hierarchy item");
    println!(
        "prepare @ {:<16} -> {} (kind {:?}, range line {})",
        "`fn leaf`", leaf_item.name, leaf_item.kind, leaf_item.range.start.line
    );

    // 2. Prepare on a CALL site: the cursor sits on `wrap(three)` inside
    //    `entry`; the server resolves the *callee* so the hierarchy walk
    //    continues from wrap.
    let call_offset = find("wrap(three)") + 1;
    let items = prepare_call_hierarchy(&db, &file_map, &prepare_at(call_offset))
        .expect("the cursor sits on a recorded call");
    let wrap_item = items.into_iter().next().expect("one hierarchy item");
    println!(
        "prepare @ {:<16} -> {} (the callee, not the caller)",
        "`wrap(three)`", wrap_item.name
    );

    // 3. Incoming calls: who calls `leaf`? The wrapper does, twice —
    //    grouped into one caller with two call-site ranges.
    let incoming = incoming_calls(
        &db,
        &file_map,
        &CallHierarchyIncomingCallsParams {
            item: leaf_item.clone(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        },
    )
    .expect("the prepared item resolves");
    for call in &incoming {
        println!(
            "incoming  <- {:<6} from_ranges: {} site(s), first at line {}",
            call.from.name,
            call.from_ranges.len(),
            call.from_ranges[0].start.line
        );
    }

    // 4. Outgoing calls: what does `entry` call? Both callees, grouped
    //    and sorted by name.
    let entry_offset = find("fn entry") + 3;
    let items = prepare_call_hierarchy(&db, &file_map, &prepare_at(entry_offset))
        .expect("entry is a function item");
    let entry_item = items.into_iter().next().expect("one hierarchy item");
    let outgoing = outgoing_calls(
        &db,
        &file_map,
        &CallHierarchyOutgoingCallsParams {
            item: entry_item,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        },
    )
    .expect("the prepared item resolves");
    for call in &outgoing {
        println!(
            "outgoing  -> {:<6} from_ranges: {} site(s), first at line {}",
            call.to.name,
            call.from_ranges.len(),
            call.from_ranges[0].start.line
        );
    }

    // 5. The honest misses: a struct and a local are not hierarchy items.
    let struct_offset = find("struct Config") + 7;
    assert!(
        prepare_call_hierarchy(&db, &file_map, &prepare_at(struct_offset)).is_none(),
        "a struct is not a call-hierarchy target"
    );
    let local_offset = find("let three") + 4;
    assert!(
        prepare_call_hierarchy(&db, &file_map, &prepare_at(local_offset)).is_none(),
        "a local binding is not a call-hierarchy target"
    );
    println!(
        "prepare @ {:<16} -> null (call hierarchies are function-shaped)",
        "struct / local"
    );
}
