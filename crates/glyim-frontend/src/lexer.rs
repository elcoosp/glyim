use glyim_diag::GlyimDiagnostic;
use glyim_span::{ByteIdx, FileId, Span, SyntaxContext};
use glyim_syntax::SyntaxKind;
use smol_str::SmolStr;

#[derive(Clone, Debug)]
/// Token.
pub struct Token {
    /// Struct.
    pub kind: SyntaxKind,
    /// Struct.
    pub span: Span,
    /// Struct.
    pub text: SmolStr,
}

impl Token {
    /// new.
    pub fn new(kind: SyntaxKind, span: Span, text: impl AsRef<str>) -> Self {
        Self {
            kind,
            span,
            text: SmolStr::from(text.as_ref()),
        }
    }
}

#[derive(Clone, Debug)]
/// LexResult.
pub struct LexResult {
    /// Struct.
    pub tokens: Vec<Token>,
    /// Struct.
    pub diagnostics: Vec<GlyimDiagnostic>,
}

/// Lexer.
pub struct Lexer<'a> {
    source: &'a str,
    file_id: FileId,
    pos: usize,
    diagnostics: Vec<GlyimDiagnostic>,
}

impl<'a> Lexer<'a> {
    /// new.
    pub fn new(source: &'a str, file_id: FileId) -> Self {
        Self {
            source,
            file_id,
            pos: 0,
            diagnostics: Vec::new(),
        }
    }

    fn span(&self, start: usize, end: usize) -> Span {
        Span::new(
            self.file_id,
            ByteIdx::from_raw(start as u32),
            ByteIdx::from_raw(end as u32),
            SyntaxContext::ROOT,
        )
    }

    fn peek(&self) -> Option<char> {
        self.source[self.pos..].chars().next()
    }
    fn peek_next(&self) -> Option<char> {
        let mut chars = self.source[self.pos..].chars();
        chars.next();
        chars.next()
    }
    fn advance(&mut self) -> Option<char> {
        let ch = self.source[self.pos..].chars().next();
        if let Some(c) = ch {
            self.pos += c.len_utf8();
        }
        ch
    }

    /// lex.
    /// Lex to a trivia-free token stream (whitespace and comments omitted).
    /// This is the historical `Lexer::lex` behavior; it is preserved for
    /// callers (tests, fragment validation) that want only meaningful tokens.
    /// The parser uses `Lexer::lex_all` so tree byte offsets match the source.
    pub fn lex(self) -> LexResult {
        let mut r = self.lex_all();
        r.tokens.retain(|t| !t.kind.is_trivia());
        r
    }

    /// Lex to a token stream that **includes** trivia tokens (whitespace,
    /// line/block/doc comments). The parser calls this so the syntax tree it
    /// builds has byte offsets that line up with the source: `GreenNode`
    /// stores only concatenated token text, so `text_range()` is measured in
    /// the byte space of that concatenation. Omitting trivia makes tree
    /// offsets diverge from source offsets (a 115 KB source produced a 56 KB
    /// tree), which is why every diagnostic span downstream pointed at the
    /// wrong source text. Existing callers that want the historical
    /// trivia-free stream use `Lexer::lex`.
    pub fn lex_all(mut self) -> LexResult {
        let mut tokens = Vec::new();

        while self.pos < self.source.len() {
            let start = self.pos;
            if self.lex_trivia() {
                let text = &self.source[start..self.pos];
                // Classify the consumed run: block comment, line comment, or
                // (bare) whitespace. Consecutive whitespace including newlines
                // is emitted as one Whitespace token.
                let kind = if text.starts_with("/*") {
                    SyntaxKind::BlockComment
                } else if text.starts_with("//") {
                    SyntaxKind::LineComment
                } else {
                    SyntaxKind::Whitespace
                };
                tokens.push(Token::new(kind, self.span(start, self.pos), text));
                continue;
            }
            if self.pos >= self.source.len() {
                break;
            }
            let Some(ch) = self.peek() else { break };

            match ch {
                'a'..='z' | 'A'..='Z' | '_' => {
                    // Byte literals: `b'x'` or `b"..."` (and uppercase `B`).
                    let nb = self.peek_next();
                    if (self.source[start..self.pos + 1].chars().count() == 1)
                        && (self.source[start..].starts_with('b')
                            || self.source[start..].starts_with('B'))
                        && (nb == Some('\'') || nb == Some('"'))
                    {
                        self.lex_byte_lit();
                        let text = &self.source[start..self.pos];
                        tokens.push(Token::new(
                            SyntaxKind::ByteLit,
                            self.span(start, self.pos),
                            text,
                        ));
                    } else {
                        self.lex_ident_or_keyword();
                        let text = &self.source[start..self.pos];
                        let kind = lookup_keyword(text);
                        tokens.push(Token::new(kind, self.span(start, self.pos), text));
                    }
                }
                '0'..='9' => {
                    let kind = self.lex_number();
                    let text = &self.source[start..self.pos];
                    tokens.push(Token::new(kind, self.span(start, self.pos), text));
                }
                '"' => {
                    self.lex_string();
                    let text = &self.source[start..self.pos];
                    tokens.push(Token::new(
                        SyntaxKind::StringLit,
                        self.span(start, self.pos),
                        text,
                    ));
                }
                '\'' => {
                    // Lifetime detection: 'identifier unless followed by closing quote
                    let after_quote = self.pos + 1;
                    let src = self.source;
                    let mut ident_end = after_quote;
                    #[allow(unused_assignments)]
                    let mut is_lifetime = false;

                    let mut chars = src[after_quote..].chars();
                    if let Some(first) = chars.next() {
                        if first.is_alphabetic() || first == '_' {
                            ident_end += first.len_utf8();
                            for c in chars {
                                if c.is_alphanumeric() || c == '_' {
                                    ident_end += c.len_utf8();
                                } else {
                                    break;
                                }
                            }
                            let next_char = src[ident_end..].chars().next();
                            if next_char == Some('\'') {
                                is_lifetime = false; // char literal
                            } else {
                                is_lifetime = true; // lifetime
                            }
                        } else {
                            is_lifetime = false;
                        }
                    } else {
                        is_lifetime = false;
                    }
                    if is_lifetime {
                        self.advance(); // skip '
                        while self.pos < ident_end {
                            self.advance();
                        }
                        let text = &self.source[start..self.pos];
                        tokens.push(Token::new(
                            SyntaxKind::Lifetime,
                            self.span(start, self.pos),
                            text,
                        ));
                    } else {
                        self.lex_char();
                        let text = &self.source[start..self.pos];
                        tokens.push(Token::new(
                            SyntaxKind::CharLit,
                            self.span(start, self.pos),
                            text,
                        ));
                    }
                }
                '(' => {
                    self.advance();
                    tokens.push(Token::new(
                        SyntaxKind::LParen,
                        self.span(start, self.pos),
                        "(",
                    ));
                }
                ')' => {
                    self.advance();
                    tokens.push(Token::new(
                        SyntaxKind::RParen,
                        self.span(start, self.pos),
                        ")",
                    ));
                }
                '{' => {
                    self.advance();
                    tokens.push(Token::new(
                        SyntaxKind::LBrace,
                        self.span(start, self.pos),
                        "{",
                    ));
                }
                '}' => {
                    self.advance();
                    tokens.push(Token::new(
                        SyntaxKind::RBrace,
                        self.span(start, self.pos),
                        "}",
                    ));
                }
                '[' => {
                    self.advance();
                    tokens.push(Token::new(
                        SyntaxKind::LBracket,
                        self.span(start, self.pos),
                        "[",
                    ));
                }
                ']' => {
                    self.advance();
                    tokens.push(Token::new(
                        SyntaxKind::RBracket,
                        self.span(start, self.pos),
                        "]",
                    ));
                }
                ',' => {
                    self.advance();
                    tokens.push(Token::new(
                        SyntaxKind::Comma,
                        self.span(start, self.pos),
                        ",",
                    ));
                }
                ';' => {
                    self.advance();
                    tokens.push(Token::new(
                        SyntaxKind::Semicolon,
                        self.span(start, self.pos),
                        ";",
                    ));
                }
                '.' => {
                    let kind = self.lex_dot();
                    let text = &self.source[start..self.pos];
                    tokens.push(Token::new(kind, self.span(start, self.pos), text));
                }
                ':' => {
                    let kind = self.lex_colon();
                    let text = &self.source[start..self.pos];
                    tokens.push(Token::new(kind, self.span(start, self.pos), text));
                }
                '+' => {
                    let k = self.lex_plus();
                    tokens.push(Token::new(
                        k,
                        self.span(start, self.pos),
                        &self.source[start..self.pos],
                    ));
                }
                '-' => {
                    let k = self.lex_minus();
                    tokens.push(Token::new(
                        k,
                        self.span(start, self.pos),
                        &self.source[start..self.pos],
                    ));
                }
                '*' => {
                    let k = self.lex_star();
                    tokens.push(Token::new(
                        k,
                        self.span(start, self.pos),
                        &self.source[start..self.pos],
                    ));
                }
                '/' => {
                    let k = self.lex_slash();
                    tokens.push(Token::new(
                        k,
                        self.span(start, self.pos),
                        &self.source[start..self.pos],
                    ));
                }
                '%' => {
                    self.advance();
                    tokens.push(Token::new(
                        SyntaxKind::Percent,
                        self.span(start, self.pos),
                        "%",
                    ));
                }
                '=' => {
                    let k = self.lex_eq();
                    tokens.push(Token::new(
                        k,
                        self.span(start, self.pos),
                        &self.source[start..self.pos],
                    ));
                }
                '!' => {
                    let k = self.lex_bang();
                    tokens.push(Token::new(
                        k,
                        self.span(start, self.pos),
                        &self.source[start..self.pos],
                    ));
                }
                '<' => {
                    let k = self.lex_lt();
                    tokens.push(Token::new(
                        k,
                        self.span(start, self.pos),
                        &self.source[start..self.pos],
                    ));
                }
                '>' => {
                    let k = self.lex_gt();
                    tokens.push(Token::new(
                        k,
                        self.span(start, self.pos),
                        &self.source[start..self.pos],
                    ));
                }
                '&' => {
                    let k = self.lex_and();
                    tokens.push(Token::new(
                        k,
                        self.span(start, self.pos),
                        &self.source[start..self.pos],
                    ));
                }
                '|' => {
                    let k = self.lex_or();
                    tokens.push(Token::new(
                        k,
                        self.span(start, self.pos),
                        &self.source[start..self.pos],
                    ));
                }
                '^' => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        tokens.push(Token::new(
                            SyntaxKind::CaretEq,
                            self.span(start, self.pos),
                            "^=",
                        ));
                    } else {
                        tokens.push(Token::new(
                            SyntaxKind::Caret,
                            self.span(start, self.pos),
                            "^",
                        ));
                    }
                }
                '@' => {
                    self.advance();
                    tokens.push(Token::new(SyntaxKind::At, self.span(start, self.pos), "@"));
                }
                '#' => {
                    self.advance();
                    tokens.push(Token::new(
                        SyntaxKind::Hash,
                        self.span(start, self.pos),
                        "#",
                    ));
                }
                '$' => {
                    self.advance();
                    tokens.push(Token::new(
                        SyntaxKind::Dollar,
                        self.span(start, self.pos),
                        "$",
                    ));
                }
                '~' => {
                    self.advance();
                    tokens.push(Token::new(
                        SyntaxKind::Tilde,
                        self.span(start, self.pos),
                        "~",
                    ));
                }
                '?' => {
                    self.advance();
                    tokens.push(Token::new(
                        SyntaxKind::Question,
                        self.span(start, self.pos),
                        "?",
                    ));
                }
                _ => {
                    self.advance();
                    self.diagnostics.push(GlyimDiagnostic::lex_error(
                        self.span(start, self.pos),
                        format!("unexpected character: '{}'", ch),
                    ));
                    tokens.push(Token::new(
                        SyntaxKind::Error,
                        self.span(start, self.pos),
                        &self.source[start..self.pos],
                    ));
                }
            }
        }

        LexResult {
            tokens,
            diagnostics: self.diagnostics,
        }
    }

    fn lex_trivia(&mut self) -> bool {
        let start = self.pos;
        while let Some(ch) = self.peek() {
            match ch {
                ' ' | '\t' | '\n' | '\r' => {
                    self.advance();
                }
                '/' if self.peek_next() == Some('/') => {
                    self.advance();
                    self.advance();
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.advance();
                    }
                }
                '/' if self.peek_next() == Some('*') => {
                    self.advance();
                    self.advance();
                    let mut depth = 1u32;
                    while depth > 0 {
                        match self.peek() {
                            Some('/') if self.peek_next() == Some('*') => {
                                self.advance();
                                self.advance();
                                depth += 1;
                            }
                            Some('*') if self.peek_next() == Some('/') => {
                                self.advance();
                                self.advance();
                                depth -= 1;
                            }
                            Some(_) => {
                                self.advance();
                            }
                            None => break,
                        }
                    }
                }
                _ => break,
            }
        }
        self.pos > start
    }

    fn lex_ident_or_keyword(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_alphanumeric() || ch == '_' {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn lex_number(&mut self) -> SyntaxKind {
        // Radix prefixes (no validation, just consume)
        if self.peek() == Some('0') {
            let next = self.peek_next();
            if next == Some('x') || next == Some('X') {
                self.advance();
                self.advance();
                while let Some(ch) = self.peek() {
                    if ch.is_ascii_hexdigit() || ch == '_' {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.lex_number_suffix();
                return SyntaxKind::IntLit;
            } else if next == Some('b') || next == Some('B') {
                self.advance();
                self.advance();
                while let Some(ch) = self.peek() {
                    if ch == '0' || ch == '1' || ch == '_' {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.lex_number_suffix();
                return SyntaxKind::IntLit;
            } else if next == Some('o') || next == Some('O') {
                self.advance();
                self.advance();
                while let Some(ch) = self.peek() {
                    if ('0'..='7').contains(&ch) || ch == '_' {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.lex_number_suffix();
                return SyntaxKind::IntLit;
            }
        }

        let mut is_float = false;

        // Decimal integer part
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() || ch == '_' {
                self.advance();
            } else {
                break;
            }
        }

        // Fractional part
        if self.peek() == Some('.') && self.peek_next().is_some_and(|n| n.is_ascii_digit()) {
            is_float = true;
            self.advance();
            while let Some(ch) = self.peek() {
                if ch.is_ascii_digit() || ch == '_' {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        // Exponent part with validation
        if self.peek() == Some('e') || self.peek() == Some('E') {
            let exp_start = self.pos;
            self.advance();
            if self.peek() == Some('+') || self.peek() == Some('-') {
                self.advance();
            }
            let mut has_exponent_digits = false;
            while let Some(ch) = self.peek() {
                if ch.is_ascii_digit() || ch == '_' {
                    self.advance();
                    has_exponent_digits = true;
                } else {
                    break;
                }
            }
            if !has_exponent_digits {
                let exp_end = self.pos;
                let exp_text = &self.source[exp_start..exp_end];
                self.diagnostics.push(GlyimDiagnostic::lex_error(
                    self.span(exp_start, exp_end),
                    format!("incomplete float exponent: '{}'", exp_text),
                ));
                self.pos = exp_start;
                return if is_float {
                    SyntaxKind::FloatLit
                } else {
                    SyntaxKind::IntLit
                };
            } else {
                is_float = true;
            }
        }

        self.lex_number_suffix();

        if is_float {
            SyntaxKind::FloatLit
        } else {
            SyntaxKind::IntLit
        }
    }

    fn lex_number_suffix(&mut self) {
        let suffix_start = self.pos;
        while let Some(ch) = self.peek() {
            if ch.is_alphanumeric() || ch == '_' {
                self.advance();
            } else {
                break;
            }
        }
        if self.pos > suffix_start {
            let suffix = &self.source[suffix_start..self.pos];
            if !is_valid_number_suffix(suffix) {
                self.diagnostics.push(GlyimDiagnostic::lex_error(
                    Span::new(
                        self.file_id,
                        ByteIdx::from_raw(suffix_start as u32),
                        ByteIdx::from_raw(self.pos as u32),
                        SyntaxContext::ROOT,
                    ),
                    format!("invalid number suffix: '{}'", suffix),
                ));
            }
        }
    }

    fn lex_string(&mut self) {
        let start = self.pos;
        self.advance(); // consume opening "
        let mut terminated = false;
        while let Some(ch) = self.peek() {
            match ch {
                '"' => {
                    self.advance();
                    terminated = true;
                    break;
                }
                '\\' => {
                    self.advance();
                    self.advance();
                }
                _ => {
                    self.advance();
                }
            }
        }
        if !terminated {
            let end = self.pos;
            self.diagnostics.push(GlyimDiagnostic::lex_error(
                self.span(start, end),
                "unterminated string literal".to_string(),
            ));
        }
    }

    fn lex_char(&mut self) {
        let start = self.pos;
        self.advance(); // consume opening '
        let mut terminated = false;
        while let Some(ch) = self.peek() {
            match ch {
                '\'' => {
                    self.advance(); // consume closing '
                    terminated = true;
                    break;
                }
                '\\' => {
                    self.advance(); // skip backslash
                    self.advance(); // skip escaped char
                }
                _ => {
                    self.advance();
                }
            }
        }
        if !terminated {
            let end = self.pos;
            self.diagnostics.push(GlyimDiagnostic::lex_error(
                self.span(start, end),
                "unterminated char literal".to_string(),
            ));
        }
    }

    fn lex_byte_lit(&mut self) {
        // Current token start is the `b`/`B`; the char after it is the opening
        // quote (`'` or `"`). Consume the `b`, the opening quote, then the body.
        self.advance(); // consume `b`/`B`
        let open = match self.peek() {
            Some(c @ ('\'' | '"')) => c,
            _ => return,
        };
        self.advance(); // consume opening quote
        let mut terminated = false;
        while let Some(ch) = self.peek() {
            if ch == open {
                self.advance(); // consume closing quote
                terminated = true;
                break;
            }
            if ch == '\\' {
                self.advance(); // skip backslash
                self.advance(); // skip escaped char
            } else {
                self.advance();
            }
        }
        if !terminated {
            let end = self.pos;
            self.diagnostics.push(GlyimDiagnostic::lex_error(
                self.span(end, end),
                "unterminated byte literal".to_string(),
            ));
        }
    }

    fn lex_dot(&mut self) -> SyntaxKind {
        self.advance();
        if self.peek() == Some('.') {
            self.advance();
            if self.peek() == Some('=') {
                self.advance();
                SyntaxKind::DotDotEq
            } else {
                SyntaxKind::DotDot
            }
        } else {
            SyntaxKind::Dot
        }
    }

    fn lex_colon(&mut self) -> SyntaxKind {
        self.advance();
        if self.peek() == Some(':') {
            self.advance();
            SyntaxKind::ColonColon
        } else {
            SyntaxKind::Colon
        }
    }

    fn lex_plus(&mut self) -> SyntaxKind {
        self.advance();
        if self.peek() == Some('=') {
            self.advance();
            SyntaxKind::PlusEq
        } else {
            SyntaxKind::Plus
        }
    }

    fn lex_minus(&mut self) -> SyntaxKind {
        self.advance();
        if self.peek() == Some('>') {
            self.advance();
            SyntaxKind::Arrow
        } else if self.peek() == Some('=') {
            self.advance();
            SyntaxKind::MinusEq
        } else {
            SyntaxKind::Minus
        }
    }

    fn lex_star(&mut self) -> SyntaxKind {
        self.advance();
        if self.peek() == Some('=') {
            self.advance();
            SyntaxKind::StarEq
        } else {
            SyntaxKind::Star
        }
    }

    fn lex_slash(&mut self) -> SyntaxKind {
        self.advance();
        if self.peek() == Some('=') {
            self.advance();
            SyntaxKind::SlashEq
        } else {
            SyntaxKind::Slash
        }
    }

    fn lex_eq(&mut self) -> SyntaxKind {
        self.advance();
        if self.peek() == Some('=') {
            self.advance();
            SyntaxKind::EqEq
        } else if self.peek() == Some('>') {
            self.advance();
            SyntaxKind::FatArrow
        } else {
            SyntaxKind::Eq
        }
    }

    fn lex_bang(&mut self) -> SyntaxKind {
        self.advance();
        if self.peek() == Some('=') {
            self.advance();
            SyntaxKind::BangEq
        } else {
            SyntaxKind::Bang
        }
    }

    fn lex_lt(&mut self) -> SyntaxKind {
        self.advance();
        if self.peek() == Some('=') {
            self.advance();
            SyntaxKind::LtEq
        } else if self.peek() == Some('<') {
            self.advance();
            if self.peek() == Some('=') {
                self.advance();
                SyntaxKind::ShlEq
            } else {
                SyntaxKind::Shl
            }
        } else {
            SyntaxKind::Lt
        }
    }

    fn lex_gt(&mut self) -> SyntaxKind {
        self.advance();
        if self.peek() == Some('=') {
            self.advance();
            SyntaxKind::GtEq
        } else if self.peek() == Some('>') {
            self.advance();
            if self.peek() == Some('=') {
                self.advance();
                SyntaxKind::ShrEq
            } else {
                SyntaxKind::Shr
            }
        } else {
            SyntaxKind::Gt
        }
    }

    fn lex_and(&mut self) -> SyntaxKind {
        self.advance();
        if self.peek() == Some('&') {
            self.advance();
            SyntaxKind::AndAnd
        } else if self.peek() == Some('=') {
            self.advance();
            SyntaxKind::AndEq
        } else {
            SyntaxKind::And
        }
    }

    fn lex_or(&mut self) -> SyntaxKind {
        self.advance();
        if self.peek() == Some('|') {
            self.advance();
            SyntaxKind::OrOr
        } else if self.peek() == Some('=') {
            self.advance();
            SyntaxKind::OrEq
        } else {
            SyntaxKind::Or
        }
    }
}

fn is_valid_number_suffix(suffix: &str) -> bool {
    matches!(
        suffix,
        "i8" | "i16"
            | "i32"
            | "i64"
            | "isize"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "usize"
            | "f32"
            | "f64"
    )
}

fn lookup_keyword(ident: &str) -> SyntaxKind {
    match ident {
        "fn" => SyntaxKind::KwFn,
        "let" => SyntaxKind::KwLet,
        "struct" => SyntaxKind::KwStruct,
        "enum" => SyntaxKind::KwEnum,
        "if" => SyntaxKind::KwIf,
        "else" => SyntaxKind::KwElse,
        "return" => SyntaxKind::KwReturn,
        "match" => SyntaxKind::KwMatch,
        "mod" => SyntaxKind::KwMod,
        "comptime" => SyntaxKind::KwComptime,
        "self" => SyntaxKind::KwSelf,
        "super" => SyntaxKind::KwSuper,
        "crate" => SyntaxKind::KwCrate,
        "true" => SyntaxKind::KwTrue,
        "false" => SyntaxKind::KwFalse,
        "mut" => SyntaxKind::KwMut,
        "move" => SyntaxKind::KwMove,
        "ref" => SyntaxKind::KwRef,
        "as" => SyntaxKind::KwAs,
        "while" => SyntaxKind::KwWhile,
        "for" => SyntaxKind::KwFor,
        "in" => SyntaxKind::KwIn,
        "break" => SyntaxKind::KwBreak,
        "continue" => SyntaxKind::KwContinue,
        "loop" => SyntaxKind::KwLoop,
        "trait" => SyntaxKind::KwTrait,
        "impl" => SyntaxKind::KwImpl,
        "where" => SyntaxKind::KwWhere,
        "dyn" => SyntaxKind::KwDyn,
        "type" => SyntaxKind::KwType,
        "use" => SyntaxKind::KwUse,
        "pub" => SyntaxKind::KwPub,
        "priv" => SyntaxKind::KwPriv,
        "extern" => SyntaxKind::KwExtern,
        "unsafe" => SyntaxKind::KwUnsafe,
        "const" => SyntaxKind::KwConst,
        "static" => SyntaxKind::KwStatic,
        "async" => SyntaxKind::KwAsync,
        "await" => SyntaxKind::KwAwait,
        "macro" => SyntaxKind::KwMacro,
        "macro_rules" => SyntaxKind::KwMacroRules,
        "_" => SyntaxKind::Underscore,
        _ => SyntaxKind::Ident,
    }
}

/// lex.
pub fn lex(source: &str, file_id: FileId) -> LexResult {
    Lexer::new(source, file_id).lex()
}

/// Lex a source string to a token stream that **includes** trivia tokens
/// (`Whitespace`, `LineComment`, `BlockComment`). Used by the parser so the
/// syntax tree's byte offsets match the source text.
pub fn lex_with_trivia(source: &str, file_id: FileId) -> LexResult {
    Lexer::new(source, file_id).lex_all()
}
