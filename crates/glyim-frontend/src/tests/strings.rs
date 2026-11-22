use crate::lexer::lex;
use glyim_span::FileId;
use glyim_syntax::SyntaxKind;

fn lex_tokens(source: &str) -> Vec<crate::lexer::Token> {
    lex(source, FileId::from_usize(0)).tokens
}

#[test]
fn simple_string() {
    let tokens = lex_tokens("\"hello\"");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::StringLit);
    assert_eq!(tokens[0].text, "\"hello\"");
}

#[test]
fn empty_string() {
    let tokens = lex_tokens("\"\"");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::StringLit);
    assert_eq!(tokens[0].text, "\"\"");
}

#[test]
fn string_with_escape_backslash() {
    let source = "\"hello\\\\world\"";
    let tokens = lex_tokens(source);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::StringLit);
    assert_eq!(tokens[0].text, source);
}

#[test]
fn string_with_escape_newline() {
    let source = "\"line1\\nline2\"";
    let tokens = lex_tokens(source);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::StringLit);
    assert_eq!(tokens[0].text, source);
}

#[test]
fn string_with_escape_tab() {
    let source = "\"col1\\tcol2\"";
    let tokens = lex_tokens(source);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::StringLit);
    assert_eq!(tokens[0].text, source);
}

#[test]
fn string_with_escape_quote() {
    let source = "\"she said \\\"hi\\\"\"";
    let tokens = lex_tokens(source);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::StringLit);
    assert_eq!(tokens[0].text, source);
}

#[test]
fn string_with_zero_escape() {
    let source = "\"null\\0char\"";
    let tokens = lex_tokens(source);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::StringLit);
    assert_eq!(tokens[0].text, source);
}

#[test]
fn simple_char() {
    let tokens = lex_tokens("'a'");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::CharLit);
    assert_eq!(tokens[0].text, "'a'");
}

#[test]
fn char_backslash_escape() {
    let source = "'\\\\'";
    let tokens = lex_tokens(source);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::CharLit);
    assert_eq!(tokens[0].text, source);
}

#[test]
fn char_newline_escape() {
    let source = "'\\n'";
    let tokens = lex_tokens(source);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::CharLit);
    assert_eq!(tokens[0].text, source);
}

#[test]
fn char_tab_escape() {
    let source = "'\\t'";
    let tokens = lex_tokens(source);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::CharLit);
    assert_eq!(tokens[0].text, source);
}

#[test]
fn char_quote_escape() {
    let source = "'\\''";
    let tokens = lex_tokens(source);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::CharLit);
    assert_eq!(tokens[0].text, source);
}

#[test]
fn char_zero_escape() {
    let source = "'\\0'";
    let tokens = lex_tokens(source);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::CharLit);
    assert_eq!(tokens[0].text, source);
}
