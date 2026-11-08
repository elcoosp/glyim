use crate::lexer::lex;
use glyim_span::FileId;
use glyim_syntax::SyntaxKind;

fn lex_single(source: &str) -> Vec<crate::lexer::Token> {
    lex(source, FileId::from_raw(0)).tokens
}

#[test]
fn all_keywords_produce_correct_kind() {
    let keywords: &[(&str, SyntaxKind)] = &[
        ("fn", SyntaxKind::KwFn),
        ("let", SyntaxKind::KwLet),
        ("struct", SyntaxKind::KwStruct),
        ("enum", SyntaxKind::KwEnum),
        ("if", SyntaxKind::KwIf),
        ("else", SyntaxKind::KwElse),
        ("return", SyntaxKind::KwReturn),
        ("match", SyntaxKind::KwMatch),
        ("mod", SyntaxKind::KwMod),
        ("comptime", SyntaxKind::KwComptime),
        ("self", SyntaxKind::KwSelf),
        ("super", SyntaxKind::KwSuper),
        ("crate", SyntaxKind::KwCrate),
        ("true", SyntaxKind::KwTrue),
        ("false", SyntaxKind::KwFalse),
        ("mut", SyntaxKind::KwMut),
        ("ref", SyntaxKind::KwRef),
        ("as", SyntaxKind::KwAs),
        ("while", SyntaxKind::KwWhile),
        ("for", SyntaxKind::KwFor),
        ("in", SyntaxKind::KwIn),
        ("break", SyntaxKind::KwBreak),
        ("continue", SyntaxKind::KwContinue),
        ("trait", SyntaxKind::KwTrait),
        ("impl", SyntaxKind::KwImpl),
        ("where", SyntaxKind::KwWhere),
        ("type", SyntaxKind::KwType),
        ("pub", SyntaxKind::KwPub),
        ("priv", SyntaxKind::KwPriv),
        ("extern", SyntaxKind::KwExtern),
        ("unsafe", SyntaxKind::KwUnsafe),
        ("const", SyntaxKind::KwConst),
        ("static", SyntaxKind::KwStatic),
        ("_", SyntaxKind::Underscore),
    ];

    for (text, expected_kind) in keywords {
        let tokens = lex_single(text);
        assert_eq!(
            tokens.len(),
            1,
            "keyword should produce exactly 1 token for '{}', got {}",
            text,
            tokens.len()
        );
        assert_eq!(
            tokens[0].kind, *expected_kind,
            "keyword '{}' should be {:?}, got {:?}",
            text, expected_kind, tokens[0].kind
        );
        assert_eq!(tokens[0].text, *text, "keyword '{text}' text mismatch");
    }
}

#[test]
fn ident_not_keyword() {
    let tokens = lex_single("foobar");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::Ident);
    assert_eq!(tokens[0].text, "foobar");
}

#[test]
fn keyword_prefix_is_ident() {
    let tokens = lex_single("func");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::Ident);
    assert_eq!(tokens[0].text, "func");
}

#[test]
fn underscore_prefix_is_ident() {
    let tokens = lex_single("_foo");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::Ident);
    assert_eq!(tokens[0].text, "_foo");
}
