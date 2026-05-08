use glyim_lex::tokenize;
use glyim_syntax::SyntaxKind;

/// Configuration for the formatter.
#[derive(Debug, Clone)]
pub struct FormatConfig {
    /// Number of spaces per indentation level.
    pub indent_width: usize,
    /// Maximum line width before wrapping (not enforced yet).
    pub max_width: usize,
    /// Whether to use spaces (true) or tabs (false) for indentation.
    pub use_spaces: bool,
    /// Whether to insert a trailing newline at end of file.
    pub trailing_newline: bool,
}

impl Default for FormatConfig {
    fn default() -> Self {
        Self {
            indent_width: 4,
            max_width: 100,
            use_spaces: true,
            trailing_newline: true,
        }
    }
}

/// Format a source string.
pub fn format_source(source: &str, config: &FormatConfig) -> Result<String, FormatError> {
    let tokens = tokenize(source);
    if tokens.is_empty() {
        return Ok(if config.trailing_newline {
            "\n".to_string()
        } else {
            String::new()
        });
    }

    let mut out = String::new();
    let mut indent: usize = 0;
    let mut at_line_start = true;
    let mut depth: i32 = 0;

    for tok in &tokens {
        match tok.kind {
            SyntaxKind::Whitespace => {
                // Skip original whitespace; we control spacing.
                continue;
            }
            SyntaxKind::LineComment => {
                if !out.ends_with('\n') {
                    out.push('\n');
                    at_line_start = true;
                }
                if at_line_start && indent > 0 {
                    for _ in 0..indent {
                        if config.use_spaces {
                            for _ in 0..config.indent_width {
                                out.push(' ');
                            }
                        } else {
                            out.push('\t');
                        }
                    }
                    // at_line_start = false;
                }
                out.push_str(tok.text);
                out.push('\n');
                at_line_start = true;
                continue;
            }
            SyntaxKind::BlockComment => {
                out.push(' ');
                out.push_str(tok.text);
                out.push(' ');
                continue;
            }
            _ => {}
        }

        if at_line_start && indent > 0 {
            for _ in 0..indent {
                if config.use_spaces {
                    for _ in 0..config.indent_width {
                        out.push(' ');
                    }
                } else {
                    out.push('\t');
                }
            }
            // at_line_start = false;
        }

        out.push_str(tok.text);

        match tok.kind {
            SyntaxKind::LBrace => {
                depth += 1;
                indent = depth as usize;
                out.push('\n');
                at_line_start = true;
            }
            SyntaxKind::RBrace => {
                depth -= 1;
                indent = depth as usize;
                if !out.ends_with('\n') {
                    out.push('\n');
                    at_line_start = true;
                }
            }
            SyntaxKind::Semicolon => {
                out.push('\n');
                at_line_start = true;
            }
            _ => {}
        }

        if out.ends_with('\n') && !at_line_start {
            // already set above
        }
    }

    if config.trailing_newline && !out.ends_with('\n') {
        out.push('\n');
    }

    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    Internal(String),
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormatError::Internal(msg) => write!(f, "format error: {}", msg),
        }
    }
}

impl std::error::Error for FormatError {}

#[cfg(test)]
mod tests;
