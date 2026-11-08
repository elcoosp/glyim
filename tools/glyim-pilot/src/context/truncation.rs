//! Smart truncation preserving structural lines (functions, modules, etc.)
//! Uses brace depth and comment/string skipping.

/// `smart_truncate`.
#[must_use]
pub fn smart_truncate(content: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() <= max_lines {
        return content.to_string();
    }

    let mut result = Vec::new();
    let mut brace_depth: usize = 0;

    for line in &lines {
        let trimmed = line.trim();
        let is_fn_sig = trimmed.starts_with("fn ")
            || trimmed.starts_with("async fn ")
            || trimmed.starts_with("pub fn ")
            || trimmed.starts_with("pub async fn ")
            || trimmed.starts_with("pub(crate) fn ")
            || trimmed.starts_with("pub(crate) async fn ")
            || trimmed.starts_with("pub const fn ")
            || trimmed.starts_with("macro_rules! ");
        let is_structural = trimmed.starts_with("pub ")
            || trimmed.starts_with("struct ")
            || trimmed.starts_with("enum ")
            || trimmed.starts_with("trait ")
            || trimmed.starts_with("type ")
            || trimmed.starts_with("const ")
            || trimmed.starts_with("use ")
            || trimmed.starts_with("mod ")
            || trimmed.starts_with("#[")
            || trimmed.starts_with("///")
            || trimmed.starts_with("//!")
            || trimmed.is_empty();

        let (opens, closes) = count_braces(trimmed);

        if is_fn_sig {
            if brace_depth > 0 {
                result.push("    ...".to_string());
                result.push("}".to_string());
                brace_depth = 0;
            }
            result.push((*line).to_string());
            brace_depth = brace_depth.saturating_add(opens).saturating_sub(closes);
        } else if is_structural {
            result.push((*line).to_string());
            brace_depth = brace_depth.saturating_add(opens).saturating_sub(closes);
        } else if brace_depth > 0 {
            brace_depth = brace_depth.saturating_add(opens).saturating_sub(closes);
            if brace_depth == 0 {
                result.push("    ...".to_string());
                result.push("}".to_string());
            }
        } else {
            brace_depth = brace_depth.saturating_add(opens).saturating_sub(closes);
            if brace_depth == 0 {
                result.push((*line).to_string());
            }
        }

        if result.len() >= max_lines {
            result.push("// ... (truncated)".to_string());
            break;
        }
    }

    if brace_depth > 0 {
        result.push("    ...".to_string());
        result.push("}".to_string());
    }

    result.join("\n")
}

fn count_braces(s: &str) -> (usize, usize) {
    let mut opens = 0;
    let mut closes = 0;
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        // Line comment
        if c == '/' && chars.peek() == Some(&'/') {
            break;
        }
        // Block comment
        if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            let mut prev_star = false;
            for next in chars.by_ref() {
                if prev_star && next == '/' {
                    break;
                }
                prev_star = next == '*';
            }
            continue;
        }
        // Raw string detection (simplified: skip everything from r" or r#... to closing)
        if c == 'r' {
            let next = chars.peek().copied();
            if next == Some('"') || next == Some('#') {
                // consume raw string entirely
                let mut hash_count = 0;
                while chars.peek() == Some(&'#') {
                    chars.next();
                    hash_count += 1;
                }
                if chars.peek() == Some(&'"') {
                    chars.next(); // consume opening "
                    let mut prev_quote = false;
                    let mut matched = 0;
                    loop {
                        match chars.next() {
                            None => break,
                            Some('"') => {
                                prev_quote = true;
                                matched = 0;
                            }
                            Some('#') if prev_quote => {
                                matched += 1;
                                if matched == hash_count {
                                    break;
                                }
                            }
                            Some(_) => {
                                prev_quote = false;
                                matched = 0;
                            }
                        }
                    }
                }
                continue;
            }
        }
        // Regular string
        if c == '"' {
            while let Some(next) = chars.next() {
                if next == '\\' {
                    chars.next();
                    continue;
                }
                if next == '"' {
                    break;
                }
            }
            continue;
        }
        // Character literal
        if c == '\'' {
            while let Some(next) = chars.next() {
                if next == '\\' {
                    chars.next();
                    continue;
                }
                if next == '\'' {
                    break;
                }
            }
            continue;
        }
        if c == '{' {
            opens += 1;
        } else if c == '}' {
            closes += 1;
        }
    }
    (opens, closes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smart_truncate_preserves_functions() {
        let input = "fn main() {\n    let x = 1;\n    let y = 2;\n}\nfn foo() {}\nfn bar() {}";
        let truncated = smart_truncate(input, 3);
        assert!(truncated.contains("fn main() {"));
        assert!(truncated.contains("..."));
        assert!(truncated.contains('}'));
        assert!(truncated.lines().count() <= 5);
    }

    #[test]
    fn test_count_braces_ignores_strings() {
        // Use simple strings, avoid nested raw strings
        let line = "let s = \"a { b\";";
        assert_eq!(count_braces(line), (0, 0));
        let line = "let s = r#\"has { }\"#;";
        assert_eq!(count_braces(line), (0, 0));
        assert_eq!(count_braces("fn main() {"), (1, 0));
        assert_eq!(count_braces("}"), (0, 1));
    }
}
