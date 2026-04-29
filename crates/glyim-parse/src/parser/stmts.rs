use crate::ast::{StmtKind, StmtNode};
use crate::parser::patterns::parse_pattern;
use crate::parser::Parser;
use glyim_diag::Span;
use glyim_syntax::SyntaxKind;

impl Parser<'_> {
    pub(crate) fn parse_let_stmt(&mut self) -> Option<StmtNode> {
        let start = self.tokens.bump()?.start;
        let mutable = if self.tokens.at(SyntaxKind::KwMut) {
            self.tokens.bump();
            true
        } else {
            false
        };
        let pattern = parse_pattern(&mut self.tokens, &mut self.interner, &mut self.errors)?;
        if self.tokens.eat(SyntaxKind::Colon).is_some() {
            crate::parser::types::parse_type_expr(&mut self.tokens, &mut self.interner);
        }
        self.tokens.expect(SyntaxKind::Eq, &mut self.errors).ok()?;
        let value = self.parse_expr(0)?;
        let value_span = value.span;
        Some(StmtNode {
            kind: StmtKind::Let {
                pattern,
                mutable,
                value,
            },
            span: Span::new(start, value_span.end),
        })
    }

    pub(crate) fn parse_assign_stmt(&mut self) -> Option<StmtNode> {
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
        self.tokens.bump(); // '='
        let target = self.interner.intern(target_tok.text);
        let value = self.parse_expr(0)?;
        let span = Span::new(target_tok.start, value.span.end);
        Some(StmtNode {
            kind: StmtKind::Assign { target, value },
            span,
        })
    }
}
