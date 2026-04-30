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
#[tracing::instrument(skip_all)]

    fn parse_attributes(&mut self) -> Vec<crate::ast::Attribute> {
        let mut attrs = vec![];
        while self.tokens.at(glyim_syntax::SyntaxKind::Hash) {
            let hash_tok = match self.tokens.bump() {
                Some(t) => t,
                None => break,
            };
            let start = hash_tok.start;
            if self
                .tokens
                .expect(glyim_syntax::SyntaxKind::OpenBracket, &mut self.errors)
                .is_err()
            {
                break;
            }
            let name_tok = match self
                .tokens
                .expect(glyim_syntax::SyntaxKind::Ident, &mut self.errors)
            {
                Ok(t) => t,
                Err(_) => {
                    break;
                }
            };
            let name = name_tok.text.to_string();

            let mut args = vec![];
            if self.tokens.eat(glyim_syntax::SyntaxKind::LParen).is_some() {
                loop {
                    if self.tokens.at(glyim_syntax::SyntaxKind::RParen) {
                        self.tokens.bump();
                        break;
                    }
                    if self.tokens.is_eof() {
                        break;
                    }
                    let key_tok = match self
                        .tokens
                        .expect(glyim_syntax::SyntaxKind::Ident, &mut self.errors)
                    {
                        Ok(t) => t,
                        Err(_) => {
                            self.tokens.eat(glyim_syntax::SyntaxKind::RParen);
                            break;
                        }
                    };
                    let key = key_tok.text.to_string();
                    let value = if self.tokens.eat(glyim_syntax::SyntaxKind::Eq).is_some() {
                        match self.tokens.peek() {
                            Some(val_tok) => {
                                let val_str = val_tok.text.to_string();
                                self.tokens.bump();
                                Some(val_str)
                            }
                            None => None,
                        }
                    } else {
                        None
                    };
                    args.push(crate::ast::AttributeArg {
                        key,
                        value,
                        span: glyim_diag::Span::new(key_tok.start, key_tok.end),
                    });
                    self.tokens.eat(glyim_syntax::SyntaxKind::Comma);
                }
            }

            self.tokens
                .expect(glyim_syntax::SyntaxKind::CloseBracket, &mut self.errors)
                .ok();
            let end = self.tokens.peek().map_or(start, |t| t.start);
            attrs.push(crate::ast::Attribute {
                name,
                args,
                span: glyim_diag::Span::new(start, end),
            });
        }
        attrs
    }
#[tracing::instrument(skip_all)]

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
#[tracing::instrument(skip_all)]
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
