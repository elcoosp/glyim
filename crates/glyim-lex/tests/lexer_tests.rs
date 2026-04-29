//! Lexer integration tests (extracted from src/lexer.rs)
use glyim_lex::tokenize;
use glyim_syntax::SyntaxKind;

#[allow(dead_code)]
#[allow(dead_code)]
fn non_trivia_tokens(input: &str) -> Vec<(SyntaxKind, &str)> {
    tokenize(input)
        .iter()
        .filter(|t| !t.kind.is_trivia())
        .map(|t| (t.kind, t.text))
        .collect()
}
#[allow(dead_code)]
#[allow(dead_code)]
fn all_tokens(input: &str) -> Vec<(SyntaxKind, &str)> {
    tokenize(input).iter().map(|t| (t.kind, t.text)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_syntax::SyntaxKind;

    #[allow(dead_code)]
    #[allow(dead_code)]
    fn non_trivia_tokens(input: &str) -> Vec<(SyntaxKind, &str)> {
        tokenize(input)
            .iter()
            .filter(|t| !t.kind.is_trivia())
            .map(|t| (t.kind, t.text))
            .collect()
    }
    #[allow(dead_code)]
    #[allow(dead_code)]
    fn all_tokens(input: &str) -> Vec<(SyntaxKind, &str)> {
        tokenize(input).iter().map(|t| (t.kind, t.text)).collect()
    }

    #[test]
    fn empty_input_produces_no_tokens() {
        assert!(tokenize("").is_empty());
    }
    #[test]
    fn lex_integer_literal() {
        assert_eq!(non_trivia_tokens("42"), vec![(SyntaxKind::IntLit, "42")]);
    }
    #[test]
    fn lex_multi_digit_integer() {
        assert_eq!(
            non_trivia_tokens("12345"),
            vec![(SyntaxKind::IntLit, "12345")]
        );
    }
    #[test]
    fn lex_identifier() {
        assert_eq!(
            non_trivia_tokens("hello"),
            vec![(SyntaxKind::Ident, "hello")]
        );
    }
    #[test]
    fn lex_identifier_with_underscore() {
        assert_eq!(
            non_trivia_tokens("my_var"),
            vec![(SyntaxKind::Ident, "my_var")]
        );
    }
    #[test]
    fn lex_identifier_with_digits() {
        assert_eq!(non_trivia_tokens("x1"), vec![(SyntaxKind::Ident, "x1")]);
    }
    #[test]
    fn lex_whitespace_as_trivia() {
        let tokens = all_tokens("  \t\n  ");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].0, SyntaxKind::Whitespace);
    }
    #[test]
    fn lex_line_comment_as_trivia() {
        let tokens = all_tokens("// hello\n42");
        assert_eq!(tokens[0].0, SyntaxKind::LineComment);
        assert_eq!(tokens[0].1, "// hello");
        assert_eq!(tokens[1].0, SyntaxKind::Whitespace);
        assert_eq!(tokens[2].0, SyntaxKind::IntLit);
    }
    #[test]
    fn lex_block_comment_as_trivia() {
        let tokens = all_tokens("/* inner */42");
        assert_eq!(tokens[0].0, SyntaxKind::BlockComment);
        assert_eq!(tokens[0].1, "/* inner */");
        assert_eq!(tokens[1].0, SyntaxKind::IntLit);
    }
    #[test]
    fn lex_nested_block_comments() {
        let tokens = all_tokens("/* a /* b */ c */42");
        assert_eq!(tokens[0].0, SyntaxKind::BlockComment);
        assert_eq!(tokens[0].1, "/* a /* b */ c */");
        assert_eq!(tokens[1].0, SyntaxKind::IntLit);
    }
    #[test]
    fn lex_unterminated_block_comment_is_error() {
        let tokens = all_tokens("/* never ends");
        assert_eq!(tokens[0].0, SyntaxKind::Error);
    }
    #[test]
    fn lex_single_char_punctuation() {
        let tokens = non_trivia_tokens("=(),;:@.");
        assert_eq!(
            tokens,
            vec![
                (SyntaxKind::Eq, "="),
                (SyntaxKind::LParen, "("),
                (SyntaxKind::RParen, ")"),
                (SyntaxKind::Comma, ","),
                (SyntaxKind::Semicolon, ";"),
                (SyntaxKind::Colon, ":"),
                (SyntaxKind::At, "@"),
                (SyntaxKind::Dot, "."),
            ]
        );
    }
    #[test]
    fn lex_braces() {
        assert_eq!(
            non_trivia_tokens("{}"),
            vec![(SyntaxKind::LBrace, "{"), (SyntaxKind::RBrace, "}")]
        );
    }
    #[test]
    fn lex_arithmetic_operators() {
        assert_eq!(
            non_trivia_tokens("+ - * / %"),
            vec![
                (SyntaxKind::Plus, "+"),
                (SyntaxKind::Minus, "-"),
                (SyntaxKind::Star, "*"),
                (SyntaxKind::Slash, "/"),
                (SyntaxKind::Percent, "%"),
            ]
        );
    }
    #[test]
    fn lex_two_char_operators() {
        assert_eq!(
            non_trivia_tokens("=> -> == != <= >= && ||"),
            vec![
                (SyntaxKind::FatArrow, "=>"),
                (SyntaxKind::Arrow, "->"),
                (SyntaxKind::EqEq, "=="),
                (SyntaxKind::BangEq, "!="),
                (SyntaxKind::LtEq, "<="),
                (SyntaxKind::GtEq, ">="),
                (SyntaxKind::AmpAmp, "&&"),
                (SyntaxKind::PipePipe, "||"),
            ]
        );
    }
    #[test]
    fn lex_single_equals_not_confused_with_double_equals() {
        assert_eq!(
            non_trivia_tokens("= ="),
            vec![(SyntaxKind::Eq, "="), (SyntaxKind::Eq, "=")]
        );
    }
    #[test]
    fn lex_at_for_macro_prefix() {
        assert_eq!(
            non_trivia_tokens("@serde"),
            vec![(SyntaxKind::At, "@"), (SyntaxKind::Ident, "serde")]
        );
    }
    #[test]
    fn lex_unknown_character_is_error() {
        let tokens = all_tokens("$");
        assert_eq!(tokens[0].0, SyntaxKind::Error);
    }
    #[test]
    fn lex_v010_hello_world() {
        assert_eq!(
            non_trivia_tokens("main = () => 42"),
            vec![
                (SyntaxKind::Ident, "main"),
                (SyntaxKind::Eq, "="),
                (SyntaxKind::LParen, "("),
                (SyntaxKind::RParen, ")"),
                (SyntaxKind::FatArrow, "=>"),
                (SyntaxKind::IntLit, "42"),
            ]
        );
    }
    #[test]
    fn lex_expression_with_precedence() {
        assert_eq!(
            non_trivia_tokens("1 + 2 * 3"),
            vec![
                (SyntaxKind::IntLit, "1"),
                (SyntaxKind::Plus, "+"),
                (SyntaxKind::IntLit, "2"),
                (SyntaxKind::Star, "*"),
                (SyntaxKind::IntLit, "3"),
            ]
        );
    }
    #[test]
    fn token_offsets_are_correct() {
        let tokens = tokenize("42");
        assert_eq!(tokens[0].start, 0);
        assert_eq!(tokens[0].end, 2);
    }
    #[test]
    fn token_offsets_with_leading_whitespace() {
        let tokens = tokenize("  42");
        assert_eq!(tokens[0].start, 0);
        assert_eq!(tokens[0].end, 2);
        assert_eq!(tokens[1].start, 2);
        assert_eq!(tokens[1].end, 4);
    }
    #[test]
    fn lex_pipe_vs_pipe_pipe() {
        assert_eq!(
            non_trivia_tokens("| || |"),
            vec![
                (SyntaxKind::Pipe, "|"),
                (SyntaxKind::PipePipe, "||"),
                (SyntaxKind::Pipe, "|"),
            ]
        );
    }
    #[test]
    fn lex_keyword_fn() {
        assert_eq!(non_trivia_tokens("fn"), vec![(SyntaxKind::KwFn, "fn")]);
    }
    #[test]
    fn lex_keyword_struct() {
        assert_eq!(
            non_trivia_tokens("struct"),
            vec![(SyntaxKind::KwStruct, "struct")]
        );
    }
    #[test]
    fn lex_keyword_let() {
        assert_eq!(non_trivia_tokens("let"), vec![(SyntaxKind::KwLet, "let")]);
    }
    #[test]
    fn lex_keyword_use() {
        assert_eq!(non_trivia_tokens("use"), vec![(SyntaxKind::KwUse, "use")]);
    }
    #[test]
    fn lex_keyword_if_else() {
        assert_eq!(
            non_trivia_tokens("if else"),
            vec![(SyntaxKind::KwIf, "if"), (SyntaxKind::KwElse, "else")]
        );
    }
    #[test]
    fn lex_keyword_return() {
        assert_eq!(
            non_trivia_tokens("return"),
            vec![(SyntaxKind::KwReturn, "return")]
        );
    }
    #[test]
    fn lex_keyword_enum() {
        assert_eq!(
            non_trivia_tokens("enum"),
            vec![(SyntaxKind::KwEnum, "enum")]
        );
    }
    #[test]
    fn ident_that_starts_with_keyword_is_still_ident() {
        assert_eq!(
            non_trivia_tokens("format"),
            vec![(SyntaxKind::Ident, "format")]
        );
    }
    #[test]
    fn keyword_followed_by_ident() {
        assert_eq!(
            non_trivia_tokens("fn myFunc"),
            vec![(SyntaxKind::KwFn, "fn"), (SyntaxKind::Ident, "myFunc")]
        );
    }

    // ── String tests ────────────────────────────────────
    #[test]
    fn lex_empty_string() {
        assert_eq!(
            non_trivia_tokens(r#""""#),
            vec![(SyntaxKind::StringLit, r#""""#)]
        );
    }
    #[test]
    fn lex_string_with_content() {
        assert_eq!(
            non_trivia_tokens(r#""hello""#),
            vec![(SyntaxKind::StringLit, r#""hello""#)]
        );
    }
    #[test]
    fn lex_string_with_spaces() {
        assert_eq!(
            non_trivia_tokens(r#""hello world""#),
            vec![(SyntaxKind::StringLit, r#""hello world""#)]
        );
    }
    #[test]
    fn lex_string_with_escaped_quotes() {
        assert_eq!(
            non_trivia_tokens(r#""say \"hi\"""#),
            vec![(SyntaxKind::StringLit, r#""say \"hi\"""#)]
        );
    }
    #[test]
    fn lex_string_with_escaped_backslash() {
        assert_eq!(
            non_trivia_tokens(r#""path\\to\\file""#),
            vec![(SyntaxKind::StringLit, r#""path\\to\\file""#)]
        );
    }
    #[test]
    fn lex_string_with_escaped_newline() {
        assert_eq!(
            non_trivia_tokens(r#""line1\nline2""#),
            vec![(SyntaxKind::StringLit, r#""line1\nline2""#)]
        );
    }
    #[test]
    fn lex_unterminated_string_is_error() {
        assert_eq!(all_tokens(r#""never ends"#)[0].0, SyntaxKind::Error);
    }
    #[test]
    fn lex_string_followed_by_identifier() {
        assert_eq!(
            non_trivia_tokens(r#""hello" world"#),
            vec![
                (SyntaxKind::StringLit, r#""hello""#),
                (SyntaxKind::Ident, "world"),
            ]
        );
    }
    #[test]
    fn lex_string_with_newline_inside_is_error() {
        assert_eq!(all_tokens("\"hello\nworld\"")[0].0, SyntaxKind::Error);
    }
    #[test]
    fn string_offsets_are_correct() {
        let tokens = tokenize(r#"  "hi"  "#);
        assert_eq!(tokens[1].start, 2);
        assert_eq!(tokens[1].end, 6);
    }

    #[test]
    fn lex_keyword_true() {
        assert_eq!(
            non_trivia_tokens("true"),
            vec![(SyntaxKind::KwTrue, "true")]
        );
    }
    #[test]
    fn lex_keyword_false() {
        assert_eq!(
            non_trivia_tokens("false"),
            vec![(SyntaxKind::KwFalse, "false")]
        );
    }
    #[test]
    fn lex_keyword_match() {
        assert_eq!(
            non_trivia_tokens("match"),
            vec![(SyntaxKind::KwMatch, "match")]
        );
    }
    #[test]
    fn lex_keyword_extern() {
        assert_eq!(
            non_trivia_tokens("extern"),
            vec![(SyntaxKind::KwExtern, "extern")]
        );
    }
    #[test]
    fn lex_keyword_as() {
        assert_eq!(non_trivia_tokens("as"), vec![(SyntaxKind::KwAs, "as")]);
    }
    #[test]
    fn lex_question_mark() {
        assert_eq!(non_trivia_tokens("?"), vec![(SyntaxKind::Question, "?")]);
    }
    #[test]
    fn ident_that_starts_with_new_keyword() {
        assert_eq!(
            non_trivia_tokens("matchbox"),
            vec![(SyntaxKind::Ident, "matchbox")]
        );
    }
}

#[test]
fn lex_hash_token() {
    let tokens = tokenize("#");
    let nt: Vec<_> = tokens.iter().filter(|t| !t.kind.is_trivia()).collect();
    assert_eq!(nt[0].kind, SyntaxKind::Hash);
}

#[test]
fn lex_empty_attribute_brackets() {
    let tokens = tokenize("#[]");
    let kinds: Vec<_> = tokens.iter().filter(|t| !t.kind.is_trivia()).map(|t| t.kind).collect();
    assert_eq!(kinds, vec![SyntaxKind::Hash, SyntaxKind::OpenBracket, SyntaxKind::CloseBracket]);
}

#[test]
fn lex_named_attribute() {
    let tokens = tokenize("#[test]");
    let nt: Vec<_> = tokens.iter().filter(|t| !t.kind.is_trivia()).collect();
    assert_eq!(nt[2].kind, SyntaxKind::Ident);
    assert_eq!(nt[2].text, "test");
}

#[test]
fn lex_attribute_with_paren_args() {
    let tokens = tokenize("#[test(should_panic)]");
    let nt: Vec<_> = tokens.iter().filter(|t| !t.kind.is_trivia()).collect();
    // Expected order: #[test(should_panic)]
    // Hash, OpenBracket, Ident("test"), LParen, Ident("should_panic"), RParen, CloseBracket
    assert_eq!(nt.len(), 7);
    assert_eq!(nt[0].kind, SyntaxKind::Hash);
    assert_eq!(nt[1].kind, SyntaxKind::OpenBracket);
    assert_eq!(nt[2].kind, SyntaxKind::Ident);
    assert_eq!(nt[2].text, "test");
    assert_eq!(nt[3].kind, SyntaxKind::LParen);
    assert_eq!(nt[4].kind, SyntaxKind::Ident);
    assert_eq!(nt[4].text, "should_panic");
    assert_eq!(nt[5].kind, SyntaxKind::RParen);
    assert_eq!(nt[6].kind, SyntaxKind::CloseBracket);
}

#[test]
fn lex_attribute_before_fn_keyword() {
    let tokens = tokenize("#[test]\nfn main() { 42 }");
    let nt: Vec<_> = tokens.iter().filter(|t| !t.kind.is_trivia()).collect();
    assert_eq!(nt[0].kind, SyntaxKind::Hash);
    assert_eq!(nt[2].kind, SyntaxKind::Ident);
    assert_eq!(nt[4].kind, SyntaxKind::KwFn);
}
