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
    StructLit {
        name: Symbol,
        fields: Vec<(Symbol, ExprNode)>,
    },
    EnumVariant {
        enum_name: Symbol,
        variant_name: Symbol,
        args: Vec<ExprNode>,
    },
    Match {
        scrutinee: Box<ExprNode>,
        arms: Vec<MatchArm>,
    },
    FieldAccess {
        object: Box<ExprNode>,
        field: Symbol,
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

/// An enum variant definition.
#[derive(Debug, Clone, PartialEq)]
pub enum VariantKind {
    /// Single unnamed field: Circle(f64)
    Unnamed(Vec<(Symbol, Span)>),
    /// Named fields: Rect { a: Point, b: Point }
    Named(Vec<(Symbol, Span)>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariant {
    pub name: Symbol,
    pub name_span: Span,
    pub kind: VariantKind,
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
    StructDef {
        name: Symbol,
        name_span: Span,
        fields: Vec<(Symbol, Span)>,
    },
    EnumDef {
        name: Symbol,
        name_span: Span,
        variants: Vec<EnumVariant>,
    },
    Stmt(StmtNode),
    Use(UseItem),
}

#[derive(Debug, Clone, PartialEq)]
/// A pattern used in match arms and destructuring.
pub enum Pattern {
    /// Wildcard `_`
    Wild,
    /// Boolean literal
    BoolLit(bool),
    /// Integer literal
    IntLit(i64),
    /// Float literal
    FloatLit(f64),
    /// String literal
    StrLit(String),
    /// Unit `()`
    Unit,
    /// Variable binding
    Var(Symbol),
    /// Struct pattern `Point { x, y }`
    Struct {
        name: Symbol,
        fields: Vec<(Symbol, Pattern)>,
    },
    /// Enum variant pattern `Shape::Circle(r)`
    EnumVariant {
        enum_name: Symbol,
        variant_name: Symbol,
        args: Vec<Pattern>,
    },
    /// Some(x)
    OptionSome(Box<Pattern>),
    /// None
    OptionNone,
    /// Ok(x)
    ResultOk(Box<Pattern>),
    /// Err(e)
    ResultErr(Box<Pattern>),
}

/// A match arm: pattern + optional guard + body expression.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<ExprNode>,
    pub body: ExprNode,
}

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
