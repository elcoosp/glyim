use crate::lexer::lex;
use glyim_span::FileId;
use glyim_syntax::SyntaxKind;

fn lex_tokens(source: &str) -> Vec<crate::lexer::Token> {
    lex(source, FileId::from_usize(0)).tokens
}

#[test]
fn int_suffix_u8() {
    let tokens = lex_tokens("0u8");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "0u8");
}

#[test]
fn int_suffix_i8() {
    let tokens = lex_tokens("1i8");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "1i8");
}

#[test]
fn int_suffix_u16() {
    let tokens = lex_tokens("100u16");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "100u16");
}

#[test]
fn int_suffix_i16() {
    let tokens = lex_tokens("100i16");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "100i16");
}

#[test]
fn int_suffix_u32() {
    let tokens = lex_tokens("100000u32");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "100000u32");
}

#[test]
fn int_suffix_i32() {
    let tokens = lex_tokens("100000i32");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "100000i32");
}

#[test]
fn int_suffix_u64() {
    let tokens = lex_tokens("9999999999u64");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "9999999999u64");
}

#[test]
fn int_suffix_i64() {
    let tokens = lex_tokens("9999999999i64");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "9999999999i64");
}

#[test]
fn int_suffix_usize() {
    let tokens = lex_tokens("42usize");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "42usize");
}

#[test]
fn int_suffix_isize() {
    let tokens = lex_tokens("42isize");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "42isize");
}

#[test]
fn hex_with_suffix() {
    let tokens = lex_tokens("0xFFu8");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "0xFFu8");
}

#[test]
fn binary_with_suffix() {
    let tokens = lex_tokens("0b1010u8");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "0b1010u8");
}

#[test]
fn octal_with_suffix() {
    let tokens = lex_tokens("0o77u8");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "0o77u8");
}

#[test]
fn float_suffix_f32() {
    let tokens = lex_tokens("3.14f32");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::FloatLit);
    assert_eq!(tokens[0].text, "3.14f32");
}

#[test]
fn float_suffix_f64() {
    let tokens = lex_tokens("3.14f64");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::FloatLit);
    assert_eq!(tokens[0].text, "3.14f64");
}

#[test]
fn float_exp_with_suffix() {
    let tokens = lex_tokens("1.5e10f64");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::FloatLit);
    assert_eq!(tokens[0].text, "1.5e10f64");
}

#[test]
fn int_with_underscore_and_suffix() {
    let tokens = lex_tokens("1_000_000i32");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "1_000_000i32");
}

#[test]
fn suffix_then_dot() {
    let tokens = lex_tokens("42u8.field");
    assert_eq!(tokens.len(), 3);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "42u8");
    assert_eq!(tokens[1].kind, SyntaxKind::Dot);
    assert_eq!(tokens[2].kind, SyntaxKind::Ident);
}

#[test]
fn suffix_then_plus() {
    let tokens = lex_tokens("42i32+1");
    assert_eq!(tokens.len(), 3);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "42i32");
    assert_eq!(tokens[1].kind, SyntaxKind::Plus);
    assert_eq!(tokens[2].kind, SyntaxKind::IntLit);
}
