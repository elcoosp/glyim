use crate::completion::provide_completions;
use crate::database::AnalysisDatabase;
use crate::database::SourceMap;
use crate::symbol_index::{DefinitionLocation, SymbolInfo, SymbolKind, TypeSignature};
use crate::tests::helpers::make_span;
use glyim_span::FileId;
use lsp_types::Uri;
use lsp_types::*;
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

fn setup_test_db() -> (AnalysisDatabase, FileId, PathBuf) {
    let db = AnalysisDatabase::new();
    let file_id = FileId::from_raw(0);
    let path = PathBuf::from("/test/main.g");
    {
        let mut fm = db.file_map.write();
        fm.get_or_create(&path);
    }
    {
        let mut sm = db.source_maps.write();
        sm.insert(
            file_id,
            SourceMap::new(
                path.clone(),
                file_id,
                "struct Point { x: i32, y: i32 }\nfn main() { let p = Point { x: 10, y: 20 }; p. }"
                    .to_string(),
            ),
        );
    }
    {
        let mut idx = db.symbol_index.write();
        idx.insert_test_symbol(
            file_id,
            SymbolInfo {
                name: "x".into(),
                kind: SymbolKind::Field,
                definition: DefinitionLocation {
                    file_id,
                    span: make_span(file_id, 14, 15),
                },
                type_signature: Some(TypeSignature {
                    params: vec![],
                    receiver_type: None,
                    return_type: Some("i32".into()),
                    generic_params: vec![],
                }),
                is_pub: true,
                documentation: None,
            },
        );
        idx.insert_test_symbol(
            file_id,
            SymbolInfo {
                name: "y".into(),
                kind: SymbolKind::Field,
                definition: DefinitionLocation {
                    file_id,
                    span: make_span(file_id, 24, 25),
                },
                type_signature: Some(TypeSignature {
                    params: vec![],
                    receiver_type: None,
                    return_type: Some("i32".into()),
                    generic_params: vec![],
                }),
                is_pub: true,
                documentation: None,
            },
        );
    }
    (db, file_id, path)
}

#[test]
fn completion_provides_struct_fields() {
    let (db, _file_id, path) = setup_test_db();
    let file_map = db.file_map.read();
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
                line: 1,
                character: 42,
            },
        },
        work_done_progress_params: WorkDoneProgressParams {
            work_done_token: None,
        },
        partial_result_params: PartialResultParams {
            partial_result_token: None,
        },
        context: None,
    };
    let response = provide_completions(&db, &file_map, &params).expect("completion response");
    if let CompletionResponse::List(list) = response {
        let labels: Vec<&str> = list.items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"x"));
        assert!(labels.contains(&"y"));
        assert_eq!(list.items.len(), 2);
    } else {
        panic!("Expected CompletionList");
    }
}

#[test]
fn completion_generic_function_emits_snippet_with_type_params() {
    // Plan §22.6: a generic function `id<T>(x: T) -> T` must produce a snippet
    // `id::<${1:T}>(${2:x})` (tab-stops for the type arg and the value arg),
    // not a bare `id()`.
    let analysis = AnalysisDatabase::new();
    let path = PathBuf::from("/test/generic.g");
    let file_id = {
        let mut file_map = analysis.file_map.write();
        file_map.get_or_create(&path)
    };
    let source_map = SourceMap::new(
        path.clone(),
        file_id,
        "fn id<T>(x: T) -> T { x }\nfn main() { id(); }".to_string(),
    );
    analysis.source_maps.write().insert(file_id, source_map);

    let span = make_span(file_id, 0, 2);
    let sym = SymbolInfo {
        name: "id".to_string(),
        kind: SymbolKind::Function,
        definition: DefinitionLocation { file_id, span },
        type_signature: Some(TypeSignature {
            params: vec![("x".to_string(), "T".to_string())],
            receiver_type: None,
            return_type: Some("T".to_string()),
            generic_params: vec!["T".to_string()],
        }),
        is_pub: true,
        documentation: None,
    };
    analysis
        .symbol_index
        .write()
        .insert_test_symbol(file_id, sym);

    let file_map_guard = analysis.file_map.read();
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
        work_done_progress_params: WorkDoneProgressParams {
            work_done_token: None,
        },
        partial_result_params: PartialResultParams {
            partial_result_token: None,
        },
        context: None,
    };
    let response =
        provide_completions(&analysis, &file_map_guard, &params).expect("completion response");
    let list = match response {
        CompletionResponse::List(list) => list,
        _ => panic!("Expected CompletionList"),
    };
    let item = list
        .items
        .iter()
        .find(|i| i.label == "id")
        .expect("generic fn `id` must be a completion candidate");
    let insert = item.insert_text.as_deref().expect("insert_text present");
    assert_eq!(
        insert, "id::<${1:T}>(${2:x})",
        "generic function must emit a `::<..>` snippet, got {:?}",
        insert
    );
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
}
