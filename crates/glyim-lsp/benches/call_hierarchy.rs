//! Call-hierarchy bench (LSP-CALL-HIERARCHY).
//!
//! Dependency-free (`harness = false`), mirroring the `type_definition` and
//! `selection_range` benches: measures the call-hierarchy request paths
//! end-to-end over a synthetic corpus — the policy computations through the
//! `CallGraphSource` seam and the full wire requests (prepare, incoming,
//! outgoing) exactly as the router handlers drive them — so CI logs can
//! track the per-request cost as the call-graph consumers grow. The
//! incoming/outgoing paths deliberately include the owner- and
//! extent-keyed joins over `iter_with_names`, the seam's linear scans,
//! because their cost is what a future owner-keyed index would buy back.
//! Run with:
//!
//! ```text
//! cargo bench -p glyim-lsp --bench call_hierarchy
//! ```

use std::hint::black_box;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Instant;

use glyim_lsp::call_hierarchy::{
    CallGraphSource, ReferenceGraphCallSource, incoming_calls, outgoing_calls,
    prepare_call_hierarchy, prepare_target,
};
use glyim_lsp::database::{FileMap, SourceMap};
use lsp_types::{
    CallHierarchyIncomingCallsParams, CallHierarchyItem, CallHierarchyOutgoingCallsParams,
    CallHierarchyPrepareParams, PartialResultParams, Position, TextDocumentIdentifier,
    TextDocumentPositionParams, WorkDoneProgressParams,
};
use url::Url;

/// One call-graph unit: a callee invoked twice by one wrapper and once by
/// an entry point, plus a leaf function that calls nothing — enough for
/// the joins to group real callers and callees.
const UNIT_SOURCE: &str = "fn leaf(x: i32) -> i32 {\n    x + 1\n}\n\
fn wrap(v: i32) -> i32 {\n    leaf(v) + leaf(v + 1)\n}\n\
fn entry() -> i32 {\n    wrap(3) + leaf(4)\n}\n";
const UNITS: usize = 32;

fn main() {
    // Build the corpus: `units` call-graph units, parsed and analyzed
    // once, the database populated exactly like the analysis driver does.
    let mut source = String::with_capacity(UNITS * UNIT_SOURCE.len());
    for _ in 0..UNITS {
        source.push_str(UNIT_SOURCE);
    }

    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/bench/call_hierarchy.g");
    // Both maps allocate ids from 0, so the db-side and caller-side ids
    // for `path` agree (same pattern as `test_utils::setup_test_db`).
    let file_id = db.file_map.write().get_or_create(&path);
    let caller_file_id = file_map.get_or_create(&path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, source.clone()));

    let crate_id = glyim_core::CrateId::from_raw(0);
    let parse_result = glyim_frontend::parse_to_syntax(&source, file_id);
    let mut interner = glyim_core::Interner::new();
    let (_def_map, _def_diags) =
        glyim_def_map::build_def_map(&parse_result.root, crate_id, interner.clone());
    let (hir, _hir_diags) =
        glyim_hir::pipeline_api::lower_crate_for_pipeline(&parse_result.root, &mut interner);
    db.reference_graph
        .write()
        .build_from_hir(file_id, &hir, &interner);

    // Cursors on the FIRST unit's `leaf` and `entry` definitions (the
    // graph is name-keyed, so `function_definition` answers the first
    // recorded site — the prepare request must aim there). The joins in
    // incoming/outgoing still span the full corpus: every unit's wrapper
    // and entry point calls the same-named callees, so the grouping sees
    // `2 * UNITS` call edges.
    let first_unit_start = 0;
    let leaf_def_offset = source[first_unit_start..]
        .find("fn leaf")
        .map(|i| first_unit_start + i + 3)
        .expect("bench fixture carries `fn leaf`");
    let entry_def_offset = source[first_unit_start..]
        .find("fn entry")
        .map(|i| first_unit_start + i + 3)
        .expect("bench fixture carries `fn entry`");

    // Policy bench: prepare + incoming + outgoing through the seam over
    // the shared graph, no database locks. The graph guard lives in this
    // block only, so the wire benches below start from a lock-free
    // database.
    {
        let graph = db.reference_graph.read();
        let source_impl = ReferenceGraphCallSource::new(&graph);
        let target = source_impl
            .function_definition("leaf")
            .expect("leaf is a fn item");

        let t0 = Instant::now();
        let mut hits = 0usize;
        for _ in 0..1_000 {
            let prepared = prepare_target(&source_impl, file_id, leaf_def_offset, "leaf").is_some();
            let incoming = glyim_lsp::call_hierarchy::compute_incoming_calls(&source_impl, &target);
            let outgoing = glyim_lsp::call_hierarchy::compute_outgoing_calls(&source_impl, &target);
            hits += usize::from(prepared)
                + usize::from(!incoming.is_empty())
                + usize::from(!outgoing.is_empty());
            black_box((&incoming, &outgoing));
        }
        let policy = t0.elapsed();
        println!(
            "policy prepare+in+out : {:>9.1?}/round (1,000 rounds, {hits} hits)",
            policy / 1_000
        );
    };

    // Wire benches: the full request paths through a real database,
    // exactly as the router handlers drive them.
    let uri = Url::from_file_path(&path)
        .ok()
        .and_then(|u| lsp_types::Uri::from_str(u.as_str()).ok())
        .expect("bench fixture path converts to a URI");
    fn position_of(source: &str, offset: usize) -> Position {
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
        Position {
            line,
            character: source[last_nl..offset]
                .chars()
                .map(char::len_utf16)
                .sum::<usize>() as u32,
        }
    }

    let prepare_params = CallHierarchyPrepareParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: position_of(&source, leaf_def_offset),
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
    };
    let t1 = Instant::now();
    let mut prepared = 0usize;
    let mut item: Option<CallHierarchyItem> = None;
    for _ in 0..1_000 {
        let items = prepare_call_hierarchy(&db, &file_map, &prepare_params)
            .expect("request hits the leaf fn");
        prepared += 1;
        item = items.into_iter().next();
        black_box(&item);
    }
    let prepare = t1.elapsed();
    println!(
        "wire prepare          : {:>9.1?}/call (1,000 calls, {prepared} resolved)",
        prepare / 1_000
    );

    let item = item.expect("prepare resolved an item");
    let incoming_params = CallHierarchyIncomingCallsParams {
        item: item.clone(),
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };
    let t2 = Instant::now();
    let mut incoming_total = 0usize;
    for _ in 0..1_000 {
        let calls = incoming_calls(&db, &file_map, &incoming_params).expect("item resolves");
        incoming_total += calls.len();
        black_box(&calls);
    }
    let incoming = t2.elapsed();
    println!(
        "wire incoming         : {:>9.1?}/call (1,000 calls, {incoming_total} callers total)",
        incoming / 1_000
    );

    let entry_item = {
        let params = CallHierarchyPrepareParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: position_of(&source, entry_def_offset),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        prepare_call_hierarchy(&db, &file_map, &params)
            .expect("entry is a fn item")
            .into_iter()
            .next()
            .expect("one item")
    };
    let outgoing_params = CallHierarchyOutgoingCallsParams {
        item: entry_item,
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };
    let t3 = Instant::now();
    let mut outgoing_total = 0usize;
    for _ in 0..1_000 {
        let calls = outgoing_calls(&db, &file_map, &outgoing_params).expect("item resolves");
        outgoing_total += calls.len();
        black_box(&calls);
    }
    let outgoing = t3.elapsed();
    println!(
        "wire outgoing         : {:>9.1?}/call (1,000 calls, {outgoing_total} callees total)",
        outgoing / 1_000
    );

    black_box((prepared, incoming_total, outgoing_total));
}
