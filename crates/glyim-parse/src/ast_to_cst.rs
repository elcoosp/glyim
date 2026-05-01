use crate::ast::*;
use crate::cst_builder::CstBuilder;
use glyim_syntax::{GreenNode, SyntaxKind, SyntaxNode};

// ── Operator helpers (unchanged from original) ─────────────────────
fn binop_kind(op: BinOp) -> SyntaxKind {
    match op {
        BinOp::Add => SyntaxKind::Plus,
        BinOp::Sub => SyntaxKind::Minus,
        BinOp::Mul => SyntaxKind::Star,
        BinOp::Div => SyntaxKind::Slash,
        BinOp::Mod => SyntaxKind::Percent,
        BinOp::Eq => SyntaxKind::EqEq,
        BinOp::Neq => SyntaxKind::BangEq,
        BinOp::Lt => SyntaxKind::Lt,
        BinOp::Gt => SyntaxKind::Gt,
        BinOp::Lte => SyntaxKind::LtEq,
        BinOp::Gte => SyntaxKind::GtEq,
        BinOp::And => SyntaxKind::AmpAmp,
        BinOp::Or => SyntaxKind::PipePipe,
    }
}
fn binop_text(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::Eq => "==",
        BinOp::Neq => "!=",
        BinOp::Lt => "<",
        BinOp::Gt => ">",
        BinOp::Lte => "<=",
        BinOp::Gte => ">=",
        BinOp::And => "&&",
        BinOp::Or => "||",
    }
}
fn unop_kind(op: UnOp) -> SyntaxKind {
    match op {
        UnOp::Neg => SyntaxKind::Minus,
        UnOp::Not => SyntaxKind::Bang,
    }
}
fn unop_text(op: UnOp) -> &'static str {
    match op {
        UnOp::Neg => "-",
        UnOp::Not => "!",
    }
}

// ── Expression → CST ────────────────────────────────────────────────
fn ast_expr_to_cst(builder: &mut CstBuilder, expr: &ExprNode) {
    match &expr.kind {
        ExprKind::IntLit(n) => {
            builder.start_node(SyntaxKind::LitExpr);
            builder.token(SyntaxKind::IntLit, &n.to_string());
            builder.finish_node();
        }
        ExprKind::FloatLit(f) => {
            builder.start_node(SyntaxKind::LitExpr);
            builder.token(SyntaxKind::FloatLit, &f.to_string());
            builder.finish_node();
        }
        ExprKind::BoolLit(b) => {
            builder.start_node(SyntaxKind::LitExpr);
            builder.token(
                if *b {
                    SyntaxKind::KwTrue
                } else {
                    SyntaxKind::KwFalse
                },
                if *b { "true" } else { "false" },
            );
            builder.finish_node();
        }
        ExprKind::StrLit(s) => {
            builder.start_node(SyntaxKind::LitExpr);
            builder.token(SyntaxKind::StringLit, s);
            builder.finish_node();
        }
        ExprKind::UnitLit => {
            builder.start_node(SyntaxKind::LitExpr);
            builder.token(SyntaxKind::LParen, "(");
            builder.token(SyntaxKind::RParen, ")");
            builder.finish_node();
        }
        ExprKind::Ident(_) => {
            builder.start_node(SyntaxKind::PathExpr);
            builder.token(SyntaxKind::Ident, "<ident>");
            builder.finish_node();
        }
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
                    BlockItem::Stmt(s) => ast_stmt_to_cst(builder, s),
                }
            }
            builder.token(SyntaxKind::RBrace, "}");
            builder.finish_node();
        }
        ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            builder.start_node(SyntaxKind::IfExpr);
            builder.token(SyntaxKind::KwIf, "if");
            ast_expr_to_cst(builder, condition);
            ast_expr_to_cst(builder, then_branch);
            if let Some(e) = else_branch {
                builder.token(SyntaxKind::KwElse, "else");
                ast_expr_to_cst(builder, e);
            }
            builder.finish_node();
        }
        ExprKind::Call { callee, args } => {
            builder.start_node(SyntaxKind::CallExpr);
            ast_expr_to_cst(builder, callee);
            builder.token(SyntaxKind::LParen, "(");
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    builder.token(SyntaxKind::Comma, ",");
                }
                ast_expr_to_cst(builder, a);
            }
            builder.token(SyntaxKind::RParen, ")");
            builder.finish_node();
        }
        ExprKind::StructLit { name: _, fields } => {
            builder.start_node(SyntaxKind::StructLitExpr);
            for (_, fe) in fields {
                ast_expr_to_cst(builder, fe);
            }
            builder.finish_node();
        }
        ExprKind::EnumVariant { .. } => {
            builder.start_node(SyntaxKind::EnumVariantExpr);
            builder.token(SyntaxKind::Ident, "<variant>");
            builder.finish_node();
        }
        ExprKind::SomeExpr(e) | ExprKind::OkExpr(e) | ExprKind::ErrExpr(e) => {
            builder.start_node(SyntaxKind::EnumVariantExpr);
            ast_expr_to_cst(builder, e);
            builder.finish_node();
        }
        ExprKind::NoneExpr => {
            builder.start_node(SyntaxKind::EnumVariantExpr);
            builder.finish_node();
        }
        ExprKind::TryExpr(e) => {
            builder.start_node(SyntaxKind::TryExpr);
            ast_expr_to_cst(builder, e);
            builder.token(SyntaxKind::Question, "?");
            builder.finish_node();
        }
        ExprKind::As { expr, .. } => {
            builder.start_node(SyntaxKind::AsExpr);
            ast_expr_to_cst(builder, expr);
            builder.token(SyntaxKind::KwAs, "as");
            builder.finish_node();
        }
        ExprKind::MacroCall { .. } => {
            builder.token(SyntaxKind::At, "@");
            builder.token(SyntaxKind::Ident, "<macro>");
        }
        ExprKind::Match { .. } => {
            builder.start_node(SyntaxKind::MatchExpr);
            builder.token(SyntaxKind::KwMatch, "match");
            builder.finish_node();
        }
        ExprKind::FieldAccess { .. } => {
            builder.start_node(SyntaxKind::FieldAccessExpr);
            builder.token(SyntaxKind::Dot, ".");
            builder.finish_node();
        }
        ExprKind::SizeOf(_) => {
            builder.token(SyntaxKind::Ident, "__size_of");
        }
        ExprKind::TupleLit(elems) => {
            builder.start_node(SyntaxKind::TupleLitExpr);
            for e in elems {
                ast_expr_to_cst(builder, e);
            }
            builder.finish_node();
        }
        ExprKind::Pointer { .. } => {
            builder.token(SyntaxKind::Star, "*");
        }
        // New variants: just emit a placeholder
        ExprKind::MethodCall { .. } => {
            builder.start_node(SyntaxKind::CallExpr);
            builder.token(SyntaxKind::Ident, "<method>");
            builder.finish_node();
        }
        ExprKind::ForIn {
            pattern: _,
            iter,
            body,
        } => {
            builder.start_node(glyim_syntax::SyntaxKind::ForInExpr);
            builder.token(glyim_syntax::SyntaxKind::KwFor, "for");
            ast_expr_to_cst(builder, iter);
            ast_expr_to_cst(builder, body);
            builder.finish_node();
        }
        ExprKind::While { condition, body } => {
            builder.start_node(SyntaxKind::WhileExpr);
            builder.token(SyntaxKind::KwWhile, "while");
            ast_expr_to_cst(builder, condition);
            ast_expr_to_cst(builder, body);
            builder.finish_node();
        }
        ExprKind::Deref(_) => {
            builder.start_node(SyntaxKind::PrefixExpr);
            builder.token(SyntaxKind::Star, "*");
            builder.finish_node();
        }
    }
}

// ── Statement → CST ─────────────────────────────────────────────────
fn ast_stmt_to_cst(builder: &mut CstBuilder, stmt: &StmtNode) {
    match &stmt.kind {
        StmtKind::Let {
            pattern: _,
            mutable: _,
            value,
            ty: _,
        } => {
            builder.start_node(SyntaxKind::LetStmt);
            builder.token(SyntaxKind::KwLet, "let");
            builder.token(SyntaxKind::Ident, "<name>");
            builder.token(SyntaxKind::Eq, "=");
            ast_expr_to_cst(builder, value);
            builder.finish_node();
        }
        StmtKind::Assign { target: _, value } => {
            builder.start_node(SyntaxKind::AssignStmt);
            ast_expr_to_cst(builder, value);
            builder.finish_node();
        }
        StmtKind::AssignDeref { target, value } => {
            builder.start_node(SyntaxKind::AssignStmt);
            ast_expr_to_cst(builder, target);
            builder.token(SyntaxKind::Eq, "=");
            ast_expr_to_cst(builder, value);
            builder.finish_node();
        }
        StmtKind::AssignField { object, value, .. } => {
            builder.start_node(SyntaxKind::AssignStmt);
            ast_expr_to_cst(builder, object);
            builder.token(SyntaxKind::Eq, "=");
            ast_expr_to_cst(builder, value);
            builder.finish_node();
        }
    }
}

// ── Item → CST ──────────────────────────────────────────────────────
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
        Item::Stmt(stmt) => ast_stmt_to_cst(builder, stmt),
        Item::Use(u) => {
            builder.token(SyntaxKind::KwUse, "use");
            builder.token(SyntaxKind::Ident, &u.path);
        }
        Item::StructDef { .. } => {
            builder.start_node(SyntaxKind::StructDef);
            builder.token(SyntaxKind::KwStruct, "struct");
            builder.token(SyntaxKind::Ident, "<name>");
            builder.finish_node();
        }
        Item::EnumDef { .. } => {
            builder.start_node(SyntaxKind::EnumDef);
            builder.token(SyntaxKind::KwEnum, "enum");
            builder.token(SyntaxKind::Ident, "<name>");
            builder.finish_node();
        }
        Item::ImplBlock { .. } => {
            builder.token(SyntaxKind::KwImpl, "impl");
            builder.token(SyntaxKind::Ident, "<name>");
        }
        Item::MacroDef { .. } => {
            builder.token(SyntaxKind::At, "@");
            builder.token(SyntaxKind::Ident, "<macro>");
        }
        Item::ExternBlock { .. } => {
            builder.start_node(SyntaxKind::ExternBlock);
            builder.token(SyntaxKind::KwExtern, "extern");
            builder.finish_node();
        }
    }
}

// ── Public API (unchanged) ──────────────────────────────────────────
pub fn ast_to_green(ast: &Ast) -> GreenNode {
    let mut builder = CstBuilder::new();
    builder.start_node(SyntaxKind::SourceFile);
    for item in &ast.items {
        ast_item_to_cst(&mut builder, item);
    }
    builder.finish_node();
    let (green, _) = builder.finish();
    green
}

pub fn ast_to_cst(ast: &Ast) -> SyntaxNode {
    let green = ast_to_green(ast);
    crate::cst_builder::green_to_syntax(green)
}
