use crate::types::{HirPattern, HirType, ExprId};
use glyim_interner::Symbol;

#[derive(Debug, Clone, PartialEq, Hash)]
pub enum HirBinOp { Add, Sub, Mul, Div, Mod, Eq, Neq, Lt, Gt, Lte, Gte, And, Or }

#[derive(Debug, Clone, PartialEq, Hash)]
pub enum HirUnOp { Neg, Not }

#[derive(Debug, Clone, PartialEq)]
pub enum HirStmt {
    Let { name: Symbol, mutable: bool, value: HirExpr },
    LetPat { pattern: HirPattern, mutable: bool, value: HirExpr },
    Assign { target: Symbol, value: HirExpr },
    Expr(HirExpr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum HirExpr {
    IntLit { id: ExprId, value: i64 },
    FloatLit { id: ExprId, value: f64 },
    BoolLit { id: ExprId, value: bool },
    StrLit { id: ExprId, value: String },
    Ident { id: ExprId, name: Symbol },
    UnitLit { id: ExprId },
    Binary { id: ExprId, op: HirBinOp, lhs: Box<HirExpr>, rhs: Box<HirExpr> },
    Unary { id: ExprId, op: HirUnOp, operand: Box<HirExpr> },
    Block { id: ExprId, stmts: Vec<HirStmt> },
    If { id: ExprId, condition: Box<HirExpr>, then_branch: Box<HirExpr>, else_branch: Option<Box<HirExpr>> },
    Println { id: ExprId, arg: Box<HirExpr> },
    Call { id: ExprId, callee: Symbol, args: Vec<HirExpr> },
    Assert { id: ExprId, condition: Box<HirExpr>, message: Option<Box<HirExpr>> },
    As { id: ExprId, expr: Box<HirExpr>, target_type: HirType },
    Match { id: ExprId, scrutinee: Box<HirExpr>, arms: Vec<(HirPattern, Option<HirExpr>, HirExpr)> },
    FieldAccess { id: ExprId, object: Box<HirExpr>, field: Symbol },
    StructLit { id: ExprId, struct_name: Symbol, fields: Vec<(Symbol, HirExpr)> },
    EnumVariant { id: ExprId, enum_name: Symbol, variant_name: Symbol, args: Vec<HirExpr> },
    TupleLit { id: ExprId, elements: Vec<HirExpr> },
}

impl HirExpr {
    pub fn get_id(&self) -> ExprId {
        match self {
            Self::IntLit { id, .. } => *id,
            Self::FloatLit { id, .. } => *id,
            Self::BoolLit { id, .. } => *id,
            Self::StrLit { id, .. } => *id,
            Self::Ident { id, .. } => *id,
            Self::UnitLit { id } => *id,
            Self::Binary { id, .. } => *id,
            Self::Unary { id, .. } => *id,
            Self::Block { id, .. } => *id,
            Self::If { id, .. } => *id,
            Self::Println { id, .. } => *id,
            Self::Call { id, .. } => *id,
            Self::Assert { id, .. } => *id,
            Self::As { id, .. } => *id,
            Self::Match { id, .. } => *id,
            Self::FieldAccess { id, .. } => *id,
            Self::StructLit { id, .. } => *id,
            Self::EnumVariant { id, .. } => *id,
            Self::TupleLit { id, .. } => *id,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: HirPattern,
    pub guard: Option<HirExpr>,
    pub body: HirExpr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirFn {
    pub name: Symbol,
    pub type_params: Vec<Symbol>,
    pub params: Vec<(Symbol, HirType)>,
    pub ret: Option<HirType>,
    pub body: HirExpr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hir {
    pub items: Vec<crate::item::HirItem>,
}
