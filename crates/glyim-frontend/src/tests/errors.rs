use crate::lexer::lex;
use glyim_span::FileId;
use glyim_syntax::SyntaxKind;

#[test]
fn unexpected_unicode_produces_error() {
    let result = lex("\u{00A9}", FileId::from_raw(0));
    assert!(
        !result.diagnostics.is_empty(),
        "unicode char should produce a diagnostic"
    );
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::Error);
}

#[test]
fn unexpected_char_span_covers_char() {
    let result = lex("\u{00A9}", FileId::from_raw(0));
    assert_eq!(result.tokens.len(), 1);
    let span = result.tokens[0].span;
    assert_eq!(span.range().start, 0);
    assert!(span.range().end > 0, "span should cover the character");
}

#[test]
fn multiple_unexpected_chars() {
    let result = lex("\u{00A9}\u{00AE}", FileId::from_raw(0));
    assert!(
        result.diagnostics.len() >= 2,
        "two unexpected chars should produce at least 2 diagnostics, got {}",
        result.diagnostics.len()
    );
}

#[test]
fn error_recovery_continues_lexing() {
    let result = lex("42 \u{00A9} 99", FileId::from_raw(0));
    let kinds: Vec<_> = result.tokens.iter().map(|t| t.kind).collect();
    assert!(
        kinds.contains(&SyntaxKind::IntLit),
        "should still lex integers after error, got {kinds:?}"
    );
    assert!(
        kinds.contains(&SyntaxKind::Error),
        "should contain error token, got {kinds:?}"
    );
}

#[test]
fn unexpected_char_diagnostic_message() {
    let result = lex("\u{00A9}", FileId::from_raw(0));
    assert!(!result.diagnostics.is_empty());
    let msg = &result.diagnostics[0].message;
    assert!(
        msg.contains("unexpected character"),
        "diagnostic message should mention unexpected character, got: {msg}"
    );
}
