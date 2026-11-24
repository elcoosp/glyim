//! Database-backed tests for the type-hierarchy requests
//! (LSP-TYPE-HIERARCHY).
//!
//! Mirrors `implementation_tests.rs`: the real parse → HIR pipeline (the
//! analysis driver's own lowering) populates the database exactly like the
//! driver does, and the public `prepare_type_hierarchy` /
//! `type_supertypes` / `type_subtypes` entry points are exercised
//! end-to-end (URI → open-file lookup → cursor offset → identifier
//! extraction → recorded-item choice → wire items, then the echoed item →
//! extent re-join → hierarchy walk). The engine reads the driver's stored
//! `CrateHir`s — no typeck results are needed on the request path, so
//! fixtures populate only what the engine reads.

use crate::AnalysisDatabase;
use crate::database::FileMap;
use crate::database::SourceMap;
use crate::type_hierarchy::{prepare_type_hierarchy, type_subtypes, type_supertypes};
use glyim_span::FileId;
use lsp_types::{
    Position, SymbolKind, TypeHierarchyItem, TypeHierarchyPrepareParams,
    TypeHierarchySubtypesParams, TypeHierarchySupertypesParams, Uri,
};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

fn uri_for(path: &PathBuf) -> Uri {
    Uri::from_str(Url::from_file_path(path).unwrap().as_ref()).unwrap()
}

/// One trait, one struct, one trait impl, one inherent impl — the exact
/// shape `implementation_tests.rs` pins, so both engines read the same
/// recorded facts.
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
                   struct Solo { z: i32 }\n\
                   fn main() -> i32 {\n\
                   let p = P { n: 5 };\n\
                   p.name()\n\
                   }\n";

/// A second session file: another trait impl of the same trait in another
/// file, an enum impl (the enum kind mapping), and a second impl of the
/// first file's type — the multi-file answers the policy must order
/// deterministically by (file id, span start).
const DOC_OTHER: &str = "struct Q { m: i32 }\n\
     impl Greet for Q {\n\
     fn name(&self) -> i32 { self.m }\n\
     }\n\
     enum E { V }\n\
     impl Greet for E {\n\
     fn name(&self) -> i32 { 0 }\n\
     }\n\
     impl Greet for P {\n\
     fn name(&self) -> i32 { 0 }\n\
     }\n";

/// Byte offset of the first `needle` in `DOC` (a cursor parked anywhere
/// inside the identifier resolves the whole token).
fn offset_of(source: &str, needle: &str) -> usize {
    source
        .find(needle)
        .unwrap_or_else(|| panic!("fixture drifted: `{needle}` not in DOC"))
}

/// The byte extent of a one-line item: from the needle's start to the
/// needle's end (the whole node text, no trailing newline).
fn line_extent(source: &str, needle: &str) -> (usize, usize) {
    let lo = offset_of(source, needle);
    (lo, lo + needle.len())
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
/// pipeline (no typeck — the type-hierarchy engine reads recorded HIR
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

/// Prepare params at a byte offset of `DOC` (the same line/UTF-16-column
/// walk the wire position mapping uses).
fn prepare_params_at(path: &PathBuf, offset: usize) -> TypeHierarchyPrepareParams {
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
    TypeHierarchyPrepareParams {
        text_document_position_params: lsp_types::TextDocumentPositionParams {
            text_document: lsp_types::TextDocumentIdentifier { uri: uri_for(path) },
            position: Position { line, character },
        },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
    }
}

fn echo_params(
    item: TypeHierarchyItem,
) -> (TypeHierarchySupertypesParams, TypeHierarchySubtypesParams) {
    (
        TypeHierarchySupertypesParams {
            item: item.clone(),
            work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
            partial_result_params: lsp_types::PartialResultParams::default(),
        },
        TypeHierarchySubtypesParams {
            item,
            work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
            partial_result_params: lsp_types::PartialResultParams::default(),
        },
    )
}

/// Prepare on a struct name answers the one-item list with the recorded
/// kind and the exact item extent (both `range` and `selection_range` —
/// the whole item is the recorded extent, the same pin call-hierarchy
/// makes).
#[test]
fn prepare_answers_the_struct_item_with_its_extent() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/type_hierarchy.gly");
    populate(&db, &mut file_map, &path, DOC);

    let (lo, hi) = line_extent(DOC, "struct P { n: i32 }");
    let params = prepare_params_at(&path, offset_of(DOC, "struct P { n: i32 }") + 7);
    let items = prepare_type_hierarchy(&db, &file_map, &params).expect("P prepares");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "P");
    assert_eq!(items[0].kind, SymbolKind::STRUCT);
    let expected = expected_range(DOC, lo, hi);
    assert_range(&items[0].range, expected);
    assert_range(&items[0].selection_range, expected);
}

/// Prepare on a trait name answers an Interface-kind item; prepare on a
/// function name (the impl method, the free `main`) answers `null` —
/// functions are call-hierarchy territory, not type-hierarchy roots.
#[test]
fn prepare_classifies_traits_and_rejects_functions() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/type_hierarchy_traits.gly");
    populate(&db, &mut file_map, &path, DOC);

    let params = prepare_params_at(&path, offset_of(DOC, "trait Greet") + 6);
    let items = prepare_type_hierarchy(&db, &file_map, &params).expect("Greet prepares");
    assert_eq!(items[0].name, "Greet");
    assert_eq!(items[0].kind, SymbolKind::INTERFACE);

    for cursor in ["fn name(&self)", "fn shout(&self)", "fn main()"] {
        let params = prepare_params_at(&path, offset_of(DOC, cursor) + 3);
        assert!(
            prepare_type_hierarchy(&db, &file_map, &params).is_none(),
            "cursor on `{cursor}` must not prepare a type hierarchy item"
        );
    }
}

/// Supertypes of the echoed struct item answer one item per trait impl
/// site — the trait impl in the same file first (file id 0), the second
/// impl from the other session file after it — and the inherent impl
/// (`impl P`) never appears.
#[test]
fn supertypes_answer_trait_impls_in_deterministic_order() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/type_hierarchy_supers.gly");
    populate(&db, &mut file_map, &path, DOC);
    let path_other = PathBuf::from("/test/type_hierarchy_supers_other.gly");
    populate(&db, &mut file_map, &path_other, DOC_OTHER);

    let params = prepare_params_at(&path, offset_of(DOC, "struct P { n: i32 }") + 7);
    let items = prepare_type_hierarchy(&db, &file_map, &params).expect("P prepares");
    let (supertypes_params, _subtypes_params) = echo_params(items.into_iter().next().unwrap());
    let supers = type_supertypes(&db, &file_map, &supertypes_params).expect("P has impls");
    assert_eq!(supers.len(), 2, "one member per recorded trait impl of P");
    assert!(supers.iter().all(|item| item.name == "Greet"));
    assert!(supers.iter().all(|item| item.kind == SymbolKind::INTERFACE));

    // The first member is the impl block in the cursor's own file, at the
    // `impl Greet for P { ... }` extent; the second is the impl block in
    // the other session file (its extent ends at the block's closing `}`
    // — the trailing newline is source whitespace outside the node).
    let trait_impl_lo = offset_of(DOC, "impl Greet for P");
    let trait_impl_hi = offset_of(DOC, "\nimpl P {");
    let expected_first = expected_range(DOC, trait_impl_lo, trait_impl_hi);
    assert_range(&supers[0].range, expected_first);
    let other_impl_lo = offset_of(DOC_OTHER, "impl Greet for P");
    let other_impl_hi = DOC_OTHER.rfind('}').expect("closing brace") + 1;
    let expected_second = expected_range(DOC_OTHER, other_impl_lo, other_impl_hi);
    assert_range(&supers[1].range, expected_second);
}

/// Supertypes of a type with no trait impls (a prepared-but-unimplemented
/// struct) and supertypes of a trait (no supertrait facts are recorded)
/// answer `null` honestly — nothing is guessed.
#[test]
fn supertypes_answer_null_without_recorded_facts() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/type_hierarchy_nulls.gly");
    populate(&db, &mut file_map, &path, DOC);

    let solo_params = prepare_params_at(&path, offset_of(DOC, "struct Solo { z: i32 }") + 7);
    let solo = prepare_type_hierarchy(&db, &file_map, &solo_params)
        .expect("Solo prepares")
        .into_iter()
        .next()
        .unwrap();
    let (solo_supertypes, _) = echo_params(solo);
    assert!(type_supertypes(&db, &file_map, &solo_supertypes).is_none());

    let greet_params = prepare_params_at(&path, offset_of(DOC, "trait Greet") + 6);
    let greet = prepare_type_hierarchy(&db, &file_map, &greet_params)
        .expect("Greet prepares")
        .into_iter()
        .next()
        .unwrap();
    let (greet_supertypes, _) = echo_params(greet);
    assert!(type_supertypes(&db, &file_map, &greet_supertypes).is_none());
}

/// Subtypes of the echoed trait item answer one item per impl self type,
/// ordered by (file id, span start): `P` from the first session file,
/// then `Q`, `E`, `P` from the second — with the enum impl answering an
/// Enum-kind item, resolved from the recorded classification.
#[test]
fn subtypes_answer_impl_self_types_in_deterministic_order() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/type_hierarchy_subs.gly");
    populate(&db, &mut file_map, &path, DOC);
    let path_other = PathBuf::from("/test/type_hierarchy_subs_other.gly");
    populate(&db, &mut file_map, &path_other, DOC_OTHER);

    let params = prepare_params_at(&path, offset_of(DOC, "trait Greet") + 6);
    let items = prepare_type_hierarchy(&db, &file_map, &params).expect("Greet prepares");
    let (_supertypes_params, subtypes_params) = echo_params(items.into_iter().next().unwrap());
    let subs = type_subtypes(&db, &file_map, &subtypes_params).expect("Greet has impls");
    let names: Vec<&str> = subs.iter().map(|item| item.name.as_str()).collect();
    assert_eq!(names, vec!["P", "Q", "E", "P"]);

    let kinds: Vec<SymbolKind> = subs.iter().map(|item| item.kind).collect();
    assert_eq!(
        kinds,
        vec![
            SymbolKind::STRUCT,
            SymbolKind::STRUCT,
            SymbolKind::ENUM,
            SymbolKind::STRUCT
        ]
    );

    // Each member anchors its impl block: the first is DOC's
    // `impl Greet for P { ... }`, the rest are DOC_OTHER's impls in
    // source order.
    let first_lo = offset_of(DOC, "impl Greet for P");
    let first_hi = offset_of(DOC, "\nimpl P {");
    assert_range(&subs[0].range, expected_range(DOC, first_lo, first_hi));
    let q_impl_lo = offset_of(DOC_OTHER, "impl Greet for Q");
    let q_impl_hi = offset_of(DOC_OTHER, "\nenum E");
    assert_range(
        &subs[1].range,
        expected_range(DOC_OTHER, q_impl_lo, q_impl_hi),
    );
}

/// Subtypes of a struct item (a type with no recorded sub-hierarchy) and
/// a fabricated echo (the right name at the wrong extent) answer `null`
/// — the echo is re-joined against the recorded facts, never trusted.
#[test]
fn subtypes_answer_null_for_types_and_fabricated_echoes() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/type_hierarchy_fabricated.gly");
    populate(&db, &mut file_map, &path, DOC);

    let params = prepare_params_at(&path, offset_of(DOC, "struct P { n: i32 }") + 7);
    let p = prepare_type_hierarchy(&db, &file_map, &params)
        .expect("P prepares")
        .into_iter()
        .next()
        .unwrap();
    let (_, p_subtypes) = echo_params(p.clone());
    assert!(type_subtypes(&db, &file_map, &p_subtypes).is_none());

    // Same name, wrong extent: the echo does not re-join against any
    // recorded item, so both walks answer null.
    let mut forged = p.clone();
    forged.selection_range = lsp_types::Range {
        start: Position {
            line: 99,
            character: 0,
        },
        end: Position {
            line: 99,
            character: 1,
        },
    };
    let (forged_supertypes, forged_subtypes) = echo_params(forged);
    assert!(type_supertypes(&db, &file_map, &forged_supertypes).is_none());
    assert!(type_subtypes(&db, &file_map, &forged_subtypes).is_none());
}

/// The wire contract the engine implements: the lsp-types request types'
/// METHOD strings are pinned so an lsp-types upgrade that reshapes the
/// endpoints surfaces here, not on a client's screen.
#[test]
fn wire_methods_are_pinned() {
    use lsp_types::request::Request;
    assert_eq!(
        lsp_types::request::TypeHierarchyPrepare::METHOD,
        "textDocument/prepareTypeHierarchy"
    );
    assert_eq!(
        lsp_types::request::TypeHierarchySupertypes::METHOD,
        "typeHierarchy/supertypes"
    );
    assert_eq!(
        lsp_types::request::TypeHierarchySubtypes::METHOD,
        "typeHierarchy/subtypes"
    );
}
