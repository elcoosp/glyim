//! Formatting request support.
//!
//! The formatter is one brace-based re-indent policy shared by two wire
//! shapes: the whole document (`textDocument/formatting`, LSP-FORMAT) and
//! a selected line span (`textDocument/rangeFormatting`, LSP-RANGE-FORMAT,
//! see [`crate::range_formatting`]). [`format_source`] is the single core:
//! the range engine seeds it with the nesting depth the unselected prefix
//! leaves (the brace balance of the text before the claim), so a range
//! edit composes with the surrounding file instead of resetting to column
//! 0, and formatting a document in two ranges agrees with formatting it in
//! one call.
//!
//! Known policy, shared with the pre-seam formatter this module shipped:
//! the walk is character-level — braces inside string literals are treated
//! as structure (the language has no escape-free nesting, so every `{`
//! opens a block in practice), commas are spaced, and operators keep the
//! source's spelling. The response is one whole-document `TextEdit` (full
//! request) or one line-span `TextEdit` (range request), or `None` when
//! the text is already formatted — no edit churn on an idempotent call.

use crate::AnalysisDatabase;
use lsp_types::*;

use url::Url;

/// Whether `text` sits on a line start: empty, all indentation after the
/// beginning, or a dangling indent after the last newline (the loop pushes
/// the next line's indent eagerly). Both the `}` branch and the final
/// truncation key off this.
fn at_line_start(text: &str) -> bool {
    match text.rfind('\n') {
        None => text.chars().all(|c| c == ' '),
        Some(idx) => text[idx + 1..].chars().all(|c| c == ' '),
    }
}

/// The shared re-indent core (`base_indent` = the nesting depth the text
/// before the formatted slice leaves; the whole-document call passes `0`).
///
/// Slice-correct by construction, so the range engine can feed it any
/// contiguous line span:
///
/// - the slice's first line is re-emitted at `base_indent` (its leading
///   whitespace is dropped, the depth replaces it);
/// - a `}` that lands on a line start (source newline before it, or the
///   loop's own dangling indent) closes at the *decremented* depth without
///   splicing a phantom blank line — the newline before a closing brace
///   comes from the source or this branch, never both;
/// - dangling indentation after a newline is truncated before the final
///   per-line trim, so a slice that already ends with a newline gains no
///   trailing blank line;
/// - the emitted text always terminates its last line with `\n`.
pub(crate) fn format_source(source: &str, base_indent: usize) -> String {
    const INDENT: &str = "    ";
    let mut result = INDENT.repeat(base_indent);
    let mut indent_level = base_indent;
    // The slice's leading whitespace belongs to the re-emitted first line;
    // drop it so `base_indent` decides the depth.
    let body = source.trim_start_matches(' ');
    let chars: Vec<char> = body.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '{' => {
                result.push('{');
                result.push('\n');
                indent_level += 1;
                result.push_str(&INDENT.repeat(indent_level));
            }
            '}' => {
                if at_line_start(&result) {
                    // Reset the line the brace closes on: drop the loop's
                    // dangling indent, or the prepended base indent when
                    // the slice itself opens on the `}` — the decremented
                    // depth below re-emits the right one.
                    match result.rfind('\n') {
                        Some(idx) => result.truncate(idx + 1),
                        None => result.clear(),
                    }
                } else {
                    result.push('\n');
                }
                indent_level = indent_level.saturating_sub(1);
                result.push_str(&INDENT.repeat(indent_level));
                result.push('}');
                result.push('\n');
                if i + 1 < chars.len() {
                    if chars[i + 1] == '\n' {
                        // The branch just terminated the brace's line; the
                        // source's own newline is redundant — consume it so
                        // no phantom blank line follows the brace.
                        i += 1;
                    } else if chars[i + 1] != '}' {
                        result.push_str(&INDENT.repeat(indent_level));
                    }
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
                    result.push_str(&INDENT.repeat(indent_level));
                }
            }
            '\n' => {
                result.push('\n');
                result.push_str(&INDENT.repeat(indent_level));
            }
            ' ' => {
                // Leading whitespace of the next line is the loop's own
                // indent already — the source's spelling never widens it.
                if !result.is_empty() && !result.ends_with(' ') && !result.ends_with('\n') {
                    result.push(' ');
                }
            }
            _ => {
                result.push(c);
            }
        }
        i += 1;
    }
    // A text that ends with the loop's dangling indent must not turn it
    // into a phantom trailing blank line.
    if at_line_start(&result) {
        if let Some(idx) = result.rfind('\n') {
            result.truncate(idx + 1);
        }
    }
    let mut out = String::new();
    for line in result.lines() {
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// `textDocument/formatting` (LSP-FORMAT): one whole-document re-indent
/// edit, or `None` when the document is already formatted (no edit churn).
///
/// The params' `options` (tab size, spaces) are deliberately not
/// consulted: the policy's four-space indent is the house style the
/// formatter has always emitted, and honouring per-request options would
/// make the two entry shapes disagree with the shared core.
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
    let formatted = format_source(content, 0);
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
