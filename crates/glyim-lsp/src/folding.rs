use crate::AnalysisDatabase;
use lsp_types::*;

pub fn provide_folding_ranges(
    db: &AnalysisDatabase,
    params: &FoldingRangeParams,
) -> Option<Vec<FoldingRange>> {
    let uri = &params.text_document.uri;
    let path = uri.to_file_path().ok()?;
    let file_id = db.file_map.read().get_by_path(&path)?;
    let source_maps = db.source_maps.read();
    let sm = source_maps.get(&file_id)?;
    let source = sm.source();

    let mut ranges = Vec::new();
    // stack of (start_line, brace_column) where start_line is u32
    let mut stack: Vec<(u32, u32)> = Vec::new();

    for (line_idx, line) in source.lines().enumerate() {
        let line_num = line_idx as u32;
        for (col, ch) in line.char_indices().collect::<Vec<_>>() {
            if ch == '{' {
                stack.push((line_num, col as u32));
            } else if ch == '}' {
                if let Some((start_line, _)) = stack.pop() {
                    ranges.push(FoldingRange {
                        start_line,
                        end_line: line_num,
                        start_character: None,
                        end_character: None,
                        kind: Some(FoldingRangeKind::Region),
                        collapsed_text: None,
                    });
                }
            }
        }
    }
    Some(ranges)
}
