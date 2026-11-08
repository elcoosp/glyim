use crate::AnalysisDatabase;
use lsp_types::*;

use url::Url;
fn format_code(source: &str) -> String {
    let mut result = String::new();
    let mut indent_level = 0;
    let chars: Vec<char> = source.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '{' => {
                result.push('{');
                result.push('\n');
                indent_level += 1;
                result.push_str(&"    ".repeat(indent_level));
            }
            '}' => {
                result.push('\n');
                indent_level = indent_level.saturating_sub(1);
                result.push_str(&"    ".repeat(indent_level));
                result.push('}');
                result.push('\n');
                if i + 1 < chars.len() && chars[i + 1] != '}' {
                    result.push_str(&"    ".repeat(indent_level));
                }
            }
            ',' => {
                result.push(',');
                result.push(' ');
            }
            ';' => {
                result.push(';');
                if i + 1 < chars.len() && chars[i + 1] != '}' && chars[i + 1] != '\n' {
                    result.push('\n');
                    result.push_str(&"    ".repeat(indent_level));
                }
            }
            '\n' => {
                result.push('\n');
                result.push_str(&"    ".repeat(indent_level));
            }
            ' ' => {
                if !result.ends_with(' ') && !result.ends_with('\n') {
                    result.push(' ');
                }
            }
            _ => {
                result.push(c);
            }
        }
        i += 1;
    }
    let trimmed: String = result
        .lines()
        .map(str::trim_end)
        .collect::<Vec<&str>>()
        .join("\n");
    trimmed + "\n"
}

/// `format_document`.
pub fn format_document(
    db: &AnalysisDatabase,
    params: &DocumentFormattingParams,
) -> Option<Vec<TextEdit>> {
    let uri = &params.text_document.uri;
    let path = Url::parse(uri.as_str()).ok()?.to_file_path().ok()?;
    let file_map = db.file_map.read();
    let file_id = file_map.get_by_path(&path)?;
    drop(file_map);
    let source_maps = db.source_maps.read();
    let sm = source_maps.get(&file_id)?;
    let content = sm.source();
    let formatted = format_code(content);
    if formatted == content {
        return None;
    }
    let line_count = content.lines().count() as u32;
    let full_range = Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: line_count,
            character: 0,
        },
    };
    Some(vec![TextEdit {
        range: full_range,
        new_text: formatted,
    }])
}
