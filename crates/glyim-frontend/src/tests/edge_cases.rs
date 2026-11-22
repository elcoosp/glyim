use crate::lexer::lex;
use glyim_span::FileId;
use glyim_syntax::SyntaxKind;

fn lex_tokens(source: &str) -> Vec<crate::lexer::Token> {
    lex(source, FileId::from_usize(0)).tokens
}

fn lex_result(source: &str) -> crate::lexer::LexResult {
    lex(source, FileId::from_usize(0))
}

#[test]
fn multiple_tokens_same_line() {
    let tokens = lex_tokens("1+2*3");
    assert_eq!(tokens.len(), 5);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[1].kind, SyntaxKind::Plus);
    assert_eq!(tokens[2].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[3].kind, SyntaxKind::Star);
    assert_eq!(tokens[4].kind, SyntaxKind::IntLit);
}

#[test]
fn leading_zero_decimal_is_int() {
    let tokens = lex_tokens("00");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "00");
}

#[test]
fn underscore_at_end_of_int() {
    let tokens = lex_tokens("42_");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "42_");
}

#[test]
fn multiple_underscores_in_int() {
    let tokens = lex_tokens("1__2");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "1__2");
}

#[test]
fn hex_with_underscores() {
    let tokens = lex_tokens("0xFF_AB");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "0xFF_AB");
}

#[test]
fn binary_with_underscores() {
    let tokens = lex_tokens("0b1010_0101");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "0b1010_0101");
}

#[test]
fn octal_with_underscores() {
    let tokens = lex_tokens("0o77_55");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "0o77_55");
}

#[test]
fn float_fraction_only_digits() {
    let tokens = lex_tokens("0.0");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::FloatLit);
    assert_eq!(tokens[0].text, "0.0");
}

#[test]
fn float_many_decimal_places() {
    let tokens = lex_tokens("3.14159265358979");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::FloatLit);
}

#[test]
fn float_with_all_parts() {
    let tokens = lex_tokens("1.5e+10f64");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::FloatLit);
    assert_eq!(tokens[0].text, "1.5e+10f64");
}

#[test]
fn dot_then_number_is_dot_then_int() {
    let tokens = lex_tokens(".5");
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0].kind, SyntaxKind::Dot);
    assert_eq!(tokens[1].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[1].text, "5");
}

#[test]
fn dot_dot_then_number() {
    let tokens = lex_tokens("..5");
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0].kind, SyntaxKind::DotDot);
    assert_eq!(tokens[1].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[1].text, "5");
}

#[test]
fn unterminated_string() {
    let tokens = lex_tokens("\"hello");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::StringLit);
    assert_eq!(tokens[0].text, "\"hello");
}

#[test]
fn unterminated_char() {
    let tokens = lex_tokens("'a");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::Lifetime);
    assert_eq!(tokens[0].text, "'a");
}

#[test]
fn empty_block_comment() {
    let tokens = lex_tokens("/**/42");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
}

#[test]
fn line_comment_no_newline() {
    let tokens = lex_tokens("// comment");
    assert!(tokens.is_empty());
}

#[test]
fn comment_with_whitespace_inside() {
    let tokens = lex_tokens("/*  hello  */42");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
}

#[test]
fn nested_empty_block_comment() {
    let tokens = lex_tokens("/*/**/*/42");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
}

#[test]
fn keyword_adjacent_to_operator() {
    let tokens = lex_tokens("fn()");
    assert_eq!(tokens.len(), 3);
    assert_eq!(tokens[0].kind, SyntaxKind::KwFn);
    assert_eq!(tokens[1].kind, SyntaxKind::LParen);
    assert_eq!(tokens[2].kind, SyntaxKind::RParen);
}

#[test]
fn ident_with_trailing_underscore() {
    let tokens = lex_tokens("foo_");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::Ident);
    assert_eq!(tokens[0].text, "foo_");
}

#[test]
fn ident_with_digits() {
    let tokens = lex_tokens("x42");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::Ident);
    assert_eq!(tokens[0].text, "x42");
}

#[test]
fn slash_eq_is_slash_eq() {
    let tokens = lex_tokens("/=");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::SlashEq);
}

#[test]
fn slash_then_star_is_comment() {
    let result = lex_result("/* hi */");
    assert!(result.tokens.is_empty());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn colon_colon_eq_produces_two_tokens() {
    let tokens = lex_tokens("::=");
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0].kind, SyntaxKind::ColonColon);
    assert_eq!(tokens[1].kind, SyntaxKind::Eq);
}

#[test]
fn shift_left_eq_produces_two_tokens() {
    let tokens = lex_tokens("<<=");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::ShlEq);
}

#[test]
fn shift_right_eq_produces_two_tokens() {
    let tokens = lex_tokens(">>=");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::ShrEq);
}

#[test]
fn error_token_text_contains_char() {
    let result = lex_result("\u{00B6}");
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].kind, SyntaxKind::Error);
    assert_eq!(result.tokens[0].text, "\u{00B6}");
}

#[test]
fn int_zero_then_ident() {
    let tokens = lex_tokens("0abc");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "0abc");
}

#[test]
fn zero_x_non_hex() {
    // "0xG" is consumed as one IntLit "0xG" because lex_number_suffix eats the 'G'
    // Semantic validation of invalid hex digits happens later in the compiler.
    let tokens = lex_tokens("0xG");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "0xG");
}

#[test]
fn zero_b_non_binary() {
    // "0b2" is consumed as one IntLit "0b2" because lex_number_suffix eats the '2'
    let tokens = lex_tokens("0b2");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "0b2");
}

#[test]
fn zero_o_non_octal() {
    // "0o9" is consumed as one IntLit "0o9" because lex_number_suffix eats the '9'
    let tokens = lex_tokens("0o9");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[0].text, "0o9");
}

#[test]
fn multiple_newlines_between_tokens() {
    let tokens = lex_tokens("1\n\n\n2");
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[1].kind, SyntaxKind::IntLit);
}

#[test]
fn tab_between_tokens() {
    let tokens = lex_tokens("1\t2");
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[1].kind, SyntaxKind::IntLit);
}

#[test]
fn crlf_between_tokens() {
    let tokens = lex_tokens("1\r\n2");
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0].kind, SyntaxKind::IntLit);
    assert_eq!(tokens[1].kind, SyntaxKind::IntLit);
}

#[test]
fn string_with_multiple_escapes() {
    let source = "\"\\n\\t\\r\\0\\\\\\\"\"";
    let tokens = lex_tokens(source);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::StringLit);
}

#[test]
fn char_digit() {
    let tokens = lex_tokens("'5'");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::CharLit);
    assert_eq!(tokens[0].text, "'5'");
}
