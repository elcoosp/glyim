//! Policy-level tests for the goto-declaration engine (LSP-DECLARATION).
//!
//! Declaration *choice* is decided through an in-memory [`MockSource`]
//! implementing [`DeclarationSource`], not the live `AnalysisDatabase` —
//! the point of the trait seam: the policy (an import declaration beats a
//! same-file definition, and a cross-file definition without an import has
//! no local declaration to name) is verified decoupled from any parse tree,
//! lock, or index. The parse-tree-backed path (real `use` statements,
//! token extraction, wire positions) is covered in `goto_declaration_tests`.

use crate::goto_declaration::{DeclarationSource, declaration_target};
use crate::symbol_index::{DefinitionLocation, SymbolKind};
use crate::tests::helpers::make_span;
use glyim_span::FileId;

fn file(id: u32) -> FileId {
    FileId::from_usize(id as usize)
}

/// In-memory declaration store: the policy-side test double. Import
/// declarations are recorded as `(binding file, name, span)` tuples;
/// definitions as the same `(kind, location)` pairs the symbol index
/// records.
#[derive(Default)]
struct MockSource {
    imports: Vec<(FileId, String, usize, usize)>,
    definitions: Vec<(String, SymbolKind, DefinitionLocation)>,
}

impl MockSource {
    fn add_import(&mut self, file: FileId, name: &str, lo: usize, hi: usize) {
        self.imports.push((file, name.to_string(), lo, hi));
    }

    fn add_definition(&mut self, name: &str, kind: SymbolKind, file: FileId, lo: usize, hi: usize) {
        self.definitions.push((
            name.to_string(),
            kind,
            DefinitionLocation {
                file_id: file,
                span: make_span(file, lo, hi),
            },
        ));
    }
}

impl DeclarationSource for MockSource {
    fn import_declarations_of(&self, file_id: FileId, name: &str) -> Vec<glyim_span::Span> {
        self.imports
            .iter()
            .filter(|(f, n, _, _)| *f == file_id && n == name)
            .map(|(_, _, lo, hi)| make_span(file_id, *lo, *hi))
            .collect()
    }

    fn definitions_of(&self, name: &str) -> Vec<(SymbolKind, DefinitionLocation)> {
        self.definitions
            .iter()
            .filter(|(n, _, _)| n == name)
            .map(|(_, kind, def)| (*kind, def.clone()))
            .collect()
    }
}

#[test]
fn import_declaration_beats_same_file_definition() {
    // `Point` is imported (the `use` statement) *and* a same-named
    // struct exists in the file: the declaration is the import — that
    // is what the user wrote to bring the name into scope.
    let a = file(0);
    let mut src = MockSource::default();
    src.add_import(a, "Point", 0, 25);
    src.add_definition("Point", SymbolKind::Struct, a, 40, 70);

    let target = declaration_target(&src, a, "Point").expect("an import exists");
    assert_eq!(target.file_id, a);
    assert_eq!(target.span.lo.to_usize(), 0, "the import span wins");
    assert_eq!(target.span.hi.to_usize(), 25);
}

#[test]
fn same_file_definition_used_when_not_imported() {
    // `helper` is declared locally with no `use` statement: the
    // declaration is the definition site itself (the LSP spec's
    // "declared in the same file" case — the response names this file).
    let a = file(0);
    let mut src = MockSource::default();
    src.add_definition("helper", SymbolKind::Function, a, 30, 55);

    let target = declaration_target(&src, a, "helper").expect("a same-file definition exists");
    assert_eq!(target.file_id, a);
    assert_eq!(target.span.lo.to_usize(), 30);
}

#[test]
fn cross_file_definition_without_import_answers_none() {
    // `Point` lives in another file and is never imported here: there is
    // no local declaration to jump to. Answering the foreign definition
    // would turn `declaration` into a second `definition` request — the
    // honest answer is `null`.
    let a = file(0);
    let b = file(1);
    let mut src = MockSource::default();
    src.add_definition("Point", SymbolKind::Struct, b, 0, 30);

    assert!(
        declaration_target(&src, a, "Point").is_none(),
        "a cross-file definition is not this file's declaration"
    );
}

#[test]
fn import_in_another_file_is_ignored() {
    // The import lives in file B but the cursor is in file A: the
    // per-file import table must not leak across files.
    let a = file(0);
    let b = file(1);
    let mut src = MockSource::default();
    src.add_import(b, "Point", 0, 25);

    assert!(
        declaration_target(&src, a, "Point").is_none(),
        "file A has no import and no same-file definition"
    );
}

#[test]
fn first_import_declaration_wins() {
    // Two imports bind the same name (a shadowing re-import): the first
    // recorded one answers, the same tie-break goto-definition applies.
    let a = file(0);
    let mut src = MockSource::default();
    src.add_import(a, "Point", 0, 25);
    src.add_import(a, "Point", 40, 65);

    let target = declaration_target(&src, a, "Point").expect("an import exists");
    assert_eq!(target.span.lo.to_usize(), 0, "first recorded import wins");
}

#[test]
fn zero_width_definitions_are_skipped() {
    // The symbol index records body locals without usable extents today
    // (span 0..0): such a record cannot name a jump target, so the
    // policy skips it and the request answers `null` instead of aiming
    // the client at the top of the file.
    let a = file(0);
    let mut src = MockSource::default();
    src.add_definition("p", SymbolKind::Local, a, 0, 0);

    assert!(
        declaration_target(&src, a, "p").is_none(),
        "a zero-width definition extent is not an answerable declaration"
    );
}

#[test]
fn unknown_name_answers_none() {
    let a = file(0);
    let src = MockSource::default();
    assert!(declaration_target(&src, a, "never_seen").is_none());
}
