//! Policy-level tests for the type-definition engine (LSP-TYPE-DEF).
//!
//! Type names are resolved to recorded symbols through an in-memory
//! [`MockSource`] implementing [`TypeSymbolSource`], not the live
//! `AnalysisDatabase` — the point of the trait seam: the *choice* policy
//! (which recorded symbol counts as "the type's definition") is verified
//! decoupled from any `TyCtx`, arena, lock, or source-map machinery. The
//! typeck-backed path (real `Ty` peeling, `TyCtx` name resolution, wire
//! location mapping) is covered in `type_definition_tests`.

use crate::symbol_index::{DefinitionLocation, SymbolKind};
use crate::tests::helpers::make_span;
use crate::type_definition::{TypeSymbolSource, type_target};
use glyim_span::FileId;

/// In-memory symbol store: the policy-side test double.
#[derive(Default)]
struct MockSource {
    entries: Vec<(String, SymbolKind, DefinitionLocation)>,
}

impl MockSource {
    fn add(&mut self, name: &str, kind: SymbolKind, file: u32, lo: usize, hi: usize) {
        let file_id = FileId::from_raw(file);
        self.entries.push((
            name.to_string(),
            kind,
            DefinitionLocation {
                file_id,
                span: make_span(file_id, lo, hi),
            },
        ));
    }
}

impl TypeSymbolSource for MockSource {
    fn definitions_of(&self, type_name: &str) -> Vec<(SymbolKind, DefinitionLocation)> {
        self.entries
            .iter()
            .filter(|(entry, _, _)| entry == type_name)
            .map(|(_, kind, def)| (*kind, def.clone()))
            .collect()
    }
}

#[test]
fn struct_beats_same_named_function() {
    // A function and a struct may share a name; the type-definition
    // request must land on the *type* regardless of recording order, so
    // the function is added first.
    let mut src = MockSource::default();
    src.add("Foo", SymbolKind::Function, 1, 40, 43);
    src.add("Foo", SymbolKind::Struct, 1, 7, 10);

    let target = type_target(&src, "Foo").expect("a struct is recorded");

    assert_eq!(target.file_id, FileId::from_raw(1));
    assert_eq!(target.span.lo.to_usize(), 7, "the struct site, not the fn");
}

#[test]
fn enum_qualifies_as_a_type_definition() {
    let mut src = MockSource::default();
    src.add("Shape", SymbolKind::Enum, 2, 3, 8);

    let target = type_target(&src, "Shape").expect("an enum is recorded");

    assert_eq!(target.file_id, FileId::from_raw(2));
    assert_eq!(target.span.lo.to_usize(), 3);
}

#[test]
fn non_type_symbols_do_not_qualify() {
    // Jumping to a same-named function or local would be a lie: the
    // request is about the *shape*, so only type-kind symbols answer.
    let mut src = MockSource::default();
    src.add("Foo", SymbolKind::Function, 1, 0, 3);
    src.add("Foo", SymbolKind::Local, 1, 9, 12);

    assert!(
        type_target(&src, "Foo").is_none(),
        "a fn/local named Foo is not a type definition"
    );
}

#[test]
fn unknown_type_name_is_none() {
    let src = MockSource::default();
    assert!(type_target(&src, "Missing").is_none());
}

#[test]
fn first_recorded_type_symbol_wins() {
    // The same ADT name can be recorded in several analyzed files
    // (cross-file same-name shapes); the index's order is the driver's
    // analysis order, so the first recorded type site is the answer.
    let mut src = MockSource::default();
    src.add("Pair", SymbolKind::Struct, 1, 5, 9);
    src.add("Pair", SymbolKind::Struct, 2, 12, 16);

    let target = type_target(&src, "Pair").expect("structs are recorded");

    assert_eq!(target.file_id, FileId::from_raw(1), "first recording");
    assert_eq!(target.span.lo.to_usize(), 5);
}
