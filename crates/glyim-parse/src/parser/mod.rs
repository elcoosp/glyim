pub(crate) mod exprs;
mod items;
mod patterns;
mod precedence;
mod recovery;
mod stmts;
mod tokens;
pub(crate) mod types;

use crate::ast::Ast;
use crate::error::ParseError;
use glyim_interner::Interner;

pub struct Parser<'a> {
    pub(crate) tokens: tokens::Tokens<'a>,
    pub errors: Vec<ParseError>,
    pub interner: Interner,
}

impl<'a> Parser<'a> {
    pub fn new(tokens: &'a [glyim_lex::Token<'a>]) -> Self {
        Self {
            tokens: tokens::Tokens::new(tokens),
            errors: vec![],
            interner: Interner::new(),
        }
    }

    pub fn parse_source_file(&mut self) -> Ast {
        let mut items = vec![];
        while !self.tokens.is_eof() {
            if let Some(item) = items::parse_item(self) {
                items.push(item);
            } else {
                self.tokens.bump();
            }
        }
        Ast { items }
    }

    #[allow(dead_code)]
    pub fn parse_source_file_recovery(&mut self) -> Ast {
        let mut items = vec![];
        while !self.tokens.is_eof() {
            if let Some(item) = items::parse_item(self) {
                items.push(item);
            } else {
                self.errors.push(ParseError::Message {
                    msg: "failed to parse item".into(),
                    span: self.current_span(),
                });
                recovery::recover(&mut self.tokens);
            }
        }
        Ast { items }
    }

    #[allow(dead_code)]
    pub fn current_span(&self) -> (usize, usize) {
        match self.tokens.peek() {
            Some(tok) => (tok.start, tok.end),
            None => (0, 0),
        }
    }
}

use glyim_lex::tokenize;

pub fn parse(source: &str) -> ParseOutput {
    let tokens = tokenize(source);
    let mut parser = Parser::new(&tokens);
    let ast = parser.parse_source_file();
    ParseOutput {
        ast,
        errors: parser.errors,
        interner: parser.interner,
    }
}

#[derive(Debug)]
pub struct ParseOutput {
    pub ast: Ast,
    pub errors: Vec<ParseError>,
    pub interner: Interner,
}
