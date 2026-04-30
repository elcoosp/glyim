mod atom;
pub(crate) mod complex;

use crate::ast::{ExprKind, ExprNode};
use crate::parser::Parser;
use glyim_diag::Span;
use glyim_syntax::SyntaxKind;

impl Parser<'_> {
    #[tracing::instrument(skip_all)]
    pub fn parse_expr(&mut self, min_bp: u8) -> Option<ExprNode> {
        let mut left = atom::parse_atom(self)?;
        while let Some(op_tok) = self.tokens.peek() {
            let op_tok = *op_tok;
            if let Some((l_bp, r_bp)) = super::precedence::infix_bp(op_tok.kind) {
                if l_bp < min_bp {
                    break;
                }
                self.tokens.bump();
                let right = self.parse_expr(r_bp)?;
                left = ExprNode {
                    kind: ExprKind::Binary {
                        op: super::precedence::to_binop(op_tok.kind),
                        lhs: Box::new(left.clone()),
                        rhs: Box::new(right.clone()),
                    },
                    span: Span::new(left.span.start, right.span.end),
                };
                continue;
            }
            // Call suffix
            if op_tok.kind == SyntaxKind::LParen && 80 >= min_bp {
                self.tokens.bump();
                let mut args = vec![];
                while !self.tokens.at(SyntaxKind::RParen) && self.tokens.peek().is_some() {
                    args.push(self.parse_expr(0)?);
                    if self.tokens.eat(SyntaxKind::Comma).is_none()
                        && !self.tokens.at(SyntaxKind::RParen)
                    {
                        break;
                    }
                }
                let rparen = match self.tokens.expect(SyntaxKind::RParen, &mut self.errors) {
                    Ok(t) => t,
                    Err(_) => break,
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
            // Enum variant construction: expr::Variant
            if op_tok.kind == SyntaxKind::Colon
                && self
                    .tokens
                    .peek2()
                    .is_some_and(|t| t.kind == SyntaxKind::Colon)
                && 90 >= min_bp
            {
                self.tokens.bump();
                self.tokens.bump(); // consume ::
                let variant_tok = match self.tokens.expect(SyntaxKind::Ident, &mut self.errors) {
                    Ok(t) => t,
                    Err(_) => break,
                };
                let variant_name = self.interner.intern(variant_tok.text);
                let mut args = vec![];
                if self.tokens.at(SyntaxKind::LParen) {
                    self.tokens.bump();
                    while !self.tokens.at(SyntaxKind::RParen) && self.tokens.peek().is_some() {
                        args.push(self.parse_expr(0)?);
                        if self.tokens.eat(SyntaxKind::Comma).is_none()
                            && !self.tokens.at(SyntaxKind::RParen)
                        {
                            break;
                        }
                    }
                    let rparen = match self.tokens.expect(SyntaxKind::RParen, &mut self.errors) {
                        Ok(t) => t,
                        Err(_) => break,
                    };
                    let enum_name = match &left.kind {
                        ExprKind::Ident(sym) => *sym,
                        _ => {
                            self.errors.push(crate::ParseError::Message {
                                msg: "expected enum name".into(),
                                span: (left.span.start, left.span.end),
                            });
                            break;
                        }
                    };
                    left = ExprNode {
                        kind: ExprKind::EnumVariant {
                            enum_name,
                            variant_name,
                            args,
                        },
                        span: Span::new(left.span.start, rparen.end),
                    };
                } else {
                    let enum_name = match &left.kind {
                        ExprKind::Ident(sym) => *sym,
                        _ => {
                            self.errors.push(crate::ParseError::Message {
                                msg: "expected enum name".into(),
                                span: (left.span.start, left.span.end),
                            });
                            break;
                        }
                    };
                    left = ExprNode {
                        kind: ExprKind::EnumVariant {
                            enum_name,
                            variant_name,
                            args,
                        },
                        span: Span::new(left.span.start, variant_tok.end),
                    };
                }
                continue;
            }
            if op_tok.kind == SyntaxKind::Question && 80 >= min_bp {
                self.tokens.bump();
                left = ExprNode {
                    kind: ExprKind::TryExpr(Box::new(left.clone())),
                    span: Span::new(left.span.start, op_tok.end),
                };
                continue;
            }
            if op_tok.kind == SyntaxKind::KwAs && 85 >= min_bp {
                self.tokens.bump();
                let target_tok = self
                    .tokens
                    .expect(SyntaxKind::Ident, &mut self.errors)
                    .ok()?;
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
            if op_tok.kind == SyntaxKind::Dot && 90 >= min_bp {
                self.tokens.bump();
                let field_tok = match self.tokens.expect(SyntaxKind::Ident, &mut self.errors) {
                    Ok(t) => t,
                    Err(_) => break,
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
}
