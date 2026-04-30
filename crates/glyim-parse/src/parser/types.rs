use crate::ast::TypeExpr;
use crate::parser::tokens::Tokens;
use glyim_interner::Interner;
use glyim_syntax::SyntaxKind;

pub(crate) fn parse_type_expr(tokens: &mut Tokens, interner: &mut Interner) -> Option<TypeExpr> {
    match tokens.peek()?.kind {
        SyntaxKind::LParen => parse_tuple_type(tokens, interner),
        SyntaxKind::Star => parse_ptr_type(tokens, interner),
        SyntaxKind::Ident => parse_named_or_generic_type(tokens, interner),
        _ => None,
    }
}

fn parse_tuple_type(tokens: &mut Tokens, interner: &mut Interner) -> Option<TypeExpr> {
    tokens.bump(); // '('
    if tokens.at(SyntaxKind::RParen) {
        tokens.bump();
        return Some(TypeExpr::Unit);
    }
    let mut elems = vec![];
    loop {
        elems.push(parse_type_expr(tokens, interner)?);
        if tokens.eat(SyntaxKind::Comma).is_none() {
            break;
        }
        if tokens.at(SyntaxKind::RParen) {
            break;
        }
    }
    tokens.expect(SyntaxKind::RParen, &mut vec![]).ok()?;
    Some(TypeExpr::Tuple(elems))
}

fn parse_ptr_type(tokens: &mut Tokens, interner: &mut Interner) -> Option<TypeExpr> {
    tokens.bump(); // '*'
    let mutable = tokens.eat(SyntaxKind::KwMut).is_some();
    if !mutable {
        // optionally eat 'const' (non-keyword identifier)
        if tokens.at(SyntaxKind::Ident) && tokens.peek().unwrap().text == "const" {
            tokens.bump();
        }
    }
    let inner = parse_type_expr(tokens, interner)?;
    Some(TypeExpr::RawPtr {
        mutable,
        inner: Box::new(inner),
    })
}

fn parse_named_or_generic_type(tokens: &mut Tokens, interner: &mut Interner) -> Option<TypeExpr> {
    let tok = tokens.bump()?;
    let sym = interner.intern(tok.text);
    match interner.resolve(sym) {
        "i64" | "Int" => return Some(TypeExpr::Int),
        "f64" | "Float" => return Some(TypeExpr::Float),
        "bool" | "Bool" => return Some(TypeExpr::Bool),
        "Str" | "str" => return Some(TypeExpr::Str),
        _ => {}
    }
    if tokens.at(SyntaxKind::Lt) {
        tokens.bump(); // '<'
        let mut args = vec![];
        loop {
            args.push(parse_type_expr(tokens, interner)?);
            if tokens.at(SyntaxKind::Gt) {
                tokens.bump();
                break;
            }
            if tokens.eat(SyntaxKind::Comma).is_none() {
                break;
            }
        }
        Some(TypeExpr::Generic(sym, args))
    } else {
        Some(TypeExpr::Named(sym))
    }
}
