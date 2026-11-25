//! Policy-level tests for the moniker engine (LSP-MONIKER).
//!
//! Mirrors `type_hierarchy.rs` and `goto_declaration.rs`: the *policy*
//! (which recorded fact answers the cursor, and which identifier it
//! carries) is pinned through an in-memory [`MockSource`] implementing
//! [`crate::moniker::MonikerSource`], decoupled from any parse tree,
//! symbol index, or lock. The production source over the driver's stored
//! `SymbolIndex` + parse trees and the full wire request are covered
//! end-to-end in `moniker_tests.rs`.

use crate::moniker::{FactKind, MonikerFact, MonikerSource, RecordedSymbol, SpelledImport};
use crate::symbol_index::DefinitionLocation;
use glyim_span::{FileId, Span, SyntaxContext};

/// In-memory moniker store: the policy-side test double. Recorded symbols
/// and spelled imports are listed in insertion order; the policy applies
/// its own containment, ordering, dedupe, and skip rules on top.
#[derive(Default)]
struct MockSource {
    symbols: Vec<RecordedSymbol>,
    imports: Vec<(FileId, SpelledImport)>,
}

impl MockSource {
    fn symbol(&mut self, name: &str, path: Option<&str>, file: u32, lo: usize, hi: usize) {
        self.symbols.push(RecordedSymbol {
            name: name.to_string(),
            qualified_path: path.map(str::to_string),
            location: DefinitionLocation {
                file_id: FileId::from_usize(file as usize),
                span: span_of(file, lo, hi),
            },
        });
    }
    fn import(&mut self, file: u32, name: &str, path: &str, lo: usize, hi: usize) {
        self.imports.push((
            FileId::from_usize(file as usize),
            SpelledImport {
                path: path.to_string(),
                span: span_of(file, lo, hi),
            },
        ));
        let _ = name;
    }
}

fn span_of(file: u32, lo: usize, hi: usize) -> Span {
    Span::range(
        FileId::from_usize(file as usize),
        glyim_span::ByteIdx::from_usize(lo),
        glyim_span::ByteIdx::from_usize(hi),
        SyntaxContext::ROOT,
    )
}

impl MonikerSource for MockSource {
    fn recorded_symbols(&self, name: &str) -> Vec<RecordedSymbol> {
        self.symbols
            .iter()
            .filter(|sym| sym.name == name)
            .cloned()
            .collect()
    }
    fn spelled_imports_of(&self, file_id: FileId, _name: &str) -> Vec<SpelledImport> {
        self.imports
            .iter()
            .filter(|(file, _)| *file == file_id)
            .map(|(_, import)| import.clone())
            .collect()
    }
}

/// A cursor parked inside a `use` statement that binds the name answers
/// the import moniker with the spelled path — the fact the cursor sits
/// on decides the kind, not the name alone.
#[test]
fn cursor_on_the_import_answers_the_spelled_path() {
    let mut mock = MockSource::default();
    mock.import(0, "Point", "crate::shapes::Point", 0, 28);
    mock.symbol("Point", Some("Point"), 1, 5, 20);

    let facts = crate::moniker::monikers_at(&mock, FileId::from_usize(0), 14, "Point");
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].kind, FactKind::Import);
    assert_eq!(facts[0].identifier, "crate::shapes::Point");
}

/// A cursor on a recorded definition extent answers the export moniker
/// with the recorded qualified path — even when the file also imports a
/// same-named symbol elsewhere (the cursor is not on the import).
#[test]
fn cursor_on_a_definition_answers_the_export() {
    let mut mock = MockSource::default();
    mock.import(0, "Point", "crate::shapes::Point", 0, 28);
    mock.symbol("Point", Some("Point"), 0, 30, 45);

    let facts = crate::moniker::monikers_at(&mock, FileId::from_usize(0), 35, "Point");
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].kind, FactKind::Export);
    assert_eq!(facts[0].identifier, "Point");
}

/// A bare usage (cursor on neither the import statement nor a definition
/// extent) resolves by how the name is bound in the file: an import
/// binding first — the same order `declaration_target` documents.
#[test]
fn a_bare_usage_of_an_imported_name_answers_the_import() {
    let mut mock = MockSource::default();
    mock.import(0, "Point", "crate::shapes::Point", 0, 28);

    let facts = crate::moniker::monikers_at(&mock, FileId::from_usize(0), 100, "Point");
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].kind, FactKind::Import);
    assert_eq!(facts[0].identifier, "crate::shapes::Point");
}

/// A bare usage of a recorded name with no import binding answers the
/// export moniker — the symbol's identity, at its usage site.
#[test]
fn a_bare_usage_of_a_recorded_name_answers_the_export() {
    let mut mock = MockSource::default();
    mock.symbol("helper", Some("helper"), 0, 40, 52);
    let facts = crate::moniker::monikers_at(&mock, FileId::from_usize(0), 90, "helper");
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].kind, FactKind::Export);
    assert_eq!(facts[0].identifier, "helper");
}

/// A name the session records nothing for — a local the index records
/// with a zero-width extent, or a total miss — answers nothing.
#[test]
fn unrecorded_names_answer_nothing() {
    let mock = MockSource::default();
    assert!(crate::moniker::monikers_at(&mock, FileId::from_usize(0), 10, "ghost").is_empty());
}

/// A recorded symbol whose extent is zero-width (the index records body
/// locals without usable extents today) cannot be vouched as an export:
/// the walk skips it rather than answering a location-less identity.
#[test]
fn zero_width_recorded_extents_are_skipped() {
    let mut mock = MockSource::default();
    mock.symbol("p", Some("p"), 0, 44, 44);
    assert!(crate::moniker::monikers_at(&mock, FileId::from_usize(0), 60, "p").is_empty());
}

/// A recorded symbol with no qualified path recorded (the fact store
/// names no identity for it) is skipped honestly — an empty identifier
/// is not a moniker.
#[test]
fn symbols_without_a_recorded_path_are_skipped() {
    let mut mock = MockSource::default();
    mock.symbol("Shadow", None, 0, 10, 30);
    assert!(crate::moniker::monikers_at(&mock, FileId::from_usize(0), 60, "Shadow").is_empty());
}

/// A blank spelled path (a degenerate use statement) cannot identify an
/// import and is skipped.
#[test]
fn blank_import_paths_are_skipped() {
    let mut mock = MockSource::default();
    mock.import(0, "Point", "", 0, 28);
    assert!(crate::moniker::monikers_at(&mock, FileId::from_usize(0), 10, "Point").is_empty());
}

/// Duplicate identities collapse: two session files recording the same
/// qualified path (or two use statements spelling the same path) answer
/// one moniker, and the export order is (file id, span start)
/// deterministic regardless of the source's walk order.
#[test]
fn duplicates_collapse_and_exports_order_deterministically() {
    let mut mock = MockSource::default();
    // Out-of-order recordings across files, plus one exact duplicate and
    // one same-path duplicate.
    mock.symbol("P", Some("P"), 1, 0, 20);
    mock.symbol("P", Some("P"), 0, 44, 60);
    mock.symbol("P", Some("P"), 0, 44, 60);

    let facts = crate::moniker::monikers_at(&mock, FileId::from_usize(0), 200, "P");
    assert_eq!(facts.len(), 1, "identical identifiers collapse");
    assert_eq!(facts[0].kind, FactKind::Export);
}

/// Two use statements spelling different paths for the same bound name
/// answer two import monikers, in source order — a client may join on
/// either spelling.
#[test]
fn distinct_import_spellings_answer_in_source_order() {
    let mut mock = MockSource::default();
    mock.import(0, "P", "late::P", 50, 60);
    mock.import(0, "P", "early::P", 0, 20);
    let facts = crate::moniker::monikers_at(&mock, FileId::from_usize(0), 200, "P");
    let identifiers: Vec<&str> = facts.iter().map(|f| f.identifier.as_str()).collect();
    assert_eq!(identifiers, vec!["early::P", "late::P"]);
    assert!(facts.iter().all(|f| f.kind == FactKind::Import));
}

/// The fact-to-wire mapping the entry point performs: every fact becomes
/// a `Moniker` under the `glyim` scheme at document uniqueness, and the
/// fact kind maps to the wire kind. Empty facts answer no wire list.
#[test]
fn facts_map_to_the_wire_contract() {
    let fact = MonikerFact {
        kind: FactKind::Import,
        identifier: "crate::shapes::Point".to_string(),
    };
    let wire = crate::moniker::wire_moniker(&fact);
    assert_eq!(wire.scheme, "glyim");
    assert_eq!(wire.identifier, "crate::shapes::Point");
    assert_eq!(wire.unique, lsp_types::UniquenessLevel::Document);
    assert_eq!(wire.kind, Some(lsp_types::MonikerKind::Import));

    let export = crate::moniker::wire_moniker(&MonikerFact {
        kind: FactKind::Export,
        identifier: "helper".to_string(),
    });
    assert_eq!(export.kind, Some(lsp_types::MonikerKind::Export));
}
