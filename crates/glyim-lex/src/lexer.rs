use glyim_syntax::SyntaxKind;
use crate::Token;

pub struct Lexer<'a> { input: &'a str, offset: usize }
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
        while let Some(c) = self.peek() { if pred(c) { self.offset += c.len_utf8() } else { break } }
        &self.input[start..self.offset]
    }
    pub fn next_token(&mut self) -> Option<Token<'a>> {
        if self.offset >= self.input.len() { return None }
        let start = self.offset;
        let kind = self.lex_one();
        let text = &self.input[start..self.offset];
        Some(Token { kind, text, start, end: self.offset })
    }
    fn lex_one(&mut self) -> SyntaxKind {
        let c = match self.peek() { Some(c) => c, None => return SyntaxKind::Eof };
        if c.is_whitespace() { self.eat_while(|ch| ch.is_whitespace()); return SyntaxKind::Whitespace }
        if c == '/' {
            match self.peek2() {
                Some('/') => return self.lex_line_comment(),
                Some('*') => return self.lex_block_comment(),
                _ => { self.advance(); return SyntaxKind::Slash }
            }
        }
        if c.is_ascii_digit() { return self.lex_number() }
        if c.is_alphabetic() || c == '_' { return self.lex_ident_or_keyword() }
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
        if let Some((kind, n)) = two { for _ in 0..n { let _ = self.advance(); } return kind }
        let single = match c {
            '=' => SyntaxKind::Eq, '(' => SyntaxKind::LParen, ')' => SyntaxKind::RParen,
            '{' => SyntaxKind::LBrace, '}' => SyntaxKind::RBrace,
            ',' => SyntaxKind::Comma, ':' => SyntaxKind::Colon, ';' => SyntaxKind::Semicolon,
            '@' => SyntaxKind::At, '.' => SyntaxKind::Dot,
            '+' => SyntaxKind::Plus, '-' => SyntaxKind::Minus, '*' => SyntaxKind::Star,
            '/' => SyntaxKind::Slash, '%' => SyntaxKind::Percent,
            '<' => SyntaxKind::Lt, '>' => SyntaxKind::Gt,
            '!' => SyntaxKind::Bang, '|' => SyntaxKind::Pipe,
            _ => SyntaxKind::Error,
        };
        self.advance();
        single
    }
    fn lex_number(&mut self) -> SyntaxKind { self.eat_while(|c| c.is_ascii_digit()); SyntaxKind::IntLit }
    fn lex_ident_or_keyword(&mut self) -> SyntaxKind {
        let start = self.offset;
        self.eat_while(|c| c.is_alphabetic() || c == '_' || c.is_ascii_digit());
        match &self.input[start..self.offset] {
            "fn" => SyntaxKind::KwFn, "struct" => SyntaxKind::KwStruct,
            "enum" => SyntaxKind::KwEnum, "let" => SyntaxKind::KwLet,
            "if" => SyntaxKind::KwIf, "else" => SyntaxKind::KwElse,
            "return" => SyntaxKind::KwReturn, "use" => SyntaxKind::KwUse,
            _ => SyntaxKind::Ident,
        }
    }
    fn lex_line_comment(&mut self) -> SyntaxKind { self.advance(); self.advance(); self.eat_while(|c| c != '\n'); SyntaxKind::LineComment }
    fn lex_block_comment(&mut self) -> SyntaxKind {
        self.advance(); self.advance();
        let mut depth = 1u32;
        while depth > 0 {
            match (self.peek(), self.peek2()) {
                (Some('*'), Some('/')) => { self.advance(); self.advance(); depth -= 1 }
                (Some('/'), Some('*')) => { self.advance(); self.advance(); depth += 1 }
                (Some(_), _) => { let _ = self.advance(); }
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
