use crate::lexer::lex;
use glyim_span::FileId;
use glyim_syntax::SyntaxKind;

fn lex_tokens(source: &str) -> Vec<crate::lexer::Token> {
    lex(source, FileId::from_usize(0)).tokens
}

#[test]
fn single_char_operators() {
    let cases: &[(&str, SyntaxKind)] = &[
        ("(", SyntaxKind::LParen),
        (")", SyntaxKind::RParen),
        ("{", SyntaxKind::LBrace),
        ("}", SyntaxKind::RBrace),
        ("[", SyntaxKind::LBracket),
        ("]", SyntaxKind::RBracket),
        (",", SyntaxKind::Comma),
        (";", SyntaxKind::Semicolon),
        (".", SyntaxKind::Dot),
        (":", SyntaxKind::Colon),
        ("+", SyntaxKind::Plus),
        ("-", SyntaxKind::Minus),
        ("*", SyntaxKind::Star),
        ("/", SyntaxKind::Slash),
        ("%", SyntaxKind::Percent),
        ("=", SyntaxKind::Eq),
        ("!", SyntaxKind::Bang),
        ("<", SyntaxKind::Lt),
        (">", SyntaxKind::Gt),
        ("&", SyntaxKind::And),
        ("|", SyntaxKind::Or),
        ("^", SyntaxKind::Caret),
        ("@", SyntaxKind::At),
        ("#", SyntaxKind::Hash),
        ("$", SyntaxKind::Dollar),
        ("~", SyntaxKind::Tilde),
        ("?", SyntaxKind::Question),
    ];

    for (text, expected_kind) in cases {
        let tokens = lex_tokens(text);
        assert_eq!(
            tokens.len(),
            1,
            "operator should produce exactly 1 token for '{}', got {}: {:?}",
            text,
            tokens.len(),
            tokens
        );
        assert_eq!(
            tokens[0].kind, *expected_kind,
            "operator '{}' should be {:?}, got {:?}",
            text, expected_kind, tokens[0].kind
        );
    }
}

#[test]
fn compound_operators() {
    let cases: &[(&str, SyntaxKind)] = &[
        ("+=", SyntaxKind::PlusEq),
        ("-=", SyntaxKind::MinusEq),
        ("*=", SyntaxKind::StarEq),
        ("/=", SyntaxKind::SlashEq),
        ("::", SyntaxKind::ColonColon),
        ("->", SyntaxKind::Arrow),
        ("=>", SyntaxKind::FatArrow),
        ("==", SyntaxKind::EqEq),
        ("!=", SyntaxKind::BangEq),
        ("<=", SyntaxKind::LtEq),
        (">=", SyntaxKind::GtEq),
        ("&&", SyntaxKind::AndAnd),
        ("||", SyntaxKind::OrOr),
        ("..", SyntaxKind::DotDot),
        ("..=", SyntaxKind::DotDotEq),
        ("<<", SyntaxKind::Shl),
        (">>", SyntaxKind::Shr),
    ];

    for (text, expected_kind) in cases {
        let tokens = lex_tokens(text);
        assert_eq!(
            tokens.len(),
            1,
            "compound operator should produce exactly 1 token for '{}', got {}: {:?}",
            text,
            tokens.len(),
            tokens
        );
        assert_eq!(
            tokens[0].kind, *expected_kind,
            "compound operator '{}' should be {:?}, got {:?}",
            text, expected_kind, tokens[0].kind
        );
    }
}

#[test]
fn compound_vs_single_resolution() {
    let tokens = lex_tokens("==");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::EqEq);

    let tokens = lex_tokens("= =");
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0].kind, SyntaxKind::Eq);
    assert_eq!(tokens[1].kind, SyntaxKind::Eq);
}

#[test]
fn arrow_vs_minus_gt() {
    let tokens = lex_tokens("->");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::Arrow);

    let tokens = lex_tokens("- >");
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0].kind, SyntaxKind::Minus);
    assert_eq!(tokens[1].kind, SyntaxKind::Gt);
}

#[test]
fn fat_arrow_vs_eq_gt() {
    let tokens = lex_tokens("=>");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::FatArrow);

    let tokens = lex_tokens("= >");
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0].kind, SyntaxKind::Eq);
    assert_eq!(tokens[1].kind, SyntaxKind::Gt);
}

#[test]
fn dot_dot_eq_chain() {
    let tokens = lex_tokens("..=");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::DotDotEq);
}
