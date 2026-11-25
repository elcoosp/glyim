use crate::lexer::lex;
use glyim_span::FileId;
use glyim_syntax::SyntaxKind;

fn lex_tokens(source: &str) -> Vec<crate::lexer::Token> {
    lex(source, FileId::from_usize(0)).tokens
}

fn lex_result(source: &str) -> crate::lexer::LexResult {
    lex(source, FileId::from_usize(0))
}

fn lex_with_fid(source: &str, fid: FileId) -> crate::lexer::LexResult {
    lex(source, fid)
}

#[test]
fn unicode_error_in_middle() {
    let result = lex_with_fid("fn \u{00A9} main()", FileId::from_usize(0));
    let kinds: Vec<_> = result.tokens.iter().map(|t| t.kind).collect();
    assert!(kinds.contains(&SyntaxKind::KwFn), "should still find 'fn'");
    assert!(
        kinds.contains(&SyntaxKind::Ident),
        "should still find 'main'"
    );
    assert!(kinds.contains(&SyntaxKind::Error));
    assert!(!result.diagnostics.is_empty());
}

#[test]
fn unicode_at_start() {
    let result = lex_with_fid("\u{00A9}fn", FileId::from_usize(0));
    assert_eq!(result.tokens[0].kind, SyntaxKind::Error);
    assert_eq!(result.tokens[1].kind, SyntaxKind::KwFn);
}

#[test]
fn unicode_at_end() {
    let result = lex_with_fid("fn\u{00A9}", FileId::from_usize(0));
    assert_eq!(result.tokens[0].kind, SyntaxKind::KwFn);
    assert_eq!(result.tokens[1].kind, SyntaxKind::Error);
}

#[test]
fn multiple_unicode_errors() {
    let result = lex_with_fid("\u{00A9}\u{00AE}\u{00B6}", FileId::from_usize(0));
    assert_eq!(result.tokens.len(), 3);
    for token in &result.tokens {
        assert_eq!(token.kind, SyntaxKind::Error);
    }
    assert_eq!(result.diagnostics.len(), 3);
}

#[test]
fn unicode_error_span_is_correct() {
    let result = lex_with_fid("fn \u{00A9} main()", FileId::from_usize(0));
    let error_tokens: Vec<_> = result
        .tokens
        .iter()
        .filter(|t| t.kind == SyntaxKind::Error)
        .collect();
    assert_eq!(error_tokens.len(), 1);
    assert_eq!(error_tokens[0].text, "\u{00A9}");
    let range = error_tokens[0].span.byte_range();
    assert_eq!(range.start, 3);
    // \u{00A9} is 2 bytes in UTF-8, so end is 3+2=5
    assert_eq!(range.end, 5);
}

#[test]
fn emoji_in_source() {
    let result = lex_with_fid("\u{1F600}", FileId::from_usize(0));
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::Error);
    assert!(!result.diagnostics.is_empty());
}

#[test]
fn unicode_in_string_is_ok() {
    let source = "\"\u{00E9}\u{00F1}\u{00FC}\"";
    let tokens = lex_tokens(source);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::StringLit);
    assert_eq!(tokens[0].text, source);
}

#[test]
fn unicode_in_comment_is_ok() {
    let result = lex_result("// \u{00E9}\n42");
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::IntLit);
    assert!(result.diagnostics.is_empty());
}

#[test]
fn unicode_in_block_comment_is_ok() {
    let result = lex_result("/* \u{00E9} */42");
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::IntLit);
    assert!(result.diagnostics.is_empty());
}

#[test]
fn null_byte_produces_error() {
    let result = lex_with_fid("\0", FileId::from_usize(0));
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::Error);
    assert!(!result.diagnostics.is_empty());
}

#[test]
fn null_byte_between_tokens() {
    let result = lex_with_fid("1\0 2", FileId::from_usize(0));
    let kinds: Vec<_> = result.tokens.iter().map(|t| t.kind).collect();
    assert!(kinds.contains(&SyntaxKind::IntLit));
    assert!(kinds.contains(&SyntaxKind::Error));
    assert!(!result.diagnostics.is_empty());
}

#[test]
fn del_character_produces_error() {
    let result = lex_with_fid("\u{007F}", FileId::from_usize(0));
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::Error);
}

#[test]
fn replacement_character_produces_error() {
    let result = lex_with_fid("\u{FFFD}", FileId::from_usize(0));
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::Error);
}

#[test]
fn byte_order_mark_produces_error() {
    let result = lex_with_fid("\u{FEFF}", FileId::from_usize(0));
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::Error);
}

#[test]
fn mixed_ascii_and_unicode_errors() {
    let result = lex_with_fid("fn \u{00A9} x + \u{00AE} 1", FileId::from_usize(0));
    let kinds: Vec<_> = result.tokens.iter().map(|t| t.kind).collect();
    assert!(kinds.contains(&SyntaxKind::KwFn));
    assert!(kinds.contains(&SyntaxKind::Ident));
    assert!(kinds.contains(&SyntaxKind::Plus));
    assert!(kinds.contains(&SyntaxKind::IntLit));
    assert_eq!(
        kinds.iter().filter(|k| **k == SyntaxKind::Error).count(),
        2,
        "should have 2 error tokens"
    );
}

#[test]
fn cjk_character_produces_error() {
    let result = lex_with_fid("\u{4E16}", FileId::from_usize(0));
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::Error);
}

#[test]
fn right_to_left_mark_produces_error() {
    let result = lex_with_fid("\u{200F}", FileId::from_usize(0));
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::Error);
}

#[test]
fn zero_width_space_produces_error() {
    let result = lex_with_fid("\u{200B}", FileId::from_usize(0));
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::Error);
}

#[test]
fn string_with_unicode_escapes_in_source() {
    let source = "\"hello \\u{00E9} world\"";
    let tokens = lex_tokens(source);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::StringLit);
}

#[test]
fn unicode_error_recovery_preserves_adjacent_tokens() {
    let result = lex_with_fid("let \u{00A9} = 42;", FileId::from_usize(0));
    let texts: Vec<_> = result.tokens.iter().map(|t| t.text.to_string()).collect();
    assert!(texts.contains(&"let".to_string()));
    assert!(texts.contains(&"=".to_string()));
    assert!(texts.contains(&"42".to_string()));
    assert!(texts.contains(&";".to_string()));
}
