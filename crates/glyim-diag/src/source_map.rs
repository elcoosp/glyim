use std::path::PathBuf;

/// Unique identifier for a source file within an analysis session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FileId(pub u32);

/// Maps byte offsets to line/column positions.
pub struct SourceMap {
    pub path: PathBuf,
    pub file_id: FileId,
    source: String,
    line_starts: Vec<usize>,
}

impl SourceMap {
    pub fn new(path: PathBuf, file_id: FileId, source: String) -> Self {
        let line_starts = Self::compute_line_starts(&source);
        Self {
            path,
            file_id,
            source,
            line_starts,
        }
    }

    /// Convert a byte-offset range to (start, end) line/col pairs.
    /// Both line and column are 0-based (LSP standard).
    pub fn span_to_position(
        &self,
        start_offset: usize,
        end_offset: usize,
    ) -> Option<(LineCol, LineCol)> {
        let start = self.offset_to_line_col(start_offset)?;
        let end = self.offset_to_line_col(end_offset)?;
        Some((start, end))
    }

    pub fn offset_to_line_col(&self, offset: usize) -> Option<LineCol> {
        let line = self.line_starts.partition_point(|&s| s <= offset) - 1;
        let line_start = *self.line_starts.get(line)?;
        let column = offset - line_start;
        Some(LineCol { line, column })
    }

    pub fn line_col_to_offset(&self, pos: LineCol) -> Option<usize> {
        let line_start = *self.line_starts.get(pos.line)?;
        Some(line_start + pos.column)
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn update(&mut self, new_source: String) {
        self.source = new_source;
        self.line_starts = Self::compute_line_starts(&self.source);
    }

    fn compute_line_starts(source: &str) -> Vec<usize> {
        let mut starts = vec![0];
        for (i, ch) in source.char_indices() {
            if ch == '\n' {
                starts.push(i + 1);
            }
        }
        starts
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LineCol {
    pub line: usize,
    pub column: usize,
}
