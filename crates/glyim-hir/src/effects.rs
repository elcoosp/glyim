use glyim_interner::{Interner, Symbol};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct EffectSet {
    pub may_read: bool,
    pub may_write: bool,
    pub may_allocate: bool,
    pub may_panic: bool,
    pub may_diverge: bool,
    pub is_pure: bool,
}

impl EffectSet {
    pub fn pure() -> Self { Self { is_pure: true, ..Default::default() } }
    pub fn impure() -> Self { Self { is_pure: false, ..Default::default() } }
}

impl Default for EffectSet {
    fn default() -> Self {
        Self { may_read: false, may_write: false, may_allocate: false, may_panic: false, may_diverge: false, is_pure: true }
    }
}

pub struct EffectAnalyzer {
    effects: HashMap<Symbol, EffectSet>,
}

impl Default for EffectAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl EffectAnalyzer {
    pub fn new() -> Self { Self { effects: HashMap::new() } }

    pub fn analyze(&mut self, hir: &crate::Hir, interner: &Interner) {
        for item in &hir.items {
            if let crate::HirItem::Fn(f) = item {
                let name_str = interner.resolve(f.name);
                let seed = match name_str {
                    "println" | "print" => EffectSet { may_write: true, is_pure: false, ..Default::default() },
                    "__glyim_alloc" | "__glyim_free" => EffectSet { may_allocate: true, is_pure: false, ..Default::default() },
                    "abort" => EffectSet { may_panic: true, may_diverge: true, is_pure: false, ..Default::default() },
                    _ => continue,
                };
                self.effects.insert(f.name, seed);
            }
        }
        for _ in 0..10 {
            for item in &hir.items {
                if let crate::HirItem::Fn(f) = item {
                    if self.effects.contains_key(&f.name) { continue; }
                    let mut effect = EffectSet::pure();
                    self.analyze_expr(&f.body, interner, &mut effect);
                    self.effects.insert(f.name, effect);
                }
            }
        }
    }

    #[allow(clippy::only_used_in_recursion)]
    fn analyze_expr(&self, expr: &crate::HirExpr, interner: &Interner, effect: &mut EffectSet) {
        match expr {
            crate::HirExpr::Call { callee, .. } => {
                if let Some(e) = self.effects.get(callee) {
                    effect.may_read |= e.may_read; effect.may_write |= e.may_write;
                    effect.may_allocate |= e.may_allocate; effect.may_panic |= e.may_panic;
                    effect.may_diverge |= e.may_diverge; effect.is_pure &= e.is_pure;
                }
            }
            crate::HirExpr::Binary { lhs, rhs, .. } => { self.analyze_expr(lhs, interner, effect); self.analyze_expr(rhs, interner, effect); }
            crate::HirExpr::Block { stmts, .. } => {
                for s in stmts {
                    match s {
                        crate::HirStmt::Expr(e) | crate::HirStmt::Let { value: e, .. } | crate::HirStmt::Assign { value: e, .. } => self.analyze_expr(e, interner, effect),
                        _ => {}
                    }
                }
            }
            crate::HirExpr::If { condition, then_branch, else_branch, .. } => {
                self.analyze_expr(condition, interner, effect); self.analyze_expr(then_branch, interner, effect);
                if let Some(e) = else_branch { self.analyze_expr(e, interner, effect); }
            }
            crate::HirExpr::While { condition, body, .. } => {
                effect.may_diverge = true; self.analyze_expr(condition, interner, effect); self.analyze_expr(body, interner, effect);
            }
            _ => {}
        }
    }

    pub fn get_effect(&self, sym: Symbol) -> Option<&EffectSet> { self.effects.get(&sym) }
    pub fn is_pure(&self, sym: Symbol) -> bool { self.get_effect(sym).map(|e| e.is_pure).unwrap_or(false) }
}
