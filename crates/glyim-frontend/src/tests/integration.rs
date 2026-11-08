use crate::lexer::{LexResult, Token, lex};
use glyim_span::{ByteIdx, FileId, Span, SyntaxContext};
use glyim_syntax::SyntaxKind;

#[test]
fn lex_returns_lex_result() {
    let result: LexResult = lex("fn main() {}", FileId::from_raw(0));
    assert!(!result.tokens.is_empty());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn lex_result_has_tokens_and_diagnostics_fields() {
    let result = lex("42 \u{00A9}", FileId::from_raw(0));
    assert!(!result.tokens.is_empty());
    assert!(!result.diagnostics.is_empty());
}

#[test]
fn token_has_kind_span_text_fields() {
    let result = lex("fn", FileId::from_raw(0));
    let token = &result.tokens[0];
    assert_eq!(token.kind, SyntaxKind::KwFn);
    assert_eq!(token.text, "fn");
    let _ = token.span;
}

#[test]
fn token_new_constructor() {
    let span = Span::new(
        FileId::from_raw(1),
        ByteIdx::from_raw(0),
        ByteIdx::from_raw(2),
        SyntaxContext::ROOT,
    );
    let token = Token::new(SyntaxKind::KwFn, span, "fn");
    assert_eq!(token.kind, SyntaxKind::KwFn);
    assert_eq!(token.text, "fn");
    assert_eq!(token.span.file, FileId::from_raw(1));
}

#[test]
fn token_is_clone_and_debug() {
    let result = lex("fn", FileId::from_raw(0));
    let token = result.tokens[0].clone();
    assert_eq!(token.kind, SyntaxKind::KwFn);
    let debug_str = format!("{token:?}");
    assert!(debug_str.contains("KwFn"));
}

#[test]
fn lex_result_is_clone_and_debug() {
    let result = lex("fn", FileId::from_raw(0));
    let cloned = result.clone();
    assert_eq!(cloned.tokens.len(), result.tokens.len());
    let debug_str = format!("{result:?}");
    assert_ne!(debug_str, "");
}

#[test]
fn file_id_propagated_to_all_tokens() {
    let fid = FileId::from_raw(99);
    let result = lex("fn main() { let x = 42; }", fid);
    for token in &result.tokens {
        assert_eq!(
            token.span.file, fid,
            "all tokens should have the same file_id"
        );
    }
}

#[test]
fn file_id_propagated_to_diagnostics() {
    let fid = FileId::from_raw(77);
    let result = lex("\u{00A9}", fid);
    assert!(!result.diagnostics.is_empty());
    for diag in &result.diagnostics {
        assert_eq!(
            diag.span.primary.file, fid,
            "all diagnostics should have the same file_id"
        );
    }
}

#[test]
fn comment_inside_string_is_not_comment() {
    let result = lex("\"// not a comment\"", FileId::from_raw(0));
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::StringLit);
    assert_eq!(result.tokens[0].text, "\"// not a comment\"");
}

#[test]
fn block_comment_inside_string_is_not_comment() {
    let result = lex("\"/* not a comment */\"", FileId::from_raw(0));
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::StringLit);
    assert_eq!(result.tokens[0].text, "\"/* not a comment */\"");
}

#[test]
fn string_after_comment() {
    let result = lex("// comment\n\"hello\"", FileId::from_raw(0));
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::StringLit);
}

#[test]
fn comment_after_string() {
    let result = lex("\"hello\"// comment", FileId::from_raw(0));
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::StringLit);
}

#[test]
fn float_dot_int_interaction() {
    let result = lex("1.2.3", FileId::from_raw(0));
    let kinds: Vec<_> = result.tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![SyntaxKind::FloatLit, SyntaxKind::Dot, SyntaxKind::IntLit]
    );
    assert_eq!(result.tokens[0].text, "1.2");
    assert_eq!(result.tokens[1].text, ".");
    assert_eq!(result.tokens[2].text, "3");
}

#[test]
fn int_dot_dot_int_is_range() {
    let result = lex("1..3", FileId::from_raw(0));
    let kinds: Vec<_> = result.tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![SyntaxKind::IntLit, SyntaxKind::DotDot, SyntaxKind::IntLit]
    );
}

#[test]
fn int_dot_dot_eq_int_is_inclusive_range() {
    let result = lex("1..=3", FileId::from_raw(0));
    let kinds: Vec<_> = result.tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![SyntaxKind::IntLit, SyntaxKind::DotDotEq, SyntaxKind::IntLit]
    );
}

#[test]
fn float_dot_dot_int() {
    let result = lex("1.0..3", FileId::from_raw(0));
    let kinds: Vec<_> = result.tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![SyntaxKind::FloatLit, SyntaxKind::DotDot, SyntaxKind::IntLit]
    );
    assert_eq!(result.tokens[0].text, "1.0");
}

#[test]
fn method_call_on_int() {
    let result = lex("1.abs()", FileId::from_raw(0));
    let kinds: Vec<_> = result.tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::IntLit,
            SyntaxKind::Dot,
            SyntaxKind::Ident,
            SyntaxKind::LParen,
            SyntaxKind::RParen
        ]
    );
}

#[test]
fn method_call_on_float() {
    let result = lex("1.0.abs()", FileId::from_raw(0));
    let kinds: Vec<_> = result.tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::FloatLit,
            SyntaxKind::Dot,
            SyntaxKind::Ident,
            SyntaxKind::LParen,
            SyntaxKind::RParen
        ]
    );
}

#[test]
fn field_access_on_int_dot() {
    let result = lex("1 .field", FileId::from_raw(0));
    let kinds: Vec<_> = result.tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![SyntaxKind::IntLit, SyntaxKind::Dot, SyntaxKind::Ident]
    );
}

#[test]
fn consecutive_strings() {
    let result = lex("\"hello\" \"world\"", FileId::from_raw(0));
    assert_eq!(result.tokens.len(), 2);
    assert_eq!(result.tokens[0].kind, SyntaxKind::StringLit);
    assert_eq!(result.tokens[1].kind, SyntaxKind::StringLit);
}

#[test]
fn consecutive_chars() {
    let result = lex("'a' 'b'", FileId::from_raw(0));
    assert_eq!(result.tokens.len(), 2);
    assert_eq!(result.tokens[0].kind, SyntaxKind::CharLit);
    assert_eq!(result.tokens[1].kind, SyntaxKind::CharLit);
}

#[test]
fn string_then_ident() {
    let result = lex("\"hello\"world", FileId::from_raw(0));
    assert_eq!(result.tokens.len(), 2);
    assert_eq!(result.tokens[0].kind, SyntaxKind::StringLit);
    assert_eq!(result.tokens[1].kind, SyntaxKind::Ident);
}

#[test]
fn ident_then_string() {
    let result = lex("foo\"bar\"", FileId::from_raw(0));
    assert_eq!(result.tokens.len(), 2);
    assert_eq!(result.tokens[0].kind, SyntaxKind::Ident);
    assert_eq!(result.tokens[1].kind, SyntaxKind::StringLit);
}

#[test]
fn int_then_ident_no_space() {
    let result = lex("42abc", FileId::from_raw(0));
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(result.tokens[0].text, "42abc");
}

#[test]
fn ident_then_int_no_space() {
    let result = lex("abc42", FileId::from_raw(0));
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::Ident);
    assert_eq!(result.tokens[0].text, "abc42");
}

#[test]
fn zero_then_dot_then_zero() {
    let result = lex("0.0", FileId::from_raw(0));
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::FloatLit);
}

#[test]
fn negative_not_lexed() {
    let result = lex("-42", FileId::from_raw(0));
    let kinds: Vec<_> = result.tokens.iter().map(|t| t.kind).collect();
    assert_eq!(kinds, vec![SyntaxKind::Minus, SyntaxKind::IntLit]);
}

#[test]
fn double_negation() {
    let result = lex("--x", FileId::from_raw(0));
    let kinds: Vec<_> = result.tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![SyntaxKind::Minus, SyntaxKind::Minus, SyntaxKind::Ident]
    );
}

#[test]
fn not_equals_vs_not_then_eq() {
    let result1 = lex("!=", FileId::from_raw(0));
    assert_eq!(result1.tokens.len(), 1);
    assert_eq!(result1.tokens[0].kind, SyntaxKind::BangEq);

    let result2 = lex("! =", FileId::from_raw(0));
    assert_eq!(result2.tokens.len(), 2);
    assert_eq!(result2.tokens[0].kind, SyntaxKind::Bang);
    assert_eq!(result2.tokens[1].kind, SyntaxKind::Eq);
}

#[test]
fn less_equal_vs_less_then_eq() {
    let result1 = lex("<=", FileId::from_raw(0));
    assert_eq!(result1.tokens.len(), 1);
    assert_eq!(result1.tokens[0].kind, SyntaxKind::LtEq);

    let result2 = lex("< =", FileId::from_raw(0));
    assert_eq!(result2.tokens.len(), 2);
    assert_eq!(result2.tokens[0].kind, SyntaxKind::Lt);
    assert_eq!(result2.tokens[1].kind, SyntaxKind::Eq);
}

#[test]
fn greater_equal_vs_greater_then_eq() {
    let result1 = lex(">=", FileId::from_raw(0));
    assert_eq!(result1.tokens.len(), 1);
    assert_eq!(result1.tokens[0].kind, SyntaxKind::GtEq);

    let result2 = lex("> =", FileId::from_raw(0));
    assert_eq!(result2.tokens.len(), 2);
    assert_eq!(result2.tokens[0].kind, SyntaxKind::Gt);
    assert_eq!(result2.tokens[1].kind, SyntaxKind::Eq);
}

#[test]
fn and_and_vs_and_then_and() {
    let result1 = lex("&&", FileId::from_raw(0));
    assert_eq!(result1.tokens.len(), 1);
    assert_eq!(result1.tokens[0].kind, SyntaxKind::AndAnd);

    let result2 = lex("& &", FileId::from_raw(0));
    assert_eq!(result2.tokens.len(), 2);
    assert_eq!(result2.tokens[0].kind, SyntaxKind::And);
    assert_eq!(result2.tokens[1].kind, SyntaxKind::And);
}

#[test]
fn or_or_vs_or_then_or() {
    let result1 = lex("||", FileId::from_raw(0));
    assert_eq!(result1.tokens.len(), 1);
    assert_eq!(result1.tokens[0].kind, SyntaxKind::OrOr);

    let result2 = lex("| |", FileId::from_raw(0));
    assert_eq!(result2.tokens.len(), 2);
    assert_eq!(result2.tokens[0].kind, SyntaxKind::Or);
    assert_eq!(result2.tokens[1].kind, SyntaxKind::Or);
}

#[test]
fn underscore_alone_is_keyword() {
    let result = lex("_", FileId::from_raw(0));
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::Underscore);
}

#[test]
fn underscore_prefix_is_ident() {
    let result = lex("_foo", FileId::from_raw(0));
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::Ident);
}

#[test]
fn underscore_digit_is_ident() {
    let result = lex("_1", FileId::from_raw(0));
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::Ident);
}

#[test]
fn double_underscore_is_ident() {
    let result = lex("__", FileId::from_raw(0));
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::Ident);
}

#[test]
fn double_underscore_prefix_is_ident() {
    let result = lex("__init", FileId::from_raw(0));
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::Ident);
}

#[test]
fn multiple_file_ids_independent() {
    let result1 = lex("fn", FileId::from_raw(1));
    let result2 = lex("fn", FileId::from_raw(2));
    assert_eq!(result1.tokens[0].span.file, FileId::from_raw(1));
    assert_eq!(result2.tokens[0].span.file, FileId::from_raw(2));
}

#[test]
fn multiple_file_ids_same_source_same_tokens() {
    let result1 = lex("fn main() {}", FileId::from_raw(1));
    let result2 = lex("fn main() {}", FileId::from_raw(2));
    assert_eq!(result1.tokens.len(), result2.tokens.len());
    for (t1, t2) in result1.tokens.iter().zip(result2.tokens.iter()) {
        assert_eq!(t1.kind, t2.kind);
        assert_eq!(t1.text, t2.text);
    }
}

#[test]
fn lex_empty_multiple_times() {
    for _ in 0..10 {
        let result = lex("", FileId::from_raw(0));
        assert!(result.tokens.is_empty());
        assert!(result.diagnostics.is_empty());
    }
}

#[test]
fn lex_same_source_idempotent() {
    let source = "fn main() -> i32 { return 42; }";
    let result1 = lex(source, FileId::from_raw(0));
    let result2 = lex(source, FileId::from_raw(0));
    assert_eq!(result1.tokens.len(), result2.tokens.len());
    for (t1, t2) in result1.tokens.iter().zip(result2.tokens.iter()) {
        assert_eq!(t1.kind, t2.kind);
        assert_eq!(t1.text, t2.text);
    }
}

#[test]
fn span_root_context() {
    let result = lex("fn", FileId::from_raw(0));
    assert_eq!(result.tokens[0].span.ctx, SyntaxContext::ROOT);
}

#[test]
fn incomplete_exponent_then_more_code() {
    let result = lex("1e + 2", FileId::from_raw(0));
    assert!(!result.diagnostics.is_empty());
    let kinds: Vec<_> = result.tokens.iter().map(|t| t.kind).collect();
    assert!(kinds.contains(&SyntaxKind::IntLit));
    assert!(kinds.contains(&SyntaxKind::Plus));
}

#[test]
fn incomplete_exponent_backtrack_text() {
    let result = lex("1e+", FileId::from_raw(0));
    assert!(!result.diagnostics.is_empty());
    assert_eq!(result.tokens[0].text, "1");
    assert_eq!(result.tokens[0].kind, SyntaxKind::IntLit);
}

#[test]
fn float_incomplete_exponent_backtrack_text() {
    let result = lex("1.5e", FileId::from_raw(0));
    assert!(!result.diagnostics.is_empty());
    assert_eq!(result.tokens[0].text, "1.5");
    assert_eq!(result.tokens[0].kind, SyntaxKind::FloatLit);
}

#[test]
fn line_comment_ends_at_newline_not_before() {
    let result = lex("1 // comment\n2", FileId::from_raw(0));
    assert_eq!(result.tokens.len(), 2);
    assert_eq!(result.tokens[0].text, "1");
    assert_eq!(result.tokens[1].text, "2");
}

#[test]
fn block_comment_with_newlines() {
    let result = lex("1 /* line1\nline2 */ 2", FileId::from_raw(0));
    assert_eq!(result.tokens.len(), 2);
    assert_eq!(result.tokens[0].text, "1");
    assert_eq!(result.tokens[1].text, "2");
}

#[test]
fn empty_source_with_only_newlines() {
    let result = lex("\n\n\n", FileId::from_raw(0));
    assert!(result.tokens.is_empty());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn source_with_only_brackets() {
    let result = lex("()[]{}", FileId::from_raw(0));
    assert_eq!(result.tokens.len(), 6);
    let kinds: Vec<_> = result.tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::LParen,
            SyntaxKind::RParen,
            SyntaxKind::LBracket,
            SyntaxKind::RBracket,
            SyntaxKind::LBrace,
            SyntaxKind::RBrace,
        ]
    );
}

#[test]
fn deeply_nested_brackets() {
    let source = "(((())))".to_string();
    let result = lex(&source, FileId::from_raw(0));
    assert_eq!(result.tokens.len(), 8);
    let parens: Vec<_> = result
        .tokens
        .iter()
        .filter(|t| t.kind == SyntaxKind::LParen)
        .collect();
    assert_eq!(parens.len(), 4);
}

#[test]
fn all_single_char_punctuation_in_one_source() {
    let source = "(){}[],;.: +-*/%=! <>&|^@#$~?";
    let result = lex(source, FileId::from_raw(0));
    assert!(!result.tokens.is_empty());
    assert!(
        result.diagnostics.is_empty(),
        "no errors for valid punctuation"
    );
}

#[test]
fn number_then_string() {
    let result = lex("42\"hello\"", FileId::from_raw(0));
    assert_eq!(result.tokens.len(), 2);
    assert_eq!(result.tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(result.tokens[1].kind, SyntaxKind::StringLit);
}

#[test]
fn string_then_number() {
    let result = lex("\"hello\"42", FileId::from_raw(0));
    assert_eq!(result.tokens.len(), 2);
    assert_eq!(result.tokens[0].kind, SyntaxKind::StringLit);
    assert_eq!(result.tokens[1].kind, SyntaxKind::IntLit);
}

#[test]
fn char_then_ident() {
    let result = lex("'a'bc", FileId::from_raw(0));
    assert_eq!(result.tokens.len(), 2);
    assert_eq!(result.tokens[0].kind, SyntaxKind::CharLit);
    assert_eq!(result.tokens[1].kind, SyntaxKind::Ident);
}

#[test]
fn multiple_errors_with_valid_code_interleaved() {
    let result = lex("fn \u{00A9} foo(\u{00AE}) {}", FileId::from_raw(0));
    let kinds: Vec<_> = result.tokens.iter().map(|t| t.kind).collect();
    assert_eq!(result.diagnostics.len(), 2);
    assert!(kinds.contains(&SyntaxKind::KwFn));
    assert!(kinds.contains(&SyntaxKind::Ident));
    assert!(kinds.contains(&SyntaxKind::Error));
    assert!(kinds.contains(&SyntaxKind::LParen));
    assert!(kinds.contains(&SyntaxKind::RParen));
}
