//! Lower parsed AST to HIR.

use glyim_parse::{BinOp, ExprKind, Item, UnOp};
use crate::{Hir, HirBinOp, HirExpr, HirFn, HirUnOp};

pub fn lower(ast: &glyim_parse::Ast) -> Hir {
    let mut fns = vec![];
    for item in &ast.items {
        match item {
            Item::Binding { name, value, .. } => {
                if let ExprKind::Lambda { params, body } = &value.kind {
                    fns.push(HirFn {
                        name: *name,
                        params: params.clone(),
                        body: lower_expr(&body.kind),
                    });
                }
            }
            Item::FnDef { name, params, body, .. } => {
                let param_syms: Vec<_> = params.iter().map(|(sym, _)| *sym).collect();
                fns.push(HirFn {
                    name: *name,
                    params: param_syms,
                    body: lower_expr(&body.kind),
                });
            }
        }
    }
    Hir { fns }
}

fn lower_expr(expr: &ExprKind) -> HirExpr {
    match expr {
        ExprKind::IntLit(n) => HirExpr::IntLit(*n),
        ExprKind::Ident(sym) => HirExpr::Ident(*sym),
        ExprKind::Binary { op, lhs, rhs } => HirExpr::Binary {
            op: lower_binop(op.clone()),
            lhs: Box::new(lower_expr(&lhs.kind)),
            rhs: Box::new(lower_expr(&rhs.kind)),
        },
        ExprKind::Unary { op, operand } => HirExpr::Unary {
            op: lower_unop(op.clone()),
            operand: Box::new(lower_expr(&operand.kind)),
        },
        ExprKind::Lambda { params: _, body } => lower_expr(&body.kind), // flatten nested lambda
        ExprKind::Block(exprs) => {
            HirExpr::Block(exprs.iter().map(|e| lower_expr(&e.kind)).collect())
        }
        ExprKind::Call { .. } => HirExpr::IntLit(0), // calls not supported in v0.1.0
    }
}

fn lower_binop(op: BinOp) -> HirBinOp {
    match op {
        BinOp::Add => HirBinOp::Add, BinOp::Sub => HirBinOp::Sub,
        BinOp::Mul => HirBinOp::Mul, BinOp::Div => HirBinOp::Div,
        BinOp::Mod => HirBinOp::Mod, BinOp::Eq => HirBinOp::Eq,
        BinOp::Neq => HirBinOp::Neq, BinOp::Lt => HirBinOp::Lt,
        BinOp::Gt => HirBinOp::Gt, BinOp::Lte => HirBinOp::Lte,
        BinOp::Gte => HirBinOp::Gte, BinOp::And => HirBinOp::And,
        BinOp::Or => HirBinOp::Or,
    }
}

fn lower_unop(op: UnOp) -> HirUnOp {
    match op { UnOp::Neg => HirUnOp::Neg, UnOp::Not => HirUnOp::Not }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_parse::parse;

    fn lower_source(source: &str) -> Hir {
        let out = parse(source);
        assert!(out.errors.is_empty());
        lower(&out.ast)
    }

    #[test] fn lower_main_lambda() {
        let hir = lower_source("main = () => 42");
        assert_eq!(hir.fns.len(), 1);
        assert_eq!(hir.fns[0].body, HirExpr::IntLit(42));
    }
    #[test] fn lower_fn_def() {
        let hir = lower_source("fn main() { 99 }");
        assert!(matches!(&hir.fns[0].body, HirExpr::Block(e) if e.len()==1 && e[0]==HirExpr::IntLit(99)));
    }
    #[test] fn lower_binary() {
        let hir = lower_source("main = () => 1 + 2");
        assert!(matches!(&hir.fns[0].body, HirExpr::Binary { op: HirBinOp::Add, .. }));
    }
    #[test] fn skip_non_lambda_binding() {
        let hir = lower_source("x = 42");
        assert!(hir.fns.is_empty());
    }
}
