use glyim_lex::Token;
use glyim_syntax::SyntaxKind;

/// Collect consecutive `//` line comments that immediately precede
/// an item (at `before_pos`). Returns None if there are no comments
/// or if a blank line separates them from the item.
pub fn collect_doc_comments(tokens: &[Token], before_pos: usize) -> Option<String> {
    let mut i = if before_pos > 0 && before_pos <= tokens.len() {
        before_pos - 1
    } else if before_pos > 0 {
        tokens.len() - 1
    } else {
        return None;
    };

    // Skip trailing whitespace before the item
    while i > 0 && tokens[i].kind == SyntaxKind::Whitespace {
        i = i.saturating_sub(1);
    }

    // Must end with a line comment
    if i >= tokens.len() || tokens[i].kind != SyntaxKind::LineComment {
        return None;
    }

    let end = i + 1;
    // Walk backwards collecting consecutive line comments
    let mut start = i;
    loop {
        if start == 0 {
            break;
        }
        start -= 1;
        let tok = &tokens[start];
        if tok.kind == SyntaxKind::Whitespace {
            if tok.text.contains('\n') && tok.text.matches('\n').count() > 1 {
                start += 1;
                break;
            }
            continue;
        }
        if tok.kind != SyntaxKind::LineComment {
            start += 1;
            break;
        }
    }

    for _t in tokens[start..end].iter() {
    }
    let comments: Vec<&str> = tokens[start..end]
        .iter()
        .filter(|t| t.kind == SyntaxKind::LineComment)
        .map(|t| t.text)
        .collect();

    if comments.is_empty() {
        return None;
    }
    let result = beautify_doc_string(&comments.join("\n"));

    Some(result)
}

/// Remove `//` prefixes and normalize whitespace from doc comments.
/// Preserves the content of fenced code blocks (```...```) by keeping
/// their internal whitespace intact.
pub fn beautify_doc_string(raw: &str) -> String {
    let mut in_code_block = false;

    // Strip leading and trailing blank lines, but preserve internal ones.
    let lines: Vec<&str> = raw.lines().collect();
    let first_nonblank = lines.iter().position(|l| !l.trim().is_empty());
    let last_nonblank = lines.iter().rposition(|l| !l.trim().is_empty());

    let effective: &[&str] = match (first_nonblank, last_nonblank) {
        (Some(f), Some(l)) => &lines[f..=l],
        _ => &[],
    };

    let raw = effective.join("\n");

    raw.lines()
        .map(|line| {
            // Track whether we're inside a fenced code block
            let trimmed = line.trim();
            if trimmed.starts_with("```") {
                in_code_block = !in_code_block;
                // For code block fences, only strip the "// " prefix, keep the backticks
                if line.starts_with("// ") {
                    return line[3..].to_string();
                } else if line.starts_with("//") {
                    return line[2..].to_string();
                }
                return line.to_string();
            }

            if in_code_block {
                // Inside a code block, preserve exact content after "//" prefix
                if line.starts_with("// ") {
                    return line[3..].to_string();
                } else if line.starts_with("//") {
                    return line[2..].to_string();
                }
                return line.to_string();
            }

            // Normal line: strip "// " or "//" prefix
            if line.starts_with("// ") {
                line[3..].to_string()
            } else if line.starts_with("//") {
                line[2..].to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_lex::tokenize;

    #[test]
    fn beautify_single_line() {
        assert_eq!(beautify_doc_string("// hello"), "hello");
    }

    #[test]
    fn beautify_multi_line() {
        assert_eq!(beautify_doc_string("// line1\n// line2"), "line1\nline2");
    }

    #[test]
    fn beautify_with_spaces() {
        assert_eq!(beautify_doc_string("//  indented"), " indented");
    }

    #[test]
    fn beautify_preserves_code_block() {
        let input = "// text\n// ```glyim\n// let x = 1;\n// ```\n// more text";
        let result = beautify_doc_string(input);
        assert!(result.contains("```glyim"), "Should preserve fenced code block");
        assert!(result.contains("let x = 1;"), "Should preserve code inside block");
        assert!(result.contains("text"), "Should preserve text before block");
        assert!(result.contains("more text"), "Should preserve text after block");
    }

    #[test]

    #[test]
    fn integration_style_doc_with_code_block() {
        let source = r#"// Adds two integers together.
//
// # Examples
//
// ```glyim
// let result = add(1, 2)
// assert(result == 3)
// ```
fn add(a: i64, b: i64) -> i64 { a + b }"#;
        let tokens = tokenize(source);
        // Find position of 'fn' keyword
        let fn_pos = tokens.iter().position(|t| t.kind == SyntaxKind::KwFn).unwrap();
        for i in 0..fn_pos {
            let t = &tokens[i];
        }
        let doc = collect_doc_comments(&tokens, fn_pos);
        assert!(doc.is_some(), "Should collect doc comment");
        let doc = doc.unwrap();
        assert!(doc.contains("Adds two integers together."), "Should contain paragraph text");
        assert!(doc.contains("let result = add(1, 2)"), "Should contain code example but got:\n{doc}");
    }

        fn collect_doc_comments_basic() {
        let source = "// hello\nfn main() {}";
        let tokens = tokenize(source);
        // Find position of 'fn' keyword
        let fn_pos = tokens.iter().position(|t| t.kind == SyntaxKind::KwFn).unwrap();
        let doc = collect_doc_comments(&tokens, fn_pos);
        assert_eq!(doc, Some("hello".to_string()));
    }
}
