use glyim_diag::{Span, Severity};
use glyim_hir::Hir;
use glyim_hir::node::{HirExpr, HirStmt};
use glyim_interner::Interner;
use glyim_interner::Symbol;
use std::collections::{HashMap, HashSet};

/// Unique identifier for a lint rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct LintId(pub &'static str);

/// Describes a lint rule.
#[derive(Debug, Clone)]
pub struct LintDescriptor {
    pub id: LintId,
    pub name: &'static str,
    pub description: &'static str,
    pub default_severity: Severity,
    pub allow_attribute: bool,
    pub group: LintGroup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintGroup {
    Correctness,
    Style,
    Complexity,
    Performance,
    Deprecated,
}

/// A single lint diagnostic.
#[derive(Debug, Clone)]
pub struct LintDiagnostic {
    pub lint_id: LintId,
    pub severity: Severity,
    pub span: Span,
    pub message: String,
    pub suggestion: Option<LintSuggestion>,
}

#[derive(Debug, Clone)]
pub struct LintSuggestion {
    pub replacement: String,
    pub span: Span,
    pub label: String,
}

/// The lint registry, containing all registered lints.
pub struct LintRegistry {
    lints: HashMap<LintId, LintDescriptor>,
}

impl LintRegistry {
    pub fn new() -> Self {
        let mut registry = Self { lints: HashMap::new() };

        registry.register(LintDescriptor {
            id: LintId("unused_function"),
            name: "Unused Function",
            description: "Function is defined but never called",
            default_severity: Severity::Warning,
            allow_attribute: true,
            group: LintGroup::Correctness,
        });
        registry.register(LintDescriptor {
            id: LintId("unused_variable"),
            name: "Unused Variable",
            description: "Variable is bound but never used",
            default_severity: Severity::Warning,
            allow_attribute: true,
            group: LintGroup::Correctness,
        });
        registry.register(LintDescriptor {
            id: LintId("unnecessary_mut"),
            name: "Unnecessary Mutability",
            description: "Variable is declared mutable but never reassigned",
            default_severity: Severity::Warning,
            allow_attribute: true,
            group: LintGroup::Style,
        });
        registry.register(LintDescriptor {
            id: LintId("dead_code"),
            name: "Dead Code",
            description: "Code is unreachable or after a return/break",
            default_severity: Severity::Warning,
            allow_attribute: true,
            group: LintGroup::Correctness,
        });

        registry
    }

    pub fn register(&mut self, descriptor: LintDescriptor) {
        self.lints.insert(descriptor.id, descriptor);
    }

    pub fn get(&self, id: LintId) -> Option<&LintDescriptor> {
        self.lints.get(&id)
    }

    pub fn all_lints(&self) -> impl Iterator<Item = &LintDescriptor> {
        self.lints.values()
    }
}

impl Default for LintRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Run all lints on the given HIR and return diagnostics.
pub fn lint(hir: &Hir, interner: &Interner, registry: &LintRegistry) -> Vec<LintDiagnostic> {
    let mut diags = Vec::new();

    // unused_function lint
    if registry.get(LintId("unused_function")).is_some() {
        let called = collect_called_symbols(hir);
        for item in &hir.items {
            if let glyim_hir::item::HirItem::Fn(f) = item {
                if !called.contains(&f.name) && interner.resolve(f.name) != "main" {
                    diags.push(LintDiagnostic {
                        lint_id: LintId("unused_function"),
                        severity: Severity::Warning,
                        span: f.span,
                        message: format!(
                            "function `{}` is never called",
                            interner.resolve(f.name)
                        ),
                        suggestion: None,
                    });
                }
            }
        }
    }

    // unused_variable lint (runs on each function body)
    if registry.get(LintId("unused_variable")).is_some() {
        for item in &hir.items {
            if let glyim_hir::item::HirItem::Fn(f) = item {
                diags.extend(check_unused_variables(&f.body, interner, &f.params.iter().map(|(s,_)| *s).collect::<Vec<_>>()));
            }
        }
    }

    // unnecessary_mut lint (runs on each function body)
    if registry.get(LintId("unnecessary_mut")).is_some() {
        for item in &hir.items {
            if let glyim_hir::item::HirItem::Fn(f) = item {
                diags.extend(check_unnecessary_mut(&f.body, interner));
            }
        }
    }

    // dead_code lint (detect unreachable code after return/break)
    if registry.get(LintId("dead_code")).is_some() {
        for item in &hir.items {
            if let glyim_hir::item::HirItem::Fn(f) = item {
                diags.extend(check_dead_code(&f.body, interner));
            }
        }
    }

    diags
}

// ----- unused_variable helpers -----

fn check_unused_variables(expr: &HirExpr, interner: &Interner, params: &[Symbol]) -> Vec<LintDiagnostic> {
    let mut used = HashSet::new();
    let mut declared: Vec<(Symbol, Span, bool)> = vec![]; // (sym, span, is_mutable)
    collect_used_symbols(expr, &mut used);
    collect_decls(expr, &mut declared, params);

    let mut diags = vec![];
    for (sym, span, _mutable) in declared {
        if !used.contains(&sym) && !params.contains(&sym) {
            diags.push(LintDiagnostic {
                lint_id: LintId("unused_variable"),
                severity: Severity::Warning,
                span,
                message: format!("unused variable `{}`", interner.resolve(sym)),
                suggestion: None,
            });
        }
    }
    diags
}

fn collect_decls(expr: &HirExpr, decls: &mut Vec<(Symbol, Span, bool)>, params: &[Symbol]) {
    match expr {
        HirExpr::Block { stmts, .. } => {
            for stmt in stmts {
                match stmt {
                    HirStmt::Let { name, mutable, value, span } => {
                        if !params.contains(name) {
                            decls.push((*name, *span, *mutable));
                        }
                        collect_decls(value, decls, params);
                    }
                    HirStmt::LetPat { pattern, mutable, value, span, .. } => {
                        // for simplicity, if pattern is a simple Var, treat like let
                        if let glyim_hir::HirPattern::Var(sym) = pattern {
                            if !params.contains(sym) {
                                decls.push((*sym, *span, *mutable));
                            }
                        }
                        collect_decls(value, decls, params);
                    }
                    HirStmt::Expr(e) => collect_decls(e, decls, params),
                    _ => {}
                }
            }
        }
        HirExpr::If { condition, then_branch, else_branch, .. } => {
            collect_decls(condition, decls, params);
            collect_decls(then_branch, decls, params);
            if let Some(e) = else_branch { collect_decls(e, decls, params); }
        }
        HirExpr::Match { scrutinee, arms, .. } => {
            collect_decls(scrutinee, decls, params);
            for arm in arms { collect_decls(&arm.body, decls, params); }
        }
        HirExpr::While { condition, body, .. } => {
            collect_decls(condition, decls, params);
            collect_decls(body, decls, params);
        }
        HirExpr::ForIn { iter, body, .. } => {
            collect_decls(iter, decls, params);
            collect_decls(body, decls, params);
        }
        _ => {}
    }
}

fn collect_used_symbols(expr: &HirExpr, used: &mut HashSet<Symbol>) {
    match expr {
        HirExpr::Ident { name, .. } => { used.insert(*name); }
        HirExpr::Binary { lhs, rhs, .. } => { collect_used_symbols(lhs, used); collect_used_symbols(rhs, used); }
        HirExpr::Unary { operand, .. } => { collect_used_symbols(operand, used); }
        HirExpr::Block { stmts, .. } => {
            for stmt in stmts {
                match stmt {
                    HirStmt::Expr(e) => collect_used_symbols(e, used),
                    HirStmt::Let { value, .. } | HirStmt::LetPat { value, .. }
                    | HirStmt::Assign { value, .. } | HirStmt::AssignDeref { value, .. }
                    | HirStmt::AssignField { value, .. } => collect_used_symbols(value, used),
                }
            }
        }
        HirExpr::If { condition, then_branch, else_branch, .. } => {
            collect_used_symbols(condition, used);
            collect_used_symbols(then_branch, used);
            if let Some(e) = else_branch { collect_used_symbols(e, used); }
        }
        HirExpr::Match { scrutinee, arms, .. } => {
            collect_used_symbols(scrutinee, used);
            for arm in arms { collect_used_symbols(&arm.body, used); }
        }
        HirExpr::While { condition, body, .. } => {
            collect_used_symbols(condition, used);
            collect_used_symbols(body, used);
        }
        HirExpr::ForIn { iter, body, .. } => {
            collect_used_symbols(iter, used);
            collect_used_symbols(body, used);
        }
        HirExpr::Call { args, .. } => {
            for a in args { collect_used_symbols(a, used); }
        }
        HirExpr::MethodCall { receiver, args, .. } => {
            collect_used_symbols(receiver, used);
            for a in args { collect_used_symbols(a, used); }
        }
        HirExpr::StructLit { fields, .. } => {
            for (_, v) in fields { collect_used_symbols(v, used); }
        }
        HirExpr::EnumVariant { args, .. } | HirExpr::TupleLit { elements: args, .. } => {
            for a in args { collect_used_symbols(a, used); }
        }
        HirExpr::Return { value: Some(v), .. } => { collect_used_symbols(v, used); }
        _ => {}
    }
}

// ----- unnecessary_mut helpers -----

fn check_unnecessary_mut(expr: &HirExpr, interner: &Interner) -> Vec<LintDiagnostic> {
    let mut diags = vec![];
    // collect all mutable declarations and check if they are reassigned
    let mut mutables: Vec<(Symbol, Span)> = vec![];
    let mut assigned: HashSet<Symbol> = HashSet::new();
    collect_mut_decls(expr, &mut mutables);
    collect_assign_targets(expr, &mut assigned);
    for (sym, span) in mutables {
        if !assigned.contains(&sym) {
            diags.push(LintDiagnostic {
                lint_id: LintId("unnecessary_mut"),
                severity: Severity::Warning,
                span,
                message: format!(
                    "variable `{}` is declared mutable but never mutated",
                    interner.resolve(sym)
                ),
                suggestion: None,
            });
        }
    }
    diags
}

fn collect_mut_decls(expr: &HirExpr, muts: &mut Vec<(Symbol, Span)>) {
    match expr {
        HirExpr::Block { stmts, .. } => {
            for stmt in stmts {
                match stmt {
                    HirStmt::Let { name, mutable: true, span, value, .. } => {
                        muts.push((*name, *span));
                        collect_mut_decls(value, muts);
                    }
                    HirStmt::LetPat { mutable: true, pattern, span, value, .. } => {
                        if let glyim_hir::HirPattern::Var(sym) = pattern {
                            muts.push((*sym, *span));
                        }
                        collect_mut_decls(value, muts);
                    }
                    HirStmt::Expr(e) => collect_mut_decls(e, muts),
                    _ => {}
                }
            }
        }
        HirExpr::If { condition, then_branch, else_branch, .. } => {
            collect_mut_decls(condition, muts);
            collect_mut_decls(then_branch, muts);
            if let Some(e) = else_branch { collect_mut_decls(e, muts); }
        }
        HirExpr::Match { scrutinee, arms, .. } => {
            collect_mut_decls(scrutinee, muts);
            for arm in arms { collect_mut_decls(&arm.body, muts); }
        }
        HirExpr::While { condition, body, .. } => {
            collect_mut_decls(condition, muts);
            collect_mut_decls(body, muts);
        }
        HirExpr::ForIn { iter, body, .. } => {
            collect_mut_decls(iter, muts);
            collect_mut_decls(body, muts);
        }
        _ => {}
    }
}

fn collect_assign_targets(expr: &HirExpr, targets: &mut HashSet<Symbol>) {
    match expr {
        HirExpr::Block { stmts, .. } => {
            for stmt in stmts {
                match stmt {
                    HirStmt::Assign { target, .. } => { targets.insert(*target); }
                    HirStmt::Expr(e) => collect_assign_targets(e, targets),
                    _ => {}
                }
            }
        }
        HirExpr::If { condition, then_branch, else_branch, .. } => {
            collect_assign_targets(condition, targets);
            collect_assign_targets(then_branch, targets);
            if let Some(e) = else_branch { collect_assign_targets(e, targets); }
        }
        HirExpr::Match { scrutinee, arms, .. } => {
            collect_assign_targets(scrutinee, targets);
            for arm in arms { collect_assign_targets(&arm.body, targets); }
        }
        HirExpr::While { condition, body, .. } => {
            collect_assign_targets(condition, targets);
            collect_assign_targets(body, targets);
        }
        HirExpr::ForIn { iter, body, .. } => {
            collect_assign_targets(iter, targets);
            collect_assign_targets(body, targets);
        }
        _ => {}
    }
}

// ----- called symbols (unused_function) -----
fn collect_called_symbols(hir: &Hir) -> HashSet<Symbol> {
    let mut called = HashSet::new();

    fn walk(expr: &HirExpr, called: &mut HashSet<Symbol>) {
        match expr {
            HirExpr::Call { callee, args, .. } => {
                called.insert(*callee);
                for a in args { walk(a, called); }
            }
            HirExpr::Block { stmts, .. } => {
                for s in stmts {
                    match s {
                        HirStmt::Expr(e) => walk(e, called),
                        HirStmt::Let { value, .. } | HirStmt::LetPat { value, .. }
                        | HirStmt::Assign { value, .. } | HirStmt::AssignDeref { value, .. }
                        | HirStmt::AssignField { value, .. } => walk(value, called),
                    }
                }
            }
            HirExpr::If { condition, then_branch, else_branch, .. } => {
                walk(condition, called);
                walk(then_branch, called);
                if let Some(e) = else_branch { walk(e, called); }
            }
            HirExpr::Match { scrutinee, arms, .. } => {
                walk(scrutinee, called);
                for arm in arms { walk(&arm.body, called); }
            }
            HirExpr::While { condition, body, .. } => {
                walk(condition, called);
                walk(body, called);
            }
            HirExpr::ForIn { iter, body, .. } => {
                walk(iter, called);
                walk(body, called);
            }
            HirExpr::Binary { lhs, rhs, .. } => {
                walk(lhs, called); walk(rhs, called);
            }
            HirExpr::Unary { operand, .. } | HirExpr::Deref { expr: operand, .. }
            | HirExpr::As { expr: operand, .. } => walk(operand, called),
            HirExpr::Return { value: Some(v), .. } => walk(v, called),
            HirExpr::MethodCall { receiver, args, .. } => {
                walk(receiver, called);
                for a in args { walk(a, called); }
            }
            HirExpr::StructLit { fields, .. } => {
                for (_, v) in fields { walk(v, called); }
            }
            HirExpr::EnumVariant { args, .. } | HirExpr::TupleLit { elements: args, .. } => {
                for a in args { walk(a, called); }
            }
            _ => {}
        }
    }

    for item in &hir.items {
        match item {
            glyim_hir::item::HirItem::Fn(f) => walk(&f.body, &mut called),
            glyim_hir::item::HirItem::Impl(imp) => {
                for m in &imp.methods { walk(&m.body, &mut called); }
            }
            _ => {}
        }
    }

    called
}



// ----- dead_code helpers -----

fn check_dead_code(expr: &HirExpr, interner: &Interner) -> Vec<LintDiagnostic> {
    let mut diags = vec![];
    find_unreachable_after_stmt(expr, interner, &mut diags);
    diags
}

fn find_unreachable_after_stmt(expr: &HirExpr, interner: &Interner, diags: &mut Vec<LintDiagnostic>) {
    match expr {
        HirExpr::Block { stmts, span, .. } => {
            let mut found_terminal = false;
            for (_i, stmt) in stmts.iter().enumerate() {
                if found_terminal {
                    diags.push(LintDiagnostic {
                        lint_id: LintId("dead_code"),
                        severity: Severity::Warning,
                        span: *span,
                        message: "unreachable statement after return".to_string(),
                        suggestion: None,
                    });
                    break;
                }
                // Check for terminal in any statement
                match stmt {
                    HirStmt::Expr(e) => {
                        if is_terminal(e) { found_terminal = true; }
                    }
                    HirStmt::Let { value: e, .. } | HirStmt::LetPat { value: e, .. }
                    | HirStmt::Assign { value: e, .. } | HirStmt::AssignDeref { value: e, .. }
                    | HirStmt::AssignField { value: e, .. } => {
                        if is_terminal(e) { found_terminal = true; }
                    }
                }
            }
        }
        HirExpr::If { condition, then_branch, else_branch, .. } => {
            find_unreachable_after_stmt(condition, interner, diags);
            find_unreachable_after_stmt(then_branch, interner, diags);
            if let Some(e) = else_branch {
                find_unreachable_after_stmt(e, interner, diags);
            }
        }
        HirExpr::Match { scrutinee, arms, .. } => {
            find_unreachable_after_stmt(scrutinee, interner, diags);
            for arm in arms {
                find_unreachable_after_stmt(&arm.body, interner, diags);
            }
        }
        HirExpr::While { condition, body, .. } => {
            find_unreachable_after_stmt(condition, interner, diags);
            find_unreachable_after_stmt(body, interner, diags);
        }
        HirExpr::ForIn { iter, body, .. } => {
            find_unreachable_after_stmt(iter, interner, diags);
            find_unreachable_after_stmt(body, interner, diags);
        }
        _ => {}
    }
}

fn is_terminal(expr: &HirExpr) -> bool {
    match expr {
        HirExpr::Return { .. } => true,
        // `return expr` is lowered to Unary(Not, expr) – treat as terminal
        HirExpr::Unary { op: glyim_hir::HirUnOp::Not, .. } => true,
        HirExpr::Block { stmts, .. } => stmts.iter().any(|s| match s {
            HirStmt::Expr(e) => is_terminal(e),
            _ => false,
        }),
        _ => false,
    }
}

#[cfg(test)]
mod tests;
