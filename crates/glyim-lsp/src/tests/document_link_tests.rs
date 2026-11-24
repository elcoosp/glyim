//! Database-backed tests for the document-link request (LSP-DOC-LINK).
//!
//! Mirrors `semantic_tokens_tests.rs`: the parse tree is stored in the
//! `AnalysisDatabase` exactly as the analysis driver stores it, a
//! `SourceMap` carries the document text, and the public `document_links`
//! entry point is exercised end-to-end (URI → open-file lookup →
//! parse-tree token walk → include-call policy → wire links). The
//! expected positions are derived from `DOC` by token search + the same
//! `SourceMap` mapping the request path uses, so the assertions cannot
//! silently drift from the fixture.

use crate::AnalysisDatabase;
use crate::database::FileMap;
use crate::document_link::document_links;
use glyim_frontend::parse_to_syntax;
use glyim_span::FileId;
use lsp_types::{
    DocumentLinkParams, PartialResultParams, TextDocumentIdentifier, Uri, WorkDoneProgressParams,
};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

fn uri_for(path: &PathBuf) -> Uri {
    Uri::from_str(Url::from_file_path(path).unwrap().as_ref()).unwrap()
}

fn params_for(uri: Uri) -> DocumentLinkParams {
    DocumentLinkParams {
        text_document: TextDocumentIdentifier { uri },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    }
}

/// Three file-taking builtin macros and one non-file macro (`env!`). The
/// links must answer in source order; `env!` is not a link site.
const DOC: &str = "include_str!(\"inc/header.g\");\n\
                   include_bytes!(\"inc/data.bin\");\n\
                   fn main() -> i32 {\n\
                   env!(\"PATH\");\n\
                   include!(\"inc/missing.g\");\n\
                   0\n\
                   }\n";

/// A document with a syntax error *after* the include calls: the links
/// must still answer (the capability works on broken code, like
/// selection-range does).
const BROKEN: &str = "include_str!(\"x.g\");\nfn {\n";

/// A non-ASCII comment before the include: the literal's wire range must
/// run through UTF-16 columns (INF-11), which the byte-extent →
/// position mapping has to translate.
const UNICODE_DOC: &str = "// héllo\ninclude!(\"a.g\");\n";

/// Byte offset of the first `needle` in `src`.
fn offset_of(src: &str, needle: &str) -> usize {
    src.find(needle)
        .unwrap_or_else(|| panic!("fixture drifted: `{needle}` not in DOC"))
}

/// Map byte offsets to the exact wire positions the engine must answer
/// with: the same `SourceMap` line/UTF-16-column mapping the request path
/// uses, so the expectations cannot drift from the mapping under test.
fn expected_range(source: &str, lo: usize, hi: usize) -> ((usize, usize), (usize, usize)) {
    let sm = crate::database::SourceMap::new(FileId::from_usize(0), source.to_string());
    sm.span_to_position(lo, hi)
        .unwrap_or_else(|| panic!("fixture extent {lo}..{hi} is not char-boundary safe"))
}

fn lsp_range(source: &str, lo: usize, hi: usize) -> lsp_types::Range {
    let ((start_line, start_col), (end_line, end_col)) = expected_range(source, lo, hi);
    lsp_types::Range {
        start: lsp_types::Position {
            line: start_line as u32,
            character: start_col as u32,
        },
        end: lsp_types::Position {
            line: end_line as u32,
            character: end_col as u32,
        },
    }
}

fn expected_uri(path: &PathBuf) -> Uri {
    uri_for(path)
}

/// Populate the database exactly like the analysis driver does: file map,
/// source map, and the parse tree (green node) for `src` at `path`.
fn populated_db(src: &str, path: &PathBuf) -> (AnalysisDatabase, FileMap, Uri, glyim_span::FileId) {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let file_id = file_map.get_or_create(path);
    db.file_map.write().get_or_create(path);
    db.source_maps.write().insert(
        file_id,
        crate::database::SourceMap::new(file_id, src.to_string()),
    );
    let green = parse_to_syntax(src, file_id).green_node;
    db.syntaxes.write().insert(file_id, green);
    let uri = uri_for(path);
    (db, file_map, uri, file_id)
}

#[test]
fn links_answer_in_source_order_with_absolute_targets() {
    let dir = tempfile::tempdir().unwrap();
    let doc_path = dir.path().join("doc.g");
    let (db, file_map, uri, _file_id) = populated_db(DOC, &doc_path);

    let links = document_links(&db, &file_map, &params_for(uri)).expect("links expected");

    assert_eq!(
        links.len(),
        3,
        "env! must not link; the three include macros must"
    );

    let header_lo = offset_of(DOC, "\"inc/header.g\"");
    let header_hi = header_lo + "\"inc/header.g\"".len();
    let data_lo = offset_of(DOC, "\"inc/data.bin\"");
    let data_hi = data_lo + "\"inc/data.bin\"".len();
    let missing_lo = offset_of(DOC, "\"inc/missing.g\"");
    let missing_hi = missing_lo + "\"inc/missing.g\"".len();

    assert_eq!(links[0].range, lsp_range(DOC, header_lo, header_hi));
    assert_eq!(links[1].range, lsp_range(DOC, data_lo, data_hi));
    assert_eq!(links[2].range, lsp_range(DOC, missing_lo, missing_hi));

    let target_of = |link: &lsp_types::DocumentLink| {
        link.target
            .as_ref()
            .expect("every link carries a target")
            .as_str()
            .to_string()
    };
    assert_eq!(
        target_of(&links[0]),
        expected_uri(&dir.path().join("inc/header.g")).as_str(),
        "target must be the absolute resolved path URI"
    );
    assert_eq!(
        target_of(&links[1]),
        expected_uri(&dir.path().join("inc/data.bin")).as_str()
    );
    // A target that does not exist yet still links: editors report the
    // missing file when clicked, which is the honest answer.
    assert_eq!(
        target_of(&links[2]),
        expected_uri(&dir.path().join("inc/missing.g")).as_str()
    );
}

#[test]
fn every_link_has_a_tooltip() {
    let dir = tempfile::tempdir().unwrap();
    let doc_path = dir.path().join("doc.g");
    let (db, file_map, uri, _file_id) = populated_db(DOC, &doc_path);
    let links = document_links(&db, &file_map, &params_for(uri)).unwrap();
    assert!(links.iter().all(|l| l.tooltip.is_some()));
    assert!(links.iter().all(|l| l.data.is_none()));
}

#[test]
fn broken_code_still_links() {
    let dir = tempfile::tempdir().unwrap();
    let doc_path = dir.path().join("broken.g");
    let (db, file_map, uri, _file_id) = populated_db(BROKEN, &doc_path);
    let links = document_links(&db, &file_map, &params_for(uri)).expect("links expected");
    assert_eq!(links.len(), 1);
    let lo = offset_of(BROKEN, "\"x.g\"");
    assert_eq!(links[0].range, lsp_range(BROKEN, lo, lo + "\"x.g\"".len()));
}

#[test]
fn positions_run_through_utf16_columns() {
    let dir = tempfile::tempdir().unwrap();
    let doc_path = dir.path().join("unicode.g");
    let (db, file_map, uri, _file_id) = populated_db(UNICODE_DOC, &doc_path);
    let links = document_links(&db, &file_map, &params_for(uri)).expect("links expected");
    assert_eq!(links.len(), 1);
    // `héllo` is 6 chars but 7 bytes; the literal sits on line 1 either
    // way, and its UTF-16 columns come from the same mapping the request
    // path uses.
    let lo = offset_of(UNICODE_DOC, "\"a.g\"");
    assert_eq!(
        links[0].range,
        lsp_range(UNICODE_DOC, lo, lo + "\"a.g\"".len())
    );
    assert_eq!(links[0].range.start.line, 1);
}

#[test]
fn unopened_document_answers_none() {
    let dir = tempfile::tempdir().unwrap();
    let doc_path = dir.path().join("doc.g");
    let (db, file_map, _uri, _file_id) = populated_db(DOC, &doc_path);
    // A URI that was never opened: no file-map entry, so no answer.
    let other = uri_for(&dir.path().join("never-opened.g"));
    assert!(document_links(&db, &file_map, &params_for(other)).is_none());
}

#[test]
fn include_free_document_answers_none() {
    let dir = tempfile::tempdir().unwrap();
    let doc_path = dir.path().join("plain.g");
    let src = "fn main() -> i32 { 0 }\n";
    let (db, file_map, uri, _file_id) = populated_db(src, &doc_path);
    assert!(document_links(&db, &file_map, &params_for(uri)).is_none());
}
