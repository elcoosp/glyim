//! Consumer walkthrough of the document-highlight seam (LSP-HIGHLIGHT):
//! the `textDocument/documentHighlight` capability the editor uses to
//! light up every occurrence of the symbol under the cursor.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lsp --example document_highlight_walkthrough
//! ```
//!
//! The example drives the public path end-to-end without a client
//! process: occurrences are injected through `insert_test_reference`
//! (the same seam the request-path tests use), a `SourceMap` carries the
//! document, and `document_highlights` — the function the router
//! registers for `DocumentHighlightRequest` — turns a cursor position
//! into typed highlight kinds (`Text` for the definition, `Read`/`Write`
//! for accesses). A second request from inside another function shows
//! the scope-owner discipline (T129 [LSP-5]): a local `x` highlights
//! only within its owning item, while an item-level name (`foo`)
//! highlights everywhere. Every offset is derived from the document by
//! token search, so the walkthrough cannot silently drift from `DOC`.

use glyim_lsp::AnalysisDatabase;
use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::document_highlight::document_highlights;
use glyim_lsp::reference_graph::{AccessKind, Reference, ReferenceGraph, ReferenceKind};
use glyim_span::{ByteIdx, FileId, Span, SyntaxContext};
use lsp_types::{
    DocumentHighlightParams, PartialResultParams, Position, TextDocumentIdentifier,
    TextDocumentPositionParams, WorkDoneProgressParams,
};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

/// The walkthrough document: two functions, each holding its own local
/// `x`, plus an item-level function `foo` referenced by both.
const DOC: &str = "fn foo() -> i32 { 42 }\n\
                   fn first() -> i32 {\n\
                   \x20   let x = foo();\n\
                   \x20   x = x + 1;\n\
                   \x20   x\n\
                   }\n\
                   fn second() -> i32 {\n\
                   \x20   let x = foo();\n\
                   \x20   x\n\
                   }\n";

/// Byte offset of the first `needle` at or after `from` in DOC.
fn find_at(from: usize, needle: &str) -> usize {
    DOC[from..]
        .find(needle)
        .map(|i| from + i)
        .unwrap_or_else(|| panic!("walkthrough fixture drifted: `{needle}` not found after {from}"))
}

/// Byte offset of the `x` token in the next `let x` statement at/after
/// `from` (4 bytes after the `let` keyword starts: `let x` → x at +4).
fn local_x_after(from: usize) -> usize {
    find_at(from, "let x") + 4
}

fn span(file_id: FileId, lo: usize, hi: usize) -> Span {
    Span::new(
        file_id,
        ByteIdx::from_usize(lo),
        ByteIdx::from_usize(hi),
        SyntaxContext::ROOT,
    )
}

/// Registers one occurrence of `name` at `[lo, lo+1)` in `file_id`.
fn add(
    graph: &mut ReferenceGraph,
    name: &str,
    file_id: FileId,
    lo: usize,
    is_definition: bool,
    is_item_level: bool,
    owner: Option<u32>,
    access: AccessKind,
) {
    let kind = if is_definition {
        ReferenceKind::Definition
    } else {
        ReferenceKind::Variable
    };
    graph.insert_test_reference(
        name,
        Reference {
            file_id,
            span: span(file_id, lo, lo + 1),
            is_definition,
            kind,
            access,
            def_id: None,
            owner_item_id: owner,
            is_item_level,
        },
    );
}

/// Wire params for a cursor at `line:character`.
fn highlight_params(path: &PathBuf, position: Position) -> DocumentHighlightParams {
    let uri = lsp_types::Uri::from_str(
        &lsp_types::Uri::from_str(Url::from_file_path(path).unwrap().as_ref())
            .unwrap()
            .to_string(),
    )
    .unwrap();
    DocumentHighlightParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position,
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    }
}

/// Line/character for a byte offset (the document is single-line-per-item,
/// so the offset maps onto its `\n`-separated line directly).
fn position_of(offset: usize) -> Position {
    let line = DOC[..offset].matches('\n').count();
    let line_start = DOC[..offset].rfind('\n').map_or(0, |i| i + 1);
    Position {
        line: line as u32,
        character: (offset - line_start) as u32,
    }
}

fn kind_name(kind: Option<lsp_types::DocumentHighlightKind>) -> &'static str {
    if kind == Some(lsp_types::DocumentHighlightKind::TEXT) {
        "Text"
    } else if kind == Some(lsp_types::DocumentHighlightKind::WRITE) {
        "Write"
    } else {
        "Read"
    }
}

fn print_highlights(label: &str, highlights: &Option<Vec<lsp_types::DocumentHighlight>>) {
    match highlights {
        Some(hs) => {
            println!("{label} — {} highlight(s):", hs.len());
            for h in hs {
                println!(
                    "  line {}:{}..{}:{}  {}",
                    h.range.start.line,
                    h.range.start.character,
                    h.range.end.line,
                    h.range.end.character,
                    kind_name(h.kind),
                );
            }
        }
        None => println!("{label} — null (no occurrence under the cursor)"),
    }
}

fn main() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/walkthrough/highlight.gly");
    let file_id = file_map.get_or_create(&path);
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, DOC.to_string()));

    // Derive every occurrence offset from the document (self-verifying).
    let foo_def = find_at(0, "fn foo") + 3; // the `foo` in `fn foo`
    let foo_in_first = find_at(0, "let x = foo()") + 8;
    let foo_in_second = find_at(find_at(0, "fn second"), "let x = foo()") + 8;
    let first_x_def = local_x_after(0);
    let first_x_assign = find_at(first_x_def, "x = x + 1");
    let first_x_write = first_x_assign;
    let first_x_read_rhs = first_x_assign + 4; // the `x` in `x + 1`
    let first_x_use = find_at(first_x_read_rhs + 1, "x\n");
    let second_x_def = local_x_after(find_at(0, "fn second"));
    let second_x_use = find_at(second_x_def + 1, "x\n");

    // Occurrence injection — in production the analysis driver builds this
    // graph from the HIR (`ReferenceGraph::build_from_hir`); the test-seam
    // equivalent keeps the walkthrough self-contained.
    let mut graph = ReferenceGraph::new();
    // `foo`: item-level, every occurrence highlights regardless of scope.
    add(
        &mut graph,
        "foo",
        file_id,
        foo_def,
        true,
        true,
        None,
        AccessKind::Read,
    );
    add(
        &mut graph,
        "foo",
        file_id,
        foo_in_first,
        false,
        true,
        None,
        AccessKind::Read,
    );
    add(
        &mut graph,
        "foo",
        file_id,
        foo_in_second,
        false,
        true,
        None,
        AccessKind::Read,
    );
    // `x` in `first` (owner 1): definition, assignment write, reads.
    add(
        &mut graph,
        "x",
        file_id,
        first_x_def,
        true,
        false,
        Some(1),
        AccessKind::Read,
    );
    add(
        &mut graph,
        "x",
        file_id,
        first_x_write,
        false,
        false,
        Some(1),
        AccessKind::Write,
    );
    add(
        &mut graph,
        "x",
        file_id,
        first_x_read_rhs,
        false,
        false,
        Some(1),
        AccessKind::Read,
    );
    add(
        &mut graph,
        "x",
        file_id,
        first_x_use,
        false,
        false,
        Some(1),
        AccessKind::Read,
    );
    // `x` in `second` (owner 2): a different local with the same name.
    add(
        &mut graph,
        "x",
        file_id,
        second_x_def,
        true,
        false,
        Some(2),
        AccessKind::Read,
    );
    add(
        &mut graph,
        "x",
        file_id,
        second_x_use,
        false,
        false,
        Some(2),
        AccessKind::Read,
    );
    *db.reference_graph.write() = graph;

    // 1. Cursor on the definition of `first`'s `x`: only that function's
    //    occurrences light up — `second`'s `x` is a distinct local even
    //    though the graph is name-keyed.
    let params = highlight_params(&path, position_of(first_x_def));
    let scoped = document_highlights(&db, &file_map, &params);
    print_highlights("cursor on `first`'s `x` definition", &scoped);
    assert!(scoped.is_some(), "cursor sits on a graphed definition");

    // 2. Cursor on the item-level `foo` in `second`: all three occurrences
    //    of `foo` highlight — item-level names are not scope-filtered.
    let params = highlight_params(&path, position_of(foo_in_second));
    let global = document_highlights(&db, &file_map, &params);
    print_highlights("cursor on `foo` call in `second`", &global);
    assert_eq!(
        global.map_or(0, |hs| hs.len()),
        3,
        "item-level occurrences are global"
    );

    // 3. Cursor off any recorded occurrence (the `fn` keyword at 0:0): the
    //    server answers null, the same miss discipline as find-references.
    let params = highlight_params(
        &path,
        Position {
            line: 0,
            character: 0,
        },
    );
    let miss = document_highlights(&db, &file_map, &params);
    print_highlights("cursor at 0:0 (`fn` keyword)", &miss);
    assert!(miss.is_none(), "`fn` is not a graphed symbol");

    // 4. Prepare-rename (LSP-PREPARE-RENAME): the same occurrence seam from
    //    face 1 answers "is this renameable, and over which exact token?".
    //    On `first`'s `x` the server pre-fills the rename input with the
    //    identifier; on the `fn` keyword it answers null, so a client never
    //    opens the input on a non-symbol.
    let prepare_on_symbol = glyim_lsp::prepare_rename::prepare_rename(
        &db,
        &file_map,
        &lsp_types::TextDocumentPositionParams {
            text_document: lsp_types::TextDocumentIdentifier {
                uri: lsp_types::Uri::from_str(
                    &lsp_types::Uri::from_str(Url::from_file_path(&path).unwrap().as_ref())
                        .unwrap()
                        .to_string(),
                )
                .unwrap(),
            },
            position: position_of(first_x_def),
        },
    );
    match &prepare_on_symbol {
        Some(lsp_types::PrepareRenameResponse::RangeWithPlaceholder { range, placeholder }) => {
            println!(
                "\nprepare rename on `first`'s `x` — range {}:{}..{}:{}, placeholder `{placeholder}`",
                range.start.line, range.start.character, range.end.line, range.end.character,
            );
        }
        other => panic!("expected RangeWithPlaceholder, got {other:?}"),
    }
    let prepare_on_keyword = glyim_lsp::prepare_rename::prepare_rename(
        &db,
        &file_map,
        &lsp_types::TextDocumentPositionParams {
            text_document: lsp_types::TextDocumentIdentifier {
                uri: lsp_types::Uri::from_str(
                    &lsp_types::Uri::from_str(Url::from_file_path(&path).unwrap().as_ref())
                        .unwrap()
                        .to_string(),
                )
                .unwrap(),
            },
            position: Position {
                line: 0,
                character: 0,
            },
        },
    );
    println!(
        "prepare rename on `fn` keyword — response: {:?}",
        prepare_on_keyword.is_none()
    );
    assert!(prepare_on_keyword.is_none(), "`fn` is not renameable");
}
