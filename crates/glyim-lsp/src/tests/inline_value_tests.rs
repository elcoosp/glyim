//! Database-backed tests for the inline-value request
//! (LSP-INLINE-VALUE).
//!
//! Mirrors `type_hierarchy_tests.rs`: the real parse → HIR pipeline (the
//! analysis driver's own lowering) populates the database exactly like
//! the driver does, and the public `inline_values` entry point is
//! exercised end-to-end (URI → open-file lookup → range offsets → the
//! recorded-binding walk over the stored HIR → wire variable lookups).
//! The engine reads the driver's stored `CrateHir`s — no typeck results
//! are needed on the request path, so fixtures populate only what the
//! engine reads.

use crate::AnalysisDatabase;
use crate::database::FileMap;
use crate::database::SourceMap;
use crate::inline_value::inline_values;
use glyim_span::FileId;
use lsp_types::request::InlineValueRequest;
use lsp_types::{InlineValueParams, Position, Range, TextDocumentIdentifier, Uri};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

/// Two un-annotated bindings, one annotated, one nested in a block — the
/// shapes a debugger frame shows. `annotated` already carries its type in
/// the source but the *binding* is still a variable a debugger may look
/// up, so the engine answers it (unlike the inlay-hint policy, which
/// skips annotated bindings as type-noise).
const DOC: &str = "fn main() -> i32 {\n\
                   \x20   let p = 5;\n\
                   \x20   let q = p + 1;\n\
                   \x20   let annotated: i32 = 2;\n\
                   \x20   {\n\
                   \x20       let nested = q;\n\
                   \x20   }\n\
                   \x20   p\n\
                   }\n";

fn uri_for(path: &PathBuf) -> Uri {
    Uri::from_str(Url::from_file_path(path).unwrap().as_ref()).unwrap()
}

/// Populate `db` + `file_map` for one document: the driver's parse → HIR
/// pipeline (no typeck — the inline-value engine reads recorded HIR
/// facts only, so the fixture stores exactly what the engine reads).
fn populate(db: &AnalysisDatabase, file_map: &mut FileMap, path: &PathBuf, source: &str) -> FileId {
    let file_id = db.file_map.write().get_or_create(path);
    let caller_file_id = file_map.get_or_create(path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, source.to_string()));
    let parse_result = glyim_frontend::parse_to_syntax(source, file_id);
    let mut interner = glyim_core::Interner::new();
    let (hir, _hir_diags) =
        glyim_hir::pipeline_api::lower_crate_for_pipeline(&parse_result.root, &mut interner);
    db.hirs.write().insert(file_id, hir);
    file_id
}

/// Params over one line range of `DOC`.
fn params_at(path: &PathBuf, start_line: u32, end_line: u32) -> InlineValueParams {
    InlineValueParams {
        text_document: TextDocumentIdentifier { uri: uri_for(path) },
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
    }
}

/// The variable lookups the response carries, as `(name, start.line)`
/// pairs — the shape an editor renders top-down.
fn lookups(values: Vec<lsp_types::InlineValue>) -> Vec<(String, u32)> {
    values
        .into_iter()
        .map(|value| match value {
            lsp_types::InlineValue::VariableLookup(lookup) => (
                lookup.variable_name.unwrap_or_default(),
                lookup.range.start.line,
            ),
            other => panic!("a variable lookup, got {other:?}"),
        })
        .collect()
}

/// A range covering the whole function answers every recorded binding,
/// including the annotated one and the nested block's, in span order.
#[test]
fn the_whole_frame_answers_every_binding_in_span_order() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/inline_value_frame.gly");
    populate(&db, &mut file_map, &path, DOC);

    let params = params_at(&path, 0, DOC.lines().count() as u32);
    let values = inline_values(&db, &file_map, &params).expect("the frame has bindings");
    let names: Vec<(String, u32)> = lookups(values);
    assert_eq!(
        names,
        vec![
            ("p".to_string(), 1),
            ("q".to_string(), 2),
            ("annotated".to_string(), 3),
            ("nested".to_string(), 5),
        ]
    );
}

/// A range covering one line answers only the binding bound there — the
/// per-line request the editor sends for a stopped frame.
#[test]
fn a_one_line_range_answers_only_that_line() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/inline_value_line.gly");
    populate(&db, &mut file_map, &path, DOC);

    let params = params_at(&path, 2, 3);
    let values = inline_values(&db, &file_map, &params).expect("line 2 binds `q`");
    assert_eq!(lookups(values), vec![("q".to_string(), 2)]);
}

/// A range over pure expression lines (no bindings) answers `null` —
/// never a guessed value.
#[test]
fn a_range_without_bindings_answers_null() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/inline_value_empty.gly");
    populate(&db, &mut file_map, &path, DOC);

    let params = params_at(&path, 7, DOC.lines().count() as u32);
    assert!(inline_values(&db, &file_map, &params).is_none());
}

/// The wire contract the engine implements: the lsp-types request type's
/// METHOD string is pinned so an lsp-types upgrade that reshapes the
/// endpoint surfaces here, not on a client's screen.
#[test]
fn wire_method_is_pinned() {
    use lsp_types::request::Request;
    assert_eq!(InlineValueRequest::METHOD, "textDocument/inlineValue");
}

/// The lookup ranges map the recorded `pat_span`s exactly: the variable
/// lookup's extent is the bound identifier, not the whole `let`.
#[test]
fn lookup_ranges_are_the_identifiers() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/inline_value_extent.gly");
    populate(&db, &mut file_map, &path, DOC);

    let params = params_at(&path, 1, 2);
    let values = inline_values(&db, &file_map, &params).expect("line 1 binds `p`");
    match values.into_iter().next().unwrap() {
        lsp_types::InlineValue::VariableLookup(lookup) => {
            // `let p = 5;` — `p` starts at column 8 (four spaces + `let `)
            // and is one character wide.
            assert_eq!(lookup.range.start.line, 1);
            assert_eq!(lookup.range.start.character, 8);
            assert_eq!(lookup.range.end.character, 9);
        }
        other => panic!("a variable lookup, got {other:?}"),
    }
}

/// A document the server does not know (not open, never analysed)
/// answers `null`.
#[test]
fn an_unopened_document_answers_null() {
    let db = AnalysisDatabase::new();
    let file_map = FileMap::new();
    let path = PathBuf::from("/test/never_opened.g");
    let params = params_at(&path, 0, 10);
    assert!(inline_values(&db, &file_map, &params).is_none());
    let _ = FileId::from_raw(0); // keep the span crate in the test's deps
}
