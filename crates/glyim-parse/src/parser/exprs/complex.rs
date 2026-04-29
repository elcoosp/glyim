use glyim_syntax::SyntaxKind;
use glyim_diag::Span;
use crate::ast::{ExprKind, ExprNode, MatchArm, BlockItem};
use crate::parser::Parser;
use crate::parser::patterns::parse_pattern;

pub(crate) fn parse_block(parser: &mut Parser) -> Option<ExprNode> {
    let start_tok = parser.tokens.bump()?; // '{'
    let start = start_tok.start;
    let mut items = vec![];
    while !parser.tokens.at(SyntaxKind::RBrace) && parser.tokens.peek().is_some() {
        if parser.tokens.at(SyntaxKind::KwLet) {
            if let Some(stmt) = parser.parse_let_stmt() {
                items.push(BlockItem::Stmt(stmt));
                parser.tokens.eat(SyntaxKind::Semicolon);
                continue;
            }
        }
        if parser.tokens.at(SyntaxKind::Ident) && parser.tokens.peek2().is_some_and(|t| t.kind == SyntaxKind::Eq) {
            if let Some(stmt) = parser.parse_assign_stmt() {
                items.push(BlockItem::Stmt(stmt));
                parser.tokens.eat(SyntaxKind::Semicolon);
                continue;
            }
        }
        if let Some(expr) = parser.parse_expr(0) {
            items.push(BlockItem::Expr(expr));
            parser.tokens.eat(SyntaxKind::Semicolon);
        } else {
            parser.tokens.bump(); // skip bad token
        }
    }
    let end_tok = match parser.tokens.expect(SyntaxKind::RBrace, &mut parser.errors) { Ok(t) => t, Err(_) => return None };
    Some(ExprNode { kind: ExprKind::Block(items), span: Span::new(start, end_tok.end) })
}

pub(crate) fn parse_if(parser: &mut Parser) -> Option<ExprNode> {
    let start = parser.tokens.bump()?.start;
    let condition = parser.parse_expr(0)?;
    let then_branch = parse_block(parser)?;
    let else_branch = if parser.tokens.eat(SyntaxKind::KwElse).is_some() {
        if parser.tokens.at(SyntaxKind::KwIf) { parse_if(parser) }
        else if parser.tokens.at(SyntaxKind::LBrace) { let b = parse_block(parser)?; Some(b) }
        else { let peek = parser.tokens.peek(); parser.errors.push(crate::ParseError::expected(SyntaxKind::LBrace, peek.map_or(SyntaxKind::Eof, |t| t.kind), peek.map_or(0, |t| t.start), peek.map_or(0, |t| t.end))); None }
    } else { None };
    let end = else_branch.as_ref().map_or(then_branch.span.end, |e| e.span.end);
    Some(ExprNode { kind: ExprKind::If { condition: Box::new(condition), then_branch: Box::new(then_branch), else_branch: else_branch.map(Box::new) }, span: Span::new(start, end) })
}

pub(crate) fn parse_lambda(parser: &mut Parser) -> Option<ExprNode> {
    let start_tok = parser.tokens.bump()?; // '('
    let start = start_tok.start;
    let mut params = vec![];
    while !parser.tokens.at(SyntaxKind::RParen) {
        let tok = match parser.tokens.expect(SyntaxKind::Ident, &mut parser.errors) { Ok(t) => t, Err(_) => break };
        params.push(parser.interner.intern(tok.text));
        if !parser.tokens.at(SyntaxKind::Comma) { break; }
        parser.tokens.bump();
    }
    parser.tokens.expect(SyntaxKind::RParen, &mut parser.errors).ok()?;
    parser.tokens.expect(SyntaxKind::FatArrow, &mut parser.errors).ok()?;
    let body = parser.parse_expr(0)?;
    Some(ExprNode { kind: ExprKind::Lambda { params, body: Box::new(body.clone()) }, span: Span::new(start, body.span.end) })
}

pub(crate) fn parse_match(parser: &mut Parser) -> Option<ExprNode> {
    let start_tok = parser.tokens.bump()?; // 'match'
    let start = start_tok.start;
    let scrutinee = parser.parse_expr(0)?;
    parser.tokens.expect(SyntaxKind::LBrace, &mut parser.errors).ok()?;
    let mut arms = vec![];
    while !parser.tokens.at(SyntaxKind::RBrace) && parser.tokens.peek().is_some() {
        let pattern = parse_pattern(&mut parser.tokens, &mut parser.interner, &mut parser.errors)?;
        let guard = if parser.tokens.eat(SyntaxKind::KwIf).is_some() { Some(parser.parse_expr(0)?) } else { None };
        parser.tokens.expect(SyntaxKind::FatArrow, &mut parser.errors).ok()?;
        let body = parser.parse_expr(0)?;
        arms.push(MatchArm { pattern, guard, body });
        if !parser.tokens.eat(SyntaxKind::Comma).is_some() && !parser.tokens.at(SyntaxKind::RBrace) { break; }
    }
    let end_tok = match parser.tokens.expect(SyntaxKind::RBrace, &mut parser.errors) { Ok(t) => t, Err(_) => return None };
    Some(ExprNode { kind: ExprKind::Match { scrutinee: Box::new(scrutinee), arms }, span: Span::new(start, end_tok.end) })
}
