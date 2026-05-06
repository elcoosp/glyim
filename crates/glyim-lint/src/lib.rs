use glyim_diag::{Span, Severity};
use glyim_hir::Hir;
use glyim_interner::Interner;
use std::collections::HashMap;

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

        // Register built-in lints
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

    // Example: unused function lint
    if let Some(desc) = registry.get(LintId("unused_function")) {
        let called: std::collections::HashSet<glyim_interner::Symbol> =
            collect_called_symbols(hir);

        for item in &hir.items {
            if let glyim_hir::item::HirItem::Fn(f) = item {
                if !called.contains(&f.name) && interner.resolve(f.name) != "main" {
                    diags.push(LintDiagnostic {
                        lint_id: desc.id,
                        severity: desc.default_severity,
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

    // TODO: additional lints

    diags
}

fn collect_called_symbols(hir: &Hir) -> std::collections::HashSet<glyim_interner::Symbol> {
    use glyim_hir::node::{HirExpr, HirStmt};
    let mut called = std::collections::HashSet::new();

    fn walk(expr: &HirExpr, called: &mut std::collections::HashSet<glyim_interner::Symbol>) {
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
