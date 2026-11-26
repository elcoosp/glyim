//! Consumer walkthrough of the code-lens seam (LSP-CODE-LENS): the
//! `textDocument/codeLens` + `codeLens/resolve` pair behind an editor's
//! above-the-line "N references" annotations.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lsp --example code_lens_walkthrough
//! ```
//!
//! The example drives the public path end-to-end without a client
//! process: a document runs through the same parse → def-map → HIR
//! pipeline the analysis driver runs (the lens engine reads the recorded
//! reference graph only — no typeck pass is on its path), the reference
//! graph is stored in `AnalysisDatabase` exactly like the driver stores
//! it, and `code_lenses` — the function the router registers for
//! `CodeLensRequest` — turns every item-level definition into a lens,
//! while scoped names (locals, params, struct fields) stay lens-free.
//! Each lens is then resolved — the function the router registers for
//! `CodeLensResolve` — and the live use count becomes the command title.
//! Every offset is derived from the document by token search, so the
//! walkthrough cannot silently drift from `DOC`.

use glyim_core::{CrateId, Interner};
use glyim_def_map::build_def_map;
use glyim_frontend::parse_to_syntax;
use glyim_hir::pipeline_api::lower_crate_for_pipeline;
use glyim_lsp::code_lens::{code_lenses, resolve_code_lens};
use glyim_lsp::database::{FileMap, SourceMap};
use lsp_types::{
    CodeLensParams, PartialResultParams, TextDocumentIdentifier, WorkDoneProgressParams,
};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

/// The walkthrough document: one called function, one uncalled function,
/// a struct, and a scoped local binding — the shape an editor user would
/// scroll past. `main` calls `helper` twice; `Point` and `Orphan` are
/// never referenced; `used_local` is scoped to its body and must not
/// carry a lens.
const DOC: &str = "fn main() -> i32 {\n\
                   let used_local = helper(0);\n\
                   helper(used_local)\n\
                   }\n\
                   fn helper(x: i32) -> i32 {\n\
                   x\n\
                   }\n\
                   struct Point {\n\
                   x: i32,\n\
                   }\n\
                   fn orphan() -> i32 {\n\
                   0\n\
                   }\n";

fn main() {
    // Populate the database exactly like the analysis driver does: file
    // map, source map, HIR-backed reference graph.
    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let doc_path = PathBuf::from("/walkthrough/code_lens.gly");
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
    db.reference_graph
        .write()
        .build_from_hir(file_id, &hir, &interner);

    // The request: exactly the params the router registers for
    // `CodeLensRequest`.
    let uri = Url::from_file_path(&doc_path)
        .ok()
        .and_then(|u| lsp_types::Uri::from_str(u.as_str()).ok())
        .expect("walkthrough path converts to a URI");
    let params = CodeLensParams {
        text_document: TextDocumentIdentifier { uri },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let lenses = code_lenses(&db, &file_map, &params).expect("walkthrough doc carries items");

    println!("document: {}", doc_path.display());
    println!("resolved {} code lens(es):", lenses.len());
    for lens in &lenses {
        // The resolve round-trip: the editor echoes the lens it is about
        // to render; the server re-derives the count from the graph.
        let resolved = resolve_code_lens(&db, &file_map, lens)
            .expect("every echoed lens re-joins against the graph");
        let title = resolved
            .command
            .as_ref()
            .map(|command| command.title.clone())
            .unwrap_or_else(|| "(no command)".to_string());
        println!(
            "  line {} ({}:{}..{}:{}): {title}",
            lens.range.start.line,
            lens.range.start.line,
            lens.range.start.character,
            lens.range.end.line,
            lens.range.end.character,
        );
    }

    // Honesty pins: four lenses (main, helper, Point, orphan), the scoped
    // `used_local` and the struct field carry none, the counts are live
    // (`helper` is called twice, `main`/`Point`/`orphan` never), and the
    // resolve titles spell the grammar out.
    let titles: Vec<String> = lenses
        .iter()
        .filter_map(|lens| {
            resolve_code_lens(&db, &file_map, lens)
                .and_then(|resolved| resolved.command.map(|command| command.title))
        })
        .collect();
    assert_eq!(
        lenses.len(),
        4,
        "main, helper, Point, orphan — nothing scoped"
    );
    assert_eq!(
        titles,
        vec![
            "0 references".to_string(), // main
            "2 references".to_string(), // helper
            "0 references".to_string(), // Point
            "0 references".to_string(), // orphan
        ],
        "the use counts are the graph's live name-keyed counts"
    );
    println!("\nwalkthrough complete: item definitions lens, scoped names stay inert.");
}
