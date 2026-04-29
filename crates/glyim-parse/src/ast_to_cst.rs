use crate::ast::{Ast, BinOp, BlockItem, ExprKind, ExprNode, Item, StmtKind, UnOp};
use crate::cst_builder::CstBuilder;
use glyim_syntax::{GreenNode, SyntaxKind, SyntaxNode};

fn binop_kind(op: BinOp) -> SyntaxKind {
    match op {
        BinOp::Add => SyntaxKind::Plus, BinOp::Sub => SyntaxKind::Minus, BinOp::Mul => SyntaxKind::Star,
        BinOp::Div => SyntaxKind::Slash, BinOp::Mod => SyntaxKind::Percent,
        BinOp::Eq => SyntaxKind::EqEq, BinOp::Neq => SyntaxKind::BangEq,
        BinOp::Lt => SyntaxKind::Lt, BinOp::Gt => SyntaxKind::Gt,
        BinOp::Lte => SyntaxKind::LtEq, BinOp::Gte => SyntaxKind::GtEq,
        BinOp::And => SyntaxKind::AmpAmp, BinOp::Or => SyntaxKind::PipePipe,
    }
}
fn binop_text(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+", BinOp::Sub => "-", BinOp::Mul => "*", BinOp::Div => "/", BinOp::Mod => "%",
        BinOp::Eq => "==", BinOp::Neq => "!=", BinOp::Lt => "<", BinOp::Gt => ">",
        BinOp::Lte => "<=", BinOp::Gte => ">=", BinOp::And => "&&", BinOp::Or => "||",
    }
}
fn unop_kind(op: UnOp) -> SyntaxKind { match op { UnOp::Neg => SyntaxKind::Minus, UnOp::Not => SyntaxKind::Bang } }
fn unop_text(op: UnOp) -> &'static str { match op { UnOp::Neg => "-", UnOp::Not => "!" } }

fn ast_expr_to_cst(builder: &mut CstBuilder, expr: &ExprNode) {
    match &expr.kind {
        ExprKind::IntLit(n) => { builder.start_node(SyntaxKind::LitExpr); builder.token(SyntaxKind::IntLit, &n.to_string()); builder.finish_node(); }
        ExprKind::StrLit(s) => { builder.start_node(SyntaxKind::LitExpr); builder.token(SyntaxKind::StringLit, s); builder.finish_node(); }
        ExprKind::Ident(_) => { builder.start_node(SyntaxKind::PathExpr); builder.token(SyntaxKind::Ident, "<ident>"); builder.finish_node(); }
        ExprKind::Binary { op, lhs, rhs } => {
            builder.start_node(SyntaxKind::BinaryExpr);
            ast_expr_to_cst(builder, lhs);
            builder.token(binop_kind(op.clone()), binop_text(op.clone()));
            ast_expr_to_cst(builder, rhs);
            builder.finish_node();
        }
        ExprKind::Unary { op, operand } => {
            builder.start_node(SyntaxKind::PrefixExpr);
            builder.token(unop_kind(op.clone()), unop_text(op.clone()));
            ast_expr_to_cst(builder, operand);
            builder.finish_node();
        }
        ExprKind::Lambda { params: _, body } => {
            builder.start_node(SyntaxKind::LambdaExpr);
            builder.token(SyntaxKind::LParen, "(");
            builder.token(SyntaxKind::RParen, ")");
            builder.token(SyntaxKind::FatArrow, "=>");
            ast_expr_to_cst(builder, body);
            builder.finish_node();
        }
        ExprKind::Block(items) => {
            builder.start_node(SyntaxKind::BlockExpr);
            builder.token(SyntaxKind::LBrace, "{");
            for item in items {
                match item {
                    BlockItem::Expr(e) => ast_expr_to_cst(builder, e),
                    BlockItem::Stmt(_) => { builder.token(SyntaxKind::Ident, "<stmt>"); }
                }
            }
            builder.token(SyntaxKind::RBrace, "}");
            builder.finish_node();
        }
        ExprKind::If { condition, then_branch, else_branch } => {
            builder.start_node(SyntaxKind::SourceFile);
            builder.token(SyntaxKind::KwIf, "if");
            ast_expr_to_cst(builder, condition);
            ast_expr_to_cst(builder, then_branch);
            if let Some(else_br) = else_branch {
                builder.token(SyntaxKind::KwElse, "else");
                ast_expr_to_cst(builder, else_br);
            }
            builder.finish_node();
        }
        ExprKind::Call { callee, args } => {
            builder.start_node(SyntaxKind::CallExpr);
            ast_expr_to_cst(builder, callee);
            builder.token(SyntaxKind::LParen, "(");
            for (i, a) in args.iter().enumerate() {
                if i > 0 { builder.token(SyntaxKind::Comma, ","); }
                ast_expr_to_cst(builder, a);
            }
            builder.token(SyntaxKind::RParen, ")");
            builder.finish_node();
        }
        _ => {}
    }
}

fn ast_item_to_cst(builder: &mut CstBuilder, item: &Item) {
    match item {
        Item::Binding { value, .. } => ast_expr_to_cst(builder, value),
        Item::FnDef { body, .. } => {
            builder.start_node(SyntaxKind::FnDef);
            builder.token(SyntaxKind::KwFn, "fn");
            builder.token(SyntaxKind::Ident, "<name>");
            builder.token(SyntaxKind::LParen, "(");
            builder.token(SyntaxKind::RParen, ")");
            ast_expr_to_cst(builder, body);
            builder.finish_node();
        }
        Item::Stmt(stmt) => {
            if let StmtKind::Let { pattern: _, mutable: _, value } = &stmt.kind {
                builder.token(SyntaxKind::KwLet, "let");
                builder.token(SyntaxKind::Ident, "<name>");
                builder.token(SyntaxKind::Eq, "=");
                ast_expr_to_cst(builder, value);
            }
        }
        Item::Use(u) => { builder.token(SyntaxKind::KwUse, "use"); builder.token(SyntaxKind::Ident, &u.path); }
        _ => {}
    }
}

pub fn ast_to_green(ast: &Ast) -> GreenNode {
    let mut builder = CstBuilder::new();
    builder.start_node(SyntaxKind::SourceFile);
    for item in &ast.items { ast_item_to_cst(&mut builder, item); }
    builder.finish_node();
    let (green, _) = builder.finish();
    green
}

pub fn ast_to_cst(ast: &Ast) -> SyntaxNode {
    let green = ast_to_green(ast);
    crate::cst_builder::green_to_syntax(green)
}
