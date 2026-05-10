pub mod manifest;
pub mod highlight;

pub use highlight::highlight_code;
pub use manifest::{DocItem, DocManifest, DocTestResult, HighlightedExample};

/// Extract fenced `glyim` code blocks from a doc string.
pub fn extract_code_blocks(doc: &str) -> Vec<(Option<String>, String)> {
    let mut blocks = Vec::new();
    let mut in_glyim_block = false;
    let mut block_title = None;
    let mut block_lines = Vec::new();
    let mut in_fence = false;
    let mut lang = String::new();

    for line in doc.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            if in_fence {
                if in_glyim_block {
                    let code = block_lines.join("\n");
                    if !code.trim().is_empty() {
                        blocks.push((block_title.take(), code));
                    }
                    in_glyim_block = false;
                    block_lines.clear();
                }
                in_fence = false;
                lang.clear();
            } else {
                in_fence = true;
                lang = trimmed.strip_prefix("```").unwrap_or("").trim().to_string();
                block_title = None;
                in_glyim_block = lang == "glyim";
            }
        } else if in_fence && in_glyim_block {
            block_lines.push(line.to_string());
        }
    }
    blocks
}
