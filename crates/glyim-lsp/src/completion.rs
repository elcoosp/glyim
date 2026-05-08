use crate::AnalysisDatabase;
use crate::database::FileMap;
use lsp_types::*;
use crate::symbol_index::{SymbolKind};

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
            SymbolKind::Function => CompletionItemKind::FUNCTION,
            SymbolKind::Struct => CompletionItemKind::STRUCT,
            SymbolKind::Enum => CompletionItemKind::ENUM,
            SymbolKind::EnumVariant => CompletionItemKind::ENUM_MEMBER,
            SymbolKind::Field => CompletionItemKind::FIELD,
            _ => CompletionItemKind::TEXT,
        };

        // Build detail from type signature
        let detail = sym.type_signature.as_ref().map(|ts| {
            let params: Vec<String> = ts.params.iter().map(|(n, t)| format!("{}: {:?}", n, t)).collect();
            let ret = ts.return_type.as_ref().map(|t| format!(" -> {:?}", t)).unwrap_or_default();
            format!("({}){}", params.join(", "), ret)
        });

        // Build documentation
        let documentation = sym.documentation.as_ref().map(|d| {
            Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: d.clone(),
            })
        });

        CompletionItem {
            label: sym.name.clone(),
            kind: Some(kind),
            detail,
            documentation,
            // Insert text with snippet placeholders for function params
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            insert_text: sym.type_signature.as_ref().map(|ts| {
                if ts.params.is_empty() {
                    format!("{}()", sym.name)
                } else {
                    let placeholders: Vec<String> = ts.params.iter().enumerate()
                        .map(|(i, (n, _))| format!("${{{}:{}}}", i + 1, n))
                        .collect();
                    format!("{}({})", sym.name, placeholders.join(", "))
                }
            }),
            sort_text: Some(match sym.kind {
                SymbolKind::Function => format!("0_{}", sym.name),
                SymbolKind::Struct => format!("1_{}", sym.name),
                SymbolKind::Enum => format!("2_{}", sym.name),
                SymbolKind::Field => format!("3_{}", sym.name),
                _ => format!("9_{}", sym.name),
            }),
            ..Default::default()
        }
    }).collect();

    Some(CompletionResponse::List(CompletionList {
        is_incomplete: false,
        items,
    }))
}
