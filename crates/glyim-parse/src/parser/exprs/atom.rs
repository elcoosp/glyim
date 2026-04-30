use crate::ast::{ExprKind, ExprNode, UnOp};
use crate::parser::Parser;
use glyim_diag::Span;
use glyim_syntax::SyntaxKind;

#[tracing::instrument(skip_all)]
pub(crate) fn parse_atom(parser: &mut Parser) -> Option<ExprNode> {
    let tok = parser.tokens.peek()?;
    match tok.kind {
        SyntaxKind::IntLit => {
            let tok = parser.tokens.bump()?;
            let v: i64 = tok.text.parse().unwrap_or(0);
            Some(ExprNode {
                kind: ExprKind::IntLit(v),
                span: Span::new(tok.start, tok.end),
            })
        }
        SyntaxKind::StringLit => {
            let tok = parser.tokens.bump()?;
            Some(ExprNode {
                kind: ExprKind::StrLit(tok.text.to_owned()),
                span: Span::new(tok.start, tok.end),
            })
        }
        SyntaxKind::KwTrue => {
            let tok = parser.tokens.bump()?;
            Some(ExprNode {
                kind: ExprKind::BoolLit(true),
                span: Span::new(tok.start, tok.end),
            })
        }
        SyntaxKind::KwFalse => {
            let tok = parser.tokens.bump()?;
            Some(ExprNode {
                kind: ExprKind::BoolLit(false),
                span: Span::new(tok.start, tok.end),
            })
        }
        SyntaxKind::FloatLit => {
            let tok = parser.tokens.bump()?;
            let v: f64 = tok.text.parse().unwrap_or(0.0);
            Some(ExprNode {
                kind: ExprKind::FloatLit(v),
                span: Span::new(tok.start, tok.end),
            })
        }
        SyntaxKind::KwReturn => {
            let ret_tok = parser.tokens.bump()?;
            let val = parser.parse_expr(0)?;
            Some(ExprNode {
                kind: ExprKind::Unary {
                    op: UnOp::Not,
                    operand: Box::new(val.clone()),
                },
                span: Span::new(ret_tok.start, val.span.end),
            })
        }
        SyntaxKind::KwMatch => super::complex::parse_match(parser),
        SyntaxKind::At => parse_macro_call(parser),
        SyntaxKind::Ident => parse_ident_expr(parser),
        SyntaxKind::LParen if parser.tokens.is_lambda_start() => {
            super::complex::parse_lambda(parser)
        }
        SyntaxKind::LParen => parse_paren_or_tuple(parser),
        SyntaxKind::LBrace => super::complex::parse_block(parser),
        SyntaxKind::KwWhile => super::complex::parse_while(parser),
        SyntaxKind::KwIf => super::complex::parse_if(parser),
        SyntaxKind::Star => {
            // Guard: *let, *mut, *const → null pointer expression (existing behavior)
            let is_null_ptr = if let Some(next) = parser.tokens.peek2() {
                matches!(next.kind, SyntaxKind::KwLet | SyntaxKind::KwMut)
                    || (next.kind == SyntaxKind::Ident && next.text == "const")
            } else {
                false
            };
            if is_null_ptr {
                parse_pointer(parser)
            } else {
                // Deref expression: *expr (high prefix precedence)
                let star_tok = parser.tokens.bump()?;
                let operand = parser.parse_expr(70)?;
                let op_span = operand.span;
                Some(ExprNode {
                    kind: ExprKind::Deref(Box::new(operand)),
                    span: Span::new(star_tok.start, op_span.end),
                })
            }
        }
        SyntaxKind::Minus | SyntaxKind::Bang => parse_unary(parser),
        _ => {
            parser.errors.push(crate::ParseError::expected_expr(
                tok.kind, tok.start, tok.end,
            ));
            None
        }
    }
}

fn parse_macro_call(parser: &mut Parser) -> Option<ExprNode> {
    let at = parser.tokens.bump()?;
    let name_tok = parser
        .tokens
        .expect(SyntaxKind::Ident, &mut parser.errors)
        .ok()?;
    let name = parser.interner.intern(name_tok.text);
    parser
        .tokens
        .expect(SyntaxKind::LParen, &mut parser.errors)
        .ok()?;
    let arg = parser.parse_expr(0)?;
    let rparen = parser
        .tokens
        .expect(SyntaxKind::RParen, &mut parser.errors)
        .ok()?;
    Some(ExprNode {
        kind: ExprKind::MacroCall {
            name,
            arg: Box::new(arg),
        },
        span: Span::new(at.start, rparen.end),
    })
}

fn parse_ident_expr(parser: &mut Parser) -> Option<ExprNode> {
    let tok = parser.tokens.bump()?;
    let sym = parser.interner.intern(tok.text);
    let start = tok.start;
    let name = parser.interner.resolve(sym);
    if name == "__size_of"
        && parser.tokens.at(SyntaxKind::Colon)
        && parser
            .tokens
            .peek2()
            .is_some_and(|t| t.kind == SyntaxKind::Colon)
    {
        parser.tokens.bump();
        parser.tokens.bump(); // ::
        parser
            .tokens
            .expect(SyntaxKind::Lt, &mut parser.errors)
            .ok()?;
        let ty = super::super::types::parse_type_expr(&mut parser.tokens, &mut parser.interner)?;
        parser
            .tokens
            .expect(SyntaxKind::Gt, &mut parser.errors)
            .ok()?;
        parser
            .tokens
            .expect(SyntaxKind::LParen, &mut parser.errors)
            .ok()?;
        parser
            .tokens
            .expect(SyntaxKind::RParen, &mut parser.errors)
            .ok()?;
        let end = parser.tokens.peek().map_or(tok.end, |t| t.start);
        return Some(ExprNode {
            kind: ExprKind::SizeOf(ty),
            span: Span::new(start, end),
        });
    }
    let is_uppercase = name.chars().next().is_some_and(|c| c.is_uppercase());

    if is_uppercase && parser.tokens.at(SyntaxKind::LBrace) {
        parse_struct_literal(parser, sym, start)
    } else if parser.tokens.at(SyntaxKind::LParen) && matches!(name, "Some" | "Ok" | "Err") {
        parse_option_result_ctor(parser, name.to_string(), start)
    } else if name == "None"
        && parser.tokens.peek2().map_or(true, |t| {
            t.kind != SyntaxKind::LParen && t.kind != SyntaxKind::LBrace
        })
    {
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

fn parse_struct_literal(
    parser: &mut Parser,
    name: glyim_interner::Symbol,
    start: usize,
) -> Option<ExprNode> {
    parser.tokens.bump(); // '{'
    let mut fields = vec![];
    while !parser.tokens.at(SyntaxKind::RBrace) && parser.tokens.peek().is_some() {
        let n = match parser.tokens.expect(SyntaxKind::Ident, &mut parser.errors) {
            Ok(t) => t,
            Err(_) => break,
        };
        let n_sym = parser.interner.intern(n.text);
        if parser.tokens.eat(SyntaxKind::Colon).is_some() {
            let val = parser.parse_expr(0)?;
            fields.push((n_sym, val));
        } else {
            // shorthand: { field } → { field: field }
            fields.push((
                n_sym,
                ExprNode {
                    kind: ExprKind::Ident(n_sym),
                    span: Span::new(n.start, n.end),
                },
            ));
        }
        if parser.tokens.eat(SyntaxKind::Comma).is_none() {
            break;
        }
    }
    let end = match parser.tokens.expect(SyntaxKind::RBrace, &mut parser.errors) {
        Ok(t) => t,
        Err(_) => return None,
    };
    Some(ExprNode {
        kind: ExprKind::StructLit { name, fields },
        span: Span::new(start, end.end),
    })
}

fn parse_option_result_ctor(parser: &mut Parser, name: String, start: usize) -> Option<ExprNode> {
    parser.tokens.bump(); // '('
    let val = parser.parse_expr(0)?;
    let rparen = parser
        .tokens
        .expect(SyntaxKind::RParen, &mut parser.errors)
        .ok()?;
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
}

fn parse_pointer(parser: &mut Parser) -> Option<ExprNode> {
    let star_tok = parser.tokens.bump()?;
    let start = star_tok.start;
    if parser.tokens.eat(SyntaxKind::KwLet).is_some() {
        let target_tok = parser
            .tokens
            .expect(SyntaxKind::Ident, &mut parser.errors)
            .ok()?;
        let target = parser.interner.intern(target_tok.text);
        Some(ExprNode {
            kind: ExprKind::Pointer {
                mutable: false,
                target,
            },
            span: Span::new(start, target_tok.end),
        })
    } else if parser.tokens.eat(SyntaxKind::KwMut).is_some() {
        let target_tok = parser
            .tokens
            .expect(SyntaxKind::Ident, &mut parser.errors)
            .ok()?;
        let target = parser.interner.intern(target_tok.text);
        Some(ExprNode {
            kind: ExprKind::Pointer {
                mutable: true,
                target,
            },
            span: Span::new(start, target_tok.end),
        })
    } else {
        parser.errors.push(crate::ParseError::Message {
            msg: "expected const or mut after *".into(),
            span: (star_tok.start, star_tok.end),
        });
        None
    }
}

fn parse_unary(parser: &mut Parser) -> Option<ExprNode> {
    let op_tok = parser.tokens.bump()?;
    let (r_bp, op) = match op_tok.kind {
        SyntaxKind::Minus => (70, UnOp::Neg),
        SyntaxKind::Bang => (70, UnOp::Not),
        _ => unreachable!(),
    };
    let operand = parser.parse_expr(r_bp)?;
    Some(ExprNode {
        kind: ExprKind::Unary {
            op,
            operand: Box::new(operand.clone()),
        },
        span: Span::new(op_tok.start, operand.span.end),
    })
}

fn parse_paren_or_tuple(parser: &mut Parser) -> Option<ExprNode> {
    let start_tok = parser.tokens.bump()?; // '('
    let start = start_tok.start;
    if parser.tokens.at(SyntaxKind::RParen) {
        parser.tokens.bump();
        return Some(ExprNode {
            kind: ExprKind::UnitLit,
            span: Span::new(start, start + 1),
        });
    }
    let first = parser.parse_expr(0)?;
    if parser.tokens.eat(SyntaxKind::Comma).is_some() {
        let mut elems = vec![first];
        while !parser.tokens.at(SyntaxKind::RParen) && parser.tokens.peek().is_some() {
            elems.push(parser.parse_expr(0)?);
            if parser.tokens.eat(SyntaxKind::Comma).is_none()
                && !parser.tokens.at(SyntaxKind::RParen)
            {
                break;
            }
        }
        let end_tok = parser
            .tokens
            .expect(SyntaxKind::RParen, &mut parser.errors)
            .ok()?;
        Some(ExprNode {
            kind: ExprKind::TupleLit(elems),
            span: Span::new(start, end_tok.end),
        })
    } else {
        parser
            .tokens
            .expect(SyntaxKind::RParen, &mut parser.errors)
            .ok()?;
        Some(first)
    }
}
