use crate::AnalysisDatabase;
use crate::symbol_index::{SymbolInfo, SymbolKind};
use glyim_type::PrintTy;
use lsp_types::*;

use url::Url;

/// Build a single `CompletionItem` from an indexed `SymbolInfo`.
fn completion_item(sym: &SymbolInfo) -> CompletionItem {
    let kind = match sym.kind {
        SymbolKind::Function => CompletionItemKind::FUNCTION,
        SymbolKind::Struct => CompletionItemKind::STRUCT,
        SymbolKind::Enum => CompletionItemKind::ENUM,
        SymbolKind::Field => CompletionItemKind::FIELD,
        SymbolKind::Local => CompletionItemKind::VARIABLE,
        _ => CompletionItemKind::TEXT,
    };
    let detail = sym.type_signature.as_ref().map(|ts| {
        let params: Vec<String> = ts
            .params
            .iter()
            .map(|(n, t)| format!("{}: {}", n, t))
            .collect();
        let ret = ts
            .return_type
            .as_ref()
            .map(|t| format!(" -> {}", t))
            .unwrap_or_default();
        format!("({}){}", params.join(", "), ret)
    });
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
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        insert_text: if sym.kind == SymbolKind::Function {
            sym.type_signature.as_ref().map(|ts| {
                // Plan §22.6: generic items get a `name::<${1:T}, ${2:U}>` snippet
                // with tab-stops, so the user can fill in type arguments inline.
                let generics = if ts.generic_params.is_empty() {
                    String::new()
                } else {
                    let g: Vec<String> = ts
                        .generic_params
                        .iter()
                        .enumerate()
                        .map(|(i, t)| format!("${{{}:{}}}", i + 1, t))
                        .collect();
                    format!("::<{}>", g.join(", "))
                };
                if ts.params.is_empty() {
                    format!("{}{}()", sym.name, generics)
                } else {
                    let start = ts.generic_params.len() + 1;
                    let placeholders: Vec<String> = ts
                        .params
                        .iter()
                        .enumerate()
                        .map(|(i, (n, _))| format!("${{{}:{}}}", start + i, n))
                        .collect();
                    format!("{}{}({})", sym.name, generics, placeholders.join(", "))
                }
            })
        } else {
            None
        },
        sort_text: Some(match sym.kind {
            SymbolKind::Function => format!("0_{}", sym.name),
            SymbolKind::Struct => format!("1_{}", sym.name),
            _ => format!("9_{}", sym.name),
        }),
        ..Default::default()
    }
}

/// If the cursor is positioned right after a `.` (a method-call completion
/// trigger), resolve the type of the receiver expression and return it as a
/// normalized string (e.g. `"Foo"`). Returns `None` when there is no `.`
/// trigger, no source map, or the receiver type cannot be resolved — in which
/// case callers should fall back to unfiltered completion.
fn receiver_type_at_cursor(
    db: &AnalysisDatabase,
    file_id: glyim_span::FileId,
    params: &CompletionParams,
) -> Option<String> {
    // Only filter when the LSP explicitly reports a `.` trigger character.
    let is_dot_trigger = params
        .context
        .as_ref()
        .and_then(|c| c.trigger_character.as_deref())
        == Some(".");
    if !is_dot_trigger {
        return None;
    }

    let source_maps = db.source_maps.read();
    let sm = source_maps.get(&file_id)?;
    let pos = params.text_document_position.position;
    let offset = sm.line_col_to_offset(pos.line as usize, pos.character as usize)?;

    // Confirm the character immediately before the cursor is a `.`.
    let src = sm.source();
    if offset == 0 || !src[..offset].ends_with('.') {
        return None;
    }

    let ty = db.type_at_offset(file_id, offset)?;
    let ty_ctx = db.ty_ctx(file_id)?;
    Some(format!("{}", PrintTy::new(ty, ty_ctx.as_ref())))
}

/// provide_completions.
pub fn provide_completions(
    db: &AnalysisDatabase,
    file_map: &crate::database::FileMap,
    params: &CompletionParams,
) -> Option<CompletionResponse> {
    let uri = &params.text_document_position.text_document.uri;
    let path = Url::parse(uri.as_str()).ok()?.to_file_path().ok()?;
    let file_id = file_map.get_by_path(&path)?;
    let symbol_index = db.symbol_index.read();
    let symbols = symbol_index.symbols_in_file(file_id);

    // Tier 6.4: filter completions by the receiver type at a `.`-method call
    // site. When a receiver type resolves, prefer methods whose recorded
    // receiver type matches; if nothing matches, fall back to the full list so
    // completion is never empty for an incomplete expression.
    // NOTE: `receiver_type_at_cursor` acquires its own `source_maps` read lock,
    // so it must run before we take that lock below (std RwLock is not reentrant).
    let receiver_ty = receiver_type_at_cursor(db, file_id, params);
    let filtered: Vec<&SymbolInfo> = if let Some(rt) = &receiver_ty {
        let methods: Vec<&SymbolInfo> = symbols
            .iter()
            .filter(|sym| {
                sym.type_signature
                    .as_ref()
                    .and_then(|ts| ts.receiver_type.as_deref())
                    == Some(rt.as_str())
            })
            .cloned()
            .collect();
        if methods.is_empty() { symbols } else { methods }
    } else {
        symbols
    };

    let mut items: Vec<CompletionItem> = filtered.iter().map(|sym| completion_item(sym)).collect();

    // --- Plan §22.6: auto-import -------------------------------------------
    // When the user is typing an identifier, also offer symbols *declared in other
    // files* whose name matches the prefix. Each such candidate carries an
    // `additional_text_edits` that inserts the corresponding `use` statement via
    // the def-map `insert_use` helper (which is idempotent and reuses an existing
    // `use` block).
    if let Some(prefix) = typed_identifier_prefix(db, file_id, params) {
        if !prefix.is_empty() {
            let source_maps = db.source_maps.read();
            let src = source_maps.get(&file_id).map(|sm| sm.source().to_string());
            if let Some(src) = src {
                for cand in symbol_index.query(&prefix, 50) {
                    if cand.definition.file_id == file_id {
                        continue; // already in this file; no import needed.
                    }
                    let Some(import_path) =
                        symbol_index.import_path_for(cand.definition.file_id, &cand.name)
                    else {
                        continue;
                    };
                    let Some((offset, text)) = glyim_def_map::insert_use_edit(&src, import_path)
                    else {
                        continue; // already imported (idempotent) or no path.
                    };
                    let Some((line, col)) = crate::uri::offset_to_position(&src, offset).ok()
                    else {
                        continue;
                    };
                    let mut item = completion_item(cand);
                    let pos = Position {
                        line: line as u32,
                        character: col as u32,
                    };
                    let edit = TextEdit {
                        range: Range {
                            start: pos,
                            end: pos,
                        },
                        new_text: text,
                    };
                    item.additional_text_edits = Some(vec![edit]);
                    item.detail = Some(format!("(auto-import) {}", import_path));
                    // Avoid duplicate suggestions for the same symbol+import.
                    if !items
                        .iter()
                        .any(|i| i.label == item.label && i.detail == item.detail)
                    {
                        items.push(item);
                    }
                }
            }
        }
    }

    if items.is_empty() {
        None
    } else {
        Some(CompletionResponse::List(CompletionList {
            is_incomplete: false,
            items,
        }))
    }
}

/// Plan §22.6 auto-import: return the identifier immediately preceding the cursor
/// (the token the user is currently typing), or `None` if there is no identifier
/// being typed. Used to scope auto-import suggestions to the typed prefix.
fn typed_identifier_prefix(
    db: &AnalysisDatabase,
    file_id: glyim_span::FileId,
    params: &CompletionParams,
) -> Option<String> {
    let source_maps = db.source_maps.read();
    let sm = source_maps.get(&file_id)?;
    let pos = params.text_document_position.position;
    let offset = sm.line_col_to_offset(pos.line as usize, pos.character as usize)?;
    let src = sm.source();
    if offset == 0 {
        return None;
    }
    let bytes = src.as_bytes();
    let mut start = offset;
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        start -= 1;
    }
    if start == offset {
        None
    } else {
        Some(src[start..offset].to_string())
    }
}
