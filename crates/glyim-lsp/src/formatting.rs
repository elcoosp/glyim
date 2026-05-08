use crate::AnalysisDatabase;
use glyim_fmt::{FormatConfig, format_source};
use lsp_types::*;

pub fn format_document(
    db: &AnalysisDatabase,
    params: &DocumentFormattingParams,
) -> Option<Vec<TextEdit>> {
    let uri = &params.text_document.uri;
    let path = uri.to_file_path().ok()?;
    let file_id = db.file_map.read().get_by_path(&path)?;
    let source_maps = db.source_maps.read();
    let sm = source_maps.get(&file_id)?;
    let original = sm.source();

    let config = FormatConfig {
        indent_width: params.options.tab_size as usize,
        use_spaces: params.options.insert_spaces,
        ..Default::default()
    };

    let formatted = format_source(original, &config).ok()?;

    if formatted == original {
        return None;
    }

    let total_lines = original.lines().count() as u32;
    Some(vec![TextEdit {
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: total_lines,
                character: 0,
            },
        },
        new_text: formatted,
    }])
}
