use crate::error::PilotError;
use crate::protocol::types::{FileOp, ParsedOps};

/// extract_ops_blocks.
pub fn extract_ops_blocks(response: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let lines: Vec<&str> = response.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed == "```glyim-ops" || trimmed.starts_with("```glyim-ops ") {
            let content_start = i + 1;
            let mut end_line = None;
            let mut inside_write_or_replace = false;

            for (j, _line) in lines.iter().enumerate().skip(i + 1) {
                let t = lines[j].trim();
                if t.starts_with("::WRITE ") || t.starts_with("::REPLACE ") {
                    inside_write_or_replace = true;
                } else if t == "::END" && inside_write_or_replace {
                    inside_write_or_replace = false;
                }
                if t.starts_with("```") && !inside_write_or_replace {
                    end_line = Some(j);
                    break;
                }
            }

            if let Some(end) = end_line {
                blocks.push(lines[content_start..end].join("\n").trim().to_string());
                i = end + 1;
            } else {
                break;
            }
        } else {
            i += 1;
        }
    }
    blocks
}

/// parse_ops_block.
pub fn parse_ops_block(input: &str) -> Result<ParsedOps, PilotError> {
    let mut ops = Vec::new();
    let mut commit_message = None;
    let mut incomplete = false;
    let mut done = false;
    let mut approved = false;
    let mut lines = input.lines().enumerate().peekable();

    while let Some((line_num, line)) = lines.next() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("::WRITE ") {
            let path = rest.trim().to_string();
            if path.is_empty() {
                return Err(PilotError::Parse {
                    line: line_num + 1,
                    message: "WRITE requires a path".into(),
                });
            }
            ops.push(FileOp::Write {
                path,
                content: read_until_end(&mut lines, line_num)?,
            });
        } else if let Some(rest) = trimmed.strip_prefix("::REPLACE ") {
            let path = rest.trim().to_string();
            if path.is_empty() {
                return Err(PilotError::Parse {
                    line: line_num + 1,
                    message: "REPLACE requires a path".into(),
                });
            }
            let (find, replace) = read_find_replace(&mut lines, line_num)?;
            ops.push(FileOp::Replace {
                path,
                find,
                replace,
            });
        } else if let Some(rest) = trimmed.strip_prefix("::DELETE ") {
            let path = rest.trim().to_string();
            if path.is_empty() {
                return Err(PilotError::Parse {
                    line: line_num + 1,
                    message: "DELETE requires a path".into(),
                });
            }
            ops.push(FileOp::Delete { path });
        } else if trimmed == "::DELETE" {
            return Err(PilotError::Parse {
                line: line_num + 1,
                message: "DELETE requires a path".into(),
            });
        } else if let Some(msg) = trimmed.strip_prefix("::COMMIT ") {
            // T158-PATCHED [PILOT-16]: strip surrounding quotes from the
            // commit message. The shipped system prompt tells the AI to
            // emit `::COMMIT "msg"`, and we used to store the literal
            // quotes, producing commit titles like `stream-S01: "msg"`.
            let cleaned = msg.trim().trim_matches(|c| c == '"' || c == '\'');
            commit_message = Some(cleaned.to_string());
        } else if trimmed == "::COMMIT" {
            commit_message = Some(String::new());
        } else if trimmed == "::INCOMPLETE" {
            incomplete = true;
        } else if trimmed == "::DONE" {
            done = true;
        } else if trimmed == "::APPROVED" {
            approved = true;
        } else if trimmed.starts_with("::") {
            // T158-PATCHED [PILOT-16]: any line that begins with `::` is
            // a directive; an unrecognized one must be a hard error. The
            // previous fall-through silently dropped typos (`::COMMITE`,
            // `::DNONE`, etc.), so a mistyped directive just disappeared.
            return Err(PilotError::Parse {
                line: line_num + 1,
                message: format!("unknown directive {trimmed:?}"),
            });
        }
    }

    Ok(ParsedOps {
        ops,
        commit_message,
        incomplete,
        done,
        approved,
    })
}

fn read_until_end<'a>(
    lines: &mut impl Iterator<Item = (usize, &'a str)>,
    start_line: usize,
) -> Result<String, PilotError> {
    let mut content_lines = Vec::new();
    for (_, line) in lines {
        if line.trim() == "::END" {
            // T158-PATCHED [PILOT-16]: strip at most *one* trailing empty
            // line, matching the single newline the protocol implies. The
            // previous `while` loop removed every trailing blank line, so
            // a `::WRITE` content that legitimately ended with an empty
            // line lost it — and any `---FIND---` block whose pattern
            // ended in a blank line could never match.
            if content_lines
                .last()
                .is_some_and(|l: &String| l.trim().is_empty())
            {
                content_lines.pop();
            }
            return Ok(content_lines.join("\n"));
        }
        content_lines.push(line.to_string());
    }
    Err(PilotError::Parse {
        line: start_line + 1,
        message: "unexpected end of input: expected ::END".into(),
    })
}

fn read_find_replace<'a>(
    lines: &mut impl Iterator<Item = (usize, &'a str)>,
    start_line: usize,
) -> Result<(String, String), PilotError> {
    let mut find_lines: Vec<String> = Vec::new();
    let mut replace_lines: Vec<String> = Vec::new();
    let mut in_find = false;
    let mut in_replace = false;

    for (_, line) in lines {
        match line.trim() {
            "---FIND---" => {
                in_find = true;
                in_replace = false;
            }
            "---REPLACE---" => {
                in_find = false;
                in_replace = true;
            }
            "::END" => {
                while find_lines
                    .last()
                    .is_some_and(|l: &String| l.trim().is_empty())
                {
                    find_lines.pop();
                }
                while replace_lines
                    .last()
                    .is_some_and(|l: &String| l.trim().is_empty())
                {
                    replace_lines.pop();
                }
                return Ok((find_lines.join("\n"), replace_lines.join("\n")));
            }
            _ => {
                if in_find {
                    find_lines.push(line.to_string());
                } else if in_replace {
                    replace_lines.push(line.to_string());
                }
            }
        }
    }
    Err(PilotError::Parse {
        line: start_line + 1,
        message: "unexpected end of input: expected ::END in REPLACE block".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_write() {
        let input = "::WRITE src/main.rs\nfn main() {}\n::END";
        let result = parse_ops_block(input).unwrap();
        assert_eq!(
            result.ops[0],
            FileOp::Write {
                path: "src/main.rs".into(),
                content: "fn main() {}".into()
            }
        );
    }

    #[test]
    fn test_parse_replace() {
        let input = "::REPLACE src/lib.rs\n---FIND---\nold\n---REPLACE---\nnew\n::END";
        let result = parse_ops_block(input).unwrap();
        assert_eq!(
            result.ops[0],
            FileOp::Replace {
                path: "src/lib.rs".into(),
                find: "old".into(),
                replace: "new".into()
            }
        );
    }

    #[test]
    fn test_extract_nested_fences_inside_write() {
        let response = "```glyim-ops\n::WRITE readme.md\n# Hello\n\n```rust\nfn main() {}\n```\n\nMore text\n::END\n```";
        let blocks = extract_ops_blocks(response);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].contains("fn main() {}"));
    }

    #[test]
    fn test_write_without_end_is_error() {
        let input = "::WRITE src/main.rs\nfn main() {}";
        assert!(parse_ops_block(input).is_err());
    }
}
