use crate::ast::Pattern;
use crate::parser::tokens::Tokens;
use glyim_interner::Interner;
use glyim_syntax::SyntaxKind;
#[tracing::instrument(skip_all)]

pub(crate) fn parse_pattern(
    tokens: &mut Tokens,
    interner: &mut Interner,
    errors: &mut Vec<crate::ParseError>,
) -> Option<Pattern> {
    match tokens.peek()?.kind {
        SyntaxKind::Ident => parse_ident_pattern(tokens, interner, errors),
        SyntaxKind::IntLit => {
            let tok = tokens.bump()?;
            Some(Pattern::IntLit(tok.text.parse().unwrap_or(0)))
        }
        SyntaxKind::FloatLit => {
            let tok = tokens.bump()?;
            Some(Pattern::FloatLit(tok.text.parse().unwrap_or(0.0)))
        }
        SyntaxKind::StringLit => {
            let tok = tokens.bump()?;
            Some(Pattern::StrLit(tok.text.to_owned()))
        }
        SyntaxKind::LParen => parse_paren_pattern(tokens, interner, errors),
        SyntaxKind::Minus => {
            tokens.bump();
            Some(Pattern::Wild)
        }
        _ => {
            let peek = tokens.peek().unwrap();
            errors.push(crate::ParseError::Message {
                msg: "expected pattern".into(),
                span: (peek.start, peek.end),
            });
            None
        }
    }
}

fn parse_ident_pattern(
    tokens: &mut Tokens,
    interner: &mut Interner,
    errors: &mut Vec<crate::ParseError>,
) -> Option<Pattern> {
    let tok = tokens.bump()?;
    let name = interner.intern(tok.text);
    let name_str = interner.resolve(name);

    // Enum variant pattern: Name::Variant
    if tokens.at(SyntaxKind::Colon) && tokens.peek2().is_some_and(|t| t.kind == SyntaxKind::Colon) {
        tokens.bump();
        tokens.bump();
        let variant_tok = tokens.expect(SyntaxKind::Ident, errors).ok()?;
        let variant_name = interner.intern(variant_tok.text);
        let mut args = vec![];
        if tokens.at(SyntaxKind::LParen) {
            tokens.bump();
            while !tokens.at(SyntaxKind::RParen) && tokens.peek().is_some() {
                args.push(parse_pattern(tokens, interner, errors)?);
                if tokens.eat(SyntaxKind::Comma).is_none() {
                    break;
                }
            }
            tokens.expect(SyntaxKind::RParen, errors).ok()?;
        }
        return Some(Pattern::EnumVariant {
            enum_name: name,
            variant_name,
            args,
        });
    }

    // Shortcuts for Some/Ok/Err/None
    match name_str {
        "Some" if tokens.at(SyntaxKind::LParen) => {
            tokens.bump();
            let inner = parse_pattern(tokens, interner, errors)?;
            tokens.expect(SyntaxKind::RParen, errors).ok()?;
            return Some(Pattern::OptionSome(Box::new(inner)));
        }
        "Ok" if tokens.at(SyntaxKind::LParen) => {
            tokens.bump();
            let inner = parse_pattern(tokens, interner, errors)?;
            tokens.expect(SyntaxKind::RParen, errors).ok()?;
            return Some(Pattern::ResultOk(Box::new(inner)));
        }
        "Err" if tokens.at(SyntaxKind::LParen) => {
            tokens.bump();
            let inner = parse_pattern(tokens, interner, errors)?;
            tokens.expect(SyntaxKind::RParen, errors).ok()?;
            return Some(Pattern::ResultErr(Box::new(inner)));
        }
        "None" => return Some(Pattern::OptionNone),
        "true" => return Some(Pattern::BoolLit(true)),
        "false" => return Some(Pattern::BoolLit(false)),
        "_" => return Some(Pattern::Wild),
        _ => {}
    }

    // Struct pattern: Name { field, .. }
    if tokens.at(SyntaxKind::LBrace) {
        tokens.bump();
        let mut fields = vec![];
        while !tokens.at(SyntaxKind::RBrace) && tokens.peek().is_some() {
            // Rest pattern: ..
            if tokens.at(SyntaxKind::Dot)
                && tokens.peek2().is_some_and(|t| t.kind == SyntaxKind::Dot)
            {
                tokens.bump();
                tokens.bump();
                if tokens.eat(SyntaxKind::Comma).is_some() {
                    continue;
                } else {
                    break;
                }
            }
            let field_tok = tokens.expect(SyntaxKind::Ident, errors).ok()?;
            let field_sym = interner.intern(field_tok.text);
            let sub_pat = if tokens.eat(SyntaxKind::Colon).is_some() {
                parse_pattern(tokens, interner, errors)?
            } else {
                Pattern::Var(field_sym)
            };
            fields.push((field_sym, sub_pat));
            if tokens.eat(SyntaxKind::Comma).is_none() {
                break;
            }
        }
        tokens.expect(SyntaxKind::RBrace, errors).ok()?;
        return Some(Pattern::Struct { name, fields });
    }

    Some(Pattern::Var(name))
}

fn parse_paren_pattern(
    tokens: &mut Tokens,
    interner: &mut Interner,
    errors: &mut Vec<crate::ParseError>,
) -> Option<Pattern> {
    tokens.bump(); // '('
    if tokens.at(SyntaxKind::RParen) {
        tokens.bump();
        return Some(Pattern::Unit);
    }
    let mut elems = vec![];
    loop {
        elems.push(parse_pattern(tokens, interner, errors)?);
        if tokens.eat(SyntaxKind::Comma).is_none() {
            break;
        }
        if tokens.at(SyntaxKind::RParen) {
            break;
        }
    }
    tokens.expect(SyntaxKind::RParen, errors).ok()?;
    Some(Pattern::Tuple(elems))
}
