use crate::data::{CoverageDump, LocationKind};
use std::collections::HashMap;

pub fn generate_html_report(dump: &CoverageDump, source: &str, file_path: &str) -> String {
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

    let mut html = String::new();
    html.push_str("<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><title>Coverage Report</title>");
    html.push_str("<style>
        body { font-family: monospace; background: #1e1e1e; color: #d4d4d4; padding: 20px; }
        h1 { border-bottom: 1px solid #444; }
        .header { margin-bottom: 20px; }
        .line { white-space: pre; }
        .covered { background: #1a3a1a; }
        .uncovered { background: #3a1a1a; }
        .line-num { color: #858585; display: inline-block; width: 50px; text-align: right; margin-right: 10px; }
        .count { color: #569cd6; display: inline-block; width: 40px; margin-right: 10px; }
        .source { display: inline; }
    </style></head><body>");

    html.push_str(&format!("<h1>Coverage Report: {}</h1>", esc_html(file_path)));
    html.push_str(&format!("<div class=\"header\"><strong>{:.1}%</strong> — {} of {} lines covered</div>", percent, covered_lines, total_lines));
    html.push_str("<pre>\n");

    for (i, line) in lines.iter().enumerate() {
        let line_num = (i + 1) as u32;
        let count = line_counts.get(&line_num).copied().unwrap_or(0);
        let css_class = if count > 0 { "covered" } else { "uncovered" };
        html.push_str(&format!(
            "<div class=\"line {}\"><span class=\"line-num\">{}</span><span class=\"count\">{}</span><span class=\"source\">{}</span></div>\n",
            css_class, line_num, count, esc_html(line)
        ));
    }

    html.push_str("</pre></body></html>");
    html
}

fn esc_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{CoverageDump, LocationKind, SourceLocation};
    use std::collections::HashMap;

    #[test]
    fn html_contains_100_percent() {
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
        let html = generate_html_report(&dump, "42\n", "test.g");
        assert!(html.contains("100.0%"), "expected 100%, got:\n{}", html);
        assert!(html.contains("class=\"line covered\""), "expected covered class");
    }

    #[test]
    fn html_uncovered_line() {
        let dump = CoverageDump {
            files: HashMap::new(),
            counters: HashMap::new(),
            metadata: HashMap::new(),
            version: 1,
        };
        let html = generate_html_report(&dump, "let x = 5;\n", "test.g");
        assert!(html.contains("class=\"line uncovered\""), "expected uncovered class");
    }
}
