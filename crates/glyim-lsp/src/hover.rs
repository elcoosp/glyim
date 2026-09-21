use crate::AnalysisDatabase;
use crate::DefinitionLocation;
use lsp_types::*;

use url::Url;
/// provide_hover.
pub fn provide_hover(
    db: &AnalysisDatabase,
    file_map: &crate::database::FileMap,
    params: &HoverParams,
) -> Option<Hover> {
    let uri = &params.text_document_position_params.text_document.uri;
    let path = Url::parse(uri.as_str()).ok()?.to_file_path().ok()?;
    let file_id = file_map.get_by_path(&path)?;
    let source_maps = db.source_maps.read();
    let sm = source_maps.get(&file_id)?;
    let pos = params.text_document_position_params.position;
    let offset = sm.line_col_to_offset(pos.line as usize, pos.character as usize)?;
    let symbol_index = db.symbol_index.read();
    let symbol = symbol_index.lookup_by_location(file_id, offset)?;
    let mut markdown = String::new();
    if let Some(ts) = &symbol.type_signature {
        let params_str: Vec<String> = ts
            .params
            .iter()
            .map(|(n, t)| format!("{}: {}", n, t))
            .collect();
        let ret_str = ts
            .return_type
            .as_ref()
            .map(|t| format!(" -> {}", t))
            .unwrap_or_default();
        markdown.push_str(&format!(
            "```glyim\nfn {}({}){}\n```\n",
            symbol.name,
            params_str.join(", "),
            ret_str
        ));
    }
    if let Some(doc) = &symbol.documentation {
        markdown.push_str(doc);
    }

    // Plan §22.5: include a "go to definition" preview — a few lines of source
    // around the definition site, so the hover shows *where* the symbol lives,
    // not just its signature/docs. Reuses the same `SourceMap` machinery as
    // goto-definition/rename rather than a second source lookup path.
    if let Some(preview) = definition_preview(db, &symbol.definition) {
        if !preview.is_empty() {
            markdown.push_str("\n\n---\n\n```glyim\n");
            markdown.push_str(&preview);
            markdown.push_str("```\n");
        }
    }

    if markdown.is_empty() {
        return None;
    }
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: markdown,
        }),
        range: None,
    })
}

/// Plan §22.5: extract the source lines covering a symbol's definition span as
/// a hover preview. Returns `None` (rather than a silent empty/placeholder) if
/// the defining file's source isn't loaded, matching the no-fallback principle.
fn definition_preview(db: &AnalysisDatabase, def: &DefinitionLocation) -> Option<String> {
    let source_maps = db.source_maps.read();
    let sm = source_maps.get(&def.file_id)?;
    let source = sm.source();
    let ((start_line, _), (end_line, _)) =
        sm.span_to_position(def.span.lo.to_usize(), def.span.hi.to_usize())?;

    let mut preview_lines: Vec<&str> = Vec::new();
    for (idx, line) in source.split_inclusive('\n').enumerate() {
        // Trim a single trailing newline for display; keep empty lines.
        let line = line.strip_suffix('\n').unwrap_or(line);
        if idx >= start_line && idx <= end_line {
            preview_lines.push(line);
        }
    }
    if preview_lines.is_empty() {
        None
    } else {
        Some(preview_lines.join("\n"))
    }
}
