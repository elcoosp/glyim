use glyim_diag::Span;
use glyim_interner::Symbol;

#[derive(Debug, Clone, PartialEq, Hash)]
pub enum BinOp {
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
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExprNode {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    IntLit(i64),
    FloatLit(f64),
    BoolLit(bool),
    StrLit(String),
    Ident(Symbol),
    UnitLit,
    Binary {
        op: BinOp,
        lhs: Box<ExprNode>,
        rhs: Box<ExprNode>,
    },
    Unary {
        op: UnOp,
        operand: Box<ExprNode>,
    },
    Lambda {
        params: Vec<Symbol>,
        body: Box<ExprNode>,
    },
    Block(Vec<BlockItem>),
    If {
        condition: Box<ExprNode>,
        then_branch: Box<ExprNode>,
        else_branch: Option<Box<ExprNode>>,
    },
    Call {
        callee: Box<ExprNode>,
        args: Vec<ExprNode>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct StmtNode {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    Let {
        name: Symbol,
        mutable: bool,
        value: ExprNode,
    },
    Assign {
        target: Symbol,
        value: ExprNode,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum BlockItem {
    Stmt(StmtNode),
    Expr(ExprNode),
}

#[derive(Debug, Clone, PartialEq)]
pub struct UseItem {
    pub path: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Binding {
        name: Symbol,
        name_span: Span,
        value: ExprNode,
    },
    FnDef {
        name: Symbol,
        name_span: Span,
        params: Vec<(Symbol, Span)>,
        body: ExprNode,
    },
    Stmt(StmtNode),
    Use(UseItem),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ast {
    pub items: Vec<Item>,
}

impl ExprNode {
    #[cfg(test)]
    pub fn dummy(kind: ExprKind) -> Self {
        Self {
            kind,
            span: Span::new(0, 0),
        }
    }
    pub fn int_lit(value: i64, span: Span) -> Self {
        Self {
            kind: ExprKind::IntLit(value),
            span,
        }
    }
    pub fn ident(sym: Symbol, span: Span) -> Self {
        Self {
            kind: ExprKind::Ident(sym),
            span,
        }
    }
}
