//! Execute-command request bench (LSP-EXEC-CMD).
//!
//! Dependency-free (`harness = false`), mirroring the `code_lens` bench:
//! measures the executed-command path end-to-end over a synthetic
//! reference-heavy corpus — the argument payload parse, the URI →
//! open-file → offset lens join (the resolve's own join), and the
//! name-keyed reference walk whose locations the request answers — so CI
//! logs can track the per-request cost as the command seam grows
//! consumers. Run with:
//!
//! ```text
//! cargo bench -p glyim-lsp --bench execute_command
//! ```

use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

use glyim_core::{CrateId, Interner};
use glyim_def_map::build_def_map;
use glyim_frontend::parse_to_syntax;
use glyim_hir::pipeline_api::lower_crate_for_pipeline;
use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::execute_command::execute_command;
use lsp_types::{ExecuteCommandParams, WorkDoneProgressParams};
use serde_json::json;
use url::Url;

/// One unit: a caller invoking a helper once — the realistic mix an
/// editor session shows. `helper` is called once per unit, so the
/// executed answer's location count scales with the corpus.
const UNIT_SOURCE: &str = "fn unit() -> i32 {\n\
                           helper(0)\n\
                           }\n\
                           fn helper(x: i32) -> i32 {\n\
                           x\n\
                           }\n";
const UNITS: usize = 64;

fn main() {
    // Build the corpus: `units` caller/callee pairs, analysed once
    // through the same pipeline the analysis driver runs (parse →
    // def-map → HIR), the reference graph populated exactly like the
    // driver populates it.
    let mut source = String::with_capacity(UNITS * UNIT_SOURCE.len());
    for _ in 0..UNITS {
        source.push_str(UNIT_SOURCE);
    }

    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/bench/execute_command.gly");
    // Both maps allocate ids from 0, so the db-side and caller-side ids
    // for `path` agree (same pattern as `test_utils::setup_test_db`).
    let file_id = db.file_map.write().get_or_create(&path);
    let caller_file_id = file_map.get_or_create(&path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, source.clone()));

    let parse_result = parse_to_syntax(&source, file_id);
    let mut interner = Interner::new();
    let (_def_map, _def_diags) =
        build_def_map(&parse_result.root, CrateId::from_raw(0), interner.clone());
    let (hir, _hir_diags) = lower_crate_for_pipeline(&parse_result.root, &mut interner);
    db.reference_graph
        .write()
        .build_from_hir(file_id, &hir, &interner);

    // The executed command, built exactly like the code-lens resolve
    // builds it: the lens range clamps to the item's first line, the
    // payload carries the URI and that range. `helper`'s definition sits
    // at the start of its line (offset 16 in each unit); the first unit's
    // helper line is what the echo anchors.
    let helper_line_start = source.find("fn helper").unwrap();
    let (line, character) = {
        let mut line = 0u32;
        let mut last_nl = 0usize;
        for (i, ch) in source.char_indices() {
            if i >= helper_line_start {
                break;
            }
            if ch == '\n' {
                line += 1;
                last_nl = i + 1;
            }
        }
        (
            line,
            source[last_nl..helper_line_start]
                .chars()
                .map(char::len_utf16)
                .sum::<usize>() as u32,
        )
    };
    let uri = Url::from_file_path(&path).unwrap().to_string();
    let params = ExecuteCommandParams {
        command: "glyim.showReferences".to_string(),
        arguments: vec![
            json!(uri),
            json!({ "start": { "line": line, "character": character }, "end": { "line": line, "character": character + 9 } }),
        ],
        work_done_progress_params: WorkDoneProgressParams::default(),
    };

    // Warm the lens join once outside the measured loop: the bench
    // measures the request path, not the analysis driver.
    let warm = execute_command(&db, &file_map, &params).expect("the command executes");
    assert!(!warm.is_empty(), "the corpus references helper");

    let requests = 200;
    let start = Instant::now();
    for _ in 0..requests {
        let locations = execute_command(&db, &file_map, black_box(&params));
        black_box(locations.expect("the command executes"));
    }
    let elapsed = start.elapsed();

    println!(
        "execute_command bench: {UNITS} caller/callee units, {requests} requests, {} locations per answer",
        warm.len()
    );
    println!(
        "  workspace/executeCommand: {:>10.1?} total, {:>8.1?}/request",
        elapsed,
        elapsed / requests
    );
}
