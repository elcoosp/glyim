//! Consumer walkthrough of the implementation seam (LSP-IMPLEMENTATION):
//! the `textDocument/implementation` capability behind an editor's
//! "go to implementation".
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lsp --example implementation_walkthrough
//! ```
//!
//! The example drives the public path end-to-end without a client
//! process: a document runs through the same parse → HIR pipeline the
//! analysis driver runs (the implementation engine reads recorded HIR
//! facts only — no typeck pass is on its path), the results are stored in
//! `AnalysisDatabase` exactly like the driver stores them, and
//! `goto_implementation` — the function the router registers for
//! `GotoImplementation` — turns a cursor on a *trait* into the location
//! of its impl block and a cursor on a *struct* into every impl block
//! implemented for it, across the session's files. A method-name cursor
//! answers `null` honestly: methods are not impl sites. Every offset is
//! derived from the document by token search, so the walkthrough cannot
//! silently drift from `DOC`.

use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::implementation::goto_implementation;
use lsp_types::{
    GotoDefinitionParams, Position, TextDocumentIdentifier, TextDocumentPositionParams,
};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

/// The walkthrough document: one trait, one struct, a trait impl of it,
/// an inherent impl, and a caller — the shape an editor user would ask
/// "go to implementation" about.
const DOC: &str = "trait Greet {\n\
                   fn name(&self) -> i32;\n\
                   }\n\
                   struct P { n: i32 }\n\
                   impl Greet for P {\n\
                   fn name(&self) -> i32 { self.n }\n\
                   }\n\
                   impl P {\n\
                   fn shout(&self) -> i32 { self.n }\n\
                   }\n\
                   fn main() -> i32 {\n\
                   let p = P { n: 5 };\n\
                   p.name()\n\
                   }\n";

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
    let path = PathBuf::from("/walkthrough/implementation.g");
    // Both maps allocate ids from 0, so the db-side and caller-side ids
    // for `path` agree (same pattern as the driver's get_or_create).
    let file_id = db.file_map.write().get_or_create(&path);
    let caller_file_id = file_map.get_or_create(&path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, DOC.to_string()));

    // The driver's parse → HIR pipeline: the implementation engine reads
    // the stored `CrateHir`s (each `impl` block carries its trait path
    // and self type), so this is everything the request path needs.
    let parse_result = glyim_frontend::parse_to_syntax(DOC, file_id);
    let mut interner = glyim_core::Interner::new();
    let (hir, _hir_diags) =
        glyim_hir::pipeline_api::lower_crate_for_pipeline(&parse_result.root, &mut interner);
    db.hirs.write().insert(file_id, hir);

    // Cursors on the trait declaration, the struct declaration, and a
    // method name inside the impl.
    let trait_offset = find("trait Greet") + "trait ".len();
    let struct_offset = find("struct P") + "struct ".len();
    let method_offset = find("fn name(&self) -> i32 { self.n }") + "fn ".len();
    let params_for = |offset: usize| GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Url::from_file_path(&path)
                    .ok()
                    .and_then(|u| lsp_types::Uri::from_str(u.as_str()).ok())
                    .expect("walkthrough path converts to a URI"),
            },
            position: {
                let (line, character) = line_col_of(offset);
                Position { line, character }
            },
        },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };

    println!("document: {DOC:?}\n");

    let params = params_for(trait_offset);
    match goto_implementation(&db, &file_map, &params) {
        Some(lsp_types::GotoDefinitionResponse::Array(locations)) => {
            println!(
                "implementation on `Greet` (byte {trait_offset}) → {} impl block(s):",
                locations.len()
            );
            for location in locations {
                let r = &location.range;
                println!(
                    "  - {}:{} .. {}:{}",
                    r.start.line, r.start.character, r.end.line, r.end.character,
                );
            }
            println!();
        }
        Some(other) => println!("implementation on `Greet` → unexpected response {other:?}\n"),
        None => println!("implementation on `Greet` → null (unexpected)\n"),
    }

    let params = params_for(struct_offset);
    match goto_implementation(&db, &file_map, &params) {
        Some(lsp_types::GotoDefinitionResponse::Array(locations)) => {
            println!(
                "implementation on `P` (byte {struct_offset}) → {} impl block(s) (trait impl + inherent impl):",
                locations.len()
            );
            for location in locations {
                let r = &location.range;
                println!(
                    "  - {}:{} .. {}:{}",
                    r.start.line, r.start.character, r.end.line, r.end.character,
                );
            }
            println!();
        }
        Some(other) => println!("implementation on `P` → unexpected response {other:?}\n"),
        None => println!("implementation on `P` → null (unexpected)\n"),
    }

    let params = params_for(method_offset);
    match goto_implementation(&db, &file_map, &params) {
        Some(_) => println!("implementation on a method name → unexpected non-null"),
        None => println!(
            "implementation on the method name (byte {method_offset}) → null \
             (methods are not impl sites)"
        ),
    }
}
