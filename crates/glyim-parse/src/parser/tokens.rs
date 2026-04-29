use glyim_lex::Token;
use glyim_syntax::SyntaxKind;

pub(crate) struct Tokens<'a> {
    tokens: &'a [Token<'a>],
    pos: usize,
}

impl<'a> Tokens<'a> {
    pub fn new(tokens: &'a [Token<'a>]) -> Self {
        Self { tokens, pos: 0 }
    }

    pub fn peek(&self) -> Option<&Token<'a>> {
        let mut p = self.pos;
        while p < self.tokens.len() && self.tokens[p].kind.is_trivia() {
            p += 1;
        }
        self.tokens.get(p)
    }

    pub fn peek2(&self) -> Option<&Token<'a>> {
        let mut p = self.pos;
        while p < self.tokens.len() && self.tokens[p].kind.is_trivia() {
            p += 1;
        }
        p += 1;
        while p < self.tokens.len() && self.tokens[p].kind.is_trivia() {
            p += 1;
        }
        self.tokens.get(p)
    }

    pub fn at(&self, kind: SyntaxKind) -> bool {
        self.peek().is_some_and(|t| t.kind == kind)
    }

    pub fn bump(&mut self) -> Option<Token<'a>> {
        self.skip_trivia();
        if self.pos < self.tokens.len() {
            let t = self.tokens[self.pos];
            self.pos += 1;
            Some(t)
        } else {
            None
        }
    }

    pub fn eat(&mut self, kind: SyntaxKind) -> Option<Token<'a>> {
        if self.at(kind) {
            self.bump()
        } else {
            None
        }
    }


    pub fn expect(
        &mut self,
        kind: SyntaxKind,
        errors: &mut Vec<crate::ParseError>,
    ) -> Result<Token<'a>, ()> {
        self.skip_trivia();
        match self.tokens.get(self.pos) {
            Some(t) if t.kind == kind => {
                let tok = *t;
                self.pos += 1;
                Ok(tok)
            }
            Some(t) => {
                errors.push(crate::ParseError::expected(kind, t.kind, t.start, t.end));
                Err(())
            }
            None => {
                errors.push(crate::ParseError::unexpected_eof(kind));
                Err(())
            }
        }
    }

    pub fn is_eof(&self) -> bool {
        self.peek().is_none()
    }

    pub fn skip_trivia(&mut self) {
        while self.pos < self.tokens.len() && self.tokens[self.pos].kind.is_trivia() {
            self.pos += 1;
        }
    }

    pub fn is_lambda_start(&self) -> bool {
        let mut p = self.pos;
        while p < self.tokens.len() && self.tokens[p].kind.is_trivia() {
            p += 1;
        }
        if self
            .tokens
            .get(p)
            .map_or(true, |t| t.kind != SyntaxKind::LParen)
        {
            return false;
        }
        p += 1;
        while p < self.tokens.len() && self.tokens[p].kind.is_trivia() {
            p += 1;
        }
        if self
            .tokens
            .get(p)
            .is_some_and(|t| t.kind == SyntaxKind::RParen)
        {
            p += 1;
            while p < self.tokens.len() && self.tokens[p].kind.is_trivia() {
                p += 1;
            }
            return self
                .tokens
                .get(p)
                .is_some_and(|t| t.kind == SyntaxKind::FatArrow);
        }
        if !self
            .tokens
            .get(p)
            .is_some_and(|t| t.kind == SyntaxKind::Ident)
        {
            return false;
        }
        p += 1;
        loop {
            while p < self.tokens.len() && self.tokens[p].kind.is_trivia() {
                p += 1;
            }
            match self.tokens.get(p) {
                Some(tok) if tok.kind == SyntaxKind::Comma => {
                    p += 1;
                    while p < self.tokens.len() && self.tokens[p].kind.is_trivia() {
                        p += 1;
                    }
                    if !self
                        .tokens
                        .get(p)
                        .is_some_and(|t| t.kind == SyntaxKind::Ident)
                    {
                        return false;
                    }
                    p += 1;
                }
                Some(tok) if tok.kind == SyntaxKind::RParen => {
                    p += 1;
                    while p < self.tokens.len() && self.tokens[p].kind.is_trivia() {
                        p += 1;
                    }
                    return self
                        .tokens
                        .get(p)
                        .is_some_and(|t| t.kind == SyntaxKind::FatArrow);
                }
                _ => return false,
            }
        }
    }
}
