use crate::ast::*;
use crate::parser::types::parse_type_expr;
use crate::parser::Parser;
use glyim_diag::Span;
use glyim_syntax::SyntaxKind;

#[tracing::instrument(skip_all)]
pub(crate) fn parse_item(parser: &mut Parser) -> Option<Item> {
    let attrs = parser.parse_attributes();
    match parser.tokens.peek()?.kind {
        SyntaxKind::At => parse_macro_def(parser),
        SyntaxKind::KwFn => parse_fn_def_with_attrs(parser, attrs),
        SyntaxKind::KwStruct => parse_struct_def(parser),
        SyntaxKind::KwEnum => parse_enum_def(parser),
        SyntaxKind::KwImpl => parse_impl_block(parser),
        SyntaxKind::KwExtern => parse_extern_block(parser),
        SyntaxKind::KwLet => parser.parse_let_stmt().map(Item::Stmt),
        SyntaxKind::KwUse => parse_use_item(parser).map(Item::Use),
        SyntaxKind::Ident => {
            if parser
                .tokens
                .peek2()
                .is_some_and(|t| t.kind == SyntaxKind::Eq)
            {
                parse_binding_with_attrs(parser, attrs)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn parse_macro_def(parser: &mut Parser) -> Option<Item> {
    parser.tokens.bump();
    let name_tok = parser
        .tokens
        .expect(SyntaxKind::Ident, &mut parser.errors)
        .ok()?;
    let _name = parser.interner.intern(name_tok.text);
    parser
        .tokens
        .expect(SyntaxKind::KwFn, &mut parser.errors)
        .ok()?;
    let fn_name_tok = parser
        .tokens
        .expect(SyntaxKind::Ident, &mut parser.errors)
        .ok()?;
    let fn_name = parser.interner.intern(fn_name_tok.text);
    let fn_name_span = Span::new(fn_name_tok.start, fn_name_tok.end);
    parser
        .tokens
        .expect(SyntaxKind::LParen, &mut parser.errors)
        .ok()?;
    let mut params = vec![];
    while !parser.tokens.at(SyntaxKind::RParen) {
        let tok = parser
            .tokens
            .expect(SyntaxKind::Ident, &mut parser.errors)
            .ok()?;
        parser.tokens.eat(SyntaxKind::Colon);
        parser
            .tokens
            .expect(SyntaxKind::Ident, &mut parser.errors)
            .ok()?;
        params.push((
            parser.interner.intern(tok.text),
            Span::new(tok.start, tok.end),
        ));
        if parser.tokens.eat(SyntaxKind::Comma).is_none() {
            break;
        }
    }
    parser
        .tokens
        .expect(SyntaxKind::RParen, &mut parser.errors)
        .ok()?;
    skip_return_type(parser);
    let body = crate::parser::exprs::complex::parse_block(parser)?;
    Some(Item::MacroDef {
        name: fn_name,
        name_span: fn_name_span,
        params,
        body,
    })
}

fn parse_binding_with_attrs(
    parser: &mut Parser,
    attrs: Vec<crate::ast::Attribute>,
) -> Option<Item> {
    let name_tok = parser.tokens.bump()?;
    let name = parser.interner.intern(name_tok.text);
    let name_span = Span::new(name_tok.start, name_tok.end);
    parser
        .tokens
        .expect(SyntaxKind::Eq, &mut parser.errors)
        .ok()?;
    let value = parser.parse_expr(0)?;
    Some(Item::Binding {
        name,
        name_span,
        value,
        attrs,
    })
}

fn parse_fn_def_with_attrs(parser: &mut Parser, attrs: Vec<crate::ast::Attribute>) -> Option<Item> {
    parser.tokens.bump(); // fn
    let _ = parser.tokens.eat(SyntaxKind::KwPub);
    let name_tok = parser
        .tokens
        .expect(SyntaxKind::Ident, &mut parser.errors)
        .ok()?;
    let name = parser.interner.intern(name_tok.text);
    let name_span = Span::new(name_tok.start, name_tok.end);
    let type_params = parse_type_params(parser);
    parser
        .tokens
        .expect(SyntaxKind::LParen, &mut parser.errors)
        .ok()?;
    let mut params = vec![];
    while !parser.tokens.at(SyntaxKind::RParen) {
        let tok = parser
            .tokens
            .expect(SyntaxKind::Ident, &mut parser.errors)
            .ok()?;
        let param_sym = parser.interner.intern(tok.text);
        let param_span = Span::new(tok.start, tok.end);
        let ty = if parser.tokens.eat(SyntaxKind::Colon).is_some() {
            parse_type_expr(&mut parser.tokens, &mut parser.interner)
        } else {
            None
        };
        params.push((param_sym, param_span, ty));
        if parser.tokens.eat(SyntaxKind::Comma).is_none() {
            break;
        }
    }
    parser
        .tokens
        .expect(SyntaxKind::RParen, &mut parser.errors)
        .ok()?;
    let ret = if parser.tokens.eat(SyntaxKind::Arrow).is_some() {
        parse_type_expr(&mut parser.tokens, &mut parser.interner)
    } else {
        None
    };
    let body = crate::parser::exprs::complex::parse_block(parser)?;
    Some(Item::FnDef {
        name,
        name_span,
        type_params,
        params,
        ret,
        body,
        attrs,
    })
}

fn parse_struct_def(parser: &mut Parser) -> Option<Item> {
    parser.tokens.bump(); // struct
    let _ = parser.tokens.eat(SyntaxKind::KwPub);
    let name_tok = parser
        .tokens
        .expect(SyntaxKind::Ident, &mut parser.errors)
        .ok()?;
    let name = parser.interner.intern(name_tok.text);
    let name_span = Span::new(name_tok.start, name_tok.end);
    let type_params = parse_type_params(parser);
    parser
        .tokens
        .expect(SyntaxKind::LBrace, &mut parser.errors)
        .ok()?;
    let mut fields = vec![];
    while !parser.tokens.at(SyntaxKind::RBrace) {
        let field_tok = parser
            .tokens
            .expect(SyntaxKind::Ident, &mut parser.errors)
            .ok()?;
        let field_sym = parser.interner.intern(field_tok.text);
        let ty = if parser.tokens.eat(SyntaxKind::Colon).is_some() {
            parse_type_expr(&mut parser.tokens, &mut parser.interner)
        } else {
            None
        };
        fields.push((field_sym, Span::new(field_tok.start, field_tok.end), ty));
        if parser.tokens.eat(SyntaxKind::Comma).is_none() {
            break;
        }
    }
    parser
        .tokens
        .expect(SyntaxKind::RBrace, &mut parser.errors)
        .ok()?;
    Some(Item::StructDef {
        name,
        name_span,
        type_params,
        fields,
    })
}

fn parse_enum_def(parser: &mut Parser) -> Option<Item> {
    parser.tokens.bump(); // enum
    let _ = parser.tokens.eat(SyntaxKind::KwPub);
    let name_tok = parser
        .tokens
        .expect(SyntaxKind::Ident, &mut parser.errors)
        .ok()?;
    let name = parser.interner.intern(name_tok.text);
    let name_span = Span::new(name_tok.start, name_tok.end);
    let type_params = parse_type_params(parser);
    parser
        .tokens
        .expect(SyntaxKind::LBrace, &mut parser.errors)
        .ok()?;
    let mut variants = vec![];
    while !parser.tokens.at(SyntaxKind::RBrace) {
        let variant_tok = parser
            .tokens
            .expect(SyntaxKind::Ident, &mut parser.errors)
            .ok()?;
        let variant_name = parser.interner.intern(variant_tok.text);
        let variant_span = Span::new(variant_tok.start, variant_tok.end);
        let kind = parse_variant_kind(parser);
        variants.push(EnumVariantRepr {
            name: variant_name,
            name_span: variant_span,
            kind,
        });
        if parser.tokens.eat(SyntaxKind::Comma).is_none() {
            break;
        }
    }
    parser
        .tokens
        .expect(SyntaxKind::RBrace, &mut parser.errors)
        .ok()?;
    Some(Item::EnumDef {
        name,
        name_span,
        type_params,
        variants,
    })
}

fn parse_impl_block(parser: &mut Parser) -> Option<Item> {
    let start_tok = parser.tokens.bump()?;
    let start = start_tok.start;
    let is_pub = parser.tokens.eat(SyntaxKind::KwPub).is_some();
    let type_params = parse_type_params(parser);
    let target_tok = parser
        .tokens
        .expect(SyntaxKind::Ident, &mut parser.errors)
        .ok()?;
    let target = parser.interner.intern(target_tok.text);
    let _ = parser.tokens.eat(SyntaxKind::Lt); // skip generics on target
    while parser.tokens.at(SyntaxKind::Ident) {
        parser.tokens.bump();
    }
    let _ = parser.tokens.eat(SyntaxKind::Gt);
    parser
        .tokens
        .expect(SyntaxKind::LBrace, &mut parser.errors)
        .ok()?;
    let mut methods = vec![];
    while !parser.tokens.at(SyntaxKind::RBrace) && parser.tokens.peek().is_some() {
        if let Some(fn_def) = parse_fn_def_with_attrs(parser, vec![]) {
            methods.push(fn_def);
        } else {
            parser.errors.push(crate::ParseError::Message {
                msg: "expected method".into(),
                span: parser.current_span(),
            });
            crate::parser::recovery::recover(&mut parser.tokens);
        }
    }
    let end_tok = parser
        .tokens
        .expect(SyntaxKind::RBrace, &mut parser.errors)
        .ok()?;
    Some(Item::ImplBlock {
        target,
        target_span: Span::new(target_tok.start, target_tok.end),
        type_params,
        is_pub,
        methods,
        span: Span::new(start, end_tok.end),
    })
}

fn parse_extern_block(parser: &mut Parser) -> Option<Item> {
    let start_tok = parser.tokens.bump()?;
    let start = start_tok.start;
    parser
        .tokens
        .expect(SyntaxKind::LBrace, &mut parser.errors)
        .ok()?;
    let mut functions = vec![];
    while !parser.tokens.at(SyntaxKind::RBrace) && parser.tokens.peek().is_some() {
        if parser
            .tokens
            .expect(SyntaxKind::KwFn, &mut parser.errors)
            .is_err()
        {
            break;
        }
        let name_tok = parser
            .tokens
            .expect(SyntaxKind::Ident, &mut parser.errors)
            .ok()?;
        let name = parser.interner.intern(name_tok.text);
        parser
            .tokens
            .expect(SyntaxKind::LParen, &mut parser.errors)
            .ok()?;
        let mut params = vec![];
        loop {
            if parser.tokens.at(SyntaxKind::RParen) || parser.tokens.is_eof() {
                break;
            }
            let param_tok = parser
                .tokens
                .expect(SyntaxKind::Ident, &mut parser.errors)
                .ok()?;
            parser.tokens.eat(SyntaxKind::Colon);
            let ty = parse_extern_type(parser);
            params.push((
                parser.interner.intern(param_tok.text),
                Span::new(param_tok.start, param_tok.end),
                ty,
            ));
            if parser.tokens.eat(SyntaxKind::Comma).is_none() {
                break;
            }
        }
        parser
            .tokens
            .expect(SyntaxKind::RParen, &mut parser.errors)
            .ok()?;
        let ret = if parser.tokens.eat(SyntaxKind::Arrow).is_some() {
            parse_extern_type(parser)
        } else {
            None
        };
        parser.tokens.eat(SyntaxKind::Semicolon);
        functions.push(ExternFn {
            name,
            name_span: Span::new(name_tok.start, name_tok.end),
            params,
            ret,
        });
    }
    let end_tok = parser
        .tokens
        .expect(SyntaxKind::RBrace, &mut parser.errors)
        .ok()?;
    Some(Item::ExternBlock {
        abi: "C".into(),
        span: Span::new(start, end_tok.end),
        functions,
    })
}

fn parse_extern_type(parser: &mut Parser) -> Option<TypeExpr> {
    if parser.tokens.at(SyntaxKind::Star) {
        parser.tokens.bump();
        let mutable = parser.tokens.eat(SyntaxKind::KwMut).is_some();
        if !mutable
            && parser.tokens.at(SyntaxKind::Ident)
            && parser.tokens.peek().unwrap().text == "const"
        {
            parser.tokens.bump();
        }
        let inner = parse_extern_type(parser)?;
        Some(TypeExpr::RawPtr {
            mutable,
            inner: Box::new(inner),
        })
    } else {
        parse_type_expr(&mut parser.tokens, &mut parser.interner)
    }
}

fn parse_use_item(parser: &mut Parser) -> Option<UseItem> {
    let start_tok = parser.tokens.bump()?;
    let mut path_parts = vec![];
    loop {
        let tok = parser
            .tokens
            .expect(SyntaxKind::Ident, &mut parser.errors)
            .ok()?;
        path_parts.push(tok.text);
        if !parser.tokens.at(SyntaxKind::Dot) {
            break;
        }
        parser.tokens.bump();
    }
    parser.tokens.eat(SyntaxKind::Semicolon);
    let end = path_parts.last().map_or(start_tok.end, |_| {
        parser.tokens.peek().map_or(start_tok.end, |t| t.start)
    });
    Some(UseItem {
        path: path_parts.join("."),
        span: Span::new(start_tok.start, end),
    })
}

fn parse_type_params(parser: &mut Parser) -> Vec<glyim_interner::Symbol> {
    if !parser.tokens.at(SyntaxKind::Lt) {
        return vec![];
    }
    parser.tokens.bump();
    let mut tp = vec![];
    while let Ok(t) = parser.tokens.expect(SyntaxKind::Ident, &mut parser.errors) {
        tp.push(parser.interner.intern(t.text));
        if parser.tokens.at(SyntaxKind::Gt) {
            parser.tokens.bump();
            break;
        }
        if parser.tokens.eat(SyntaxKind::Comma).is_none() {
            break;
        }
    }
    tp
}

fn parse_variant_kind(parser: &mut Parser) -> VariantKind {
    if parser.tokens.at(SyntaxKind::LParen) {
        parser.tokens.bump();
        let mut fields = vec![];
        while !parser.tokens.at(SyntaxKind::RParen) {
            let field_tok = match parser.tokens.expect(SyntaxKind::Ident, &mut parser.errors) {
                Ok(t) => t,
                Err(_) => break,
            };
            let f_sym = parser.interner.intern(field_tok.text);
            let ty = if parser.tokens.eat(SyntaxKind::Colon).is_some() {
                parse_type_expr(&mut parser.tokens, &mut parser.interner)
            } else {
                None
            };
            fields.push((f_sym, Span::new(field_tok.start, field_tok.end), ty));
            if parser.tokens.eat(SyntaxKind::Comma).is_none() {
                break;
            }
        }
        parser
            .tokens
            .expect(SyntaxKind::RParen, &mut parser.errors)
            .ok();
        VariantKind::Unnamed(fields)
    } else if parser.tokens.at(SyntaxKind::LBrace) {
        parser.tokens.bump();
        let mut fields = vec![];
        while !parser.tokens.at(SyntaxKind::RBrace) {
            let field_tok = match parser.tokens.expect(SyntaxKind::Ident, &mut parser.errors) {
                Ok(t) => t,
                Err(_) => break,
            };
            let f_sym = parser.interner.intern(field_tok.text);
            let ty = if parser.tokens.eat(SyntaxKind::Colon).is_some() {
                parse_type_expr(&mut parser.tokens, &mut parser.interner)
            } else {
                None
            };
            fields.push((f_sym, Span::new(field_tok.start, field_tok.end), ty));
            if parser.tokens.eat(SyntaxKind::Comma).is_none() {
                break;
            }
        }
        parser
            .tokens
            .expect(SyntaxKind::RBrace, &mut parser.errors)
            .ok();
        VariantKind::Named(fields)
    } else {
        VariantKind::Unnamed(vec![])
    }
}

fn skip_return_type(parser: &mut Parser) {
    if parser.tokens.eat(SyntaxKind::Arrow).is_some() {
        let mut depth = 0u32;
        while let Some(tok) = parser.tokens.peek() {
            if tok.kind == SyntaxKind::LBrace && depth == 0 {
                break;
            }
            if tok.kind == SyntaxKind::Lt || tok.kind == SyntaxKind::LParen {
                depth += 1;
            }
            if tok.kind == SyntaxKind::Gt || tok.kind == SyntaxKind::RParen {
                depth = depth.saturating_sub(1);
            }
            parser.tokens.bump();
        }
    }
}
