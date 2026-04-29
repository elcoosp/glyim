use glyim_diag::Span;
use glyim_interner::Interner;
use glyim_lex::Token;
use glyim_syntax::SyntaxKind;

use crate::ast::{
    Ast, BinOp, BlockItem, EnumVariantRepr as EnumVariant, ExprKind, ExprNode, ExternFn, Item,
    StmtKind, StmtNode, UnOp, UseItem, VariantKind,
};
use crate::error::ParseError;

struct Tokens<'a> {
    tokens: &'a [Token<'a>],
    pos: usize,
}

impl<'a> Tokens<'a> {
    fn new(tokens: &'a [Token<'a>]) -> Self {
        Self { tokens, pos: 0 }
    }
    fn peek(&self) -> Option<&Token<'a>> {
        let mut p = self.pos;
        while p < self.tokens.len() && self.tokens[p].kind.is_trivia() {
            p += 1;
        }
        self.tokens.get(p)
    }
    fn peek2(&self) -> Option<&Token<'a>> {
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
    fn at(&self, kind: SyntaxKind) -> bool {
        self.peek().is_some_and(|t| t.kind == kind)
    }
    fn bump(&mut self) -> Option<Token<'a>> {
        self.skip_trivia();
        if self.pos < self.tokens.len() {
            let t = self.tokens[self.pos];
            self.pos += 1;
            Some(t)
        } else {
            None
        }
    }
    fn eat(&mut self, kind: SyntaxKind) -> Option<Token<'a>> {
        if self.at(kind) {
            self.bump()
        } else {
            None
        }
    }
    fn eat_ident(&mut self, text: &str) -> Option<Token<'a>> {
        if self.at(SyntaxKind::Ident) && self.peek().is_some_and(|t| t.text == text) {
            self.bump()
        } else {
            None
        }
    }
    fn expect(&mut self, kind: SyntaxKind) -> Result<Token<'a>, ParseError> {
        self.skip_trivia();
        match self.tokens.get(self.pos) {
            Some(t) if t.kind == kind => {
                let tok = *t;
                self.pos += 1;
                Ok(tok)
            }
            Some(t) => Err(ParseError::expected(kind, t.kind, t.start, t.end)),
            None => Err(ParseError::unexpected_eof(kind)),
        }
    }
    fn is_eof(&self) -> bool {
        self.peek().is_none()
    }
    fn skip_trivia(&mut self) {
        while self.pos < self.tokens.len() && self.tokens[self.pos].kind.is_trivia() {
            self.pos += 1;
        }
    }
    fn is_lambda_start(&self) -> bool {
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
                Some(t) if t.kind == SyntaxKind::Comma => {
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
                Some(t) if t.kind == SyntaxKind::RParen => {
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

pub struct Parser<'a> {
    tokens: Tokens<'a>,
    pub errors: Vec<ParseError>,
    pub interner: Interner,
}

impl<'a> Parser<'a> {
    pub fn new(tokens: &'a [Token<'a>]) -> Self {
        Self {
            tokens: Tokens::new(tokens),
            errors: vec![],
            interner: Interner::new(),
        }
    }

    pub fn parse_source_file(&mut self) -> Ast {
        let mut items = vec![];
        while !self.tokens.is_eof() {
            if let Some(item) = self.parse_item() {
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
            if let Some(item) = self.parse_item() {
                items.push(item);
            } else {
                self.errors.push(ParseError::Message {
                    msg: "failed to parse item".into(),
                    span: self.current_span(),
                });
                self.recover();
            }
        }
        Ast { items }
    }

    fn parse_item(&mut self) -> Option<Item> {
        match self.tokens.peek()?.kind {
            SyntaxKind::At => {
                self.tokens.bump();
                let name_tok = self.tokens.expect(SyntaxKind::Ident).ok()?;
                let name = self.interner.intern(name_tok.text);
                self.tokens.expect(SyntaxKind::KwFn).ok()?;
                let fn_name_tok = self.tokens.expect(SyntaxKind::Ident).ok()?;
                let fn_name = self.interner.intern(fn_name_tok.text);
                let fn_name_span = Span::new(fn_name_tok.start, fn_name_tok.end);
                self.tokens.expect(SyntaxKind::LParen).ok()?;
                let mut params = vec![];
                while !self.tokens.at(SyntaxKind::RParen) {
                    let tok = self.tokens.expect(SyntaxKind::Ident).ok()?;
                    self.tokens.eat(SyntaxKind::Colon);
                    self.tokens.expect(SyntaxKind::Ident).ok()?;
                    params.push((self.interner.intern(tok.text), Span::new(tok.start, tok.end)));
                    self.tokens.eat(SyntaxKind::Comma);
                }
                self.tokens.expect(SyntaxKind::RParen).ok()?;
                if self.tokens.eat(SyntaxKind::Arrow).is_some() {
                    let mut depth = 0u32;
                    while self.tokens.peek().is_some() {
                        let kind = self.tokens.peek().unwrap().kind;
                        if kind == SyntaxKind::LBrace && depth == 0 { break; }
                        if kind == SyntaxKind::Lt || kind == SyntaxKind::LParen { depth += 1; }
                        if kind == SyntaxKind::Gt || kind == SyntaxKind::RParen { if depth > 0 { depth -= 1; } }
                        self.tokens.bump();
                    }
                }
                let body = self.parse_block_expr()?;
                Some(Item::MacroDef { name: fn_name, name_span: fn_name_span, params, body })
            }
            SyntaxKind::KwFn => self.parse_fn_def(),
            SyntaxKind::KwStruct => self.parse_struct_def(),
            SyntaxKind::KwEnum => self.parse_enum_def(),
            SyntaxKind::KwExtern => self.parse_extern_block(),
            SyntaxKind::KwLet => self.parse_let_stmt().map(Item::Stmt),
            SyntaxKind::KwUse => self.parse_use_item().map(Item::Use),
            SyntaxKind::Ident => {
                if self
                    .tokens
                    .peek2()
                    .is_some_and(|t| t.kind == SyntaxKind::Eq)
                {
                    self.parse_binding()
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn parse_binding(&mut self) -> Option<Item> {
        let name_tok = self.tokens.bump()?;
        let name = self.interner.intern(name_tok.text);
        let name_span = Span::new(name_tok.start, name_tok.end);
        if let Err(e) = self.tokens.expect(SyntaxKind::Eq) {
            self.errors.push(e);
            return None;
        }
        let value = self.parse_expr(0)?;
        Some(Item::Binding {
            name,
            name_span,
            value,
        })
    }

    fn parse_fn_def(&mut self) -> Option<Item> {
        self.tokens.bump()?;
        let name_tok = match self.tokens.expect(SyntaxKind::Ident) {
            Ok(t) => t,
            Err(e) => {
                self.errors.push(e);
                return None;
            }
        };
        let name = self.interner.intern(name_tok.text);
        let name_span = Span::new(name_tok.start, name_tok.end);
        if let Err(e) = self.tokens.expect(SyntaxKind::LParen) {
            self.errors.push(e);
            return None;
        }
        let mut params = vec![];
        while !self.tokens.at(SyntaxKind::RParen) {
            let tok = match self.tokens.expect(SyntaxKind::Ident) {
                Ok(t) => t,
                Err(e) => {
                    self.errors.push(e);
                    break;
                }
            };
            params.push((
                self.interner.intern(tok.text),
                Span::new(tok.start, tok.end),
            ));
            self.tokens.eat(SyntaxKind::Comma);
        }
        if let Err(e) = self.tokens.expect(SyntaxKind::RParen) {
            self.errors.push(e);
        }
        // Skip return type annotation if present (e.g., -> Result<i64, Str>)
        if self.tokens.eat(SyntaxKind::Arrow).is_some() {
            let mut depth = 0u32;
            while self.tokens.peek().is_some() {
                let kind = self.tokens.peek().unwrap().kind;
                if kind == SyntaxKind::LBrace && depth == 0 {
                    break;
                }
                if kind == SyntaxKind::Lt || kind == SyntaxKind::LParen {
                    depth += 1;
                }
                if kind == SyntaxKind::Gt || kind == SyntaxKind::RParen {
                    if depth > 0 {
                        depth -= 1;
                    }
                }
                self.tokens.bump();
            }
        }
        let body = self.parse_block_expr()?;
        Some(Item::FnDef {
            name,
            name_span,
            params,
            body,
        })
    }

    fn parse_struct_def(&mut self) -> Option<Item> {
        let _start_tok = self.tokens.bump()?; // .struct.
        let name_tok = match self.tokens.expect(SyntaxKind::Ident) {
            Ok(t) => t,
            Err(e) => {
                self.errors.push(e);
                return None;
            }
        };
        let name = self.interner.intern(name_tok.text);
        let name_span = Span::new(name_tok.start, name_tok.end);

        if let Err(e) = self.tokens.expect(SyntaxKind::LBrace) {
            self.errors.push(e);
            return None;
        }

        let mut fields = vec![];
        while !self.tokens.at(SyntaxKind::RBrace) {
            let field_tok = match self.tokens.expect(SyntaxKind::Ident) {
                Ok(t) => t,
                Err(e) => {
                    self.errors.push(e);
                    break;
                }
            };
            fields.push((
                self.interner.intern(field_tok.text),
                Span::new(field_tok.start, field_tok.end),
            ));

            self.tokens.eat(SyntaxKind::Colon); // skip type annotation for now

            if let Some(tok) = self.tokens.peek() {
                if tok.kind == SyntaxKind::Ident {
                    self.tokens.bump(); // consume type name
                }
            }

            self.tokens.eat(SyntaxKind::Comma);
        }

        if let Err(e) = self.tokens.expect(SyntaxKind::RBrace) {
            self.errors.push(e);
        }

        Some(Item::StructDef {
            name,
            name_span,
            fields,
        })
    }

    fn parse_enum_def(&mut self) -> Option<Item> {
        self.tokens.bump()?; // 'enum'
        let name_tok = match self.tokens.expect(SyntaxKind::Ident) {
            Ok(t) => t,
            Err(e) => {
                self.errors.push(e);
                return None;
            }
        };
        let name = self.interner.intern(name_tok.text);
        let name_span = Span::new(name_tok.start, name_tok.end);

        if let Err(e) = self.tokens.expect(SyntaxKind::LBrace) {
            self.errors.push(e);
            return None;
        }

        let mut variants = vec![];
        while !self.tokens.at(SyntaxKind::RBrace) {
            let variant_tok = match self.tokens.expect(SyntaxKind::Ident) {
                Ok(t) => t,
                Err(e) => {
                    self.errors.push(e);
                    break;
                }
            };
            let variant_name = self.interner.intern(variant_tok.text);
            let variant_span = Span::new(variant_tok.start, variant_tok.end);

            let kind = if self.tokens.at(SyntaxKind::LParen) {
                // Unnamed fields: Variant(Type, Type, ...)
                self.tokens.bump();
                let mut fields = vec![];
                while !self.tokens.at(SyntaxKind::RParen) {
                    let field_tok = match self.tokens.expect(SyntaxKind::Ident) {
                        Ok(t) => t,
                        Err(e) => {
                            self.errors.push(e);
                            break;
                        }
                    };
                    fields.push((
                        self.interner.intern(field_tok.text),
                        Span::new(field_tok.start, field_tok.end),
                    ));
                    self.tokens.eat(SyntaxKind::Comma);
                }
                if let Err(e) = self.tokens.expect(SyntaxKind::RParen) {
                    self.errors.push(e);
                }
                VariantKind::Unnamed(fields)
            } else if self.tokens.at(SyntaxKind::LBrace) {
                // Named fields: Variant { field: Type, ... }
                self.tokens.bump();
                let mut fields = vec![];
                while !self.tokens.at(SyntaxKind::RBrace) {
                    let field_tok = match self.tokens.expect(SyntaxKind::Ident) {
                        Ok(t) => t,
                        Err(e) => {
                            self.errors.push(e);
                            break;
                        }
                    };
                    fields.push((
                        self.interner.intern(field_tok.text),
                        Span::new(field_tok.start, field_tok.end),
                    ));
                    self.tokens.eat(SyntaxKind::Colon);
                    if let Some(tok) = self.tokens.peek() {
                        if tok.kind == SyntaxKind::Ident {
                            self.tokens.bump();
                        }
                    }
                    self.tokens.eat(SyntaxKind::Comma);
                }
                if let Err(e) = self.tokens.expect(SyntaxKind::RBrace) {
                    self.errors.push(e);
                }
                VariantKind::Named(fields)
            } else {
                // No data variant: Variant
                VariantKind::Unnamed(vec![])
            };

            variants.push(EnumVariant {
                name: variant_name,
                name_span: variant_span,
                kind,
            });
            self.tokens.eat(SyntaxKind::Comma);
        }

        if let Err(e) = self.tokens.expect(SyntaxKind::RBrace) {
            self.errors.push(e);
        }

        Some(Item::EnumDef {
            name,
            name_span,
            variants,
        })
    }

    fn parse_extern_block(&mut self) -> Option<Item> {
        let start_tok = self.tokens.bump()?;
        let start = start_tok.start;
        if let Err(e) = self.tokens.expect(SyntaxKind::LBrace) {
            self.errors.push(e);
            return None;
        }
        let mut functions = vec![];
        while !self.tokens.at(SyntaxKind::RBrace) && self.tokens.peek().is_some() {
            if let Err(e) = self.tokens.expect(SyntaxKind::KwFn) {
                self.errors.push(e);
                break;
            }
            let name_tok = match self.tokens.expect(SyntaxKind::Ident) {
                Ok(t) => t,
                Err(e) => {
                    self.errors.push(e);
                    break;
                }
            };
            let name = self.interner.intern(name_tok.text);
            let name_span = Span::new(name_tok.start, name_tok.end);
            if let Err(e) = self.tokens.expect(SyntaxKind::LParen) {
                self.errors.push(e);
                break;
            }
            let mut params = vec![];
            while !self.tokens.at(SyntaxKind::RParen) {
                let param_tok = match self.tokens.expect(SyntaxKind::Ident) {
                    Ok(t) => t,
                    Err(e) => {
                        self.errors.push(e);
                        break;
                    }
                };
                self.tokens.eat(SyntaxKind::Colon);
                let type_tok = if self.tokens.at(SyntaxKind::Star) {
                    self.tokens.bump();
                    self.tokens.eat(SyntaxKind::KwMut);
                    self.tokens.eat(SyntaxKind::KwLet);
                    self.tokens.expect(SyntaxKind::Ident).ok()?
                } else {
                    self.tokens.expect(SyntaxKind::Ident).ok()?
                };
                params.push((
                    self.interner.intern(param_tok.text),
                    Span::new(param_tok.start, type_tok.end),
                ));
                self.tokens.eat(SyntaxKind::Comma);
            }
            if let Err(e) = self.tokens.expect(SyntaxKind::RParen) {
                self.errors.push(e);
            }
            let ret = if self.tokens.eat(SyntaxKind::Arrow).is_some() {
                let tok = self.tokens.expect(SyntaxKind::Ident).ok()?;
                Some((
                    self.interner.intern(tok.text),
                    Span::new(tok.start, tok.end),
                ))
            } else {
                None
            };
            self.tokens.eat(SyntaxKind::Semicolon);
            functions.push(ExternFn {
                name,
                name_span,
                params,
                ret,
            });
        }
        let end_tok = match self.tokens.expect(SyntaxKind::RBrace) {
            Ok(t) => t,
            Err(e) => {
                self.errors.push(e);
                return None;
            }
        };
        Some(Item::ExternBlock {
            abi: "C".into(),
            span: Span::new(start, end_tok.end),
            functions,
        })
    }

    fn parse_use_item(&mut self) -> Option<UseItem> {
        let start_tok = self.tokens.bump()?;
        let mut path_parts = vec![];
        loop {
            let tok = match self.tokens.expect(SyntaxKind::Ident) {
                Ok(t) => t,
                Err(e) => {
                    self.errors.push(e);
                    break;
                }
            };
            path_parts.push(tok.text);
            if !self.tokens.at(SyntaxKind::Dot) {
                break;
            }
            self.tokens.bump();
        }
        self.tokens.eat(SyntaxKind::Semicolon);
        let end = path_parts.last().map_or(start_tok.end, |_| {
            self.tokens
                .peek()
                .map(|t| t.start)
                .unwrap_or(start_tok.end + path_parts.join(".").len())
        });
        Some(UseItem {
            path: path_parts.join("."),
            span: Span::new(start_tok.start, end),
        })
    }

    fn parse_let_stmt(&mut self) -> Option<StmtNode> {
        let start = self.tokens.bump()?.start;
        let mutable = self.tokens.eat_ident("mut").is_some();
        let name_tok = match self.tokens.expect(SyntaxKind::Ident) {
            Ok(t) => t,
            Err(e) => {
                self.errors.push(e);
                return None;
            }
        };
        let name = self.interner.intern(name_tok.text);
        if let Err(e) = self.tokens.expect(SyntaxKind::Eq) {
            self.errors.push(e);
            return None;
        }
        let value = self.parse_expr(0)?;
        let value_span = value.span;
        Some(StmtNode {
            kind: StmtKind::Let {
                name,
                mutable,
                value: value.clone(),
            },
            span: Span::new(start, value_span.end),
        })
    }

    fn parse_assign_stmt(&mut self) -> Option<StmtNode> {
        let name_tok = self.tokens.peek()?;
        if name_tok.kind != SyntaxKind::Ident {
            return None;
        }
        if !self
            .tokens
            .peek2()
            .is_some_and(|t| t.kind == SyntaxKind::Eq)
        {
            return None;
        }
        let target_tok = self.tokens.bump()?;
        self.tokens.bump();
        let target = self.interner.intern(target_tok.text);
        let value = self.parse_expr(0)?;
        let start = target_tok.start;
        let span = Span::new(start, value.span.end);
        Some(StmtNode {
            kind: StmtKind::Assign { target, value },
            span,
        })
    }

    pub fn parse_expr(&mut self, min_bp: u8) -> Option<ExprNode> {
        let mut left = self.parse_expr_atom()?;
        #[allow(clippy::while_let_loop)]
        loop {
            let op_tok = match self.tokens.peek() {
                Some(t) => *t,
                None => break,
            };
            if let Some((l_bp, r_bp)) = Self::infix_bp(op_tok.kind) {
                if l_bp < min_bp {
                    break;
                }
                self.tokens.bump();
                let right = self.parse_expr(r_bp)?;
                left = ExprNode {
                    kind: ExprKind::Binary {
                        op: Self::to_binop(op_tok.kind),
                        lhs: Box::new(left.clone()),
                        rhs: Box::new(right.clone()),
                    },
                    span: Span::new(left.span.start, right.span.end),
                };
                continue;
            }
            if op_tok.kind == SyntaxKind::LParen && 80 >= min_bp {
                self.tokens.bump();
                let mut args = vec![];
                while !self.tokens.at(SyntaxKind::RParen) && self.tokens.peek().is_some() {
                    args.push(self.parse_expr(0)?);
                    self.tokens.eat(SyntaxKind::Comma);
                }
                let rparen = match self.tokens.expect(SyntaxKind::RParen) {
                    Ok(t) => t,
                    Err(e) => {
                        self.errors.push(e);
                        break;
                    }
                };
                left = ExprNode {
                    kind: ExprKind::Call {
                        callee: Box::new(left.clone()),
                        args,
                    },
                    span: Span::new(left.span.start, rparen.end),
                };
                continue;
            }
            // Enum variant construction: expr::Variant(args...)
            if op_tok.kind == SyntaxKind::Colon
                && self
                    .tokens
                    .peek2()
                    .is_some_and(|t| t.kind == SyntaxKind::Colon)
                && 90 >= min_bp
            {
                self.tokens.bump(); // first ':'
                self.tokens.bump(); // second ':'
                let variant_tok = match self.tokens.expect(SyntaxKind::Ident) {
                    Ok(t) => t,
                    Err(e) => {
                        self.errors.push(e);
                        break;
                    }
                };
                let variant_name = self.interner.intern(variant_tok.text);
                let mut args = vec![];
                if self.tokens.at(SyntaxKind::LParen) {
                    self.tokens.bump();
                    while !self.tokens.at(SyntaxKind::RParen) && self.tokens.peek().is_some() {
                        args.push(self.parse_expr(0)?);
                        self.tokens.eat(SyntaxKind::Comma);
                    }
                    let rparen = match self.tokens.expect(SyntaxKind::RParen) {
                        Ok(t) => t,
                        Err(e) => {
                            self.errors.push(e);
                            break;
                        }
                    };
                    left = ExprNode {
                        kind: ExprKind::EnumVariant {
                            enum_name: if let ExprKind::Ident(sym) = &left.kind {
                                *sym
                            } else {
                                self.errors.push(ParseError::Message {
                                    msg: "expected enum name".into(),
                                    span: (left.span.start, left.span.end),
                                });
                                break;
                            },
                            variant_name,
                            args,
                        },
                        span: Span::new(left.span.start, rparen.end),
                    };
                } else {
                    // No args: Unit variant
                    left = ExprNode {
                        kind: ExprKind::EnumVariant {
                            enum_name: if let ExprKind::Ident(sym) = &left.kind {
                                *sym
                            } else {
                                self.errors.push(ParseError::Message {
                                    msg: "expected enum name".into(),
                                    span: (left.span.start, left.span.end),
                                });
                                break;
                            },
                            variant_name,
                            args: vec![],
                        },
                        span: Span::new(left.span.start, variant_tok.end),
                    };
                }
                continue;
            }
            // Try operator: expr?
            if op_tok.kind == SyntaxKind::Question && 80 >= min_bp {
                self.tokens.bump();
                left = ExprNode {
                    kind: ExprKind::TryExpr(Box::new(left.clone())),
                    span: Span::new(left.span.start, op_tok.end),
                };
                continue;
            }
            // Try operator: expr?
            if op_tok.kind == SyntaxKind::Question && 80 >= min_bp {
                self.tokens.bump();
                left = ExprNode {
                    kind: ExprKind::TryExpr(Box::new(left.clone())),
                    span: Span::new(left.span.start, op_tok.end),
                };
                continue;
            }
            // As cast: expr as Type
            if op_tok.kind == SyntaxKind::KwAs && 85 >= min_bp {
                self.tokens.bump();
                let target_tok = self.tokens.expect(SyntaxKind::Ident).ok()?;
                let target = self.interner.intern(target_tok.text);
                left = ExprNode {
                    kind: ExprKind::As {
                        expr: Box::new(left.clone()),
                        target_type: target,
                    },
                    span: Span::new(left.span.start, target_tok.end),
                };
                continue;
            }
            // Field access: expr.field
            if op_tok.kind == SyntaxKind::Dot && 90 >= min_bp {
                self.tokens.bump(); // consume '.'
                let field_tok = match self.tokens.expect(SyntaxKind::Ident) {
                    Ok(t) => t,
                    Err(e) => {
                        self.errors.push(e);
                        break;
                    }
                };
                let field = self.interner.intern(field_tok.text);
                left = ExprNode {
                    kind: ExprKind::FieldAccess {
                        object: Box::new(left.clone()),
                        field,
                    },
                    span: Span::new(left.span.start, field_tok.end),
                };
                continue;
            }
            break;
        }
        Some(left)
    }

    fn parse_expr_atom(&mut self) -> Option<ExprNode> {
        let tok = self.tokens.peek()?;
        match tok.kind {
            SyntaxKind::IntLit => {
                let tok = self.tokens.bump()?;
                let v: i64 = tok.text.parse().unwrap_or(0);
                Some(ExprNode {
                    kind: ExprKind::IntLit(v),
                    span: Span::new(tok.start, tok.end),
                })
            }
            SyntaxKind::StringLit => {
                let tok = self.tokens.bump()?;
                Some(ExprNode {
                    kind: ExprKind::StrLit(tok.text.to_owned()),
                    span: Span::new(tok.start, tok.end),
                })
            }
            SyntaxKind::KwTrue => {
                let tok = self.tokens.bump()?;
                Some(ExprNode {
                    kind: ExprKind::BoolLit(true),
                    span: Span::new(tok.start, tok.end),
                })
            }
            SyntaxKind::KwFalse => {
                let tok = self.tokens.bump()?;
                Some(ExprNode {
                    kind: ExprKind::BoolLit(false),
                    span: Span::new(tok.start, tok.end),
                })
            }
            SyntaxKind::FloatLit => {
                let tok = self.tokens.bump()?;
                let v: f64 = tok.text.parse().unwrap_or(0.0);
                Some(ExprNode {
                    kind: ExprKind::FloatLit(v),
                    span: Span::new(tok.start, tok.end),
                })
            }
            SyntaxKind::KwReturn => {
                let ret_tok = self.tokens.bump()?;
                let val = self.parse_expr(0)?;
                Some(ExprNode {
                    kind: ExprKind::Unary {
                        op: UnOp::Not,
                        operand: Box::new(val.clone()),
                    },
                    span: Span::new(ret_tok.start, val.span.end),
                })
            }
            SyntaxKind::KwMatch => self.parse_match_expr(),
            SyntaxKind::At => {
                let at = self.tokens.bump()?;
                let name_tok = self.tokens.expect(SyntaxKind::Ident).ok()?;
                let name = self.interner.intern(name_tok.text);
                self.tokens.expect(SyntaxKind::LParen).ok()?;
                let arg = self.parse_expr(0)?;
                let rparen = self.tokens.expect(SyntaxKind::RParen).ok()?;
                Some(ExprNode {
                    kind: ExprKind::MacroCall {
                        name,
                        arg: Box::new(arg),
                    },
                    span: Span::new(at.start, rparen.end),
                })
            }
            SyntaxKind::Ident => {
                let tok = self.tokens.bump()?;
                let sym = self.interner.intern(tok.text);
                let start = tok.start;
                if self.tokens.at(SyntaxKind::LBrace) {
                    self.tokens.bump();
                    let mut fields = vec![];
                    while !self.tokens.at(SyntaxKind::RBrace) && self.tokens.peek().is_some() {
                        let n = match self.tokens.expect(SyntaxKind::Ident) {
                            Ok(t) => t,
                            Err(e) => {
                                self.errors.push(e);
                                break;
                            }
                        };
                        let n_sym = self.interner.intern(n.text);
                        self.tokens.eat(SyntaxKind::Colon);
                        let val = self.parse_expr(0)?;
                        fields.push((n_sym, val));
                        self.tokens.eat(SyntaxKind::Comma);
                    }
                    let end = match self.tokens.expect(SyntaxKind::RBrace) {
                        Ok(t) => t,
                        Err(e) => {
                            self.errors.push(e);
                            return None;
                        }
                    };
                    Some(ExprNode {
                        kind: ExprKind::StructLit { name: sym, fields },
                        span: Span::new(start, end.end),
                    })
                } else if self
                    .tokens
                    .peek()
                    .is_some_and(|t| t.kind == SyntaxKind::LParen)
                    && matches!(self.interner.resolve(sym), "Some" | "Ok" | "Err")
                {
                    let name = self.interner.resolve(sym).to_string();
                    self.tokens.bump();
                    let val = self.parse_expr(0)?;
                    let rparen = self.tokens.expect(SyntaxKind::RParen).ok()?;
                    let kind = match name.as_str() {
                        "Some" => ExprKind::SomeExpr(Box::new(val)),
                        "Ok" => ExprKind::OkExpr(Box::new(val)),
                        "Err" => ExprKind::ErrExpr(Box::new(val)),
                        _ => return None,
                    };
                    Some(ExprNode {
                        kind,
                        span: Span::new(start, rparen.end),
                    })
                } else if self.tokens.peek().is_some_and(|t| t.text == "None")
                    && !self.tokens.peek2().is_some_and(|t| {
                        t.kind == SyntaxKind::LParen || t.kind == SyntaxKind::LBrace
                    })
                {
                    self.tokens.bump();
                    Some(ExprNode {
                        kind: ExprKind::NoneExpr,
                        span: Span::new(start, tok.end),
                    })
                } else {
                    Some(ExprNode {
                        kind: ExprKind::Ident(sym),
                        span: Span::new(tok.start, tok.end),
                    })
                }
            }
            SyntaxKind::LParen if self.tokens.is_lambda_start() => self.parse_lambda(),
            SyntaxKind::LParen => self.parse_paren_expr(),
            SyntaxKind::LBrace => self.parse_block_expr(),
            SyntaxKind::KwIf => self.parse_if_expr(),
            SyntaxKind::Star => {
                let star_tok = self.tokens.bump()?;
                let start = star_tok.start;
                if self.tokens.eat(SyntaxKind::KwLet).is_some() {
                    let target_tok = self.tokens.expect(SyntaxKind::Ident).ok()?;
                    let target = self.interner.intern(target_tok.text);
                    Some(ExprNode {
                        kind: ExprKind::Pointer {
                            mutable: false,
                            target,
                        },
                        span: Span::new(start, target_tok.end),
                    })
                } else if self.tokens.eat(SyntaxKind::KwMut).is_some() {
                    let target_tok = self.tokens.expect(SyntaxKind::Ident).ok()?;
                    let target = self.interner.intern(target_tok.text);
                    Some(ExprNode {
                        kind: ExprKind::Pointer {
                            mutable: true,
                            target,
                        },
                        span: Span::new(start, target_tok.end),
                    })
                } else {
                    self.errors.push(ParseError::Message {
                        msg: "expected const or mut after *".into(),
                        span: (star_tok.start, star_tok.end),
                    });
                    None
                }
            }
            SyntaxKind::Minus | SyntaxKind::Bang => {
                let op_tok = self.tokens.bump()?;
                let (r_bp, op) = match op_tok.kind {
                    SyntaxKind::Minus => (70, UnOp::Neg),
                    SyntaxKind::Bang => (70, UnOp::Not),
                    _ => unreachable!(),
                };
                let operand = self.parse_expr(r_bp)?;
                Some(ExprNode {
                    kind: ExprKind::Unary {
                        op,
                        operand: Box::new(operand.clone()),
                    },
                    span: Span::new(op_tok.start, operand.span.end),
                })
            }
            _ => {
                self.errors
                    .push(ParseError::expected_expr(tok.kind, tok.start, tok.end));
                None
            }
        }
    }

    fn parse_block_expr(&mut self) -> Option<ExprNode> {
        let start_tok = self.tokens.bump()?;
        let start = start_tok.start;
        let mut items = vec![];
        while !self.tokens.at(SyntaxKind::RBrace) && self.tokens.peek().is_some() {
            if self.tokens.at(SyntaxKind::KwLet) {
                if let Some(stmt) = self.parse_let_stmt() {
                    items.push(BlockItem::Stmt(stmt));
                    self.tokens.eat(SyntaxKind::Semicolon);
                    continue;
                }
            }
            if self.tokens.at(SyntaxKind::Ident)
                && self
                    .tokens
                    .peek2()
                    .is_some_and(|t| t.kind == SyntaxKind::Eq)
            {
                if let Some(stmt) = self.parse_assign_stmt() {
                    items.push(BlockItem::Stmt(stmt));
                    self.tokens.eat(SyntaxKind::Semicolon);
                    continue;
                }
            }
            if let Some(expr) = self.parse_expr(0) {
                items.push(BlockItem::Expr(expr));
                self.tokens.eat(SyntaxKind::Semicolon);
            } else {
                self.tokens.bump();
            }
        }
        let end_tok = match self.tokens.expect(SyntaxKind::RBrace) {
            Ok(t) => t,
            Err(e) => {
                self.errors.push(e);
                return None;
            }
        };
        Some(ExprNode {
            kind: ExprKind::Block(items),
            span: Span::new(start, end_tok.end),
        })
    }

    fn parse_if_expr(&mut self) -> Option<ExprNode> {
        let start = self.tokens.bump()?.start;
        let condition = self.parse_expr(0)?;
        let then_branch = self.parse_block_expr()?;
        let else_branch = if self.tokens.eat(SyntaxKind::KwElse).is_some() {
            if self.tokens.at(SyntaxKind::KwIf) {
                self.parse_if_expr()
            } else if self.tokens.at(SyntaxKind::LBrace) {
                let else_block = self.parse_block_expr()?;
                Some(else_block)
            } else {
                let peek = self.tokens.peek();
                self.errors.push(ParseError::expected(
                    SyntaxKind::LBrace,
                    peek.map_or(SyntaxKind::Eof, |t| t.kind),
                    peek.map_or(0, |t| t.start),
                    peek.map_or(0, |t| t.end),
                ));
                None
            }
        } else {
            None
        };
        let end = else_branch
            .as_ref()
            .map_or(then_branch.span.end, |e| e.span.end);
        Some(ExprNode {
            kind: ExprKind::If {
                condition: Box::new(condition),
                then_branch: Box::new(then_branch),
                else_branch: else_branch.map(Box::new),
            },
            span: Span::new(start, end),
        })
    }

    fn parse_lambda(&mut self) -> Option<ExprNode> {
        let start_tok = self.tokens.bump()?;
        let start = start_tok.start;
        let mut params = vec![];
        while !self.tokens.at(SyntaxKind::RParen) {
            let tok = match self.tokens.expect(SyntaxKind::Ident) {
                Ok(t) => t,
                Err(e) => {
                    self.errors.push(e);
                    break;
                }
            };
            params.push(self.interner.intern(tok.text));
            if !self.tokens.at(SyntaxKind::Comma) {
                break;
            }
            self.tokens.bump();
        }
        if let Err(e) = self.tokens.expect(SyntaxKind::RParen) {
            self.errors.push(e);
        }
        if let Err(e) = self.tokens.expect(SyntaxKind::FatArrow) {
            self.errors.push(e);
        }
        let body = self.parse_expr(0)?;
        Some(ExprNode {
            kind: ExprKind::Lambda {
                params,
                body: Box::new(body.clone()),
            },
            span: Span::new(start, body.span.end),
        })
    }

    fn parse_match_expr(&mut self) -> Option<ExprNode> {
        let start_tok = self.tokens.bump()?; // 'match'
        let start = start_tok.start;
        let scrutinee = self.parse_expr(0)?;
        if let Err(e) = self.tokens.expect(SyntaxKind::LBrace) {
            self.errors.push(e);
            return None;
        }
        let mut arms = vec![];
        while !self.tokens.at(SyntaxKind::RBrace) && self.tokens.peek().is_some() {
            let pattern = self.parse_pattern()?;
            let guard = if self.tokens.eat(SyntaxKind::KwIf).is_some() {
                Some(self.parse_expr(0)?)
            } else {
                None
            };
            if let Err(e) = self.tokens.expect(SyntaxKind::FatArrow) {
                self.errors.push(e);
                break;
            }
            let body = self.parse_expr(0)?;
            arms.push(crate::ast::MatchArm {
                pattern,
                guard,
                body,
            });
            self.tokens.eat(SyntaxKind::Comma);
        }
        let end_tok = match self.tokens.expect(SyntaxKind::RBrace) {
            Ok(t) => t,
            Err(e) => {
                self.errors.push(e);
                return None;
            }
        };
        Some(ExprNode {
            kind: ExprKind::Match {
                scrutinee: Box::new(scrutinee),
                arms,
            },
            span: Span::new(start, end_tok.end),
        })
    }

    fn parse_pattern(&mut self) -> Option<crate::ast::Pattern> {
        match self.tokens.peek()?.kind {
            SyntaxKind::Ident => {
                let tok = self.tokens.bump()?;
                let name = self.interner.intern(tok.text);
                match self.interner.resolve(name) {
                    "true" => Some(crate::ast::Pattern::BoolLit(true)),
                    "false" => Some(crate::ast::Pattern::BoolLit(false)),
                    "_" => Some(crate::ast::Pattern::Wild),
                    _ => Some(crate::ast::Pattern::Var(name)),
                }
            }
            SyntaxKind::IntLit => {
                let tok = self.tokens.bump()?;
                Some(crate::ast::Pattern::IntLit(tok.text.parse().unwrap_or(0)))
            }
            SyntaxKind::FloatLit => {
                let tok = self.tokens.bump()?;
                Some(crate::ast::Pattern::FloatLit(
                    tok.text.parse().unwrap_or(0.0),
                ))
            }
            SyntaxKind::StringLit => {
                let tok = self.tokens.bump()?;
                Some(crate::ast::Pattern::StrLit(tok.text.to_owned()))
            }
            SyntaxKind::LParen => {
                self.tokens.bump();
                self.tokens.expect(SyntaxKind::RParen).ok()?;
                Some(crate::ast::Pattern::Unit)
            }
            SyntaxKind::Minus => {
                self.tokens.bump();
                Some(crate::ast::Pattern::Wild)
            }
            _ => {
                self.errors.push(ParseError::Message {
                    msg: "expected pattern".into(),
                    span: (self.tokens.peek()?.start, self.tokens.peek()?.end),
                });
                None
            }
        }
    }

    fn parse_paren_expr(&mut self) -> Option<ExprNode> {
        self.tokens.bump();
        let inner = self.parse_expr(0)?;
        if let Err(e) = self.tokens.expect(SyntaxKind::RParen) {
            self.errors.push(e);
        }
        Some(inner)
    }

    fn infix_bp(kind: SyntaxKind) -> Option<(u8, u8)> {
        match kind {
            SyntaxKind::PipePipe => Some((10, 11)),
            SyntaxKind::AmpAmp => Some((20, 21)),
            SyntaxKind::EqEq | SyntaxKind::BangEq => Some((30, 31)),
            SyntaxKind::Lt | SyntaxKind::Gt | SyntaxKind::LtEq | SyntaxKind::GtEq => Some((40, 41)),
            SyntaxKind::Plus | SyntaxKind::Minus => Some((50, 51)),
            SyntaxKind::Star | SyntaxKind::Slash | SyntaxKind::Percent => Some((60, 61)),
            _ => None,
        }
    }
    fn to_binop(kind: SyntaxKind) -> BinOp {
        match kind {
            SyntaxKind::Plus => BinOp::Add,
            SyntaxKind::Minus => BinOp::Sub,
            SyntaxKind::Star => BinOp::Mul,
            SyntaxKind::Slash => BinOp::Div,
            SyntaxKind::Percent => BinOp::Mod,
            SyntaxKind::EqEq => BinOp::Eq,
            SyntaxKind::BangEq => BinOp::Neq,
            SyntaxKind::Lt => BinOp::Lt,
            SyntaxKind::Gt => BinOp::Gt,
            SyntaxKind::LtEq => BinOp::Lte,
            SyntaxKind::GtEq => BinOp::Gte,
            SyntaxKind::AmpAmp => BinOp::And,
            SyntaxKind::PipePipe => BinOp::Or,
            _ => unreachable!(),
        }
    }

    #[allow(dead_code)]
    pub fn recover(&mut self) {
        loop {
            match self.tokens.peek() {
                None => break,
                Some(tok) if crate::recovery::is_sync_point(tok.kind) => break,
                Some(tok) if crate::recovery::is_block_end(tok.kind) => {
                    self.tokens.bump();
                    break;
                }
                _ => {
                    self.tokens.bump();
                }
            }
        }
    }
    #[allow(dead_code)]
    pub fn current_span(&self) -> (usize, usize) {
        match self.tokens.peek() {
            Some(tok) => (tok.start, tok.end),
            None => (0, 0),
        }
    }
}

pub fn parse(source: &str) -> ParseOutput {
    let tokens = glyim_lex::tokenize(source);
    let mut parser = Parser::new(&tokens);
    let ast = parser.parse_source_file();
    ParseOutput {
        ast,
        errors: parser.errors,
        interner: parser.interner,
    }
}

pub struct ParseOutput {
    pub ast: Ast,
    pub errors: Vec<ParseError>,
    pub interner: Interner,
}
