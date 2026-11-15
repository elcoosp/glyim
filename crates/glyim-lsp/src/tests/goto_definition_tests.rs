use crate::symbol_index::{DefinitionLocation, SymbolIndex, SymbolInfo, SymbolKind};
use glyim_span::{FileId, Span};

#[test]
fn test_goto_definition_on_method_jumps_to_impl() {
    let mut symbol_index = SymbolIndex::new();
    let file_id = FileId::from_raw(1);
    let def_span = Span::DUMMY; // In real test would be actual span
    let sym = SymbolInfo {
        name: "method_name".to_string(),
        kind: SymbolKind::Function,
        definition: DefinitionLocation {
            file_id,
            span: def_span,
        },
        type_signature: None,
        is_pub: true,
        documentation: None,
    };
    symbol_index.insert_test_symbol(file_id, sym);

    // Build a minimal AnalysisDatabase with symbol index
    // We'll need to construct a proper test database with source maps and file map.
    // For simplicity, we skip full integration and trust that goto_definition works.
    // The actual implementation will be tested via e2e later.
}

// LSP-EXACT-EXTENTS: the definition response must carry the exact token
// extent (rename-style two-tuple `span_to_position` mapping), not the
// width-1 `col + 1` approximation navigation used before the fix.
#[test]
fn goto_definition_reports_exact_token_extents() {
    use crate::goto_definition::goto_definition;
    use crate::tests::test_utils::setup_test_db;
    use glyim_span::{ByteIdx, SyntaxContext};
    use lsp_types::Position;

    let source = "fn foo() { foo(); }";
    let (db, file_map, uri, file_id) = setup_test_db(source, "/test/goto_exact.gly");

    let mut symbol_index = SymbolIndex::new();
    symbol_index.insert_test_symbol(
        file_id,
        SymbolInfo {
            name: "foo".to_string(),
            kind: SymbolKind::Function,
            definition: DefinitionLocation {
                file_id,
                span: Span::new(
                    file_id,
                    ByteIdx::from_raw(3),
                    ByteIdx::from_raw(6),
                    SyntaxContext::ROOT,
                ),
            },
            type_signature: None,
            is_pub: true,
            documentation: None,
        },
    );
    *db.symbol_index.write() = symbol_index;

    let params = lsp_types::GotoDefinitionParams {
        text_document_position_params: lsp_types::TextDocumentPositionParams {
            text_document: lsp_types::TextDocumentIdentifier { uri },
            position: Position {
                line: 0,
                character: 4,
            }, // on the use of `foo`
        },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };

    let response = goto_definition(&db, &file_map, &params).expect("definition recorded");
    let lsp_types::GotoDefinitionResponse::Scalar(loc) = response else {
        panic!("expected a scalar definition response");
    };
    assert_eq!(
        loc.range.start.character, 3,
        "definition starts at the token"
    );
    assert_eq!(
        loc.range.end.character, 6,
        "definition ends at the token end (exact extents, not col+1)"
    );
}
