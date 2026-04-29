use glyim_hir::item::{EnumDef, ExternBlock, FnSig, HirVariant, StructDef, StructField};
use glyim_hir::node::{Hir, HirExpr, HirFn, HirStmt};
use glyim_hir::types::{ExprId, HirType};
use glyim_interner::Symbol;
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
}

#[derive(Clone)]
pub struct StructInfo {
    pub fields: Vec<StructField>,
    pub field_map: HashMap<Symbol, usize>,
}

#[derive(Clone)]
pub struct EnumInfo {
    pub variants: Vec<HirVariant>,
    pub variant_map: HashMap<Symbol, usize>,
}

pub struct TypeChecker {
    pub bindings: HashMap<Symbol, HirType>,
    pub structs: HashMap<Symbol, StructInfo>,
    pub enums: HashMap<Symbol, EnumInfo>,
    pub extern_fns: HashMap<Symbol, FnSig>,
    pub expr_types: Vec<HirType>,
    pub return_type: Option<HirType>,
    pub errors: Vec<TypeError>,
    next_expr_id: ExprId,
}

impl TypeChecker {
    pub fn new() -> Self {
        TypeChecker {
            bindings: HashMap::new(),
            structs: HashMap::new(),
            enums: HashMap::new(),
            extern_fns: HashMap::new(),
            expr_types: Vec::new(),
            return_type: None,
            errors: Vec::new(),
            next_expr_id: 0,
        }
    }

    fn fresh_id(&mut self) -> ExprId {
        let id = self.next_expr_id;
        self.next_expr_id += 1;
        self.expr_types.push(HirType::Never);
        id
    }

    fn set_type(&mut self, id: ExprId, ty: HirType) {
        self.expr_types[id as usize] = ty;
    }

    pub fn check(&mut self, hir: &Hir) -> Result<(), Vec<TypeError>> {
        for item in &hir.items {
            match item {
                glyim_hir::item::HirItem::Struct(s) => self.register_struct(s),
                glyim_hir::item::HirItem::Enum(e) => self.register_enum(e),
                glyim_hir::item::HirItem::Extern(ext) => self.register_extern(ext),
                _ => {}
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
            },
        );
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

    fn check_fn(&mut self, f: &HirFn) {
        self.bindings.clear();
        for param in &f.params {
            self.bindings.insert(*param, HirType::Int);
        }
        self.return_type = None;
        let body_type = self.check_expr(&f.body);
        if let Some(body_type) = body_type {
            self.return_type = Some(body_type);
        }
    }

    fn check_expr(&mut self, expr: &HirExpr) -> Option<HirType> {
        let id = self.fresh_id();
        let ty = match expr {
            HirExpr::IntLit(_) => HirType::Int,
            HirExpr::FloatLit(_) => HirType::Float,
            HirExpr::BoolLit(_) => HirType::Bool,
            HirExpr::StrLit(_) => HirType::Str,
            HirExpr::Ident(sym) => self.bindings.get(sym).cloned().unwrap_or(HirType::Int),
            HirExpr::UnitLit => HirType::Unit,
            HirExpr::Binary { lhs, rhs, .. } => {
                self.check_expr(lhs);
                self.check_expr(rhs);
                HirType::Int
            }
            HirExpr::Unary { operand, .. } => {
                self.check_expr(operand);
                HirType::Int
            }
            HirExpr::Block(stmts) => {
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
            } => {
                self.check_expr(condition);
                let then_type = self.check_expr(then_branch);
                if let Some(else_br) = else_branch {
                    self.check_expr(else_br);
                }
                then_type.unwrap_or(HirType::Unit)
            }
            HirExpr::Println(arg) => {
                self.check_expr(arg);
                HirType::Unit
            }
            HirExpr::Assert { condition, message } => {
                self.check_expr(condition);
                if let Some(msg) = message {
                    self.check_expr(msg);
                }
                HirType::Unit
            }
            HirExpr::StructLit {
                struct_name,
                fields,
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
            HirExpr::FieldAccess { object, field } => {
                let obj_type = self.check_expr(object);
                if let Some(HirType::Named(name)) = obj_type {
                    let info = self.structs.get(&name).cloned();
                    if let Some(ref info) = info {
                        if !info.field_map.contains_key(field) {
                            self.errors.push(TypeError::UnknownField {
                                struct_name: name,
                                field: *field,
                            });
                        }
                    }
                }
                HirType::Int
            }
            HirExpr::EnumVariant {
                enum_name,
                variant_name,
                args,
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
            HirExpr::Match { scrutinee, arms } => {
                self.check_expr(scrutinee);
                let mut arm_types = vec![];
                for (_, _, body) in arms {
                    if let Some(t) = self.check_expr(body) {
                        arm_types.push(t);
                    }
                }
                arm_types.first().cloned().unwrap_or(HirType::Unit)
            }
            HirExpr::As { expr, target_type } => {
                self.check_expr(expr);
                target_type.clone()
            }
        };
        self.set_type(id, ty.clone());
        Some(ty)
    }

    fn check_stmt(&mut self, stmt: &HirStmt) -> Option<HirType> {
        match stmt {
            HirStmt::Let { name, value, .. } => {
                let ty = self.check_expr(value).unwrap_or(HirType::Int);
                self.bindings.insert(*name, ty);
                None
            }
            HirStmt::Assign { target, value } => {
                let ty = self.check_expr(value).unwrap_or(HirType::Int);
                self.bindings.insert(*target, ty.clone());
                Some(ty)
            }
            HirStmt::Expr(e) => self.check_expr(e),
        }
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
        }
    }
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

