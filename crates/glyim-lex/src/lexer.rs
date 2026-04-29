//! Hand-rolled character-by-character lexer for .xyz source files.
use glyim_syntax::SyntaxKind;
use crate::Token;

pub struct Lexer<'a> {
    input: &'a str,
    offset: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self { Self { input, offset: 0 } }
    fn remaining(&self) -> &'a str { &self.input[self.offset..] }
    fn peek(&self) -> Option<char> { self.remaining().chars().next() }
    fn peek2(&self) -> Option<char> { self.remaining().chars().nth(1) }
    fn advance(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.offset += c.len_utf8();
        Some(c)
    }
    fn eat_while<F: FnMut(char) -> bool>(&mut self, mut pred: F) -> &'a str {
        let start = self.offset;
        while let Some(c) = self.peek() {
            if pred(c) { self.offset += c.len_utf8(); } else { break; }
        }
        &self.input[start..self.offset]
    }

    pub fn next_token(&mut self) -> Option<Token<'a>> {
        if self.offset >= self.input.len() { return None; }
        let start = self.offset;
        let kind = self.lex_one();
        let text = &self.input[start..self.offset];
        let end = self.offset;
        Some(Token { kind, text, start, end })
    }

    fn lex_one(&mut self) -> SyntaxKind {
        let c = match self.peek() { Some(c) => c, None => return SyntaxKind::Eof };
        if c.is_whitespace() { self.eat_while(|ch| ch.is_whitespace()); return SyntaxKind::Whitespace; }
        if c == '/' {
            match self.peek2() {
                Some('/') => return self.lex_line_comment(),
                Some('*') => return self.lex_block_comment(),
                _ => { self.advance(); return SyntaxKind::Slash; }
            }
        }
        if c.is_ascii_digit() { return self.lex_number(); }
        if c == '"' { return self.lex_string(); }
        if c.is_alphabetic() || c == '_' { return self.lex_ident_or_keyword(); }

        let two = match (c, self.peek2()) {
            ('=', Some('>')) => Some((SyntaxKind::FatArrow, 2)),
            ('-', Some('>')) => Some((SyntaxKind::Arrow, 2)),
            ('=', Some('=')) => Some((SyntaxKind::EqEq, 2)),
            ('!', Some('=')) => Some((SyntaxKind::BangEq, 2)),
            ('<', Some('=')) => Some((SyntaxKind::LtEq, 2)),
            ('>', Some('=')) => Some((SyntaxKind::GtEq, 2)),
            ('&', Some('&')) => Some((SyntaxKind::AmpAmp, 2)),
            ('|', Some('|')) => Some((SyntaxKind::PipePipe, 2)),
            _ => None,
        };
        if let Some((kind, n)) = two { for _ in 0..n { let _ = self.advance(); } return kind; }

        let single = match c {
            '=' => SyntaxKind::Eq, '(' => SyntaxKind::LParen, ')' => SyntaxKind::RParen,
            '{' => SyntaxKind::LBrace, '}' => SyntaxKind::RBrace,
            ',' => SyntaxKind::Comma, ':' => SyntaxKind::Colon, ';' => SyntaxKind::Semicolon,
            '@' => SyntaxKind::At, '.' => SyntaxKind::Dot,
            '+' => SyntaxKind::Plus, '-' => SyntaxKind::Minus, '*' => SyntaxKind::Star,
            '/' => SyntaxKind::Slash, '%' => SyntaxKind::Percent,
            '<' => SyntaxKind::Lt, '>' => SyntaxKind::Gt,
            '!' => SyntaxKind::Bang, '|' => SyntaxKind::Pipe,
            '?' => SyntaxKind::Question,
            _ => SyntaxKind::Error,
        };
        self.advance();
        single
    }

    fn lex_number(&mut self) -> SyntaxKind {
        self.eat_while(|c| c.is_ascii_digit());
        SyntaxKind::IntLit
    }

    fn lex_string(&mut self) -> SyntaxKind {
        self.advance(); // consume opening "
        loop {
            match self.peek() {
                None => return SyntaxKind::Error,
                Some('"') => { self.advance(); return SyntaxKind::StringLit; }
                Some('\\') => {
                    self.advance(); // consume backslash
                    match self.peek() {
                        Some('"') | Some('\\') | Some('n') | Some('t') | Some('r') | Some('0') => {
                            self.advance(); // consume escaped char
                        }
                        _ => {
                            if self.peek().is_some() { self.advance(); }
                        }
                    }
                }
                Some('\n') => return SyntaxKind::Error,
                Some(_) => { self.advance(); }
            }
        }
    }

    fn lex_ident_or_keyword(&mut self) -> SyntaxKind {
        let start = self.offset;
        self.eat_while(|c| c.is_alphabetic() || c == '_' || c.is_ascii_digit());
        let text = &self.input[start..self.offset];
        match text {
            "fn" => SyntaxKind::KwFn, "struct" => SyntaxKind::KwStruct,
            "enum" => SyntaxKind::KwEnum, "let" => SyntaxKind::KwLet,
            "if" => SyntaxKind::KwIf, "else" => SyntaxKind::KwElse,
            "return" => SyntaxKind::KwReturn, "use" => SyntaxKind::KwUse,
            "true" => SyntaxKind::KwTrue, "false" => SyntaxKind::KwFalse,
            "match" => SyntaxKind::KwMatch, "extern" => SyntaxKind::KwExtern,
            "as" => SyntaxKind::KwAs,
            _ => SyntaxKind::Ident,
        }
    }

    fn lex_line_comment(&mut self) -> SyntaxKind {
        self.advance(); self.advance();
        self.eat_while(|c| c != '\n');
        SyntaxKind::LineComment
    }

    fn lex_block_comment(&mut self) -> SyntaxKind {
        self.advance(); self.advance();
        let mut depth: u32 = 1;
        while depth > 0 {
            match (self.peek(), self.peek2()) {
                (Some('*'), Some('/')) => { self.advance(); self.advance(); depth -= 1; }
                (Some('/'), Some('*')) => { self.advance(); self.advance(); depth += 1; }
                (Some(_), _) => { self.advance(); }
                (None, _) => return SyntaxKind::Error,
            }
        }
        SyntaxKind::BlockComment
    }
}

pub fn tokenize(input: &str) -> Vec<Token<'_>> {
    let mut lexer = Lexer::new(input);
    let mut tokens = Vec::new();
    while let Some(tok) = lexer.next_token() { tokens.push(tok); }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_syntax::SyntaxKind;

    fn non_trivia_tokens(input: &str) -> Vec<(SyntaxKind, &str)> {
        tokenize(input).iter().filter(|t| !t.kind.is_trivia()).map(|t| (t.kind, t.text)).collect()
    }
    fn all_tokens(input: &str) -> Vec<(SyntaxKind, &str)> {
        tokenize(input).iter().map(|t| (t.kind, t.text)).collect()
    }

    #[test] fn empty_input_produces_no_tokens() { assert!(tokenize("").is_empty()); }
    #[test] fn lex_integer_literal() { assert_eq!(non_trivia_tokens("42"), vec![(SyntaxKind::IntLit, "42")]); }
    #[test] fn lex_multi_digit_integer() { assert_eq!(non_trivia_tokens("12345"), vec![(SyntaxKind::IntLit, "12345")]); }
    #[test] fn lex_identifier() { assert_eq!(non_trivia_tokens("hello"), vec![(SyntaxKind::Ident, "hello")]); }
    #[test] fn lex_identifier_with_underscore() { assert_eq!(non_trivia_tokens("my_var"), vec![(SyntaxKind::Ident, "my_var")]); }
    #[test] fn lex_identifier_with_digits() { assert_eq!(non_trivia_tokens("x1"), vec![(SyntaxKind::Ident, "x1")]); }
    #[test] fn lex_whitespace_as_trivia() {
        let tokens = all_tokens("  \t\n  ");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].0, SyntaxKind::Whitespace);
    }
    #[test] fn lex_line_comment_as_trivia() {
        let tokens = all_tokens("// hello\n42");
        assert_eq!(tokens[0].0, SyntaxKind::LineComment);
        assert_eq!(tokens[0].1, "// hello");
        assert_eq!(tokens[1].0, SyntaxKind::Whitespace);
        assert_eq!(tokens[2].0, SyntaxKind::IntLit);
    }
    #[test] fn lex_block_comment_as_trivia() {
        let tokens = all_tokens("/* inner */42");
        assert_eq!(tokens[0].0, SyntaxKind::BlockComment);
        assert_eq!(tokens[0].1, "/* inner */");
        assert_eq!(tokens[1].0, SyntaxKind::IntLit);
    }
    #[test] fn lex_nested_block_comments() {
        let tokens = all_tokens("/* a /* b */ c */42");
        assert_eq!(tokens[0].0, SyntaxKind::BlockComment);
        assert_eq!(tokens[0].1, "/* a /* b */ c */");
        assert_eq!(tokens[1].0, SyntaxKind::IntLit);
    }
    #[test] fn lex_unterminated_block_comment_is_error() {
        let tokens = all_tokens("/* never ends");
        assert_eq!(tokens[0].0, SyntaxKind::Error);
    }
    #[test] fn lex_single_char_punctuation() {
        let tokens = non_trivia_tokens("=(),;:@.");
        assert_eq!(tokens, vec![
            (SyntaxKind::Eq, "="), (SyntaxKind::LParen, "("), (SyntaxKind::RParen, ")"),
            (SyntaxKind::Comma, ","), (SyntaxKind::Semicolon, ";"), (SyntaxKind::Colon, ":"),
            (SyntaxKind::At, "@"), (SyntaxKind::Dot, "."),
        ]);
    }
    #[test] fn lex_braces() {
        assert_eq!(non_trivia_tokens("{}"), vec![(SyntaxKind::LBrace, "{"), (SyntaxKind::RBrace, "}")]);
    }
    #[test] fn lex_arithmetic_operators() {
        assert_eq!(non_trivia_tokens("+ - * / %"), vec![
            (SyntaxKind::Plus, "+"), (SyntaxKind::Minus, "-"), (SyntaxKind::Star, "*"),
            (SyntaxKind::Slash, "/"), (SyntaxKind::Percent, "%"),
        ]);
    }
    #[test] fn lex_two_char_operators() {
        assert_eq!(non_trivia_tokens("=> -> == != <= >= && ||"), vec![
            (SyntaxKind::FatArrow, "=>"), (SyntaxKind::Arrow, "->"), (SyntaxKind::EqEq, "=="),
            (SyntaxKind::BangEq, "!="), (SyntaxKind::LtEq, "<="), (SyntaxKind::GtEq, ">="),
            (SyntaxKind::AmpAmp, "&&"), (SyntaxKind::PipePipe, "||"),
        ]);
    }
    #[test] fn lex_single_equals_not_confused_with_double_equals() {
        assert_eq!(non_trivia_tokens("= ="), vec![(SyntaxKind::Eq, "="), (SyntaxKind::Eq, "=")]);
    }
    #[test] fn lex_at_for_macro_prefix() {
        assert_eq!(non_trivia_tokens("@serde"), vec![(SyntaxKind::At, "@"), (SyntaxKind::Ident, "serde")]);
    }
    #[test] fn lex_unknown_character_is_error() {
        let tokens = all_tokens("$");
        assert_eq!(tokens[0].0, SyntaxKind::Error);
    }
    #[test] fn lex_v010_hello_world() {
        assert_eq!(non_trivia_tokens("main = () => 42"), vec![
            (SyntaxKind::Ident, "main"), (SyntaxKind::Eq, "="), (SyntaxKind::LParen, "("),
            (SyntaxKind::RParen, ")"), (SyntaxKind::FatArrow, "=>"), (SyntaxKind::IntLit, "42"),
        ]);
    }
    #[test] fn lex_expression_with_precedence() {
        assert_eq!(non_trivia_tokens("1 + 2 * 3"), vec![
            (SyntaxKind::IntLit, "1"), (SyntaxKind::Plus, "+"),
            (SyntaxKind::IntLit, "2"), (SyntaxKind::Star, "*"), (SyntaxKind::IntLit, "3"),
        ]);
    }
    #[test] fn token_offsets_are_correct() {
        let tokens = tokenize("42");
        assert_eq!(tokens[0].start, 0);
        assert_eq!(tokens[0].end, 2);
    }
    #[test] fn token_offsets_with_leading_whitespace() {
        let tokens = tokenize("  42");
        assert_eq!(tokens[0].start, 0); assert_eq!(tokens[0].end, 2);
        assert_eq!(tokens[1].start, 2); assert_eq!(tokens[1].end, 4);
    }
    #[test] fn lex_pipe_vs_pipe_pipe() {
        assert_eq!(non_trivia_tokens("| || |"), vec![
            (SyntaxKind::Pipe, "|"), (SyntaxKind::PipePipe, "||"), (SyntaxKind::Pipe, "|"),
        ]);
    }
    #[test] fn lex_keyword_fn() { assert_eq!(non_trivia_tokens("fn"), vec![(SyntaxKind::KwFn, "fn")]); }
    #[test] fn lex_keyword_struct() { assert_eq!(non_trivia_tokens("struct"), vec![(SyntaxKind::KwStruct, "struct")]); }
    #[test] fn lex_keyword_let() { assert_eq!(non_trivia_tokens("let"), vec![(SyntaxKind::KwLet, "let")]); }
    #[test] fn lex_keyword_use() { assert_eq!(non_trivia_tokens("use"), vec![(SyntaxKind::KwUse, "use")]); }
    #[test] fn lex_keyword_if_else() {
        assert_eq!(non_trivia_tokens("if else"), vec![(SyntaxKind::KwIf, "if"), (SyntaxKind::KwElse, "else")]);
    }
    #[test] fn lex_keyword_return() { assert_eq!(non_trivia_tokens("return"), vec![(SyntaxKind::KwReturn, "return")]); }
    #[test] fn lex_keyword_enum() { assert_eq!(non_trivia_tokens("enum"), vec![(SyntaxKind::KwEnum, "enum")]); }
    #[test] fn ident_that_starts_with_keyword_is_still_ident() {
        assert_eq!(non_trivia_tokens("format"), vec![(SyntaxKind::Ident, "format")]);
    }
    #[test] fn keyword_followed_by_ident() {
        assert_eq!(non_trivia_tokens("fn myFunc"), vec![(SyntaxKind::KwFn, "fn"), (SyntaxKind::Ident, "myFunc")]);
    }

    // ── String tests ────────────────────────────────────
    #[test] fn lex_empty_string() {
        assert_eq!(non_trivia_tokens(r#""""#), vec![(SyntaxKind::StringLit, r#""""#)]);
    }
    #[test] fn lex_string_with_content() {
        assert_eq!(non_trivia_tokens(r#""hello""#), vec![(SyntaxKind::StringLit, r#""hello""#)]);
    }
    #[test] fn lex_string_with_spaces() {
        assert_eq!(non_trivia_tokens(r#""hello world""#), vec![(SyntaxKind::StringLit, r#""hello world""#)]);
    }
    #[test] fn lex_string_with_escaped_quotes() {
        assert_eq!(non_trivia_tokens(r#""say \"hi\"""#), vec![(SyntaxKind::StringLit, r#""say \"hi\"""#)]);
    }
    #[test] fn lex_string_with_escaped_backslash() {
        assert_eq!(non_trivia_tokens(r#""path\\to\\file""#), vec![(SyntaxKind::StringLit, r#""path\\to\\file""#)]);
    }
    #[test] fn lex_string_with_escaped_newline() {
        assert_eq!(non_trivia_tokens(r#""line1\nline2""#), vec![(SyntaxKind::StringLit, r#""line1\nline2""#)]);
    }
    #[test] fn lex_unterminated_string_is_error() {
        assert_eq!(all_tokens(r#""never ends"#)[0].0, SyntaxKind::Error);
    }
    #[test] fn lex_string_followed_by_identifier() {
        assert_eq!(non_trivia_tokens(r#""hello" world"#), vec![
            (SyntaxKind::StringLit, r#""hello""#), (SyntaxKind::Ident, "world"),
        ]);
    }
    #[test] fn lex_string_with_newline_inside_is_error() {
        assert_eq!(all_tokens("\"hello\nworld\"")[0].0, SyntaxKind::Error);
    }
    #[test] fn string_offsets_are_correct() {
        let tokens = tokenize(r#"  "hi"  "#);
        assert_eq!(tokens[1].start, 2);
        assert_eq!(tokens[1].end, 6);
    }

#[test] fn lex_true_keyword() {
    let tokens = tokenize("true");
    let nt: Vec<_> = tokens.iter().filter(|t| !t.kind.is_trivia()).collect();
    assert_eq!(nt[0].kind, SyntaxKind::KwTrue);
    assert_eq!(nt[0].text, "true");
}
#[test] fn lex_false_keyword() {
    let tokens = tokenize("false");
    let nt: Vec<_> = tokens.iter().filter(|t| !t.kind.is_trivia()).collect();
    assert_eq!(nt[0].kind, SyntaxKind::KwFalse);
}
#[test] fn lex_match_keyword() {
    let tokens = tokenize("match");
    let nt: Vec<_> = tokens.iter().filter(|t| !t.kind.is_trivia()).collect();
    assert_eq!(nt[0].kind, SyntaxKind::KwMatch);
}
#[test] fn lex_extern_keyword() {
    let tokens = tokenize("extern");
    let nt: Vec<_> = tokens.iter().filter(|t| !t.kind.is_trivia()).collect();
    assert_eq!(nt[0].kind, SyntaxKind::KwExtern);
}
#[test] fn lex_as_keyword() {
    let tokens = tokenize("as");
    let nt: Vec<_> = tokens.iter().filter(|t| !t.kind.is_trivia()).collect();
    assert_eq!(nt[0].kind, SyntaxKind::KwAs);
}
#[test] fn lex_question_mark() {
    let tokens = tokenize("?");
    let nt: Vec<_> = tokens.iter().filter(|t| !t.kind.is_trivia()).collect();
    assert_eq!(nt[0].kind, SyntaxKind::Question);
    assert_eq!(nt[0].text, "?");
}
#[test] fn trueish_is_identifier_not_keyword() {
    let tokens = tokenize("trueish");
    let nt: Vec<_> = tokens.iter().filter(|t| !t.kind.is_trivia()).collect();
    assert_eq!(nt[0].kind, SyntaxKind::Ident);
}
#[test] fn existing_tokens_still_work_after_keyword_additions() {
    let tokens = tokenize("1 + 2 == 3");
    let kinds: Vec<_> = tokens.iter().filter(|t| !t.kind.is_trivia()).map(|t| t.kind).collect();
    assert_eq!(kinds, vec![
        SyntaxKind::IntLit, SyntaxKind::Plus,
        SyntaxKind::IntLit, SyntaxKind::EqEq,
        SyntaxKind::IntLit,
    ]);
}

}
