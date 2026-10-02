use crate::AnalysisDatabase;
use crate::database::FileMap;
use lsp_types::Uri;
use lsp_types::*;
use std::str::FromStr;
use url::Url;

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
    let source = sm.source();

    // Byte-offset, char-boundary-safe identifier extraction (INF-12).
    let symbol_name = crate::navigation::identifier_at_offset(source, offset)?;
    let symbol_name = symbol_name.as_str();

    let symbol_index = db.symbol_index.read();
    let symbols = symbol_index.lookup_by_name(symbol_name);
    let symbol = symbols.first()?;
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
