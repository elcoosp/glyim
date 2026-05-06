use crate::node::{HirBinOp, HirExpr, HirFn, HirStmt, HirUnOp, MatchArm};
use crate::types::{HirPattern, HirType};
use glyim_interner::{Interner, Symbol};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NormalizedExpr {
    IntLit(i64),
    FloatLit(u64),
    BoolLit(bool),
    StrLit(String),
    UnitLit,
    Local(u32),
    Name(String),
    Binary { op: HirBinOp, lhs: Box<NormalizedExpr>, rhs: Box<NormalizedExpr> },
    Unary { op: HirUnOp, operand: Box<NormalizedExpr> },
    Block { stmts: Vec<NormalizedStmt> },
    If { condition: Box<NormalizedExpr>, then_branch: Box<NormalizedExpr>, else_branch: Option<Box<NormalizedExpr>> },
    Call { callee: String, args: Vec<NormalizedExpr> },
    MethodCall { receiver: Box<NormalizedExpr>, method_name: String, resolved_callee: Option<String>, args: Vec<NormalizedExpr> },
    Assert { condition: Box<NormalizedExpr>, message: Option<Box<NormalizedExpr>> },
    Match { scrutinee: Box<NormalizedExpr>, arms: Vec<NormalizedMatchArm> },
    FieldAccess { object: Box<NormalizedExpr>, field: String },
    StructLit { struct_name: String, fields: Vec<(String, NormalizedExpr)> },
    EnumVariant { enum_name: String, variant_name: String, args: Vec<NormalizedExpr> },
    ForIn { pattern: NormalizedPattern, iter: Box<NormalizedExpr>, body: Box<NormalizedExpr> },
    While { condition: Box<NormalizedExpr>, body: Box<NormalizedExpr> },
    Return { value: Option<Box<NormalizedExpr>> },
    As { expr: Box<NormalizedExpr>, target_type: HirType },
    SizeOf { target_type: HirType },
    TupleLit { elements: Vec<NormalizedExpr> },
    AddrOf { target: String },
    Deref { expr: Box<NormalizedExpr> },
    Println { arg: Box<NormalizedExpr> },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NormalizedStmt {
    Let { local_id: u32, mutable: bool, value: NormalizedExpr },
    Assign { local_id: u32, value: NormalizedExpr },
    AssignField { object: NormalizedExpr, field: String, value: NormalizedExpr },
    AssignDeref { target: NormalizedExpr, value: NormalizedExpr },
    Expr(NormalizedExpr),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NormalizedMatchArm {
    pub pattern: NormalizedPattern,
    pub guard: Option<NormalizedExpr>,
    pub body: NormalizedExpr,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NormalizedPattern {
    Wild,
    BoolLit(bool),
    IntLit(i64),
    FloatLit(u64),
    StrLit(String),
    Unit,
    Local(u32),
    Struct { name: String, bindings: Vec<(String, NormalizedPattern)> },
    EnumVariant { enum_name: String, variant_name: String, bindings: Vec<(String, NormalizedPattern)> },
    Tuple { elements: Vec<NormalizedPattern> },
    OptionSome(Box<NormalizedPattern>),
    OptionNone,
    ResultOk(Box<NormalizedPattern>),
    ResultErr(Box<NormalizedPattern>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NormalizedHirFn {
    pub name: String,
    pub type_params: Vec<String>,
    pub params: Vec<(String, HirType)>,
    pub param_mutability: Vec<bool>,
    pub ret: Option<HirType>,
    pub body: NormalizedExpr,
    pub is_pub: bool,
    pub is_extern_backed: bool,
}

pub struct SemanticNormalizer<'a> {
    interner: &'a Interner,
    local_map: HashMap<Symbol, u32>,
    next_local: u32,
    param_set: HashMap<Symbol, u32>,
}

impl<'a> SemanticNormalizer<'a> {
    pub fn new(interner: &'a Interner) -> Self {
        Self { interner, local_map: HashMap::new(), next_local: 0, param_set: HashMap::new() }
    }

    pub fn register_param(&mut self, sym: Symbol) {
        let idx = self.next_local;
        self.next_local += 1;
        self.local_map.insert(sym, idx);
        self.param_set.insert(sym, idx);
    }

    fn register_local(&mut self, sym: Symbol) -> u32 {
        let idx = self.next_local;
        self.next_local += 1;
        self.local_map.insert(sym, idx);
        idx
    }

    fn resolve_ident(&self, sym: Symbol) -> NormalizedExpr {
        if let Some(&idx) = self.local_map.get(&sym) {
            NormalizedExpr::Local(idx)
        } else {
            NormalizedExpr::Name(self.interner.resolve(sym).to_string())
        }
    }

    fn resolve_name(&self, sym: Symbol) -> String {
        self.interner
            .try_resolve(sym)
            .unwrap_or("<unknown>")
            .to_string()
    }

    pub fn normalize_fn(&mut self, hir_fn: &HirFn) -> NormalizedHirFn {
        self.local_map.clear();
        self.next_local = 0;
        self.param_set.clear();
        for &(sym, _) in &hir_fn.params {
            self.register_param(sym);
        }
        let body = self.normalize_expr(&hir_fn.body);
        // canonicalize parameter names to _p0, _p1, …
        let canon_params: Vec<(String, HirType)> = hir_fn.params.iter().enumerate().map(|(i, (_, t))| (format!("_p{}", i), t.clone())).collect();
        NormalizedHirFn {
            name: self.resolve_name(hir_fn.name),
            type_params: hir_fn.type_params.iter().map(|&s| self.resolve_name(s)).collect(),
            params: canon_params,
            param_mutability: hir_fn.param_mutability.clone(),
            ret: hir_fn.ret.clone(),
            body,
            is_pub: hir_fn.is_pub,
            is_extern_backed: hir_fn.is_extern_backed,
            // is_test and test_config are NOT part of normalized representation
        }
    }

    pub fn normalize_expr(&mut self, expr: &HirExpr) -> NormalizedExpr {
        match expr {
            HirExpr::IntLit { value, .. } => NormalizedExpr::IntLit(*value),
            HirExpr::FloatLit { value, .. } => NormalizedExpr::FloatLit(value.to_bits()),
            HirExpr::BoolLit { value, .. } => NormalizedExpr::BoolLit(*value),
            HirExpr::StrLit { value, .. } => NormalizedExpr::StrLit(value.clone()),
            HirExpr::UnitLit { .. } => NormalizedExpr::UnitLit,
            HirExpr::Ident { name, .. } => self.resolve_ident(*name),
            HirExpr::Binary { op, lhs, rhs, .. } => {
                let lhs_n = self.normalize_expr(lhs);
                let rhs_n = self.normalize_expr(rhs);
                if op.is_commutative() && lhs_n > rhs_n {
                    NormalizedExpr::Binary { op: *op, lhs: Box::new(rhs_n), rhs: Box::new(lhs_n) }
                } else {
                    NormalizedExpr::Binary { op: *op, lhs: Box::new(lhs_n), rhs: Box::new(rhs_n) }
                }
            }
            HirExpr::Unary { op, operand, .. } => {
                let operand_n = self.normalize_expr(operand);
                if let HirUnOp::Not = op
                    && let NormalizedExpr::Unary { op: HirUnOp::Not, operand: inner } = &operand_n {
                        return (**inner).clone();
                    }
                NormalizedExpr::Unary { op: *op, operand: Box::new(operand_n) }
            }
            HirExpr::Block { stmts, .. } => {
                NormalizedExpr::Block { stmts: stmts.iter().map(|s| self.normalize_stmt(s)).collect() }
            }
            HirExpr::If { condition, then_branch, else_branch, .. } => {
                NormalizedExpr::If {
                    condition: Box::new(self.normalize_expr(condition)),
                    then_branch: Box::new(self.normalize_expr(then_branch)),
                    else_branch: else_branch.as_ref().map(|e| Box::new(self.normalize_expr(e))),
                }
            }
            HirExpr::Call { callee, args, .. } => {
                NormalizedExpr::Call {
                    callee: self.resolve_name(*callee),
                    args: args.iter().map(|a| self.normalize_expr(a)).collect(),
                }
            }
            HirExpr::MethodCall { receiver, method_name, resolved_callee, args, .. } => {
                NormalizedExpr::MethodCall {
                    receiver: Box::new(self.normalize_expr(receiver)),
                    method_name: self.resolve_name(*method_name),
                    resolved_callee: resolved_callee.map(|s| self.resolve_name(s)),
                    args: args.iter().map(|a| self.normalize_expr(a)).collect(),
                }
            }
            HirExpr::Assert { condition, message, .. } => {
                NormalizedExpr::Assert {
                    condition: Box::new(self.normalize_expr(condition)),
                    message: message.as_ref().map(|e| Box::new(self.normalize_expr(e))),
                }
            }
            HirExpr::Match { scrutinee, arms, .. } => {
                NormalizedExpr::Match {
                    scrutinee: Box::new(self.normalize_expr(scrutinee)),
                    arms: arms.iter().map(|arm| self.normalize_match_arm(arm)).collect(),
                }
            }
            HirExpr::FieldAccess { object, field, .. } => {
                NormalizedExpr::FieldAccess {
                    object: Box::new(self.normalize_expr(object)),
                    field: self.resolve_name(*field),
                }
            }
            HirExpr::StructLit { struct_name, fields, .. } => {
                NormalizedExpr::StructLit {
                    struct_name: self.resolve_name(*struct_name),
                    fields: fields.iter().map(|(n, e)| (self.resolve_name(*n), self.normalize_expr(e))).collect(),
                }
            }
            HirExpr::EnumVariant { enum_name, variant_name, args, .. } => {
                NormalizedExpr::EnumVariant {
                    enum_name: self.resolve_name(*enum_name),
                    variant_name: self.resolve_name(*variant_name),
                    args: args.iter().map(|a| self.normalize_expr(a)).collect(),
                }
            }
            HirExpr::ForIn { pattern, iter, body, .. } => {
                let pattern_n = self.normalize_pattern(pattern);
                NormalizedExpr::ForIn {
                    pattern: pattern_n,
                    iter: Box::new(self.normalize_expr(iter)),
                    body: Box::new(self.normalize_expr(body)),
                }
            }
            HirExpr::While { condition, body, .. } => {
                NormalizedExpr::While {
                    condition: Box::new(self.normalize_expr(condition)),
                    body: Box::new(self.normalize_expr(body)),
                }
            }
            HirExpr::Return { value, .. } => {
                NormalizedExpr::Return {
                    value: value.as_ref().map(|e| Box::new(self.normalize_expr(e))),
                }
            }
            HirExpr::As { expr, target_type, .. } => {
                NormalizedExpr::As {
                    expr: Box::new(self.normalize_expr(expr)),
                    target_type: target_type.clone(),
                }
            }
            HirExpr::SizeOf { target_type, .. } => {
                NormalizedExpr::SizeOf { target_type: target_type.clone() }
            }
            HirExpr::TupleLit { elements, .. } => {
                NormalizedExpr::TupleLit {
                    elements: elements.iter().map(|e| self.normalize_expr(e)).collect(),
                }
            }
            HirExpr::AddrOf { target, .. } => {
                NormalizedExpr::AddrOf { target: self.resolve_name(*target) }
            }
            HirExpr::Deref { expr, .. } => {
                NormalizedExpr::Deref { expr: Box::new(self.normalize_expr(expr)) }
            }
            HirExpr::Println { arg, .. } => {
                NormalizedExpr::Println { arg: Box::new(self.normalize_expr(arg)) }
            }
        }
    }

    pub fn normalize_stmt(&mut self, stmt: &HirStmt) -> NormalizedStmt {
        match stmt {
            HirStmt::Let { name, mutable, value, .. } => {
                let local_id = self.register_local(*name);
                NormalizedStmt::Let { local_id, mutable: *mutable, value: self.normalize_expr(value) }
            }
            HirStmt::LetPat { pattern, mutable, value, .. } => {
                let local_id = self.register_pattern_bindings(pattern);
                NormalizedStmt::Let { local_id, mutable: *mutable, value: self.normalize_expr(value) }
            }
            HirStmt::Assign { target, value, .. } => {
                let local_id = self.local_map.get(target).copied().expect("assign target must be known local");
                NormalizedStmt::Assign { local_id, value: self.normalize_expr(value) }
            }
            HirStmt::AssignField { object, field, value, .. } => {
                NormalizedStmt::AssignField {
                    object: self.normalize_expr(object),
                    field: self.resolve_name(*field),
                    value: self.normalize_expr(value),
                }
            }
            HirStmt::AssignDeref { target, value, .. } => {
                NormalizedStmt::AssignDeref {
                    target: self.normalize_expr(target),
                    value: self.normalize_expr(value),
                }
            }
            HirStmt::Expr(expr) => NormalizedStmt::Expr(self.normalize_expr(expr)),
        }
    }

    fn normalize_match_arm(&mut self, arm: &MatchArm) -> NormalizedMatchArm {
        let pattern = self.normalize_pattern(&arm.pattern);
        let guard = arm.guard.as_ref().map(|e| self.normalize_expr(e));
        let body = self.normalize_expr(&arm.body);
        NormalizedMatchArm { pattern, guard, body }
    }

    fn normalize_pattern(&mut self, pat: &HirPattern) -> NormalizedPattern {
        match pat {
            HirPattern::Wild => NormalizedPattern::Wild,
            HirPattern::BoolLit(b) => NormalizedPattern::BoolLit(*b),
            HirPattern::IntLit(n) => NormalizedPattern::IntLit(*n),
            HirPattern::FloatLit(f) => NormalizedPattern::FloatLit(f.to_bits()),
            HirPattern::StrLit(s) => NormalizedPattern::StrLit(s.clone()),
            HirPattern::Unit => NormalizedPattern::Unit,
            HirPattern::Var(sym) => {
                if self.local_map.contains_key(sym) {
                    NormalizedPattern::Local(self.local_map[sym])
                } else {
                    let idx = self.register_local(*sym);
                    NormalizedPattern::Local(idx)
                }
            }
            HirPattern::Struct { name, bindings, .. } => {
                NormalizedPattern::Struct {
                    name: self.resolve_name(*name),
                    bindings: bindings.iter().map(|(f, p)| (self.resolve_name(*f), self.normalize_pattern(p))).collect(),
                }
            }
            HirPattern::EnumVariant { enum_name, variant_name, bindings, .. } => {
                NormalizedPattern::EnumVariant {
                    enum_name: self.resolve_name(*enum_name),
                    variant_name: self.resolve_name(*variant_name),
                    bindings: bindings.iter().map(|(n, p)| (self.resolve_name(*n), self.normalize_pattern(p))).collect(),
                }
            }
            HirPattern::Tuple { elements, .. } => {
                NormalizedPattern::Tuple { elements: elements.iter().map(|p| self.normalize_pattern(p)).collect() }
            }
            HirPattern::OptionSome(inner) => NormalizedPattern::OptionSome(Box::new(self.normalize_pattern(inner))),
            HirPattern::OptionNone => NormalizedPattern::OptionNone,
            HirPattern::ResultOk(inner) => NormalizedPattern::ResultOk(Box::new(self.normalize_pattern(inner))),
            HirPattern::ResultErr(inner) => NormalizedPattern::ResultErr(Box::new(self.normalize_pattern(inner))),
        }
    }

    fn register_pattern_bindings(&mut self, pat: &HirPattern) -> u32 {
        match pat {
            HirPattern::Var(sym) => self.register_local(*sym),
            HirPattern::Struct { bindings, .. } => {
                let first = if let Some((_, sub)) = bindings.first() { self.register_pattern_bindings(sub) } else { self.next_local };
                for (_, sub) in &bindings[1..] { self.register_pattern_bindings(sub); }
                first
            }
            HirPattern::Tuple { elements, .. } => {
                let first = if let Some(sub) = elements.first() { self.register_pattern_bindings(sub) } else { self.next_local };
                for sub in &elements[1..] { self.register_pattern_bindings(sub); }
                first
            }
            HirPattern::EnumVariant { bindings, .. } => {
                let first = if let Some((_, sub)) = bindings.first() { self.register_pattern_bindings(sub) } else { self.next_local };
                for (_, sub) in &bindings[1..] { self.register_pattern_bindings(sub); }
                first
            }
            HirPattern::OptionSome(inner) => self.register_pattern_bindings(inner),
            HirPattern::ResultOk(inner) => self.register_pattern_bindings(inner),
            HirPattern::ResultErr(inner) => self.register_pattern_bindings(inner),
            _ => self.next_local,
        }
    }
}

impl HirBinOp {
    pub fn is_commutative(&self) -> bool {
        matches!(self, HirBinOp::Add | HirBinOp::Mul | HirBinOp::Eq | HirBinOp::Neq | HirBinOp::And | HirBinOp::Or)
    }
}

impl NormalizedHirFn {
    pub fn from_hir_fn(hir_fn: &HirFn, interner: &Interner) -> Self {
        let mut normalizer = SemanticNormalizer::new(interner);
        normalizer.normalize_fn(hir_fn)
    }
}
