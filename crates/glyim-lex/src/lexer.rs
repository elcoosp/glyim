//! Hand-rolled character-by-character lexer for .g source files.
use crate::Token;
use glyim_syntax::SyntaxKind;

pub struct Lexer<'a> {
    input: &'a str,
    offset: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self { input, offset: 0 }
    }
    fn remaining(&self) -> &'a str {
        &self.input[self.offset..]
    }
    fn peek(&self) -> Option<char> {
        self.remaining().chars().next()
    }
    fn peek2(&self) -> Option<char> {
        self.remaining().chars().nth(1)
    }
    fn advance(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.offset += c.len_utf8();
        Some(c)
    }
    fn eat_while<F: FnMut(char) -> bool>(&mut self, mut pred: F) -> &'a str {
        let start = self.offset;
        while let Some(c) = self.peek() {
            if pred(c) {
                self.offset += c.len_utf8();
            } else {
                break;
            }
        }
        &self.input[start..self.offset]
    }

    pub fn next_token(&mut self) -> Option<Token<'a>> {
        if self.offset >= self.input.len() {
            return None;
        }
        let start = self.offset;
        let kind = self.lex_one();
        let text = &self.input[start..self.offset];
        let end = self.offset;
        Some(Token {
            kind,
            text,
            start,
            end,
        })
    }

    fn lex_one(&mut self) -> SyntaxKind {
        let c = match self.peek() {
            Some(c) => c,
            None => return SyntaxKind::Eof,
        };
        if c.is_whitespace() {
            self.eat_while(|ch| ch.is_whitespace());
            return SyntaxKind::Whitespace;
        }
        if c == '/' {
            match self.peek2() {
                Some('/') => return self.lex_line_comment(),
                Some('*') => return self.lex_block_comment(),
                _ => {
                    self.advance();
                    return SyntaxKind::Slash;
                }
            }
        }
        if c.is_ascii_digit() {
            return self.lex_number_or_float();
        }
        if c == '"' {
            return self.lex_string();
        }
        if c.is_alphabetic() || c == '_' {
            return self.lex_ident_or_keyword();
        }

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
        if let Some((kind, n)) = two {
            for _ in 0..n {
                let _ = self.advance();
            }
            return kind;
        }

        let single = match c {
            '=' => SyntaxKind::Eq,
            '(' => SyntaxKind::LParen,
            ')' => SyntaxKind::RParen,
            '{' => SyntaxKind::LBrace,
            '}' => SyntaxKind::RBrace,
            ',' => SyntaxKind::Comma,
            ':' => SyntaxKind::Colon,
            ';' => SyntaxKind::Semicolon,
            '@' => SyntaxKind::At,
            '.' => SyntaxKind::Dot,
            '+' => SyntaxKind::Plus,
            '-' => SyntaxKind::Minus,
            '*' => SyntaxKind::Star,
            '/' => SyntaxKind::Slash,
            '%' => SyntaxKind::Percent,
            '<' => SyntaxKind::Lt,
            '>' => SyntaxKind::Gt,
            '!' => SyntaxKind::Bang,
            '|' => SyntaxKind::Pipe,
            '?' => SyntaxKind::Question,
            '#' => SyntaxKind::Hash,
            '[' => SyntaxKind::OpenBracket,
            ']' => SyntaxKind::CloseBracket,
            _ => SyntaxKind::Error,
        };
        self.advance();
        single
    }

    fn lex_number_or_float(&mut self) -> SyntaxKind {
        let _start = self.offset;
        self.eat_while(|c| c.is_ascii_digit());
        if self.peek() == Some('.') && self.peek2().is_some_and(|c| c.is_ascii_digit()) {
            self.advance();
            self.eat_while(|c| c.is_ascii_digit());
            return SyntaxKind::FloatLit;
        }
        SyntaxKind::IntLit
    }

    fn lex_string(&mut self) -> SyntaxKind {
        self.advance();
        loop {
            match self.peek() {
                None => return SyntaxKind::Error,
                Some('"') => {
                    self.advance();
                    return SyntaxKind::StringLit;
                }
                Some('\\') => {
                    self.advance();
                    match self.peek() {
                        Some('"') | Some('\\') | Some('n') | Some('t') | Some('r') | Some('0') => {
                            self.advance();
                        }
                        _ => {
                            if self.peek().is_some() {
                                self.advance();
                            }
                        }
                    }
                }
                Some('\n') => return SyntaxKind::Error,
                Some(_) => {
                    self.advance();
                }
            }
        }
    }

    fn lex_ident_or_keyword(&mut self) -> SyntaxKind {
        let start = self.offset;
        self.eat_while(|c| c.is_alphabetic() || c == '_' || c.is_ascii_digit());
        let text = &self.input[start..self.offset];
        match text {
            "fn" => SyntaxKind::KwFn,
            "self" => SyntaxKind::KwSelf,
            "struct" => SyntaxKind::KwStruct,
            "enum" => SyntaxKind::KwEnum,
            "let" => SyntaxKind::KwLet,
            "in" => SyntaxKind::KwIn,
            "if" => SyntaxKind::KwIf,
            "else" => SyntaxKind::KwElse,
            "return" => SyntaxKind::KwReturn,
            "use" => SyntaxKind::KwUse,
            "true" => SyntaxKind::KwTrue,
            "false" => SyntaxKind::KwFalse,
            "match" => SyntaxKind::KwMatch,
            "extern" => SyntaxKind::KwExtern,
            "as" => SyntaxKind::KwAs,
            "pub" => SyntaxKind::KwPub,
            "mut" => SyntaxKind::KwMut,
            "for" => SyntaxKind::KwFor,
            "while" => SyntaxKind::KwWhile,
            "impl" => SyntaxKind::KwImpl,
            _ => SyntaxKind::Ident,
        }
    }

    fn lex_line_comment(&mut self) -> SyntaxKind {
        self.advance();
        self.advance();
        self.eat_while(|c| c != '\n');
        SyntaxKind::LineComment
    }

    fn lex_block_comment(&mut self) -> SyntaxKind {
        self.advance();
        self.advance();
        let mut depth: u32 = 1;
        while depth > 0 {
            match (self.peek(), self.peek2()) {
                (Some('*'), Some('/')) => {
                    self.advance();
                    self.advance();
                    depth -= 1;
                }
                (Some('/'), Some('*')) => {
                    self.advance();
                    self.advance();
                    depth += 1;
                }
                (Some(_), _) => {
                    self.advance();
                }
                (None, _) => return SyntaxKind::Error,
            }
        }
        SyntaxKind::BlockComment
    }
}

pub fn tokenize(input: &str) -> Vec<Token<'_>> {
    let mut lexer = Lexer::new(input);
    let mut tokens = Vec::new();
    while let Some(tok) = lexer.next_token() {
        tokens.push(tok);
    }
    tokens
}
