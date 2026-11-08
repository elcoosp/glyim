use crate::lexer::lex;
use glyim_span::FileId;
use glyim_syntax::SyntaxKind;

fn lex_result(source: &str) -> crate::lexer::LexResult {
    lex(source, FileId::from_raw(0))
}

#[test]
fn error_between_valid_tokens_recovers() {
    let result = lex_result("fn \u{00A9} foo() {}");
    let kinds: Vec<_> = result.tokens.iter().map(|t| t.kind).collect();
    assert!(kinds.contains(&SyntaxKind::KwFn), "should still find 'fn'");
    assert!(
        kinds.contains(&SyntaxKind::Ident),
        "should still find 'foo'"
    );
    assert!(
        kinds.contains(&SyntaxKind::Error),
        "should have error token"
    );
    assert!(!result.diagnostics.is_empty());
}

#[test]
fn multiple_errors_all_reported() {
    let result = lex_result("\u{00A9}\u{00AE}\u{00B6}");
    assert_eq!(
        result.diagnostics.len(),
        3,
        "each bad char should produce a diagnostic"
    );
    assert_eq!(result.tokens.len(), 3);
    for token in &result.tokens {
        assert_eq!(token.kind, SyntaxKind::Error);
    }
}

#[test]
fn error_does_not_infinite_loop() {
    let result = lex_result("\u{00A9}");
    assert_eq!(result.tokens.len(), 1, "should make progress past error");
}

#[test]
fn error_spans_are_non_overlapping() {
    let result = lex_result("\u{00A9} \u{00AE} \u{00B6}");
    let mut prev_end = 0usize;
    for token in &result.tokens {
        let range = token.span.range();
        assert!(
            range.start >= prev_end,
            "error token spans should not overlap: prev_end={}, start={}",
            prev_end,
            range.start
        );
        prev_end = range.end;
    }
}

#[test]
fn diagnostic_spans_match_error_tokens() {
    let result = lex_result("\u{00A9}");
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.tokens.len(), 1);
    let diag_span = result.diagnostics[0].span.primary;
    let token_span = result.tokens[0].span;
    assert_eq!(diag_span.range(), token_span.range());
}

#[test]
fn incomplete_float_exponent_emits_diagnostic() {
    let result = lex_result("1e");
    assert!(
        !result.diagnostics.is_empty(),
        "1e should produce a diagnostic"
    );
    assert!(result.diagnostics[0].message.contains("exponent"));
}

#[test]
fn incomplete_float_exponent_sign_emits_diagnostic() {
    let result = lex_result("1.5e+");
    assert!(
        !result.diagnostics.is_empty(),
        "1.5e+ should produce a diagnostic"
    );
}

#[test]
fn recovery_after_incomplete_exponent() {
    let result = lex_result("1e + 2");
    let kinds: Vec<_> = result.tokens.iter().map(|t| t.kind).collect();
    assert!(
        kinds.contains(&SyntaxKind::IntLit),
        "should still lex integers, got {kinds:?}"
    );
    assert!(
        kinds.contains(&SyntaxKind::Plus),
        "should still lex plus, got {kinds:?}"
    );
}

#[test]
fn valid_tokens_surrounding_errors_preserve_text() {
    let result = lex_result("let \u{00A9} x = 1;");
    let texts: Vec<_> = result.tokens.iter().map(|t| t.text.as_str()).collect();
    assert!(texts.contains(&"let"), "should preserve 'let' text");
    assert!(texts.contains(&"x"), "should preserve 'x' text");
    assert!(texts.contains(&"="), "should preserve '=' text");
    assert!(texts.contains(&"1"), "should preserve '1' text");
}

#[test]
fn unterminated_string_no_crash() {
    let result = lex_result("\"unterminated");
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::StringLit);
}

#[test]
fn unterminated_char_no_crash() {
    let result = lex_result("'u");
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::Lifetime);
}

#[test]
fn unterminated_string_followed_by_more() {
    let result = lex_result("\"hello world");
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::StringLit);
    assert_eq!(result.tokens[0].text, "\"hello world");
}

#[test]
fn unterminated_char_with_escape() {
    let result = lex_result("'\\n");
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::CharLit);
}

#[test]
fn unterminated_block_comment_no_crash() {
    let result = lex_result("/* never ending");
    assert!(
        result.tokens.is_empty(),
        "unterminated block comment is trivia"
    );
}

#[test]
fn deeply_nested_comments() {
    let source = "/* a /* b /* c /* d */ c2 */ b2 */ a2 */ 42";
    let result = lex_result(source);
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(result.tokens[0].text, "42");
}

#[test]
fn comment_with_stars_inside() {
    let result = lex_result("/* ** * */ 42");
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::IntLit);
}

#[test]
fn line_comment_with_backslash_n_in_text() {
    let result = lex_result("// hello\\nworld\n42");
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::IntLit);
}

#[test]
fn error_token_kind_is_error() {
    let result = lex_result("\u{2603}");
    assert_eq!(result.tokens[0].kind, SyntaxKind::Error);
}

#[test]
fn multiple_different_errors_in_sequence() {
    let result = lex_result("\u{00A9} 42 \u{00AE} fn \u{00B6}");
    let kinds: Vec<_> = result.tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::Error,
            SyntaxKind::IntLit,
            SyntaxKind::Error,
            SyntaxKind::KwFn,
            SyntaxKind::Error,
        ]
    );
    assert_eq!(result.diagnostics.len(), 3);
}
