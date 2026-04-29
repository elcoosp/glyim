use crate::types::{HirPattern, HirType};
use glyim_interner::Symbol;

#[derive(Debug, Clone, PartialEq, Hash)]
pub enum HirBinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Neq,
    Lt,
    Gt,
    Lte,
    Gte,
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq, Hash)]
pub enum HirUnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HirStmt {
    Let {
        name: Symbol,
        mutable: bool,
        value: HirExpr,
    },
    Assign {
        target: Symbol,
        value: HirExpr,
    },
    Expr(HirExpr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum HirExpr {
    IntLit(i64),
    FloatLit(f64),
    BoolLit(bool),
    StrLit(String),
    Ident(Symbol),
    UnitLit,
    Binary {
        op: HirBinOp,
        lhs: Box<HirExpr>,
        rhs: Box<HirExpr>,
    },
    Unary {
        op: HirUnOp,
        operand: Box<HirExpr>,
    },
    Block(Vec<HirStmt>),
    If {
        condition: Box<HirExpr>,
        then_branch: Box<HirExpr>,
        else_branch: Option<Box<HirExpr>>,
    },
    Println(Box<HirExpr>),
    Call {
        callee: Symbol,
        args: Vec<HirExpr>,
    },
    Assert {
        condition: Box<HirExpr>,
        message: Option<Box<HirExpr>>,
    },
    /// Type cast: expr as Type
    As {
        expr: Box<HirExpr>,
        target_type: HirType,
    },
    /// Match expression: arms are (pattern, optional guard, body)
    Match {
        scrutinee: Box<HirExpr>,
        arms: Vec<(HirPattern, Option<HirExpr>, HirExpr)>,
    },
    /// Field access: expr.field
    FieldAccess {
        object: Box<HirExpr>,
        field: Symbol,
    },
    /// Struct literal: Name { field1: val1, field2: val2, ... }
    StructLit {
        struct_name: Symbol,
        fields: Vec<(Symbol, HirExpr)>,
    },
    /// Enum variant constructor: Name::Variant(args...)
    EnumVariant {
        enum_name: Symbol,
        variant_name: Symbol,
        args: Vec<HirExpr>,
    },
}

/// A match arm: pattern + optional guard + body expression.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: HirPattern,
    pub guard: Option<HirExpr>,
    pub body: HirExpr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirFn {
    pub name: Symbol,
    pub params: Vec<Symbol>,
    pub body: HirExpr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hir {
    pub items: Vec<crate::item::HirItem>,
}
