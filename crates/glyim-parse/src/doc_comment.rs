use glyim_lex::Token;
use glyim_syntax::SyntaxKind;

/// Collect consecutive `//` line comments that immediately precede
/// an item (at `before_pos`). Returns None if there are no comments
/// or if a blank line separates them from the item.
pub fn collect_doc_comments(tokens: &[Token], before_pos: usize) -> Option<String> {
    // Find the token immediately before `before_pos`, skipping whitespace
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
    if tokens[i].kind != SyntaxKind::LineComment {
        return None;
    }

    let end = i + 1; // exclusive
    // Walk backwards collecting consecutive line comments
    let mut start = i;
    loop {
        if start == 0 {
            break;
        }
        start -= 1;
        let tok = &tokens[start];
        if tok.kind == SyntaxKind::Whitespace {
            // If the whitespace contains a newline and the next token is also whitespace
            // or it's a blank line, break
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

    let comments: Vec<&str> = tokens[start..end]
        .iter()
        .filter(|t| t.kind == SyntaxKind::LineComment)
        .map(|t| t.text)
        .collect();

    if comments.is_empty() {
        return None;
    }

    Some(beautify_doc_string(&comments.join("\n")))
}

/// Remove `//` prefixes and normalize whitespace from doc comments.
pub fn beautify_doc_string(raw: &str) -> String {
    raw.lines()
        .map(|line| {
            if line.starts_with("//") {
                let rest = &line[2..];
                if rest.starts_with(' ') {
                    rest[1..].to_string()
                } else {
                    rest.to_string()
                }
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
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
}
