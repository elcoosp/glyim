use std::collections::HashMap;
use crate::AnalysisDatabase;
use crate::database::FileMap;
use lsp_types::{*, Url};
use glyim_diag::LineCol;

pub fn goto_definition(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &GotoDefinitionParams,
) -> Option<GotoDefinitionResponse> {
    let uri = &params.text_document_position_params.text_document.uri;
    let path = uri.to_file_path().ok()?;
    let file_id = file_map.get_by_path(&path)?;

    let source_maps = db.source_maps.read().unwrap();
    let sm = source_maps.get(&file_id)?;

    let pos = params.text_document_position_params.position;
    let offset = sm.line_col_to_offset(LineCol {
        line: pos.line as usize,
        column: pos.character as usize,
    })?;

    let symbol_index = db.symbol_index.read().unwrap();
    let symbol = symbol_index.lookup_by_location(file_id, offset)?;
    let def = &symbol.definition;

    let def_sm = source_maps.get(&def.file_id)?;
    let (start, end) = def_sm.span_to_position(def.span.start, def.span.end)?;

    let target_path = file_map.path(def.file_id)?;
    let target_uri = Url::from_file_path(target_path).ok()?;

    Some(GotoDefinitionResponse::Scalar(Location {
        uri: target_uri,
        range: Range {
            start: Position { line: start.line as u32, character: start.column as u32 },
            end: Position { line: end.line as u32, character: end.column as u32 },
        },
    }))
}

pub fn find_references(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &ReferenceParams,
) -> Option<Vec<Location>> {
    let uri = &params.text_document_position.text_document.uri;
    let path = uri.to_file_path().ok()?;
    let file_id = file_map.get_by_path(&path)?;

    let source_maps = db.source_maps.read().unwrap();
    let sm = source_maps.get(&file_id)?;

    let pos = params.text_document_position.position;
    let offset = sm.line_col_to_offset(LineCol {
        line: pos.line as usize,
        column: pos.character as usize,
    })?;

    let symbol_index = db.symbol_index.read().unwrap();
    let symbol = symbol_index.lookup_by_location(file_id, offset)?;

    let ref_graph = db.reference_graph.read().unwrap();
    let refs = ref_graph.find_references(&symbol.name);

    let locations: Vec<Location> = refs.iter().filter_map(|r| {
        let sm = source_maps.get(&r.file_id)?;
        let (start, end) = sm.span_to_position(r.span.start, r.span.end)?;
        let path = file_map.path(r.file_id)?;
        let uri = Url::from_file_path(path).ok()?;
        Some(Location {
            uri,
            range: Range {
                start: Position { line: start.line as u32, character: start.column as u32 },
                end: Position { line: end.line as u32, character: end.column as u32 },
            },
        })
    }).collect();

    Some(locations)
}

pub fn document_symbols(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &DocumentSymbolParams,
) -> Option<DocumentSymbolResponse> {
    let uri = &params.text_document.uri;
    let path = uri.to_file_path().ok()?;
    let file_id = file_map.get_by_path(&path)?;

    let source_maps = db.source_maps.read().unwrap();
    let sm = source_maps.get(&file_id)?;
    let symbol_index = db.symbol_index.read().unwrap();
    let symbols = symbol_index.symbols_in_file(file_id);

    let mut results = Vec::new();
    for sym in symbols {
        let (start, end) = sm.span_to_position(sym.definition.span.start, sym.definition.span.end)?;
        results.push(DocumentSymbol {
            name: sym.name.clone(),
            kind: match sym.kind {
                crate::symbol_index::SymbolKind::Function => SymbolKind::FUNCTION,
                crate::symbol_index::SymbolKind::Struct => SymbolKind::STRUCT,
                crate::symbol_index::SymbolKind::Enum => SymbolKind::ENUM,
                _ => SymbolKind::VARIABLE,
            },
            range: Range {
                start: Position { line: start.line as u32, character: start.column as u32 },
                end: Position { line: end.line as u32, character: end.column as u32 },
            },
            selection_range: Range {
                start: Position { line: start.line as u32, character: start.column as u32 },
                end: Position { line: start.line as u32, character: start.column as u32 },
            },
            children: None,
            #[allow(deprecated)]
            deprecated: None,
            detail: sym.type_signature.as_ref().map(|ts| {
                let params: Vec<String> = ts.params.iter().map(|(n, t)| format!("{}: {:?}", n, t)).collect();
                format!("({})", params.join(", "))
            }),
            tags: None,
        });
    }
    Some(DocumentSymbolResponse::Nested(results))
}

pub fn rename(
    db: &AnalysisDatabase,
    params: &RenameParams,
) -> Option<WorkspaceEdit> {
    let uri = &params.text_document_position.text_document.uri;
    let path = uri.to_file_path().ok()?;
    let file_id = db.file_map.read().unwrap().get_by_path(&path)?;

    let source_maps = db.source_maps.read().unwrap();
    let sm = source_maps.get(&file_id)?;
    let pos = params.text_document_position.position;
    let offset = sm.line_col_to_offset(LineCol {
        line: pos.line as usize,
        column: pos.character as usize,
    })?;

    let symbol_index = db.symbol_index.read().unwrap();
    let symbol = symbol_index.lookup_by_location(file_id, offset)?;

    let ref_graph = db.reference_graph.read().unwrap();
    let refs = ref_graph.find_references(&symbol.name);

    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
    for r in refs {
        let sm = source_maps.get(&r.file_id)?;
        let (start, end) = sm.span_to_position(r.span.start, r.span.end)?;
        let fm = db.file_map.read().unwrap();
        let path = fm.path(r.file_id)?;
        let uri = Url::from_file_path(path).ok()?;
        let edit = TextEdit {
            range: Range {
                start: Position { line: start.line as u32, character: start.column as u32 },
                end: Position { line: end.line as u32, character: end.column as u32 },
            },
            new_text: params.new_name.clone(),
        };
        changes.entry(uri).or_default().push(edit);
    }

    Some(WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    })
}

pub fn workspace_symbols(
    db: &AnalysisDatabase,
    params: &WorkspaceSymbolParams,
) -> Option<Vec<SymbolInformation>> {
    let query = params.query.as_str();
    let symbol_index = db.symbol_index.read().unwrap();
    let matches = symbol_index.query(query, 20);
    let source_maps = db.source_maps.read().unwrap();
    let file_map = db.file_map.read().unwrap();

    let result = matches.iter().filter_map(|info| {
        let sm = source_maps.get(&info.definition.file_id)?;
        let (start, _) = sm.span_to_position(info.definition.span.start, info.definition.span.end)?;
        let path = file_map.path(info.definition.file_id)?;
        let uri = Url::from_file_path(path).ok()?;
        Some(SymbolInformation {
            name: info.name.clone(),
            kind: match info.kind {
                crate::symbol_index::SymbolKind::Function => SymbolKind::FUNCTION,
                crate::symbol_index::SymbolKind::Struct => SymbolKind::STRUCT,
                crate::symbol_index::SymbolKind::Enum => SymbolKind::ENUM,
                _ => SymbolKind::VARIABLE,
            },
            location: Location {
                uri,
                range: Range {
                    start: Position { line: start.line as u32, character: start.column as u32 },
                    end: Position { line: start.line as u32, character: start.column as u32 + 1 },
                },
            },
            container_name: None,
            deprecated: None,
            tags: None,
        })
    }).collect();

    Some(result)
}
