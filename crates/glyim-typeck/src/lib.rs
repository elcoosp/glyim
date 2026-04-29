use glyim_hir::item::{EnumDef, ExternBlock, FnSig, HirVariant, StructDef, StructField};
use glyim_hir::node::{Hir, HirExpr, HirFn, HirStmt};
use glyim_hir::types::{ExprId, HirType};
use glyim_hir::HirPattern;
use glyim_interner::{Interner, Symbol};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum TypeError {
    MismatchedTypes {
        expected: HirType,
        found: HirType,
        expr_id: ExprId,
    },
    UnknownType {
        name: Symbol,
    },
    UnknownField {
        struct_name: Symbol,
        field: Symbol,
    },
    MissingField {
        struct_name: Symbol,
        field: Symbol,
    },
    ExtraField {
        struct_name: Symbol,
        field: Symbol,
    },
    NonExhaustiveMatch {
        missing: Vec<String>,
    },
    InvalidQuestion {
        expr_id: ExprId,
    },
    ExpectedFunction {
        expr_id: ExprId,
    },
    InvalidReturnType {
        expected: HirType,
        found: HirType,
    },
}

#[derive(Clone)]
pub struct StructInfo {
    pub fields: Vec<StructField>,
    pub field_map: HashMap<Symbol, usize>,
    pub type_params: Vec<Symbol>,
}

#[derive(Clone)]
pub struct EnumInfo {
    pub variants: Vec<HirVariant>,
    pub variant_map: HashMap<Symbol, usize>,
    pub type_params: Vec<Symbol>,
}

#[derive(Clone)]
pub(crate) struct Scope {
    bindings: HashMap<Symbol, HirType>,
}

pub struct TypeChecker {
    pub interner: Interner,
    pub(crate) scopes: Vec<Scope>,
    pub structs: HashMap<Symbol, StructInfo>,
    pub enums: HashMap<Symbol, EnumInfo>,
    pub extern_fns: HashMap<Symbol, FnSig>,
    pub impl_methods: HashMap<Symbol, Vec<(Symbol, HirFn)>>,
    pub expr_types: Vec<HirType>,
    pub return_type: Option<HirType>,
    pub errors: Vec<TypeError>,
    fns: Vec<HirFn>,
}

impl TypeChecker {
    pub fn new(interner: Interner) -> Self {
        TypeChecker {
            interner,
            scopes: Vec::new(),
            structs: HashMap::new(),
            enums: HashMap::new(),
            extern_fns: HashMap::new(),
            impl_methods: HashMap::new(),
            expr_types: Vec::new(),
            return_type: None,
            errors: Vec::new(),
            fns: Vec::new(),
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(Scope {
            bindings: HashMap::new(),
        });
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn insert_binding(&mut self, name: Symbol, ty: HirType) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.bindings.insert(name, ty);
        }
    }

    fn lookup_binding(&self, name: &Symbol) -> Option<HirType> {
        for scope in self.scopes.iter().rev() {
            if let Some(ty) = scope.bindings.get(name) {
                return Some(ty.clone());
            }
        }
        None
    }

    fn set_type(&mut self, id: ExprId, ty: HirType) {
        let idx = id.as_usize();
        if idx >= self.expr_types.len() {
            self.expr_types.resize(idx + 1, HirType::Never);
        }
        self.expr_types[idx] = ty;
    }

    pub fn check(&mut self, hir: &Hir) -> Result<(), Vec<TypeError>> {
        for item in &hir.items {
            match item {
                glyim_hir::item::HirItem::Struct(s) => self.register_struct(s),
                glyim_hir::item::HirItem::Enum(e) => self.register_enum(e),
                glyim_hir::item::HirItem::Extern(ext) => self.register_extern(ext),
                glyim_hir::item::HirItem::Impl(imp) => self.register_impl(imp),
                glyim_hir::item::HirItem::Fn(f) => self.fns.push(f.clone()),
            }
        }
        for item in &hir.items {
            if let glyim_hir::item::HirItem::Fn(f) = item {
                self.check_fn(f);
            }
        }
        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.errors.clone())
        }
    }

    fn register_struct(&mut self, s: &StructDef) {
        let mut field_map = HashMap::new();
        for (i, field) in s.fields.iter().enumerate() {
            field_map.insert(field.name, i);
        }
        self.structs.insert(
            s.name,
            StructInfo {
                fields: s.fields.clone(),
                field_map,
                type_params: s.type_params.clone(),
            },
        );
    }

    fn register_enum(&mut self, e: &EnumDef) {
        let mut variant_map = HashMap::new();
        for (i, v) in e.variants.iter().enumerate() {
            variant_map.insert(v.name, i);
        }
        self.enums.insert(
            e.name,
            EnumInfo {
                variants: e.variants.clone(),
                variant_map,
                type_params: e.type_params.clone(),
            },
        );
    }

    fn register_impl(&mut self, imp: &glyim_hir::item::HirImplDef) {
        let methods: Vec<(Symbol, HirFn)> =
            imp.methods.iter().map(|m| (m.name, m.clone())).collect();
        self.impl_methods.insert(imp.target_name, methods);
    }

    fn register_extern(&mut self, ext: &ExternBlock) {
        for f in &ext.functions {
            self.extern_fns.insert(
                f.name,
                FnSig {
                    params: f.params.clone(),
                    ret: f.ret.clone(),
                },
            );
        }
    }

    fn dummy_symbol(&self) -> Symbol {
        // Return a known dummy symbol (first interned string in a temporary interner)
        // This is used for error reporting where we need a placeholder Symbol.
        glyim_interner::Interner::new().intern("__dummy")
    }

    fn check_fn(&mut self, f: &HirFn) {
        self.push_scope();
        for (sym, ty) in &f.params {
            self.insert_binding(*sym, ty.clone());
        }
        let body_type = self.check_expr(&f.body);
        if let Some(ref ret_ty) = f.ret {
            if let Some(ref actual) = body_type {
                if ret_ty != actual {
                    self.errors.push(TypeError::InvalidReturnType {
                        expected: ret_ty.clone(),
                        found: actual.clone(),
                    });
                }
            }
        }
        self.pop_scope();
    }

    fn check_expr(&mut self, expr: &HirExpr) -> Option<HirType> {
        let id = match expr {
            HirExpr::IntLit { id, .. } => *id,
            HirExpr::FloatLit { id, .. } => *id,
            HirExpr::BoolLit { id, .. } => *id,
            HirExpr::StrLit { id, .. } => *id,
            HirExpr::Ident { id, .. } => *id,
            HirExpr::UnitLit { id } => *id,
            HirExpr::Binary { id, .. } => *id,
            HirExpr::Unary { id, .. } => *id,
            HirExpr::Block { id, .. } => *id,
            HirExpr::If { id, .. } => *id,
            HirExpr::Println { id, .. } => *id,
            HirExpr::Call { id, .. } => *id,
            HirExpr::Assert { id, .. } => *id,
            HirExpr::As { id, .. } => *id,
            HirExpr::Match { id, .. } => *id,
            HirExpr::FieldAccess { id, .. } => *id,
            HirExpr::StructLit { id, .. } => *id,
            HirExpr::EnumVariant { id, .. } => *id,
            HirExpr::TupleLit { id, .. } => *id,
        };
        let ty = match expr {
            HirExpr::IntLit { .. } => HirType::Int,
            HirExpr::FloatLit { .. } => HirType::Float,
            HirExpr::BoolLit { .. } => HirType::Bool,
            HirExpr::StrLit { .. } => HirType::Str,
            HirExpr::Ident { name: sym, .. } => self.lookup_binding(sym).unwrap_or(HirType::Int),
            HirExpr::UnitLit { .. } => HirType::Unit,
            HirExpr::Binary { lhs, rhs, .. } => {
                self.check_expr(lhs);
                self.check_expr(rhs);
                HirType::Int
            }
            HirExpr::Unary { operand, .. } => {
                self.check_expr(operand);
                HirType::Int
            }
            HirExpr::Block { stmts, .. } => {
                let mut last = HirType::Unit;
                for stmt in stmts {
                    if let Some(t) = self.check_stmt(stmt) {
                        last = t;
                    }
                }
                last
            }
            HirExpr::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                self.check_expr(condition);
                let then_type = self.check_expr(then_branch);
                if let Some(else_br) = else_branch {
                    self.check_expr(else_br);
                }
                then_type.unwrap_or(HirType::Unit)
            }
            HirExpr::Println { arg, .. } => {
                self.check_expr(arg);
                HirType::Unit
            }
            HirExpr::Assert {
                condition, message, ..
            } => {
                self.check_expr(condition);
                if let Some(msg) = message {
                    self.check_expr(msg);
                }
                HirType::Unit
            }
            HirExpr::StructLit {
                struct_name,
                fields,
                ..
            } => {
                let field_names: Vec<Symbol> = fields.iter().map(|(sym, _)| *sym).collect();
                let field_count = fields.len();
                let info = self.structs.get(struct_name).cloned();
                if let Some(ref info) = info {
                    for field_sym in &field_names {
                        if !info.field_map.contains_key(field_sym) {
                            self.errors.push(TypeError::UnknownField {
                                struct_name: *struct_name,
                                field: *field_sym,
                            });
                        }
                    }
                    if field_count != info.fields.len() {
                        for field in &info.fields {
                            if !field_names.contains(&field.name) {
                                self.errors.push(TypeError::MissingField {
                                    struct_name: *struct_name,
                                    field: field.name,
                                });
                            }
                        }
                    }
                }
                for (_, val) in fields {
                    self.check_expr(val);
                }
                HirType::Named(*struct_name)
            }
            HirExpr::FieldAccess { object, field, .. } => {
                let obj_type = self.check_expr(object);
                match &obj_type {
                    Some(HirType::Tuple(elems)) => {
                        // Parse field name as _N to get index
                        let field_name = self.interner.resolve(*field);
                        if let Some(index_str) = field_name.strip_prefix('_') {
                            if let Ok(idx) = index_str.parse::<usize>() {
                                if idx < elems.len() {
                                    return Some(elems[idx].clone());
                                } else {
                                    self.errors.push(TypeError::UnknownField {
                                        struct_name: self.dummy_symbol(),
                                        field: *field,
                                    });
                                    return Some(HirType::Int);
                                }
                            }
                        }
                        self.errors.push(TypeError::UnknownField {
                            struct_name: self.dummy_symbol(),
                            field: *field,
                        });
                        HirType::Int
                    }
                    Some(HirType::Named(name)) => {
                        let info = self.structs.get(name).cloned();
                        if let Some(ref info) = info {
                            if !info.field_map.contains_key(field) {
                                self.errors.push(TypeError::UnknownField {
                                    struct_name: *name,
                                    field: *field,
                                });
                            }
                        }
                        HirType::Int
                    }
                    _ => HirType::Int,
                }
            }
            HirExpr::EnumVariant {
                enum_name,
                variant_name,
                args,
                ..
            } => {
                let info = self.enums.get(enum_name).cloned();
                if let Some(ref info) = info {
                    if !info.variant_map.contains_key(variant_name) {
                        self.errors.push(TypeError::UnknownField {
                            struct_name: *enum_name,
                            field: *variant_name,
                        });
                    }
                }
                for arg in args {
                    self.check_expr(arg);
                }
                HirType::Named(*enum_name)
            }
            HirExpr::Match {
                scrutinee, arms, ..
            } => {
                let scrutinee_ty = self.check_expr(scrutinee).unwrap_or(HirType::Never);
                self.check_match_exhaustiveness(&scrutinee_ty, arms);
                let mut arm_types = vec![];
                for (_, guard, body) in arms {
                    if let Some(g) = guard {
                        self.check_expr(g);
                    }
                    if let Some(t) = self.check_expr(body) {
                        arm_types.push(t);
                    }
                }
                arm_types.first().cloned().unwrap_or(HirType::Unit)
            }
            HirExpr::Call { callee, args, .. } => {
                for a in args {
                    self.check_expr(a);
                }
                // Look up function return type
                if let Some(fn_def) = self.fns.iter().find(|f| f.name == *callee) {
                    fn_def.ret.clone().unwrap_or(HirType::Int)
                } else if self.extern_fns.contains_key(callee) {
                    self.extern_fns
                        .get(callee)
                        .map(|sig| sig.ret.clone())
                        .unwrap_or(HirType::Int)
                } else {
                    // Search in impl methods
                    for methods in self.impl_methods.values() {
                        if let Some((_, fn_def)) = methods.iter().find(|(name, _)| name == callee) {
                            return Some(fn_def.ret.clone().unwrap_or(HirType::Int));
                        }
                    }
                    HirType::Int
                }
            }
            HirExpr::As {
                expr, target_type, ..
            } => {
                let from_ty = self.check_expr(expr).unwrap_or(HirType::Int);
                // Resolve named types to primitives using interner
                let resolve = |ty: &HirType| -> HirType {
                    match ty {
                        HirType::Named(sym) => match self.interner.resolve(*sym) {
                            "f64" | "Float" => HirType::Float,
                            "i64" | "Int" => HirType::Int,
                            "bool" | "Bool" => HirType::Bool,
                            "Str" | "str" => HirType::Str,
                            _ => ty.clone(),
                        },
                        _ => ty.clone(),
                    }
                };
                let resolved_target = resolve(target_type);
                let resolved_from = resolve(&from_ty);
                if !is_valid_cast(&resolved_from, &resolved_target) {
                    self.errors.push(TypeError::MismatchedTypes {
                        expected: target_type.clone(),
                        found: from_ty.clone(),
                        expr_id: ExprId::new(0),
                    });
                }
                target_type.clone()
            }
            HirExpr::TupleLit { elements, .. } => {
                let elem_types: Vec<HirType> =
                    elements.iter().filter_map(|e| self.check_expr(e)).collect();
                HirType::Tuple(elem_types)
            }
        };
        self.set_type(id, ty.clone());
        Some(ty)
    }

    fn check_stmt(&mut self, stmt: &HirStmt) -> Option<HirType> {
        match stmt {
            HirStmt::Let { name, value, .. } => {
                let ty = self.check_expr(value).unwrap_or(HirType::Int);
                self.insert_binding(*name, ty.clone());
                None
            }
            HirStmt::LetPat { pattern, value, .. } => {
                let ty = self.check_expr(value).unwrap_or(HirType::Int);
                self.check_let_pat(pattern, &ty);
                None
            }
            HirStmt::Assign { target, value } => {
                let ty = self.check_expr(value).unwrap_or(HirType::Int);
                self.insert_binding(*target, ty.clone());
                Some(ty)
            }
            HirStmt::Expr(e) => self.check_expr(e),
        }
    }

    fn check_let_pat(&mut self, pattern: &HirPattern, value_ty: &HirType) {
        match pattern {
            HirPattern::Var(sym) => {
                self.insert_binding(*sym, value_ty.clone());
            }
            HirPattern::Wild => {}
            HirPattern::Tuple { elements } => {
                if let HirType::Tuple(elem_types) = value_ty {
                    for (pat, ty) in elements.iter().zip(elem_types.iter()) {
                        self.check_let_pat(pat, ty);
                    }
                }
            }
            _ => {}
        }
    }

    fn check_match_exhaustiveness(
        &mut self,
        scrutinee_type: &HirType,
        arms: &[(HirPattern, Option<HirExpr>, HirExpr)],
    ) {
        let enum_variants = match scrutinee_type {
            HirType::Named(name) => {
                if let Some(info) = self.enums.get(name) {
                    info.variants.iter().map(|v| v.name).collect()
                } else {
                    let name_str = format!("{:?}", name);
                    if name_str.contains("Option") {
                        vec![self.interner.intern("Some"), self.interner.intern("None")]
                    } else if name_str.contains("Result") {
                        vec![self.interner.intern("Ok"), self.interner.intern("Err")]
                    } else {
                        return;
                    }
                }
            }
            HirType::Option(_) => vec![self.interner.intern("Some"), self.interner.intern("None")],
            HirType::Result(_, _) => vec![self.interner.intern("Ok"), self.interner.intern("Err")],
            _ => return,
        };
        let has_wildcard = arms
            .iter()
            .any(|(pat, _, _)| matches!(pat, HirPattern::Wild));
        if has_wildcard {
            return;
        }
        let covered: Vec<Symbol> = arms
            .iter()
            .filter_map(|(pat, _, _)| match pat {
                HirPattern::EnumVariant { variant_name, .. } => Some(*variant_name),
                HirPattern::OptionSome(_) => Some(self.interner.intern("Some")),
                HirPattern::OptionNone => Some(self.interner.intern("None")),
                HirPattern::ResultOk(_) => Some(self.interner.intern("Ok")),
                HirPattern::ResultErr(_) => Some(self.interner.intern("Err")),
                _ => None,
            })
            .collect();
        let missing: Vec<String> = enum_variants
            .iter()
            .filter(|v| !covered.contains(v))
            .map(|v| self.interner.resolve(*v).to_string())
            .collect();
        if !missing.is_empty() {
            self.errors.push(TypeError::NonExhaustiveMatch { missing });
        }
    }
}

fn is_valid_cast(from: &HirType, to: &HirType) -> bool {
    // Resolve named types to their primitive equivalents
    let resolve = |ty: &HirType| -> HirType {
        match ty {
            HirType::Named(sym) => {
                // We need to resolve the symbol name to a primitive
                // Since we don't have an interner here, we check the Display/Debug output
                let name = format!("{:?}", sym);
                if name.contains("f64") || name.contains("Float") {
                    HirType::Float
                } else if name.contains("i64") || name.contains("Int") {
                    HirType::Int
                } else if name.contains("bool") || name.contains("Bool") {
                    HirType::Bool
                } else if name.contains("Str") || name.contains("str") {
                    HirType::Str
                } else {
                    ty.clone()
                }
            }
            _ => ty.clone(),
        }
    };
    let from = resolve(from);
    let to = resolve(to);
    match (&from, &to) {
        (HirType::Int, HirType::Float) | (HirType::Float, HirType::Int) => true,
        (HirType::Int, HirType::Int) | (HirType::Float, HirType::Float) => true,
        (_, HirType::RawPtr { .. }) => true,
        (a, b) if a == b => true,
        _ => false,
    }
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new(Interner::new())
    }
}

impl std::fmt::Display for TypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypeError::MismatchedTypes {
                expected, found, ..
            } => write!(
                f,
                "type mismatch: expected {:?}, found {:?}",
                expected, found
            ),
            TypeError::UnknownType { name } => write!(f, "unknown type: {:?}", name),
            TypeError::UnknownField { struct_name, field } => write!(
                f,
                "unknown field '{:?}' on struct '{:?}'",
                field, struct_name
            ),
            TypeError::MissingField { struct_name, field } => write!(
                f,
                "missing field '{:?}' in struct '{:?}'",
                field, struct_name
            ),
            TypeError::ExtraField { struct_name, field } => {
                write!(f, "extra field '{:?}' in struct '{:?}'", field, struct_name)
            }
            TypeError::NonExhaustiveMatch { missing } => {
                write!(f, "non-exhaustive match, missing variants: {:?}", missing)
            }
            TypeError::InvalidQuestion { .. } => {
                write!(f, "? operator used outside of Result-returning function")
            }
            TypeError::ExpectedFunction { .. } => write!(f, "expected function call"),
            TypeError::InvalidReturnType { expected, found } => write!(
                f,
                "invalid return type: expected {:?}, found {:?}",
                expected, found
            ),
        }
    }
}
