use crate::AnalysisDatabase;
use crate::database::FileMap;
use lsp_types::*;

pub fn provide_completions(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &CompletionParams,
) -> Option<CompletionResponse> {
    let uri = &params.text_document_position.text_document.uri;
    let path = uri.to_file_path().ok()?;
    let file_id = file_map.get_by_path(&path)?;

    let symbol_index = db.symbol_index.read();
    let symbols = symbol_index.symbols_in_file(file_id);

    let items: Vec<CompletionItem> = symbols.iter().map(|sym| {
        let kind = match sym.kind {
            crate::symbol_index::SymbolKind::Function => CompletionItemKind::FUNCTION,
            crate::symbol_index::SymbolKind::Struct => CompletionItemKind::STRUCT,
            crate::symbol_index::SymbolKind::Enum => CompletionItemKind::ENUM,
            crate::symbol_index::SymbolKind::EnumVariant => CompletionItemKind::ENUM_MEMBER,
            crate::symbol_index::SymbolKind::Field => CompletionItemKind::FIELD,
            _ => CompletionItemKind::TEXT,
        };
        CompletionItem {
            label: sym.name.clone(),
            kind: Some(kind),
            detail: sym.type_signature.as_ref().map(|ts| {
                let params: Vec<String> = ts.params.iter().map(|(n, t)| format!("{}: {:?}", n, t)).collect();
                format!("fn {}({})", sym.name, params.join(", "))
            }),
            documentation: sym.documentation.as_ref().map(|d: &String| {
                Documentation::MarkupContent(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: d.clone(),
                })
            }),
            ..Default::default()
        }
    }).collect();

    Some(CompletionResponse::List(CompletionList {
        is_incomplete: false,
        items,
    }))
}
