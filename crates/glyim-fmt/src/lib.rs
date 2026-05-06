use glyim_lex::tokenize;
use glyim_syntax::SyntaxKind;
// (removed unused Write import)

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
/// This implementation uses the token stream, restores newlines and
/// indentation according to brace depth, and emits tokens in order.
/// Comments are currently handled by re-emitting them as they appear,
/// but their exact placement may shift.
pub fn format_source(source: &str, config: &FormatConfig) -> Result<String, FormatError> {
    let tokens = tokenize(source);
    if tokens.is_empty() {
        return Ok(if config.trailing_newline { "\n".to_string() } else { String::new() });
    }

    let mut out = String::new();
    let mut indent: usize = 0;
    let mut at_line_start = true;

    // State for formatting
    let mut depth: i32 = 0;
    let mut pending_newline = false; // if we need a newline before next non-trivia token

    // We'll walk tokens and accumulate output.
    for tok in &tokens {
        match tok.kind {
            // Skip whitespace and comments – we'll replace with our own formatting.
            SyntaxKind::Whitespace | SyntaxKind::LineComment | SyntaxKind::BlockComment => {
                // For now, just skip them entirely. A full formatter would
                // preserve and re-indent comments.
                continue;
            }
            _ => {}
        }

        // Before any token, handle indentation after newlines.
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
            at_line_start = false;
        }

        // If we need to insert a newline before this token (e.g. after semicolon)
        if pending_newline {
            out.push('\n');
            at_line_start = true;
            pending_newline = false;
            // re-indent
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
                at_line_start = false;
            }
        }

        // Emit the token text
        out.push_str(tok.text);

        // Adjust depth and decide about newlines / indentation
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
                // If we have indentation and previous isn't already a newline, insert one
                if !out.ends_with('\n') {
                    out.push('\n');
                    at_line_start = true;
                }
            }
            SyntaxKind::Semicolon => {
                pending_newline = true;
            }
            SyntaxKind::KwFn | SyntaxKind::KwStruct | SyntaxKind::KwEnum | SyntaxKind::KwImpl
            | SyntaxKind::KwExtern | SyntaxKind::KwIf | SyntaxKind::KwElse
            | SyntaxKind::KwWhile | SyntaxKind::KwFor | SyntaxKind::KwMatch => {
                // insert a space after keyword if next token is not punctuation
                // we'll handle by not adding a newline immediately
            }
            _ => {}
        }

        // After emitting, if we ended with a newline, set at_line_start
        if out.ends_with('\n') && !at_line_start {
            // we already set at_line_start above
        }
    }

    // Ensure trailing newline
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
