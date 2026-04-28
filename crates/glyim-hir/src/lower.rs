use glyim_interner::Interner;
use glyim_parse::{BinOp, BlockItem, ExprKind, Item, StmtKind, UnOp};
use crate::{Hir, HirBinOp, HirExpr, HirFn, HirStmt, HirUnOp};

pub fn lower(ast: &glyim_parse::Ast, interner: &Interner) -> Hir {
    let mut fns = vec![];
    for item in &ast.items {
        match item {
            Item::Binding { name, value, .. } => {
                if let ExprKind::Lambda { params, body } = &value.kind {
                    fns.push(HirFn { name: *name, params: params.clone(), body: lower_expr(&body.kind, interner) });
                }
            }
            Item::FnDef { name, params, body, .. } => {
                let param_syms: Vec<_> = params.iter().map(|(sym,_)| *sym).collect();
                fns.push(HirFn { name: *name, params: param_syms, body: lower_expr(&body.kind, interner) });
            }
            Item::Stmt(_stmt) => {
                // Top-level let statement -> wrap in a main function if not already present? For now ignore.
            }
            Item::Use(_) => {} // No-op
        }
    }
    Hir { fns }
}

fn lower_expr(expr: &ExprKind, interner: &Interner) -> HirExpr {
    match expr {
        ExprKind::IntLit(n) => HirExpr::IntLit(*n),
        ExprKind::StrLit(s) => HirExpr::StrLit(s.clone()),
        ExprKind::Ident(sym) => HirExpr::Ident(*sym),
        ExprKind::Binary { op, lhs, rhs } => HirExpr::Binary {
            op: lower_binop(op.clone()),
            lhs: Box::new(lower_expr(&lhs.kind, interner)),
            rhs: Box::new(lower_expr(&rhs.kind, interner)),
        },
        ExprKind::Unary { op, operand } => HirExpr::Unary {
            op: lower_unop(op.clone()),
            operand: Box::new(lower_expr(&operand.kind, interner)),
        },
        ExprKind::Lambda { params: _, body } => lower_expr(&body.kind, interner),
        ExprKind::Block(items) => {
            let stmts: Vec<HirStmt> = items.iter().map(|item| match item {
                BlockItem::Expr(e) => HirStmt::Expr(lower_expr(&e.kind, interner)),
                BlockItem::Stmt(s) => match &s.kind {
                    StmtKind::Let { name, mutable, value } => HirStmt::Let {
                        name: *name, mutable: *mutable, value: lower_expr(&value.kind, interner),
                    },
                    StmtKind::Assign { target, value } => HirStmt::Assign {
                        target: *target, value: lower_expr(&value.kind, interner),
                    },
                },
            }).collect();
            HirExpr::Block(stmts)
        }
        ExprKind::If { condition, then_branch, else_branch } => HirExpr::If {
            condition: Box::new(lower_expr(&condition.kind, interner)),
            then_branch: Box::new(lower_expr(&then_branch.kind, interner)),
            else_branch: else_branch.as_ref().map(|e| Box::new(lower_expr(&e.kind, interner))),
        },
        ExprKind::Call { callee, args } => {
            if let ExprKind::Ident(sym) = &callee.kind {
                let name = interner.resolve(*sym);
                match name {
                    "println" => {
                        let arg = args.first().map(|a| lower_expr(&a.kind, interner)).unwrap_or(HirExpr::IntLit(0));
                        return HirExpr::Println(Box::new(arg));
                    }
                    "assert" => {
                        let cond = args.first().map(|a| lower_expr(&a.kind, interner)).unwrap_or(HirExpr::IntLit(0));
                        let msg = args.get(1).map(|a| lower_expr(&a.kind, interner)).map(Box::new);
                        return HirExpr::Assert { condition: Box::new(cond), message: msg };
                    }
                    _ => {}
                }
            }
            HirExpr::IntLit(0) // fallback for non-builtin calls
        }
    }
}

fn lower_binop(op: BinOp) -> HirBinOp {
    match op {
        BinOp::Add => HirBinOp::Add, BinOp::Sub => HirBinOp::Sub, BinOp::Mul => HirBinOp::Mul,
        BinOp::Div => HirBinOp::Div, BinOp::Mod => HirBinOp::Mod, BinOp::Eq => HirBinOp::Eq,
        BinOp::Neq => HirBinOp::Neq, BinOp::Lt => HirBinOp::Lt, BinOp::Gt => HirBinOp::Gt,
        BinOp::Lte => HirBinOp::Lte, BinOp::Gte => HirBinOp::Gte, BinOp::And => HirBinOp::And,
        BinOp::Or => HirBinOp::Or,
    }
}
fn lower_unop(op: UnOp) -> HirUnOp { match op { UnOp::Neg => HirUnOp::Neg, UnOp::Not => HirUnOp::Not } }
