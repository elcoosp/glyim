use glyim_hir::{Hir, HirExpr, HirStmt, HirItem, HirFn};
use glyim_interner::Symbol;
use crate::config::{MutationConfig, MutationOperator};
use crate::operators;

#[derive(Debug, Clone)]
pub struct Mutation {
    pub id: u64,
    pub function_name: String,
    pub operator: MutationOperator,
    pub original: String,
    pub replacement: String,
}

pub struct MutationEngine {
    config: MutationConfig,
    next_id: u64,
}

impl MutationEngine {
    pub fn new(config: MutationConfig) -> Self {
        Self { config, next_id: 0 }
    }

    pub fn generate_mutations(&mut self, hir: &Hir) -> Vec<Mutation> {
        let mut mutations = Vec::new();
        for item in &hir.items {
            if let HirItem::Fn(f) = item {
                let fn_name = f.name;
                // Skip test functions if configured
                if self.config.skip_tests && f.is_test {
                    continue;
                }
                let mut count = 0;
                self.collect_expr_mutations(&f.body, fn_name, &mut mutations, &mut count);
                self.collect_stmt_deletions(&f.body, fn_name, &mut mutations, &mut count);
            }
        }
        mutations
    }

    fn collect_expr_mutations(
        &mut self,
        expr: &HirExpr,
        fn_name: Symbol,
        mutations: &mut Vec<Mutation>,
        count: &mut usize,
    ) {
        if *count >= self.config.max_mutations_per_fn {
            return;
        }
        for op in &self.config.operators {
            if *op == MutationOperator::StatementDeletion { continue; }
            if let Some(mutated) = operators::apply_operator(expr, *op) {
                let mutation = Mutation {
                    id: self.next_id,
                    function_name: format!("{:?}", fn_name),
                    operator: *op,
                    original: format!("{:?}", expr),
                    replacement: format!("{:?}", mutated),
                };
                self.next_id += 1;
                mutations.push(mutation);
                *count += 1;
            }
        }
        // Recurse into children
        match expr {
            HirExpr::Binary { lhs, rhs, .. } => {
                self.collect_expr_mutations(lhs, fn_name, mutations, count);
                self.collect_expr_mutations(rhs, fn_name, mutations, count);
            }
            HirExpr::Unary { operand, .. } => self.collect_expr_mutations(operand, fn_name, mutations, count),
            HirExpr::Block { stmts, .. } => {
                for stmt in stmts {
                    match stmt {
                        HirStmt::Expr(e) => self.collect_expr_mutations(e, fn_name, mutations, count),
                        HirStmt::Let { value, .. } | HirStmt::Assign { value, .. } => self.collect_expr_mutations(value, fn_name, mutations, count),
                        _ => {}
                    }
                }
            }
            HirExpr::If { condition, then_branch, else_branch, .. } => {
                self.collect_expr_mutations(condition, fn_name, mutations, count);
                self.collect_expr_mutations(then_branch, fn_name, mutations, count);
                if let Some(eb) = else_branch { self.collect_expr_mutations(eb, fn_name, mutations, count); }
            }
            HirExpr::While { condition, body, .. } => {
                self.collect_expr_mutations(condition, fn_name, mutations, count);
                self.collect_expr_mutations(body, fn_name, mutations, count);
            }
            HirExpr::ForIn { iter, body, .. } => {
                self.collect_expr_mutations(iter, fn_name, mutations, count);
                self.collect_expr_mutations(body, fn_name, mutations, count);
            }
            HirExpr::Match { scrutinee, arms, .. } => {
                self.collect_expr_mutations(scrutinee, fn_name, mutations, count);
                for arm in arms { self.collect_expr_mutations(&arm.body, fn_name, mutations, count); }
            }
            HirExpr::Call { args, .. } => for a in args { self.collect_expr_mutations(a, fn_name, mutations, count); }
            HirExpr::StructLit { fields, .. } => for (_, v) in fields { self.collect_expr_mutations(v, fn_name, mutations, count); }
            HirExpr::EnumVariant { args, .. } | HirExpr::TupleLit { elements: args, .. } => for a in args { self.collect_expr_mutations(a, fn_name, mutations, count); }
            HirExpr::Println { arg, .. } => self.collect_expr_mutations(arg, fn_name, mutations, count),
            HirExpr::Assert { condition, message, .. } => {
                self.collect_expr_mutations(condition, fn_name, mutations, count);
                if let Some(m) = message { self.collect_expr_mutations(m, fn_name, mutations, count); }
            }
            HirExpr::As { expr, .. } => self.collect_expr_mutations(expr, fn_name, mutations, count),
            HirExpr::Deref { expr, .. } => self.collect_expr_mutations(expr, fn_name, mutations, count),
            HirExpr::Return { value, .. } => if let Some(v) = value { self.collect_expr_mutations(v, fn_name, mutations, count); }
            _ => {}
        }
    }

    fn collect_stmt_deletions(
        &mut self,
        expr: &HirExpr,
        fn_name: Symbol,
        mutations: &mut Vec<Mutation>,
        count: &mut usize,
    ) {
        if *count >= self.config.max_mutations_per_fn { return; }
        if self.config.operators.contains(&MutationOperator::StatementDeletion) {
            if let HirExpr::Block { stmts, .. } = expr {
                for (i, stmt) in stmts.iter().enumerate() {
                    if *count >= self.config.max_mutations_per_fn { break; }
                    if matches!(stmt, HirStmt::Expr(_) | HirStmt::Let { .. }) {
                        // We can mutate by removing this statement
                        let mutation = Mutation {
                            id: self.next_id,
                            function_name: format!("{:?}", fn_name),
                            operator: MutationOperator::StatementDeletion,
                            original: format!("stmt {}", i),
                            replacement: "(removed)".to_string(),
                        };
                        self.next_id += 1;
                        mutations.push(mutation);
                        *count += 1;
                    }
                }
            }
        }
    }
}
