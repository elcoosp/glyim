//! Database-backed tests for the goto-implementation request
//! (LSP-IMPLEMENTATION).
//!
//! Mirrors `type_definition_tests.rs`: the real parse → HIR pipeline (the
//! analysis driver's own lowering) populates the database exactly like the
//! driver does, and the public `goto_implementation` entry point is
//! exercised end-to-end (URI → open-file lookup → cursor offset →
//! identifier extraction → impl-site choice → wire locations). The engine
//! reads the driver's stored `CrateHir`s — no typeck results are needed on
//! the request path, so fixtures populate only what the engine reads.

use crate::AnalysisDatabase;
use crate::database::FileMap;
use crate::database::SourceMap;
use crate::implementation::goto_implementation;
use glyim_span::FileId;
use lsp_types::{GotoDefinitionResponse, Position, Uri};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

fn uri_for(path: &PathBuf) -> Uri {
    Uri::from_str(Url::from_file_path(path).unwrap().as_ref()).unwrap()
}

/// One trait, one struct, one trait impl, one inherent impl. The exact
/// syntax the parser accepts (`trait_default_methods.g` fixture shape).
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

/// A second session file: another trait impl of the same trait in another
/// file (the multi-file answer the policy must order deterministically).
const DOC_OTHER: &str =
    "struct Q { m: i32 }\nimpl Greet for Q {\nfn name(&self) -> i32 { self.m }\n}\n";

/// Byte offset of the first `needle` in DOC (a cursor parked anywhere
/// inside the identifier resolves the whole token).
fn offset_of(needle: &str) -> usize {
    DOC.find(needle)
        .unwrap_or_else(|| panic!("fixture drifted: `{needle}` not in DOC"))
}

/// The byte extents of the two impl blocks in DOC: `node_span` records the
/// whole `impl ... { ... }` node, so a target extent runs from the `impl`
/// keyword to the block's closing `}` (the trailing newline is source
/// whitespace outside the node).
fn impl_block_extents() -> (std::ops::Range<usize>, std::ops::Range<usize>) {
    let trait_impl = DOC.find("impl Greet").unwrap()..DOC.find("\nimpl P {").unwrap();
    let inherent = DOC.find("impl P {").unwrap()..DOC.find("\nfn main").unwrap();
    (trait_impl, inherent)
}

/// Map byte offsets to the exact wire positions the engine must answer
/// with: the same `SourceMap` line/UTF-16-column mapping the request path
/// uses, so the expectations cannot drift from the mapping under test.
fn expected_range(source: &str, lo: usize, hi: usize) -> ((u32, u32), (u32, u32)) {
    let sm = SourceMap::new(FileId::from_usize(0), source.to_string());
    sm.span_to_position(lo, hi)
        .map(|((l0, c0), (l1, c1))| ((l0 as u32, c0 as u32), (l1 as u32, c1 as u32)))
        .expect("fixture extents map to positions")
}

fn assert_range(actual: &lsp_types::Range, expected: ((u32, u32), (u32, u32))) {
    assert_eq!(actual.start.line, expected.0.0, "start line");
    assert_eq!(actual.start.character, expected.0.1, "start column");
    assert_eq!(actual.end.line, expected.1.0, "end line");
    assert_eq!(actual.end.character, expected.1.1, "end column");
}

/// Populate `db` + `file_map` for one document: the driver's parse → HIR
/// pipeline (no typeck — the implementation engine reads recorded HIR
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

fn params_at(path: &PathBuf, offset: usize) -> lsp_types::GotoDefinitionParams {
    let (line, character) = {
        let mut line = 0u32;
        let mut last_nl = 0usize;
        for (i, ch) in DOC.char_indices() {
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
            DOC[last_nl..offset]
                .chars()
                .map(char::len_utf16)
                .sum::<usize>() as u32,
        )
    };
    lsp_types::GotoDefinitionParams {
        text_document_position_params: lsp_types::TextDocumentPositionParams {
            text_document: lsp_types::TextDocumentIdentifier { uri: uri_for(path) },
            position: Position { line, character },
        },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    }
}

#[test]
fn goto_implementation_answers_impl_blocks_for_the_trait() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/impl.g");
    populate(&db, &mut file_map, &path, DOC);

    // Cursor on the trait *declaration* name and on the trait name inside
    // the impl head — both resolve the same symbol.
    for needle in ["trait Greet", "impl Greet for P"] {
        let offset = offset_of(needle) + needle.find("Greet").unwrap();
        let params = params_at(&path, offset);
        let response =
            goto_implementation(&db, &file_map, &params).expect("Greet has one impl here");
        match response {
            GotoDefinitionResponse::Array(locations) => {
                assert_eq!(locations.len(), 1, "one impl of Greet in DOC");
                assert_eq!(locations[0].uri, uri_for(&path), "same-file target");
                let extent = impl_block_extents().0;
                let expected = expected_range(DOC, extent.start, extent.end);
                assert_range(&locations[0].range, expected);
            }
            other => panic!("expected a location array, got {other:?}"),
        }
    }
}

#[test]
fn goto_implementation_answers_impl_blocks_for_the_type() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/impl.g");
    populate(&db, &mut file_map, &path, DOC);

    // Cursor on the struct *name* (the type symbol): both the trait impl
    // and the inherent impl implement P. Ordered by span start.
    let offset = offset_of("struct P") + "struct ".len();
    let params = params_at(&path, offset);
    let response = goto_implementation(&db, &file_map, &params).expect("P has two impls here");
    match response {
        GotoDefinitionResponse::Array(locations) => {
            assert_eq!(locations.len(), 2, "trait impl + inherent impl");
            let (trait_impl, inherent) = impl_block_extents();
            assert_range(
                &locations[0].range,
                expected_range(DOC, trait_impl.start, trait_impl.end),
            );
            assert_range(
                &locations[1].range,
                expected_range(DOC, inherent.start, inherent.end),
            );
        }
        other => panic!("expected a location array, got {other:?}"),
    }
}

#[test]
fn goto_implementation_none_for_a_method_or_local_name() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/impl.g");
    populate(&db, &mut file_map, &path, DOC);

    // `name` is a method, `p` a local — neither names an impl site; the
    // request answers null rather than a wrong guess.
    let method_offset = offset_of("fn name(&self) -> i32 { self.n }") + "fn ".len();
    let params = params_at(&path, method_offset);
    assert!(goto_implementation(&db, &file_map, &params).is_none());

    let local_offset = offset_of("let p = P") + "let ".len();
    let params = params_at(&path, local_offset);
    assert!(goto_implementation(&db, &file_map, &params).is_none());
}

#[test]
fn goto_implementation_spans_files_and_orders_deterministically() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/impl.g");
    let first_id = populate(&db, &mut file_map, &path, DOC);
    let other_path = PathBuf::from("/test/impl_other.g");
    let other_id = populate(&db, &mut file_map, &other_path, DOC_OTHER);
    assert!(
        other_id.to_raw() > first_id.to_raw(),
        "the second file allocates the higher id (creation order)"
    );

    // Cursor on the trait in file 1: the answer spans both files, ordered
    // by (file id, span start) — file 1's impl first, then file 2's.
    let offset = offset_of("trait Greet") + "trait ".len();
    let params = params_at(&path, offset);
    let response =
        goto_implementation(&db, &file_map, &params).expect("Greet has impls in both files");
    match response {
        GotoDefinitionResponse::Array(locations) => {
            assert_eq!(locations.len(), 2, "one impl per file");
            assert_eq!(locations[0].uri, uri_for(&path));
            assert_eq!(locations[1].uri, uri_for(&other_path));
            let other_extent = DOC_OTHER.find("impl Greet").unwrap()..DOC_OTHER.len() - 1;
            assert_range(
                &locations[1].range,
                expected_range(DOC_OTHER, other_extent.start, other_extent.end),
            );
        }
        other => panic!("expected a location array, got {other:?}"),
    }
}

#[test]
fn goto_implementation_none_for_an_unknown_document() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/impl.g");
    populate(&db, &mut file_map, &path, DOC);

    let closed = PathBuf::from("/test/never-opened.g");
    let params = params_at(&closed, offset_of("trait Greet"));
    assert!(goto_implementation(&db, &file_map, &params).is_none());
}

#[test]
fn goto_implementation_none_on_a_cursor_outside_any_identifier() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/impl.g");
    populate(&db, &mut file_map, &path, DOC);

    // A cursor parked on the `{` after the impl head carries no
    // identifier; nothing to resolve, so null.
    let brace = offset_of("impl Greet for P {") + "impl Greet for P ".len();
    let params = params_at(&path, brace);
    assert!(goto_implementation(&db, &file_map, &params).is_none());
}
