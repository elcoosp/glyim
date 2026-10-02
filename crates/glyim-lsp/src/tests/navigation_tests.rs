use crate::database::{AnalysisDatabase, SourceMap};
use crate::navigation::workspace_symbols;
use glyim_core::Interner;
use glyim_span::FileId;
use lsp_types::Uri;
use lsp_types::{
    GotoDefinitionParams, Position, TextDocumentIdentifier, TextDocumentPositionParams,
    WorkspaceSymbolParams,
};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

fn setup_db_with_file(path: &str, source: &str) -> (AnalysisDatabase, FileId) {
    let db = AnalysisDatabase::new();
    let path_buf = PathBuf::from(path);
    let file_id = db.file_map.write().get_or_create(&path_buf);
    let sm = SourceMap::new(path_buf.clone(), file_id, source.to_string());
    db.source_maps.write().insert(file_id, sm);

    let parse_result = glyim_frontend::parse_to_syntax(source, file_id);
    let mut interner = Interner::new();
    let (hir, _diags) =
        glyim_hir::pipeline_api::lower_crate_for_pipeline(&parse_result.root, &mut interner);
    db.symbol_index
        .write()
        .build_from_hir(file_id, &hir, &interner);
    db.reference_graph
        .write()
        .build_from_hir(file_id, &hir, &interner);
    db.hirs.write().insert(file_id, hir);

    (db, file_id)
}

#[test]
fn test_goto_definition_jumps_to_definition() {
    let source = "fn foo() {}\nfn bar() { foo(); }";
    let (db, _file_id) = setup_db_with_file("/test/goto.g", source);
    let file_map = db.file_map.read();

    let uri = Uri::from_str(Url::from_file_path("/test/goto.g").unwrap().as_ref()).unwrap();
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 1,
                character: 12,
            },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result = crate::goto_definition::goto_definition(&db, &file_map, &params);
    assert!(
        result.is_some(),
        "goto_definition should find the definition"
    );

    if let Some(lsp_types::GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(loc.uri, uri, "Definition should be in same file");
        assert_eq!(loc.range.start.line, 0, "Definition should be on line 0");
    } else {
        panic!("Expected scalar location response");
    }
}

#[test]
fn test_workspace_symbols_returns_matching_prefix() {
    let source = "fn alpha() {}\nfn beta() {}\nstruct Gamma {}\n";
    let (db, _file_id) = setup_db_with_file("/test/workspace.g", source);

    let params = WorkspaceSymbolParams {
        query: "al".to_string(),
        ..Default::default()
    };

    let result = workspace_symbols(&db, &params);
    assert!(result.is_some(), "workspace_symbols should return results");
    let symbols = result.unwrap();
    assert!(!symbols.is_empty(), "Expected at least one symbol");
    assert!(
        symbols.iter().any(|s| s.name == "alpha"),
        "Expected alpha in results"
    );
    assert!(
        !symbols.iter().any(|s| s.name == "beta"),
        "Expected beta not in results"
    );
    assert!(
        !symbols.iter().any(|s| s.name == "Gamma"),
        "Expected Gamma not in results"
    );
}

#[test]
fn test_workspace_symbols_empty_query_returns_none_or_all() {
    let source = "fn foo() {}";
    let (db, _file_id) = setup_db_with_file("/test/workspace2.g", source);

    let params = WorkspaceSymbolParams {
        query: "nonexistent".to_string(),
        ..Default::default()
    };

    let result = workspace_symbols(&db, &params);
    if let Some(symbols) = result {
        assert!(
            symbols.is_empty(),
            "Expected no symbols for nonexistent query"
        );
    }
}

/// INF-12 regression: identifier extraction must use byte offsets on char
/// boundaries. The old code indexed `Vec<char>` with a *byte* offset and
/// panicked ("not a char boundary") on any non-ASCII source.
#[test]
fn identifier_at_offset_handles_non_ascii_without_panic() {
    // "é" is 2 bytes; the identifier `ident` follows it.
    let src = "let é = ident;";
    // byte offset of `i` in `ident` (é is 2 bytes).
    let off = src.find("ident").unwrap();
    assert_eq!(
        crate::navigation::identifier_at_offset(src, off).as_deref(),
        Some("ident"),
    );
    // A position *inside* the multibyte `é` must return None, not panic.
    let inside = src.find('é').unwrap() + 1; // second byte of é
    assert_eq!(crate::navigation::identifier_at_offset(src, inside), None);
}

/// Digits are part of an identifier (`foo1`), unlike the old `is_alphabetic`.
#[test]
fn identifier_at_offset_includes_digits() {
    let src = "let foo1 = 0;";
    let off = src.find("foo1").unwrap() + 2;
    assert_eq!(
        crate::navigation::identifier_at_offset(src, off).as_deref(),
        Some("foo1"),
    );
}
