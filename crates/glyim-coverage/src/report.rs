use crate::data::{CoverageDump, LocationKind};
use std::collections::HashMap;
use std::fs;

pub struct TextReport {
    pub annotated_lines: String,
}

pub fn generate_text_report(dump: &CoverageDump, source: &str) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let total_lines = lines.len();

    let mut line_counts: HashMap<u32, u64> = HashMap::new();

    for (counter_id, count) in &dump.counters {
        if let Some(loc) = dump.metadata.get(counter_id) {
            let entry = line_counts.entry(loc.start_line).or_insert(0);
            *entry += *count as u64;
        }
    }

    let covered_lines: usize = line_counts.keys().filter(|&&l| l <= total_lines as u32).count();
    let percent = if total_lines > 0 {
        (covered_lines as f64 / total_lines as f64) * 100.0
    } else {
        0.0
    };

    let mut out = String::new();
    out.push_str(&format!("Coverage: {:.1}% ({}/{})\n", percent, covered_lines, total_lines));
    out.push_str("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    for (i, line) in lines.iter().enumerate() {
        let line_num = (i + 1) as u32;
        let count = line_counts.get(&line_num).copied().unwrap_or(0);
        let marker = if count > 0 { "✓" } else { "✗" };
        out.push_str(&format!("{:<5} {:>4}  {}\n", marker, count, line));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{CoverageDump, FileInfo, LocationKind, SourceLocation};
    use std::collections::HashMap;

    #[test]
    fn empty_dump_zero_percent() {
        let dump = CoverageDump {
            files: HashMap::new(),
            counters: HashMap::new(),
            metadata: HashMap::new(),
            version: 1,
        };
        let report = generate_text_report(&dump, "fn main() {\n    42\n}\n");
        assert!(report.contains("0.0%"), "expected 0%, got:\n{}", report);
    }

    #[test]
    fn single_line_covered_shows_100_percent() {
        let mut counters = HashMap::new();
        counters.insert(0, 1);
        let mut metadata = HashMap::new();
        metadata.insert(0, SourceLocation {
            file_id: 0,
            start_line: 1,
            start_col: 0,
            end_line: 1,
            end_col: 0,
            kind: LocationKind::FunctionEntry,
        });
        let dump = CoverageDump {
            files: HashMap::new(),
            counters,
            metadata,
            version: 1,
        };
        let report = generate_text_report(&dump, "42\n");
        assert!(report.contains("100.0%"), "expected 100%, got:\n{}", report);
        assert!(report.contains("✓"), "line should be marked covered");
    }

    #[test]
    fn uncovered_line_has_cross() {
        let dump = CoverageDump {
            files: HashMap::new(),
            counters: HashMap::new(),
            metadata: HashMap::new(),
            version: 1,
        };
        let report = generate_text_report(&dump, "let x = 1;\nlet y = 2;\n");
        assert!(report.contains("✗"), "lines should be marked uncovered");
    }
}
