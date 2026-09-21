use crate::completion;
use crate::database::{AnalysisDatabase, SourceMap};
use crate::symbol_index::{DefinitionLocation, SymbolInfo, SymbolKind, TypeSignature};
use glyim_span::{ByteIdx, FileId, Span, SyntaxContext};
use lsp_types::Uri;
use lsp_types::{
    CompletionItemKind, CompletionParams, Documentation, MarkupKind, Position,
    TextDocumentIdentifier, TextDocumentPositionParams,
};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use url::Url;

#[test]
fn test_provide_completions() {
    let db = Arc::new(AnalysisDatabase::new());
    let mut file_map = crate::FileMap::new();
    let path = PathBuf::from("/test/main.g");
    let file_id = file_map.get_or_create(&path);

    let mut index = db.symbol_index.write();
    index.insert_test_symbol(
        file_id,
        SymbolInfo {
            name: "my_func".to_string(),
            kind: SymbolKind::Function,
            definition: DefinitionLocation {
                file_id,
                span: Span::new(
                    FileId::from_raw(0),
                    ByteIdx::from_raw(0),
                    ByteIdx::from_raw(10),
                    SyntaxContext::ROOT,
                ),
            },
            type_signature: Some(TypeSignature {
                params: vec![("x".to_string(), "i32".to_string())],
                receiver_type: None,
                return_type: Some("i32".to_string()),
                generic_params: vec![],
            }),
            is_pub: true,
            documentation: Some("Does something".to_string()),
        },
    );
    drop(index);

    let uri = Uri::from_str(
        &Uri::from_str(Url::from_file_path(&path).unwrap().as_ref())
            .unwrap()
            .to_string(),
    )
    .unwrap();
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 0,
                character: 0,
            },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };

    let result = completion::provide_completions(&db, &file_map, &params);

    assert!(result.is_some());
    if let lsp_types::CompletionResponse::List(list) = result.unwrap() {
        assert!(!list.items.is_empty());
        let item = &list.items[0];
        assert_eq!(item.label, "my_func");
        assert_eq!(item.kind, Some(CompletionItemKind::FUNCTION));
        assert_eq!(item.detail, Some("(x: i32) -> i32".to_string()));

        match item.documentation {
            Some(Documentation::MarkupContent(ref mc)) => {
                assert_eq!(mc.kind, MarkupKind::Markdown);
                assert_eq!(mc.value, "Does something");
            }
            _ => panic!("Expected MarkupContent documentation"),
        }
    } else {
        panic!("Expected CompletionList");
    }
}

/// Plan §22.6 auto-import: a symbol declared in *another* file (with a recorded
/// import path) should be offered as a completion while typing its prefix, and the
/// completion must carry an `additional_text_edits` that inserts the `use` statement.
#[test]
fn test_auto_import_offers_symbol_from_other_file() {
    let db = Arc::new(AnalysisDatabase::new());
    let mut file_map = crate::FileMap::new();

    // Current file A: cursor is right after the typed prefix `Wid`.
    let path_a = PathBuf::from("/test/main.g");
    let file_id_a = file_map.get_or_create(&path_a);
    let source_a = "fn main() { Wid }";
    db.source_maps.write().insert(
        file_id_a,
        SourceMap::new(path_a.clone(), file_id_a, source_a.to_string()),
    );

    // Other file B: declares `Widget` with a known import path.
    let path_b = PathBuf::from("/test/widgets.g");
    let file_id_b = file_map.get_or_create(&path_b);

    let mut index = db.symbol_index.write();
    index.insert_test_symbol(
        file_id_b,
        SymbolInfo {
            name: "Widget".to_string(),
            kind: SymbolKind::Struct,
            definition: DefinitionLocation {
                file_id: file_id_b,
                span: Span::new(
                    FileId::from_raw(0),
                    ByteIdx::from_raw(0),
                    ByteIdx::from_raw(10),
                    SyntaxContext::ROOT,
                ),
            },
            type_signature: None,
            is_pub: true,
            documentation: None,
        },
    );
    index.insert_test_import_path(file_id_b, "Widget", "crate::widgets::Widget");
    drop(index);

    let uri = Uri::from_str(
        &Uri::from_str(Url::from_file_path(&path_a).unwrap().as_ref())
            .unwrap()
            .to_string(),
    )
    .unwrap();
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            // Cursor right after `Wid` (line 0, char 14).
            position: Position {
                line: 0,
                character: 14,
            },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };

    let result = completion::provide_completions(&db, &file_map, &params);
    assert!(result.is_some());
    let list = match result.unwrap() {
        lsp_types::CompletionResponse::List(list) => list,
        _ => panic!("Expected CompletionList"),
    };

    let widget = list
        .items
        .iter()
        .find(|i| i.label == "Widget")
        .expect("expected a `Widget` completion from the other file");
    assert_eq!(
        widget.detail,
        Some("(auto-import) crate::widgets::Widget".to_string())
    );
    let edits = widget
        .additional_text_edits
        .as_ref()
        .expect("expected additional_text_edits for the auto-import");
    assert_eq!(edits.len(), 1);
    assert!(
        edits[0].new_text.contains("use crate::widgets::Widget;"),
        "import edit should insert the use statement, got: {:?}",
        edits[0].new_text
    );
}
