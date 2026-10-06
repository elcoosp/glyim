#![allow(deprecated)]

use crate::AnalysisDatabase;
use crate::database::FileMap;
use lsp_types::Uri;
use lsp_types::*;
use std::str::FromStr;
use url::Url;

/// Extract the identifier covering byte `offset` in `source`, walking on
/// char boundaries and using BYTE offsets throughout (INF-12). The previous
/// code collected `Vec<char>` but indexed it with a *byte* offset, which
/// panicked ("not a char boundary") on any non-ASCII source and used
/// `is_alphabetic` (excluding digits).
pub(crate) fn identifier_at_offset(source: &str, offset: usize) -> Option<String> {
    if offset > source.len() || !source.is_char_boundary(offset) {
        return None;
    }
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let start = source[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_word(*c))
        .map(|(i, _)| i)
        .last()
        .unwrap_or(offset);
    let end = source[offset..]
        .char_indices()
        .take_while(|(_, c)| is_word(*c))
        .map(|(i, c)| offset + i + c.len_utf8())
        .last()
        .unwrap_or(offset);
    if start == end {
        None
    } else {
        Some(source[start..end].to_string())
    }
}

fn get_symbol_name_at_position(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    uri: &Uri,
    position: Position,
) -> Option<String> {
    let path = Url::parse(uri.as_str()).ok()?.to_file_path().ok()?;
    let file_id = file_map.get_by_path(&path)?;
    let source_maps = db.source_maps.read();
    let sm = source_maps.get(&file_id)?;
    let offset = sm.line_col_to_offset(position.line as usize, position.character as usize)?;
    let source = sm.source();
    identifier_at_offset(source, offset)
}

/// goto_definition.
pub fn goto_definition(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &GotoDefinitionParams,
) -> Option<GotoDefinitionResponse> {
    let uri = &params.text_document_position_params.text_document.uri;
    let path = Url::parse(uri.as_str()).ok()?.to_file_path().ok()?;
    let file_id = file_map.get_by_path(&path)?;
    let source_maps = db.source_maps.read();
    let sm = source_maps.get(&file_id)?;
    let pos = params.text_document_position_params.position;
    let offset = sm.line_col_to_offset(pos.line as usize, pos.character as usize)?;
    let symbol_index = db.symbol_index.read();
    let symbol = symbol_index.lookup_by_location(file_id, offset)?;
    let def = &symbol.definition;
    let def_sm = source_maps.get(&def.file_id)?;
    let (start_line, start_col) = def_sm
        .span_to_position(def.span.lo.to_usize(), def.span.hi.to_usize())
        .unwrap_or(((0, 0), (0, 0)))
        .0;
    let target_path = file_map.path(def.file_id)?;
    let target_uri = Url::from_file_path(target_path).ok()?;
    Some(GotoDefinitionResponse::Scalar(Location {
        uri: Uri::from_str(target_uri.as_str()).unwrap(),
        range: Range {
            start: Position {
                line: start_line as u32,
                character: start_col as u32,
            },
            end: Position {
                line: start_line as u32,
                character: (start_col + 1) as u32,
            },
        },
    }))
}

/// find_references.
pub fn find_references(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &ReferenceParams,
) -> Option<Vec<Location>> {
    let uri = &params.text_document_position.text_document.uri;
    let symbol_name =
        get_symbol_name_at_position(db, file_map, uri, params.text_document_position.position)?;
    let ref_graph = db.reference_graph.read();
    let references = ref_graph.find_references(&symbol_name);
    if references.is_empty() {
        return None;
    }
    let source_maps = db.source_maps.read();
    let mut locations = Vec::new();
    for r in references {
        // T053-PATCHED-NAV [LSP-4]: skip refs whose span is DUMMY
        // (pattern bindings currently have no real span). Returning
        // `0:0` for those would place a bogus reference at the top
        // of the file.
        if r.span.is_dummy() {
            continue;
        }
        let sm = source_maps.get(&r.file_id)?;
        let (start_line, start_col) = sm
            .span_to_position(r.span.lo.to_usize(), r.span.hi.to_usize())
            .unwrap_or(((0, 0), (0, 0)))
            .0;
        let path = file_map.path(r.file_id)?;
        let loc_uri = Url::from_file_path(path).ok()?;
        locations.push(Location {
            uri: Uri::from_str(loc_uri.as_str()).unwrap(),
            range: Range {
                start: Position {
                    line: start_line as u32,
                    character: start_col as u32,
                },
                end: Position {
                    line: start_line as u32,
                    character: (start_col + 1) as u32,
                },
            },
        });
    }
    Some(locations)
}

/// document_symbols.
pub fn document_symbols(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &DocumentSymbolParams,
) -> Option<DocumentSymbolResponse> {
    let uri = &params.text_document.uri;
    let path = Url::parse(uri.as_str()).ok()?.to_file_path().ok()?;
    let file_id = file_map.get_by_path(&path)?;
    let source_maps = db.source_maps.read();
    let sm = source_maps.get(&file_id)?;
    let symbol_index = db.symbol_index.read();
    let symbols = symbol_index.symbols_in_file(file_id);
    let mut results = Vec::new();
    for sym in symbols {
        let (start_line, start_col) = sm
            .span_to_position(
                sym.definition.span.lo.to_usize(),
                sym.definition.span.hi.to_usize(),
            )
            .unwrap_or(((0, 0), (0, 0)))
            .0;
        let kind = match sym.kind {
            crate::symbol_index::SymbolKind::Function => SymbolKind::FUNCTION,
            crate::symbol_index::SymbolKind::Struct => SymbolKind::STRUCT,
            crate::symbol_index::SymbolKind::Enum => SymbolKind::ENUM,
            crate::symbol_index::SymbolKind::Field => SymbolKind::FIELD,
            crate::symbol_index::SymbolKind::Local => SymbolKind::VARIABLE,
            _ => SymbolKind::VARIABLE,
        };
        results.push(DocumentSymbol {
            name: sym.name.clone(),
            kind,
            range: Range {
                start: Position {
                    line: start_line as u32,
                    character: start_col as u32,
                },
                end: Position {
                    line: start_line as u32,
                    character: (start_col + 1) as u32,
                },
            },
            selection_range: Range {
                start: Position {
                    line: start_line as u32,
                    character: start_col as u32,
                },
                end: Position {
                    line: start_line as u32,
                    character: start_col as u32,
                },
            },
            children: None,
            detail: sym.type_signature.as_ref().map(|ts| {
                let params: Vec<String> = ts
                    .params
                    .iter()
                    .map(|(n, t)| format!("{}: {}", n, t))
                    .collect();
                format!("({})", params.join(", "))
            }),
            tags: None,
            deprecated: None,
        });
    }
    Some(DocumentSymbolResponse::Nested(results))
}

/// workspace_symbols.
pub fn workspace_symbols(
    db: &AnalysisDatabase,
    params: &WorkspaceSymbolParams,
) -> Option<Vec<SymbolInformation>> {
    let query = params.query.as_str();
    let symbol_index = db.symbol_index.read();
    let matches = symbol_index.query(query, 20);
    let source_maps = db.source_maps.read();
    let file_map = db.file_map.read();
    let mut results = Vec::new();
    for info in matches {
        let sm = source_maps.get(&info.definition.file_id)?;
        let (start_line, start_col) = sm
            .span_to_position(
                info.definition.span.lo.to_usize(),
                info.definition.span.hi.to_usize(),
            )
            .unwrap_or(((0, 0), (0, 0)))
            .0;
        let path = file_map.path(info.definition.file_id)?;
        let uri = Url::from_file_path(path).ok()?;
        let kind = match info.kind {
            crate::symbol_index::SymbolKind::Function => SymbolKind::FUNCTION,
            crate::symbol_index::SymbolKind::Struct => SymbolKind::STRUCT,
            crate::symbol_index::SymbolKind::Enum => SymbolKind::ENUM,
            crate::symbol_index::SymbolKind::Field => SymbolKind::FIELD,
            _ => SymbolKind::VARIABLE,
        };
        results.push(SymbolInformation {
            name: info.name.clone(),
            kind,
            location: Location {
                uri: Uri::from_str(uri.as_str()).unwrap(),
                range: Range {
                    start: Position {
                        line: start_line as u32,
                        character: start_col as u32,
                    },
                    end: Position {
                        line: start_line as u32,
                        character: (start_col + 1) as u32,
                    },
                },
            },
            container_name: None,
            tags: None,
            deprecated: None,
        });
    }
    Some(results)
}
