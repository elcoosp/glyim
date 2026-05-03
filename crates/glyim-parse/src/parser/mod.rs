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
    pub(crate) known_structs: std::collections::HashSet<glyim_interner::Symbol>,
}

impl<'a> Parser<'a> {
    pub fn new(tokens: &'a [glyim_lex::Token<'a>]) -> Self {
        Self {
            tokens: tokens::Tokens::new(tokens),
            errors: vec![],
            interner: Interner::new(),
            known_structs: std::collections::HashSet::new(),
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

    
    /// Two‑pass parser: first scans for type declarations, then parses
    /// full bodies. This resolves forward references without changing the API.
    pub fn parse_source_file_two_pass(&mut self) -> Ast {
        // Pass 1: collect struct and enum names
        // Save current position by cloning the tokens? No — we'll reset later.
        let mut type_names = std::collections::HashSet::new();
        while !self.tokens.is_eof() {
            match self.tokens.peek().map(|t| t.kind) {
                Some(glyim_syntax::SyntaxKind::KwStruct) | Some(glyim_syntax::SyntaxKind::KwEnum) => {
                    if let Some(kind) = self.tokens.peek().map(|t| t.kind) {
                        if kind == glyim_syntax::SyntaxKind::KwStruct {
                            self.tokens.bump(); // skip 'struct'
                        } else {
                            self.tokens.bump(); // skip 'enum'
                        }
                        if let Some(name_tok) = self.tokens.bump() {
                            if name_tok.kind == glyim_syntax::SyntaxKind::Ident {
                                let sym = self.interner.intern(name_tok.text);
                                type_names.insert(sym);
                            }
                        }
                        // Skip the rest of the declaration (fields/variants)
                        let mut depth = 0u32;
                        while let Some(tok) = self.tokens.peek() {
                            match tok.kind {
                                glyim_syntax::SyntaxKind::LBrace => { self.tokens.bump(); depth += 1; }
                                glyim_syntax::SyntaxKind::RBrace => {
                                    self.tokens.bump();
                                    if depth == 0 { break; }
                                    depth -= 1;
                                }
                                _ => { self.tokens.bump(); }
                            }
                        }
                    }
                }
                _ => { self.tokens.bump(); }
            }
        }

        // Reset token position and build struct_names set
        self.tokens.reset();
        for sym in type_names {
            // Store in parser's own state - we'll check it during item parsing
            // For now, add to a new field: known_structs
            self.known_structs.insert(sym);
        }

        // Pass 2: full parse
        self.parse_source_file()
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
    #[tracing::instrument(skip_all)]
    
    /// Parse only declarations, skipping function bodies.
    pub fn parse_source_file_declarations_only(&mut self) -> Ast {
        let mut items = vec![];
        while !self.tokens.is_eof() {
            if let Some(item) = items::parse_item_declaration(self) {
                items.push(item);
            } else {
                self.tokens.bump();
            }
        }
        Ast { items }
    }

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
#[tracing::instrument(skip_all)]

pub fn parse(source: &str) -> ParseOutput {
    let tokens = tokenize(source);
    let mut parser = Parser::new(&tokens);
    let ast = parser.parse_source_file_two_pass();
    ParseOutput {
        ast,
        errors: parser.errors,
        interner: parser.interner,
    }
}

#[derive(Debug)]
pub struct ParseOutput<T = Ast> {
    pub ast: T,
    pub errors: Vec<ParseError>,
    pub interner: Interner,
}
