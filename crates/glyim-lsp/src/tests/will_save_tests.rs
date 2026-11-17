//! Database-backed tests for the save-time edit request
//! (LSP-WILL-SAVE).
//!
//! Mirrors `range_formatting_tests.rs`: the driver's stored `SourceMap`
//! populates the database exactly like the analysis driver does, and the
//! public `will_save_edits` entry point is exercised end-to-end (URI →
//! open-file lookup → the stored-text policy over the shared formatter
//! core → the wire `TextEdit`s). The engine reads the stored document
//! text only — no parse, HIR, or typeck facts are on its path, so the
//! fixtures populate exactly what the engine reads.

use crate::AnalysisDatabase;
use crate::database::{FileMap, SourceMap};
use crate::will_save::will_save_edits;
use glyim_span::FileId;
use lsp_types::notification::{Notification, WillSaveTextDocument as WillSaveNotification};
use lsp_types::request::{Request, WillSaveWaitUntil};
use lsp_types::{TextDocumentIdentifier, TextDocumentSaveReason, Uri, WillSaveTextDocumentParams};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

/// One formatted head line, one mis-indented middle line, one formatted
/// tail line — the shape an editor hands the server on save after a
/// small edit left the document partially formatted.
const DOC: &str = "fn main() -> i32 {\n    let head = 1;\nlet broken = 2;\n    let tail = 3;\n}\n";

fn uri_for(path: &PathBuf) -> Uri {
    Uri::from_str(Url::from_file_path(path).unwrap().as_ref()).unwrap()
}

/// Populate `db` + `file_map` for one document: the driver's stored
/// source map (the will-save engine reads the stored text only).
fn populate(db: &AnalysisDatabase, file_map: &mut FileMap, path: &PathBuf, source: &str) -> FileId {
    let file_id = db.file_map.write().get_or_create(path);
    let caller_file_id = file_map.get_or_create(path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, source.to_string()));
    file_id
}

/// Params for one document with the given save reason.
fn params_for(path: &PathBuf, reason: TextDocumentSaveReason) -> WillSaveTextDocumentParams {
    WillSaveTextDocumentParams {
        text_document: TextDocumentIdentifier { uri: uri_for(path) },
        reason,
    }
}

/// The unformatted middle line answers one wire `TextEdit` whose range
/// covers exactly the changed line and whose replacement is the
/// formatted form — applying it yields the document the shared
/// formatter core would answer for the whole file.
#[test]
fn an_unformatted_document_answers_the_wire_edit() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/will_save_unformatted.gly");
    populate(&db, &mut file_map, &path, DOC);

    let params = params_for(&path, TextDocumentSaveReason::MANUAL);
    let edits = will_save_edits(&db, &file_map, &params).expect("the middle line drifts");
    assert_eq!(edits.len(), 1, "one changed region, one edit");
    let edit = &edits[0];
    // "fn main() -> i32 {\n" (19) + "    let head = 1;\n" (18): the
    // mis-indented line starts at byte 37 on line 2.
    assert_eq!(edit.range.start.line, 2);
    assert_eq!(edit.range.start.character, 0);
    assert_eq!(edit.range.end.line, 3);
    assert_eq!(edit.range.end.character, 0);
    assert_eq!(edit.new_text, "    let broken = 2;\n");

    // Round trip: the same line/column mapping the request offsets come
    // from maps the wire range back to the drifted line's bytes.
    let source_maps = db.source_maps.read();
    let file_id = file_map.get_by_path(&path).expect("the file is open");
    let sm = source_maps.get(&file_id).expect("the source map is stored");
    let start = sm
        .line_col_to_offset(
            edit.range.start.line as usize,
            edit.range.start.character as usize,
        )
        .expect("the range start maps");
    let end = sm
        .line_col_to_offset(
            edit.range.end.line as usize,
            edit.range.end.character as usize,
        )
        .expect("the range end maps");
    assert_eq!(&sm.source()[start..end], "let broken = 2;\n");
}

/// An already formatted document answers `null` — no edit churn on an
/// idempotent save, the same contract the full-document formatter
/// honours.
#[test]
fn an_already_formatted_document_answers_null() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/will_save_formatted.gly");
    let formatted = "fn main() -> i32 {\n    let p = 5;\n    p\n}\n";
    populate(&db, &mut file_map, &path, formatted);

    let params = params_for(&path, TextDocumentSaveReason::MANUAL);
    assert!(will_save_edits(&db, &file_map, &params).is_none());
}

/// A document the server does not know (not open, never analysed)
/// answers `null`.
#[test]
fn an_unopened_document_answers_null() {
    let db = AnalysisDatabase::new();
    let file_map = FileMap::new();
    let path = PathBuf::from("/test/never_opened_will_save.gly");
    let params = params_for(&path, TextDocumentSaveReason::MANUAL);
    assert!(will_save_edits(&db, &file_map, &params).is_none());
}

/// The save reason is deliberately not consulted: the formatter is
/// reason-agnostic, so `MANUAL` and `AFTER_DELAY` answer the same edits —
/// refining per reason would make the two wire shapes disagree with the
/// shared core.
#[test]
fn the_save_reason_is_not_consulted() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/will_save_reasons.gly");
    populate(&db, &mut file_map, &path, DOC);

    let manual = will_save_edits(
        &db,
        &file_map,
        &params_for(&path, TextDocumentSaveReason::MANUAL),
    );
    let delayed = will_save_edits(
        &db,
        &file_map,
        &params_for(&path, TextDocumentSaveReason::AFTER_DELAY),
    );
    assert_eq!(manual, delayed, "the policy is reason-agnostic");
}

/// The wire contracts the engine implements: the lsp-types request and
/// notification METHOD strings are pinned so an lsp-types upgrade that
/// reshapes the endpoints surfaces here, not on a client's screen.
#[test]
fn wire_methods_are_pinned() {
    assert_eq!(WillSaveWaitUntil::METHOD, "textDocument/willSaveWaitUntil");
    assert_eq!(WillSaveNotification::METHOD, "textDocument/willSave");
}
