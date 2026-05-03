use crate::ast::{StmtKind, StmtNode};
use crate::parser::Parser;
use crate::parser::patterns::parse_pattern;
use crate::parser::types::parse_type_expr;
use glyim_diag::Span;
use glyim_syntax::SyntaxKind;

impl Parser<'_> {
    #[tracing::instrument(skip_all)]
    pub(crate) fn parse_let_stmt(&mut self) -> Option<StmtNode> {
        let start = self.tokens.bump()?.start;
        let mutable = if self.tokens.at(SyntaxKind::KwMut) {
            self.tokens.bump();
            true
        } else {
            false
        };
        let pattern = parse_pattern(&mut self.tokens, &mut self.interner, &mut self.errors)?;
        // Capture optional type annotation (the original parser already consumed it but discarded)
        let ty = if self.tokens.eat(SyntaxKind::Colon).is_some() {
            parse_type_expr(&mut self.tokens, &mut self.interner)
        } else {
            None
        };
        self.tokens.expect(SyntaxKind::Eq, &mut self.errors).ok()?;
        let value = self.parse_expr(0)?;
        let value_span = value.span;
        Some(StmtNode {
            kind: StmtKind::Let {
                pattern,
                mutable,
                value,
                ty,
            },
            span: Span::new(start, value_span.end),
        })
    }

    #[allow(dead_code)]
    pub(crate) fn parse_assign_stmt(&mut self) -> Option<StmtNode> {
        let name_tok = self.tokens.peek()?;
        if name_tok.kind != SyntaxKind::Ident {
            return None;
        }
        if self.tokens.peek2().is_none_or(|t| t.kind != SyntaxKind::Eq) {
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
