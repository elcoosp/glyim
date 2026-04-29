use glyim_syntax::SyntaxKind;
use glyim_diag::Span;
use crate::ast::*;
use crate::parser::Parser;
use crate::parser::types::parse_type_expr;

pub(crate) fn parse_item(parser: &mut Parser) -> Option<Item> {
    match parser.tokens.peek()?.kind {
        SyntaxKind::At => parse_macro_def(parser),
        SyntaxKind::KwFn => parse_fn_def(parser),
        SyntaxKind::KwStruct => parse_struct_def(parser),
        SyntaxKind::KwEnum => parse_enum_def(parser),
        SyntaxKind::KwImpl => parse_impl_block(parser),
        SyntaxKind::KwExtern => parse_extern_block(parser),
        SyntaxKind::KwLet => parser.parse_let_stmt().map(Item::Stmt),
        SyntaxKind::KwUse => parse_use_item(parser).map(Item::Use),
        SyntaxKind::Ident => {
            if parser.tokens.peek2().is_some_and(|t| t.kind == SyntaxKind::Eq) { parse_binding(parser) } else { None }
        }
        _ => None,
    }
}

fn parse_macro_def(parser: &mut Parser) -> Option<Item> {
    parser.tokens.bump(); // '@'
    let name_tok = parser.tokens.expect(SyntaxKind::Ident, &mut parser.errors).ok()?;
    let _name = parser.interner.intern(name_tok.text);
    parser.tokens.expect(SyntaxKind::KwFn, &mut parser.errors).ok()?;
    let fn_name_tok = parser.tokens.expect(SyntaxKind::Ident, &mut parser.errors).ok()?;
    let fn_name = parser.interner.intern(fn_name_tok.text);
    let fn_name_span = Span::new(fn_name_tok.start, fn_name_tok.end);
    parser.tokens.expect(SyntaxKind::LParen, &mut parser.errors).ok()?;
    let mut params = vec![];
    while !parser.tokens.at(SyntaxKind::RParen) {
        let tok = parser.tokens.expect(SyntaxKind::Ident, &mut parser.errors).ok()?;
        parser.tokens.eat(SyntaxKind::Colon);
        parser.tokens.expect(SyntaxKind::Ident, &mut parser.errors).ok()?;
        params.push((parser.interner.intern(tok.text), Span::new(tok.start, tok.end)));
        if !parser.tokens.eat(SyntaxKind::Comma).is_some() { break; }
    }
    parser.tokens.expect(SyntaxKind::RParen, &mut parser.errors).ok()?;
    skip_return_type(parser);
    let body = crate::parser::exprs::complex::parse_block(parser)?;
    Some(Item::MacroDef { name: fn_name, name_span: fn_name_span, params, body })
}

fn parse_binding(parser: &mut Parser) -> Option<Item> {
    let name_tok = parser.tokens.bump()?;
    let name = parser.interner.intern(name_tok.text);
    let name_span = Span::new(name_tok.start, name_tok.end);
    parser.tokens.expect(SyntaxKind::Eq, &mut parser.errors).ok()?;
    let value = parser.parse_expr(0)?;
    Some(Item::Binding { name, name_span, value })
}

fn parse_fn_def(parser: &mut Parser) -> Option<Item> {
    parser.tokens.bump(); // 'fn'
    let _is_pub = parser.tokens.eat(SyntaxKind::KwPub).is_some();
    let name_tok = parser.tokens.expect(SyntaxKind::Ident, &mut parser.errors).ok()?;
    let name = parser.interner.intern(name_tok.text);
    let name_span = Span::new(name_tok.start, name_tok.end);
    let type_params = parse_type_params(parser);
    parser.tokens.expect(SyntaxKind::LParen, &mut parser.errors).ok()?;
    let mut params = vec![];
    while !parser.tokens.at(SyntaxKind::RParen) {
        let tok = parser.tokens.expect(SyntaxKind::Ident, &mut parser.errors).ok()?;
        let param_sym = parser.interner.intern(tok.text);
        let param_span = Span::new(tok.start, tok.end);
        let ty = if parser.tokens.eat(SyntaxKind::Colon).is_some() { parse_type_expr(&mut parser.tokens, &mut parser.interner) } else { None };
        params.push((param_sym, param_span, ty));
        if !parser.tokens.eat(SyntaxKind::Comma).is_some() { break; }
    }
    parser.tokens.expect(SyntaxKind::RParen, &mut parser.errors).ok()?;
    let ret = if parser.tokens.eat(SyntaxKind::Arrow).is_some() { parse_type_expr(&mut parser.tokens, &mut parser.interner) } else { None };
    let body = crate::parser::exprs::complex::parse_block(parser)?;
    Some(Item::FnDef { name, name_span, type_params, params, ret, body })
}

fn parse_struct_def(parser: &mut Parser) -> Option<Item> {
    parser.tokens.bump(); // 'struct'
    let _is_pub = parser.tokens.eat(SyntaxKind::KwPub).is_some();
    let name_tok = parser.tokens.expect(SyntaxKind::Ident, &mut parser.errors).ok()?;
    let name = parser.interner.intern(name_tok.text);
    let name_span = Span::new(name_tok.start, name_tok.end);
    let type_params = parse_type_params(parser);
    parser.tokens.expect(SyntaxKind::LBrace, &mut parser.errors).ok()?;
    let mut fields = vec![];
    while !parser.tokens.at(SyntaxKind::RBrace) {
        let field_tok = parser.tokens.expect(SyntaxKind::Ident, &mut parser.errors).ok()?;
        let field_sym = parser.interner.intern(field_tok.text);
        let ty = if parser.tokens.eat(SyntaxKind::Colon).is_some() { parse_type_expr(&mut parser.tokens, &mut parser.interner) } else { None };
        fields.push((field_sym, Span::new(field_tok.start, field_tok.end), ty));
        if !parser.tokens.eat(SyntaxKind::Comma).is_some() { break; }
    }
    parser.tokens.expect(SyntaxKind::RBrace, &mut parser.errors).ok()?;
    Some(Item::StructDef { name, name_span, type_params, fields })
}

fn parse_enum_def(parser: &mut Parser) -> Option<Item> {
    parser.tokens.bump(); // 'enum'
    let _is_pub = parser.tokens.eat(SyntaxKind::KwPub).is_some();
    let name_tok = parser.tokens.expect(SyntaxKind::Ident, &mut parser.errors).ok()?;
    let name = parser.interner.intern(name_tok.text);
    let name_span = Span::new(name_tok.start, name_tok.end);
    let type_params = parse_type_params(parser);
    parser.tokens.expect(SyntaxKind::LBrace, &mut parser.errors).ok()?;
    let mut variants = vec![];
    while !parser.tokens.at(SyntaxKind::RBrace) {
        let variant_tok = parser.tokens.expect(SyntaxKind::Ident, &mut parser.errors).ok()?;
        let variant_name = parser.interner.intern(variant_tok.text);
        let variant_span = Span::new(variant_tok.start, variant_tok.end);
        let kind = parse_variant_kind(parser);
        variants.push(EnumVariantRepr { name: variant_name, name_span: variant_span, kind });
        if !parser.tokens.eat(SyntaxKind::Comma).is_some() { break; }
    }
    parser.tokens.expect(SyntaxKind::RBrace, &mut parser.errors).ok()?;
    Some(Item::EnumDef { name, name_span, type_params, variants })
}

fn parse_impl_block(parser: &mut Parser) -> Option<Item> {
    let start_tok = parser.tokens.bump()?; // 'impl'
    let start = start_tok.start;
    let is_pub = parser.tokens.eat(SyntaxKind::KwPub).is_some();
    let type_params = parse_type_params(parser);
    let target_tok = parser.tokens.expect(SyntaxKind::Ident, &mut parser.errors).ok()?;
    let target = parser.interner.intern(target_tok.text);
    let target_span = Span::new(target_tok.start, target_tok.end);
    // eat optional generic arguments on the target name (e.g., Edge<T>)
    if parser.tokens.at(SyntaxKind::Lt) {
        parser.tokens.bump(); // <
        loop {
            if parser.tokens.at(SyntaxKind::Ident) {
                parser.tokens.bump(); // type param name
            }
            if parser.tokens.at(SyntaxKind::Gt) { parser.tokens.bump(); break; }
            if parser.tokens.at(SyntaxKind::Comma) { parser.tokens.bump(); continue; }
            break;
        }
    }
    parser.tokens.expect(SyntaxKind::LBrace, &mut parser.errors).ok()?;
    let mut methods = vec![];
    while !parser.tokens.at(SyntaxKind::RBrace) && parser.tokens.peek().is_some() {
        if let Some(fn_def) = parse_fn_def(parser) {
            methods.push(fn_def);
        } else {
            parser.errors.push(crate::ParseError::Message { msg: "expected method".into(), span: parser.current_span() });
            crate::parser::recovery::recover(&mut parser.tokens);
        }
    }
    let end_tok = parser.tokens.expect(SyntaxKind::RBrace, &mut parser.errors).ok()?;
    Some(Item::ImplBlock { target, target_span, type_params, is_pub, methods, span: Span::new(start, end_tok.end) })
}

fn parse_extern_block(parser: &mut Parser) -> Option<Item> {
    let start_tok = parser.tokens.bump()?;
    let start = start_tok.start;
    parser.tokens.expect(SyntaxKind::LBrace, &mut parser.errors).ok()?;
    let mut functions = vec![];
    while !parser.tokens.at(SyntaxKind::RBrace) && parser.tokens.peek().is_some() {
        if parser.tokens.expect(SyntaxKind::KwFn, &mut parser.errors).is_err() { break; }
        let name_tok = parser.tokens.expect(SyntaxKind::Ident, &mut parser.errors).ok()?;
        let name = parser.interner.intern(name_tok.text);
        let name_span = Span::new(name_tok.start, name_tok.end);
        parser.tokens.expect(SyntaxKind::LParen, &mut parser.errors).ok()?;
        let mut params = vec![];
        loop {
            if parser.tokens.at(SyntaxKind::RParen) { break; }
            let param_tok = parser.tokens.expect(SyntaxKind::Ident, &mut parser.errors).ok()?;
            parser.tokens.eat(SyntaxKind::Colon);
            parse_extern_type(parser);
            params.push((parser.interner.intern(param_tok.text), Span::new(param_tok.start, param_tok.end)));
            if !parser.tokens.eat(SyntaxKind::Comma).is_some() { break; }
        }
        parser.tokens.expect(SyntaxKind::RParen, &mut parser.errors).ok()?;
        let ret = if parser.tokens.eat(SyntaxKind::Arrow).is_some() {
            parse_extern_type(parser);
            Some((parser.interner.intern("unknown"), Span::new(0, 0)))
        } else { None };
        parser.tokens.eat(SyntaxKind::Semicolon);
        functions.push(ExternFn { name, name_span, params, ret });
    }
    let end_tok = parser.tokens.expect(SyntaxKind::RBrace, &mut parser.errors).ok()?;
    Some(Item::ExternBlock { abi: "C".into(), span: Span::new(start, end_tok.end), functions })
}

fn parse_extern_type(parser: &mut Parser) {
    // Handle pointer types: *mut T, *const T
    if parser.tokens.at(SyntaxKind::Star) {
        parser.tokens.bump(); // '*'
        parser.tokens.eat(SyntaxKind::KwMut);
        // const is not a keyword, just an identifier
        if parser.tokens.at(SyntaxKind::Ident) && parser.tokens.peek().unwrap().text == "const" {
            parser.tokens.bump();
        }
        parse_extern_type(parser);
    } else {
        parse_type_expr(&mut parser.tokens, &mut parser.interner);
    }
}

fn parse_use_item(parser: &mut Parser) -> Option<UseItem> {
    let start_tok = parser.tokens.bump()?;
    let mut path_parts = vec![];
    loop {
        let tok = parser.tokens.expect(SyntaxKind::Ident, &mut parser.errors).ok()?;
        path_parts.push(tok.text);
        if !parser.tokens.at(SyntaxKind::Dot) { break; }
        parser.tokens.bump();
    }
    parser.tokens.eat(SyntaxKind::Semicolon);
    let end = path_parts.last().map_or(start_tok.end, |_| parser.tokens.peek().map_or(start_tok.end, |t| t.start));
    Some(UseItem { path: path_parts.join("."), span: Span::new(start_tok.start, end) })
}

fn parse_type_params(parser: &mut Parser) -> Vec<glyim_interner::Symbol> {
    if !parser.tokens.at(SyntaxKind::Lt) { return vec![]; }
    parser.tokens.bump();
    let mut tp = vec![];
    loop {
        let tok = match parser.tokens.expect(SyntaxKind::Ident, &mut parser.errors) { Ok(t) => t, Err(_) => break };
        tp.push(parser.interner.intern(tok.text));
        if parser.tokens.at(SyntaxKind::Gt) { parser.tokens.bump(); break; }
        if !parser.tokens.eat(SyntaxKind::Comma).is_some() { break; }
    }
    tp
}

fn parse_variant_kind(parser: &mut Parser) -> VariantKind {
    if parser.tokens.at(SyntaxKind::LParen) {
        parser.tokens.bump();
        let mut fields = vec![];
        while !parser.tokens.at(SyntaxKind::RParen) {
            let field_tok = match parser.tokens.expect(SyntaxKind::Ident, &mut parser.errors) { Ok(t) => t, Err(_) => break };
            let f_sym = parser.interner.intern(field_tok.text);
            let ty = if parser.tokens.eat(SyntaxKind::Colon).is_some() { parse_type_expr(&mut parser.tokens, &mut parser.interner) } else { None };
            fields.push((f_sym, Span::new(field_tok.start, field_tok.end), ty));
            if !parser.tokens.eat(SyntaxKind::Comma).is_some() { break; }
        }
        parser.tokens.expect(SyntaxKind::RParen, &mut parser.errors).ok();
        VariantKind::Unnamed(fields)
    } else if parser.tokens.at(SyntaxKind::LBrace) {
        parser.tokens.bump();
        let mut fields = vec![];
        while !parser.tokens.at(SyntaxKind::RBrace) {
            let field_tok = match parser.tokens.expect(SyntaxKind::Ident, &mut parser.errors) { Ok(t) => t, Err(_) => break };
            let f_sym = parser.interner.intern(field_tok.text);
            let ty = if parser.tokens.eat(SyntaxKind::Colon).is_some() { parse_type_expr(&mut parser.tokens, &mut parser.interner) } else { None };
            fields.push((f_sym, Span::new(field_tok.start, field_tok.end), ty));
            if !parser.tokens.eat(SyntaxKind::Comma).is_some() { break; }
        }
        parser.tokens.expect(SyntaxKind::RBrace, &mut parser.errors).ok();
        VariantKind::Named(fields)
    } else {
        VariantKind::Unnamed(vec![])
    }
}

fn skip_return_type(parser: &mut Parser) {
    if parser.tokens.eat(SyntaxKind::Arrow).is_some() {
        let mut depth = 0u32;
        while parser.tokens.peek().is_some() {
            let kind = parser.tokens.peek().unwrap().kind;
            if kind == SyntaxKind::LBrace && depth == 0 { break; }
            if kind == SyntaxKind::Lt || kind == SyntaxKind::LParen { depth += 1; }
            if kind == SyntaxKind::Gt || kind == SyntaxKind::RParen { depth = depth.saturating_sub(1); }
            parser.tokens.bump();
        }
    }
}
