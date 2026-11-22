use crate::lexer::lex;
use glyim_span::FileId;
use glyim_syntax::SyntaxKind;

fn lex_tokens(source: &str) -> Vec<crate::lexer::Token> {
    lex(source, FileId::from_usize(0)).tokens
}

#[test]
fn decimal_zero() {
    let tokens = lex_tokens("0");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "0");
}

#[test]
fn decimal_42() {
    let tokens = lex_tokens("42");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "42");
}

#[test]
fn decimal_large() {
    let tokens = lex_tokens("999999");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "999999");
}

#[test]
fn hex_ff() {
    let tokens = lex_tokens("0xFF");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "0xFF");
}

#[test]
fn hex_zero() {
    let tokens = lex_tokens("0x0");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "0x0");
}

#[test]
fn hex_uppercase_prefix() {
    let tokens = lex_tokens("0XAB");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "0XAB");
}

#[test]
fn binary_zero() {
    let tokens = lex_tokens("0b0");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "0b0");
}

#[test]
fn binary_1010() {
    let tokens = lex_tokens("0b1010");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "0b1010");
}

#[test]
fn binary_uppercase_prefix() {
    let tokens = lex_tokens("0B1101");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "0B1101");
}

#[test]
fn octal_zero() {
    let tokens = lex_tokens("0o0");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "0o0");
}

#[test]
fn octal_77() {
    let tokens = lex_tokens("0o77");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "0o77");
}

#[test]
fn octal_uppercase_prefix() {
    let tokens = lex_tokens("0O55");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "0O55");
}

#[test]
fn int_suffix_u8() {
    let tokens = lex_tokens("42u8");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "42u8");
}

#[test]
fn int_suffix_i32() {
    let tokens = lex_tokens("0xFFi32");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "0xFFi32");
}

#[test]
fn int_suffix_i64() {
    let tokens = lex_tokens("123i64");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "123i64");
}

#[test]
fn int_with_underscores() {
    let tokens = lex_tokens("1_000_000");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "1_000_000");
}

#[test]
fn int_adjacent_to_ident() {
    let tokens = lex_tokens("42abc");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "42abc");
}
