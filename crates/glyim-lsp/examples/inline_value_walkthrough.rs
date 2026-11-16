//! Consumer walkthrough of the inline-value seam (LSP-INLINE-VALUE):
//! the `textDocument/inlineValue` request behind a debug adapter's
//! inline variable rendering.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lsp --example inline_value_walkthrough
//! ```
//!
//! The example drives the public path end-to-end without a client
//! process: a document runs through the same parse → def-map → HIR
//! pipeline the analysis driver runs (the inline-value engine reads the
//! recorded HIR facts only — no typeck pass is on its path), the HIR is
//! stored in `AnalysisDatabase` exactly like the driver stores it, and
//! `inline_values` — the function the router registers for
//! `InlineValueRequest` — turns the requested range into the wire
//! variable lookups. Every offset is derived from the document by token
//! search, so the walkthrough cannot silently drift from `DOC`.

use glyim_core::{CrateId, Interner};
use glyim_def_map::build_def_map;
use glyim_frontend::parse_to_syntax;
use glyim_hir::pipeline_api::lower_crate_for_pipeline;
use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::inline_value::inline_values;
use lsp_types::{InlineValueParams, Position, Range, TextDocumentIdentifier, Uri};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

/// The walkthrough document: one outer binding, one nested block
/// binding, one annotated binding — the shape a debugger frame shows.
/// The annotated binding still answers (a debugger may look up a
/// variable regardless of whether its type was written in the source).
const DOC: &str = "fn main() -> i32 {\n\
                   \x20   let outer = 1;\n\
                   \x20   {\n\
                   \x20       let inner = outer + 1;\n\
                   \x20   }\n\
                   \x20   let annotated: i32 = 3;\n\
                   \x20   outer\n\
                   }\n";

fn main() {
    // Populate the database exactly like the analysis driver does: file
    // map, source map, stored HIRs.
    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let doc_path = PathBuf::from("/walkthrough/inline_value.gly");
    let file_id = file_map.get_or_create(&doc_path);
    db.file_map.write().get_or_create(&doc_path);
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, DOC.to_string()));

    let crate_id = CrateId::from_raw(0);
    let parse_result = parse_to_syntax(DOC, file_id);
    let mut interner = Interner::new();
    let (_def_map, _def_diags) = build_def_map(&parse_result.root, crate_id, interner.clone());
    let (hir, _hir_diags) = lower_crate_for_pipeline(&parse_result.root, &mut interner);
    db.hirs.write().insert(file_id, hir);

    let uri = Uri::from_str(Url::from_file_path(&doc_path).unwrap().as_ref()).unwrap();
    let params_at = |start_line: u32, end_line: u32| InlineValueParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        range: Range {
            start: Position {
                line: start_line,
                character: 0,
            },
            end: Position {
                line: end_line,
                character: 0,
            },
        },
        context: lsp_types::InlineValueContext {
            frame_id: 0,
            stopped_location: Range {
                start: Position {
                    line: start_line,
                    character: 0,
                },
                end: Position {
                    line: start_line,
                    character: 0,
                },
            },
        },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
    };
    let lines = DOC.lines().count() as u32;

    // The whole-frame request: every recorded binding in span order.
    let frame =
        inline_values(&db, &file_map, &params_at(0, lines)).expect("the frame has bindings");
    println!("textDocument/inlineValue over the whole frame:");
    for value in &frame {
        if let lsp_types::InlineValue::VariableLookup(lookup) = value {
            println!(
                "  {} at {}:{}..{}",
                lookup.variable_name.as_deref().unwrap_or_default(),
                lookup.range.start.line,
                lookup.range.start.character,
                lookup.range.end.character,
            );
        }
    }

    // The nested block's line: the per-line request a debug adapter
    // sends for a stopped frame.
    let inner = inline_values(&db, &file_map, &params_at(3, 4)).expect("line 3 binds `inner`");
    println!("textDocument/inlineValue over the stopped line:");
    for value in &inner {
        if let lsp_types::InlineValue::VariableLookup(lookup) = value {
            println!(
                "  {} at {}:{}..{}",
                lookup.variable_name.as_deref().unwrap_or_default(),
                lookup.range.start.line,
                lookup.range.start.character,
                lookup.range.end.character,
            );
        }
    }

    // Honesty pins: the whole frame answers exactly the three bindings in
    // span order (`annotated` included — the request carries no type
    // payload), the stopped line answers only `inner`, and a range over
    // the pure-expression tail answers nothing.
    let names: Vec<&str> = frame
        .iter()
        .map(|value| match value {
            lsp_types::InlineValue::VariableLookup(lookup) => {
                lookup.variable_name.as_deref().unwrap_or_default()
            }
            other => panic!("a variable lookup, got {other:?}"),
        })
        .collect();
    assert_eq!(names, vec!["outer", "inner", "annotated"]);
    let inner_names: Vec<&str> = inner
        .iter()
        .map(|value| match value {
            lsp_types::InlineValue::VariableLookup(lookup) => {
                lookup.variable_name.as_deref().unwrap_or_default()
            }
            other => panic!("a variable lookup, got {other:?}"),
        })
        .collect();
    assert_eq!(inner_names, vec!["inner"]);
    let tail = inline_values(&db, &file_map, &params_at(6, lines));
    assert!(
        tail.is_none(),
        "a range over pure expression lines answers null"
    );
    println!("\nwalkthrough complete: recorded bindings answer the lookups, never a guess.");
}
