use crate::lexer::lex;
use glyim_span::FileId;
use glyim_syntax::SyntaxKind;

fn lex_tokens(source: &str) -> Vec<crate::lexer::Token> {
    lex(source, FileId::from_usize(0)).tokens
}

#[test]
fn float_pi() {
    let tokens = lex_tokens("3.14");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::FloatLit);
    assert_eq!(tokens[0].text, "3.14");
}

#[test]
fn float_1_0() {
    let tokens = lex_tokens("1.0");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::FloatLit);
    assert_eq!(tokens[0].text, "1.0");
}

#[test]
fn float_0_5() {
    let tokens = lex_tokens("0.5");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::FloatLit);
    assert_eq!(tokens[0].text, "0.5");
}

#[test]
fn float_with_underscore() {
    let tokens = lex_tokens("1_000.5_0");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::FloatLit);
    assert_eq!(tokens[0].text, "1_000.5_0");
}

#[test]
fn float_exp_1e10() {
    let tokens = lex_tokens("1e10");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::FloatLit);
    assert_eq!(tokens[0].text, "1e10");
}

#[test]
fn float_exp_uppercase() {
    let tokens = lex_tokens("1E10");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::FloatLit);
    assert_eq!(tokens[0].text, "1E10");
}

#[test]
fn float_exp_1_5e_minus_3() {
    let tokens = lex_tokens("1.5e-3");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::FloatLit);
    assert_eq!(tokens[0].text, "1.5e-3");
}

#[test]
fn float_exp_positive_sign() {
    let tokens = lex_tokens("2e+5");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::FloatLit);
    assert_eq!(tokens[0].text, "2e+5");
}

#[test]
fn float_suffix_f64() {
    let tokens = lex_tokens("3.14f64");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::FloatLit);
    assert_eq!(tokens[0].text, "3.14f64");
}

#[test]
fn float_suffix_f32() {
    let tokens = lex_tokens("1.0f32");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::FloatLit);
    assert_eq!(tokens[0].text, "1.0f32");
}

#[test]
fn int_not_float_without_fraction() {
    let tokens = lex_tokens("1.");
    assert_eq!(tokens.len(), 2, "1. should be IntLit then Dot");
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "1");
    assert_eq!(tokens[1].kind, SyntaxKind::Dot);
}

#[test]
fn float_error_incomplete_exponent_1e() {
    let result = lex("1e", FileId::from_usize(0));
    assert!(
        !result.diagnostics.is_empty(),
        "1e should produce an error diagnostic"
    );
    assert!(
        !result.tokens.is_empty(),
        "1e should produce at least one token"
    );
    assert_eq!(
        result.tokens[0].kind,
        SyntaxKind::IntLit,
        "1e should produce IntLit for the digits, got {:?}",
        result.tokens[0].kind
    );
    assert_eq!(result.tokens[0].text, "1");
}

#[test]
fn float_error_exponent_sign_only_1e_plus() {
    let result = lex("1e+", FileId::from_usize(0));
    assert!(
        !result.diagnostics.is_empty(),
        "1e+ should produce an error diagnostic"
    );
    assert!(
        !result.tokens.is_empty(),
        "1e+ should produce at least one token"
    );
    assert_eq!(
        result.tokens[0].kind,
        SyntaxKind::IntLit,
        "1e+ should produce IntLit for the digits, got {:?}",
        result.tokens[0].kind
    );
    assert_eq!(result.tokens[0].text, "1");
}

#[test]
fn float_error_incomplete_exponent_with_fraction() {
    let result = lex("1.0e", FileId::from_usize(0));
    assert!(
        !result.diagnostics.is_empty(),
        "1.0e should produce an error diagnostic"
    );
    assert!(
        !result.tokens.is_empty(),
        "1.0e should produce at least one token"
    );
    assert_eq!(
        result.tokens[0].kind,
        SyntaxKind::FloatLit,
        "1.0e should produce FloatLit for the digits, got {:?}",
        result.tokens[0].kind
    );
    assert_eq!(result.tokens[0].text, "1.0");
}
