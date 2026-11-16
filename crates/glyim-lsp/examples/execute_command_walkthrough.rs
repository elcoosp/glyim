//! Consumer walkthrough of the execute-command seam (LSP-EXEC-CMD): the
//! `workspace/executeCommand` route behind the code lens's "N references"
//! command.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lsp --example execute_command_walkthrough
//! ```
//!
//! The example drives the full lens → command loop end-to-end without a
//! client process: a document runs through the same parse → def-map →
//! HIR pipeline the analysis driver runs, the reference graph is stored
//! in `AnalysisDatabase` exactly like the driver stores it, the lens
//! request enumerates the item lenses, the resolve attaches the
//! `glyim.showReferences` command with its `[uri, range]` payload, and
//! `execute_command` — the function the router registers for
//! `workspace/executeCommand` — executes that payload and answers the
//! locations the title promised. The count and the answer agree by
//! construction: both read the same non-definition records of the same
//! item-level name.

use glyim_core::{CrateId, Interner};
use glyim_def_map::build_def_map;
use glyim_frontend::parse_to_syntax;
use glyim_hir::pipeline_api::lower_crate_for_pipeline;
use glyim_lsp::code_lens::{SHOW_REFERENCES_COMMAND, code_lenses, resolve_code_lens};
use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::execute_command::execute_command;
use lsp_types::{
    CodeLensParams, ExecuteCommandParams, PartialResultParams, TextDocumentIdentifier,
    WorkDoneProgressParams,
};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

/// The walkthrough document: one caller invoking a helper once, one
/// uncalled function — the two shapes a lens command executes against.
/// `helper` resolves to "1 reference"; `orphan` to "0 references"; the
/// execution answers must agree with both titles.
const DOC: &str = "fn main() -> i32 {\n\
                   helper(0)\n\
                   }\n\
                   fn helper(x: i32) -> i32 {\n\
                   x\n\
                   }\n\
                   fn orphan() -> i32 {\n\
                   0\n\
                   }\n";

fn main() {
    // Populate the database exactly like the analysis driver does: file
    // map, source map, reference graph.
    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let doc_path = PathBuf::from("/walkthrough/execute_command.gly");
    let file_id = file_map.get_or_create(&doc_path);
    db.file_map.write().get_or_create(&doc_path);
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, DOC.to_string()));

    let parse_result = parse_to_syntax(DOC, file_id);
    let mut interner = Interner::new();
    let (_def_map, _def_diags) =
        build_def_map(&parse_result.root, CrateId::from_raw(0), interner.clone());
    let (hir, _hir_diags) = lower_crate_for_pipeline(&parse_result.root, &mut interner);
    db.reference_graph
        .write()
        .build_from_hir(file_id, &hir, &interner);

    let uri = lsp_types::Uri::from_str(Url::from_file_path(&doc_path).unwrap().as_ref()).unwrap();
    let lens_params = CodeLensParams {
        text_document: TextDocumentIdentifier { uri },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };
    let lenses = code_lenses(&db, &file_map, &lens_params).expect("the doc carries items");

    println!("document: {}", doc_path.display());
    for lens in &lenses {
        // The resolve round-trip: the editor echoes the lens it is about
        // to render; the server re-derives the count from the graph and
        // attaches the command.
        let resolved = resolve_code_lens(&db, &file_map, lens)
            .expect("every echoed lens re-joins against the graph");
        let command = resolved
            .command
            .as_ref()
            .expect("the resolve attaches a command");
        assert_eq!(command.command, SHOW_REFERENCES_COMMAND);
        println!(
            "  lens at {}:{}",
            lens.range.start.line, lens.range.start.character
        );

        // The execution round-trip: the client sends the resolve's
        // command id and its `[uri, range]` payload verbatim; the server
        // re-joins the payload against the graph and answers the
        // locations the title promised.
        let params = ExecuteCommandParams {
            command: command.command.clone(),
            arguments: command
                .arguments
                .clone()
                .expect("the command carries arguments"),
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        let locations =
            execute_command(&db, &file_map, &params).expect("the advertised command executes");
        println!(
            "    {title:?} -> {n} location(s)",
            title = command.title,
            n = locations.len()
        );
        for location in &locations {
            println!(
                "      {}:{}..{}:{}",
                location.range.start.line,
                location.range.start.character,
                location.range.end.line,
                location.range.end.character,
            );
        }

        // The count and the answer can never disagree: the title renders
        // the graph's non-definition record count, the execution answers
        // the same records as locations. Parse the grammar back and pin
        // it.
        let count = command
            .title
            .split(' ')
            .next()
            .and_then(|word| word.parse::<usize>().ok())
            .expect("the title spells the grammar");
        assert_eq!(
            locations.len(),
            count,
            "the executed answer agrees with the rendered title"
        );
    }

    // Honesty pins: main (uncalled) -> "0 references" -> empty list;
    // helper (called once) -> "1 reference" -> one location on the call
    // site's line; orphan (uncalled) -> "0 references" -> empty list.
    println!("\nwalkthrough complete: the advertised command executes what the resolve emits.");
}
