use crate::AnalysisDatabase;
use lsp_types::*;

use url::Url;
fn find_braced_ranges(source: &str) -> Vec<FoldingRange> {
    // T133-PATCHED [LSP-10]: skip braces that appear inside string/char
    // literals, line comments, or block comments. The previous bare
    // character scan treated `"}"` or `// }` as a closing brace, popping
    // the enclosing function's fold early and producing wrong (or
    // missing) folding ranges.
    let lines: Vec<&str> = source.lines().collect();
    let mut ranges = Vec::new();
    let mut brace_stack: Vec<(usize, usize)> = Vec::new();
    let mut in_block_comment = false;

    for (line_idx, line) in lines.iter().enumerate() {
        let mut in_string = false;
        let mut in_char = false;
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0usize;
        while i < chars.len() {
            let ch = chars[i];
            let next = chars.get(i + 1).copied();

            if in_block_comment {
                if ch == '*' && next == Some('/') {
                    in_block_comment = false;
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }
            if in_string {
                if ch == '\\' {
                    i += 2;
                    continue;
                }
                if ch == '"' {
                    in_string = false;
                }
                i += 1;
                continue;
            }
            if in_char {
                if ch == '\\' {
                    i += 2;
                    continue;
                }
                if ch == '\'' {
                    in_char = false;
                }
                i += 1;
                continue;
            }
            match ch {
                '/' if next == Some('/') => {
                    // Rest of the line is a comment; nothing more to scan.
                    break;
                }
                '/' if next == Some('*') => {
                    in_block_comment = true;
                    i += 2;
                    continue;
                }
                '"' => {
                    in_string = true;
                }
                '\'' => {
                    in_char = true;
                }
                '{' => {
                    brace_stack.push((line_idx, i));
                }
                '}' => {
                    if let Some((start_line, start_col)) = brace_stack.pop() {
                        ranges.push(FoldingRange {
                            start_line: start_line as u32,
                            start_character: Some(start_col as u32),
                            end_line: line_idx as u32,
                            end_character: Some(i as u32),
                            kind: Some(FoldingRangeKind::Region),
                            collapsed_text: None,
                        });
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }
    ranges
}

/// provide_folding_ranges.
pub fn provide_folding_ranges(
    db: &AnalysisDatabase,
    params: &FoldingRangeParams,
) -> Option<Vec<FoldingRange>> {
    let uri = &params.text_document.uri;
    let path = Url::parse(uri.as_str()).ok()?.to_file_path().ok()?;
    let file_map = db.file_map.read();
    let file_id = file_map.get_by_path(&path)?;
    drop(file_map);
    let source_maps = db.source_maps.read();
    let sm = source_maps.get(&file_id)?;
    let source = sm.source();

    let ranges = find_braced_ranges(source);
    if ranges.is_empty() {
        None
    } else {
        Some(ranges)
    }
}
