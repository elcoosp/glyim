use crate::types::{ExprId, HirPattern, HirType};
use glyim_diag::Span;
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
        span: Span,
    },
    LetPat {
        pattern: HirPattern,
        mutable: bool,
        value: HirExpr,
        ty: Option<crate::types::HirType>,
        span: Span,
    },
    AssignDeref {
        target: Box<HirExpr>,
        value: HirExpr,
        span: Span,
    },
    AssignField {
        object: Box<HirExpr>,
        field: Symbol,
        value: HirExpr,
        span: Span,
    },
    Assign {
        target: Symbol,
        value: HirExpr,
        span: Span,
    },
    Expr(HirExpr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum HirExpr {
    IntLit {
        id: ExprId,
        value: i64,
        span: Span,
    },
    FloatLit {
        id: ExprId,
        value: f64,
        span: Span,
    },
    BoolLit {
        id: ExprId,
        value: bool,
        span: Span,
    },
    StrLit {
        id: ExprId,
        value: String,
        span: Span,
    },
    Ident {
        id: ExprId,
        name: Symbol,
        span: Span,
    },
    UnitLit {
        id: ExprId,
        span: Span,
    },
    Binary {
        id: ExprId,
        op: HirBinOp,
        lhs: Box<HirExpr>,
        rhs: Box<HirExpr>,
        span: Span,
    },
    Unary {
        id: ExprId,
        op: HirUnOp,
        operand: Box<HirExpr>,
        span: Span,
    },
    Block {
        id: ExprId,
        stmts: Vec<HirStmt>,
        span: Span,
    },
    If {
        id: ExprId,
        condition: Box<HirExpr>,
        then_branch: Box<HirExpr>,
        else_branch: Option<Box<HirExpr>>,
        span: Span,
    },
    Println {
        id: ExprId,
        arg: Box<HirExpr>,
        span: Span,
    },
    MethodCall {
        id: ExprId,
        receiver: Box<HirExpr>,
        method_name: Symbol,
        args: Vec<HirExpr>,
        span: Span,
    },
    Call {
        id: ExprId,
        callee: Symbol,
        args: Vec<HirExpr>,
        span: Span,
    },
    Assert {
        id: ExprId,
        condition: Box<HirExpr>,
        message: Option<Box<HirExpr>>,
        span: Span,
    },
    As {
        id: ExprId,
        expr: Box<HirExpr>,
        target_type: HirType,
        span: Span,
    },
    Match {
        id: ExprId,
        scrutinee: Box<HirExpr>,
        arms: Vec<(HirPattern, Option<HirExpr>, HirExpr)>,
        span: Span,
    },
    FieldAccess {
        id: ExprId,
        object: Box<HirExpr>,
        field: Symbol,
        span: Span,
    },
    StructLit {
        id: ExprId,
        struct_name: Symbol,
        fields: Vec<(Symbol, HirExpr)>,
        span: Span,
    },
    EnumVariant {
        id: ExprId,
        enum_name: Symbol,
        variant_name: Symbol,
        args: Vec<HirExpr>,
        span: Span,
    },
    ForIn {
        id: ExprId,
        pattern: HirPattern,
        iter: Box<HirExpr>,
        body: Box<HirExpr>,
        span: Span,
    },
    While {
        id: ExprId,
        condition: Box<HirExpr>,
        body: Box<HirExpr>,
        span: Span,
    },
    SizeOf {
        id: ExprId,
        target_type: HirType,
        span: Span,
    },
    TupleLit {
        id: ExprId,
        elements: Vec<HirExpr>,
        span: Span,
    },
    Deref {
        id: ExprId,
        expr: Box<HirExpr>,
        span: Span,
    },
    Return {
        id: ExprId,
        value: Option<Box<HirExpr>>,
        span: Span,
    },
}

impl HirExpr {
    pub fn get_id(&self) -> ExprId {
        match self {
            Self::IntLit { id, .. } => *id,
            Self::FloatLit { id, .. } => *id,
            Self::BoolLit { id, .. } => *id,
            Self::StrLit { id, .. } => *id,
            Self::Ident { id, .. } => *id,
            Self::UnitLit { id, .. } => *id,
            Self::Binary { id, .. } => *id,
            Self::Unary { id, .. } => *id,
            Self::Block { id, .. } => *id,
            Self::If { id, .. } => *id,
            Self::Println { id, .. } => *id,
            Self::Call { id, .. } => *id,
            Self::MethodCall { id, .. } => *id,
            Self::Assert { id, .. } => *id,
            Self::As { id, .. } => *id,
            Self::Match { id, .. } => *id,
            Self::FieldAccess { id, .. } => *id,
            Self::StructLit { id, .. } => *id,
            Self::EnumVariant { id, .. } => *id,
            Self::ForIn { id, .. } => *id,
            Self::While { id, .. } => *id,
            Self::SizeOf { id, .. } => *id,
            Self::TupleLit { id, .. } => *id,
            Self::Deref { id, .. } => *id,
            Self::Return { id, .. } => *id,
        }
    }

    pub fn get_span(&self) -> Span {
        match self {
            Self::IntLit { span, .. } => *span,
            Self::FloatLit { span, .. } => *span,
            Self::BoolLit { span, .. } => *span,
            Self::StrLit { span, .. } => *span,
            Self::Ident { span, .. } => *span,
            Self::UnitLit { span, .. } => *span,
            Self::Binary { span, .. } => *span,
            Self::Unary { span, .. } => *span,
            Self::Block { span, .. } => *span,
            Self::If { span, .. } => *span,
            Self::Println { span, .. } => *span,
            Self::Call { span, .. } => *span,
            Self::MethodCall { span, .. } => *span,
            Self::Assert { span, .. } => *span,
            Self::As { span, .. } => *span,
            Self::Match { span, .. } => *span,
            Self::FieldAccess { span, .. } => *span,
            Self::StructLit { span, .. } => *span,
            Self::EnumVariant { span, .. } => *span,
            Self::ForIn { span, .. } => *span,
            Self::While { span, .. } => *span,
            Self::SizeOf { span, .. } => *span,
            Self::TupleLit { span, .. } => *span,
            Self::Deref { span, .. } => *span,
            Self::Return { span, .. } => *span,
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
    pub param_mutability: Vec<bool>,
    pub ret: Option<HirType>,
    pub body: HirExpr,
    pub span: Span,
    pub is_pub: bool,
    pub is_macro_generated: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hir {
    pub items: Vec<crate::item::HirItem>,
}
