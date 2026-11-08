use crate::{
    AnalysisDatabase, DefinitionLocation, SymbolInfo, SymbolKind, TypeSignature,
    completion::provide_completions,
};
use glyim_span::{ByteIdx, Span, SyntaxContext};
use lsp_types::Uri;
use lsp_types::*;
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

fn get_test_path(filename: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(filename);
    path
}

#[test]
fn completions_include_function_name() {
    let analysis = AnalysisDatabase::new();
    let path = get_test_path("main.g");
    let file_id = {
        let mut file_map = analysis.file_map.write();
        file_map.get_or_create(&path)
    };
    let source_map =
        crate::SourceMap::new(file_id, "fn foo() {}\nfn main() { foo(); }".to_string());
    analysis.source_maps.write().insert(file_id, source_map);

    let span = Span::new(
        file_id,
        ByteIdx::ZERO,
        ByteIdx::from_raw(3),
        SyntaxContext::ROOT,
    );
    let foo_info = SymbolInfo {
        name: "foo".to_string(),
        kind: SymbolKind::Function,
        definition: DefinitionLocation { file_id, span },
        type_signature: Some(TypeSignature {
            params: vec![],
            receiver_type: None,
            return_type: Some("()".to_string()),
            generic_params: vec![],
        }),
        is_pub: true,
        documentation: None,
    };
    {
        let mut symbol_index = analysis.symbol_index.write();
        symbol_index.insert_test_symbol(file_id, foo_info);
    }

    let file_map_guard = analysis.file_map.read();
    let file_map = &*file_map_guard;

    let completions = provide_completions(
        &analysis,
        file_map,
        &CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Uri::from_str(
                        &Uri::from_str(Url::from_file_path(&path).unwrap().as_ref())
                            .unwrap()
                            .to_string(),
                    )
                    .unwrap(),
                },
                position: Position {
                    line: 1,
                    character: 12,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        },
    );

    assert!(completions.is_some());
    if let Some(CompletionResponse::List(list)) = completions {
        let labels: Vec<_> = list.items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"foo"));
    } else {
        panic!("Expected completion list");
    }
}
