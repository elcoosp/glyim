/// pattern.
pub mod pattern;

pub use pattern::MatchPattern;

use glyim_diag::DiagSeverity;

#[derive(Clone, Debug)]
/// Annotation.
pub struct Annotation {
    /// Struct.
    pub line: usize,
    /// Struct.
    pub line_offset: usize,
    /// Struct.
    pub severity: DiagSeverity,
    /// Struct.
    pub pattern: MatchPattern,
    /// Struct.
    pub optional: bool,
    /// Struct.
    pub fuzzy: bool,
}

impl Annotation {
    /// target_line.
    pub fn target_line(&self) -> usize {
        self.line.saturating_sub(self.line_offset)
    }

    /// parse_all.
    pub fn parse_all(source: &str) -> Result<Vec<Self>, String> {
        let mut annotations = Vec::new();
        let mut last_target_line: Option<usize> = None;

        for (line_idx, line) in source.lines().enumerate() {
            let mut search_from = 0;

            while let Some(start) = line[search_from..].find("//") {
                let abs_start = search_from + start;
                search_from = abs_start + 2;
                let rest = &line[abs_start + 2..];

                let (fuzzy, rest) = if let Some(r) = rest.strip_prefix("~~") {
                    (true, r)
                } else if let Some(r) = rest.strip_prefix('~') {
                    (false, r)
                } else {
                    continue;
                };

                let (optional, rest) = if let Some(r) = rest.strip_prefix('?') {
                    (true, r.trim_start())
                } else {
                    (false, rest)
                };

                let (is_continuation, rest) = if let Some(r) = rest.strip_prefix('|') {
                    (true, r.trim_start())
                } else {
                    (false, rest)
                };

                let (line_offset, rest) = if is_continuation {
                    (0, rest)
                } else {
                    let count = rest.chars().take_while(|c| *c == '^').count();
                    (count, &rest[count..])
                };
                let rest = rest.trim_start();

                let (severity, rest) = parse_severity(rest);

                let pattern_text = rest.trim();
                let pattern = if pattern_text.is_empty() {
                    MatchPattern::Any
                } else {
                    MatchPattern::substring(pattern_text)
                };

                let target_line = if is_continuation {
                    last_target_line.ok_or_else(|| {
                        format!("line {}: //~| without preceding annotation", line_idx + 1)
                    })?
                } else {
                    line_idx.saturating_sub(line_offset)
                };

                last_target_line = Some(target_line);

                annotations.push(Annotation {
                    line: line_idx,
                    line_offset: if is_continuation {
                        line_idx.saturating_sub(target_line)
                    } else {
                        line_offset
                    },
                    severity,
                    pattern,
                    optional,
                    fuzzy,
                });
            }
        }

        Ok(annotations)
    }
}

fn parse_severity(s: &str) -> (DiagSeverity, &str) {
    let (word, rest) = s.split_once(char::is_whitespace).unwrap_or((s, ""));
    match word {
        "ERROR" => (DiagSeverity::Error, rest.trim_start()),
        "WARNING" => (DiagSeverity::Warning, rest.trim_start()),
        "NOTE" => (DiagSeverity::Note, rest.trim_start()),
        "HELP" => (DiagSeverity::Help, rest.trim_start()),
        _ => (DiagSeverity::Error, s),
    }
}
