//! Consumer walkthrough: drive the frontend's two public entry points —
//! [`glyim_frontend::lex`] and [`glyim_frontend::parse_to_syntax`] — over
//! a small glyim program, the same seam the pipeline driver and the test
//! harness consume.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-frontend --example lex_parse
//! ```
//!
//! The example prints the lexed token stream (trivia retained), the CST
//! root's kind with its top-level children, and every lexer/parser
//! diagnostic (Display-rendered through `glyim-diag`). Feed it
//! ill-formed source to see the diagnostics face: change `PROGRAM` below
//! or pass a file path as the first argument to lex+parse any `.g` file.

use glyim_frontend::{lex, parse_to_syntax};
use glyim_span::FileId;

/// A small glyim program exercising several token classes (keywords,
/// identifiers, literals, punctuation) and two items.
const PROGRAM: &str = "\
fn main() {
    let total: i64 = 40 + 2;
    if total > 41 {
        println_int(total);
    }
}
";

fn main() {
    // A file path argument lexes+parses that file; the built-in program is
    // the default face.
    let (source, name) = match std::env::args().nth(1) {
        Some(path) => (
            std::fs::read_to_string(&path).unwrap_or_else(|err| {
                panic!("cannot read {path}: {err}");
            }),
            path,
        ),
        None => (PROGRAM.to_string(), "example.g".to_string()),
    };
    let file_id = FileId::from_raw(0);
    println!("source: {name} ({} bytes)", source.len());

    // Face 1: the lexer. `lex_with_trivia` keeps trivia (the parser's
    // input); `lex` strips it (scanner consumers).
    let lexed = glyim_frontend::lex_with_trivia(&source, file_id);
    let stripped = lex(&source, file_id);
    println!();
    println!(
        "tokens: {} with trivia, {} stripped ({} trivia tokens)",
        lexed.tokens.len(),
        stripped.tokens.len(),
        lexed.tokens.len() - stripped.tokens.len(),
    );
    for token in lexed.tokens.iter().take(10) {
        println!("  {:?} {:?} {:?}", token.kind, token.text, token.span);
    }
    println!("  … ({} more)", lexed.tokens.len().saturating_sub(10));
    for diagnostic in &lexed.diagnostics {
        println!("lex diagnostic: {diagnostic}");
    }

    // Face 2: the parser. The CST root is `SourceFile`; its children are
    // the top-level items. Diagnostics accumulate across both stages.
    let parsed = parse_to_syntax(&source, file_id);
    println!();
    println!("cst root: {:?}", parsed.root.kind());
    for child in parsed.root.children() {
        println!("  item: {:?}", child.kind());
    }
    println!("diagnostics: {}", parsed.diagnostics.len());
    for diagnostic in &parsed.diagnostics {
        println!("  {diagnostic}");
    }
}
