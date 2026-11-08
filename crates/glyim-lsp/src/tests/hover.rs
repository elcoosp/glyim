use crate::database::{AnalysisDatabase, SourceMap};
use crate::hover;
use crate::symbol_index::{DefinitionLocation, SymbolInfo, SymbolKind, TypeSignature};
use glyim_span::{ByteIdx, FileId, Span, SyntaxContext};
use lsp_types::Uri;
use lsp_types::{
    HoverContents, MarkupKind, Position, TextDocumentIdentifier, TextDocumentPositionParams,
};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use url::Url;

#[test]
fn test_provide_hover() {
    let db = Arc::new(AnalysisDatabase::new());
    let mut file_map = crate::FileMap::new();
    let path = PathBuf::from("/test/main.g");
    let file_id = file_map.get_or_create(&path);

    let content = "fn main() {}";
    let sm = SourceMap::new(file_id, content.to_string());
    db.source_maps.write().insert(file_id, sm);

    let mut index = db.symbol_index.write();
    index.insert_test_symbol(
        file_id,
        SymbolInfo {
            name: "main".to_string(),
            kind: SymbolKind::Function,
            definition: DefinitionLocation {
                file_id,
                span: Span::new(
                    FileId::from_raw(0),
                    ByteIdx::from_raw(0),
                    ByteIdx::from_raw(4),
                    SyntaxContext::ROOT,
                ),
            },
            type_signature: Some(TypeSignature {
                params: vec![],
                receiver_type: None,
                return_type: None,
                generic_params: vec![],
            }),
            is_pub: true,
            documentation: Some("Entry point".to_string()),
        },
    );
    drop(index);

    let uri = Uri::from_str(
        &Uri::from_str(Url::from_file_path(&path).unwrap().as_ref())
            .unwrap()
            .to_string(),
    )
    .unwrap();
    let params = lsp_types::HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 0,
                character: 0,
            },
        },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
    };

    let result = hover::provide_hover(&db, &file_map, &params);

    assert!(result.is_some());
    let hover = result.unwrap();
    assert!(hover.range.is_none());

    if let HoverContents::Markup(content) = hover.contents {
        assert_eq!(content.kind, MarkupKind::Markdown);
        assert!(content.value.contains("fn main()"));
        assert!(content.value.contains("Entry point"));
    } else {
        panic!("Expected MarkupContent");
    }
}
