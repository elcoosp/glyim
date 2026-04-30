use glyim_diag::Span;
use glyim_interner::Symbol;

#[derive(Debug, Clone, PartialEq)]
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

#[derive(Debug, Clone, PartialEq)]
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
    SomeExpr(Box<ExprNode>),
    NoneExpr,
    OkExpr(Box<ExprNode>),
    ErrExpr(Box<ExprNode>),
    TryExpr(Box<ExprNode>),
    Pointer {
        mutable: bool,
        target: Symbol,
    },
    As {
        expr: Box<ExprNode>,
        target_type: Symbol,
    },
    MacroCall {
        name: Symbol,
        arg: Box<ExprNode>,
    },
    Match {
        scrutinee: Box<ExprNode>,
        arms: Vec<MatchArm>,
    },
    FieldAccess {
        object: Box<ExprNode>,
        field: Symbol,
    },
    MethodCall {
        receiver: Box<ExprNode>,
        method: Symbol,
        args: Vec<ExprNode>,
    },
    Call {
        callee: Box<ExprNode>,
        args: Vec<ExprNode>,
    },
    SizeOf(TypeExpr),
    TupleLit(Vec<ExprNode>),
    Deref(Box<ExprNode>),
    ForIn {
        pattern: Pattern,
        iter: Box<ExprNode>,
        body: Box<ExprNode>,
    },
    While {
        condition: Box<ExprNode>,
        body: Box<ExprNode>,
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
        pattern: Pattern,
        mutable: bool,
        value: ExprNode,
    },
    AssignDeref {
        target: Box<ExprNode>,
        value: ExprNode,
    },
    Assign {
        target: Symbol,
        value: ExprNode,
    },
    AssignField {
        object: Box<ExprNode>,
        field: Symbol,
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
pub enum VariantKind {
    Unnamed(Vec<(Symbol, Span, Option<TypeExpr>)>),
    Named(Vec<(Symbol, Span, Option<TypeExpr>)>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariantRepr {
    pub name: Symbol,
    pub name_span: Span,
    pub kind: VariantKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Wild,
    BoolLit(bool),
    IntLit(i64),
    FloatLit(f64),
    StrLit(String),
    Unit,
    Var(Symbol),
    Struct {
        name: Symbol,
        fields: Vec<(Symbol, Pattern)>,
    },
    EnumVariant {
        enum_name: Symbol,
        variant_name: Symbol,
        args: Vec<Pattern>,
    },
    Tuple(Vec<Pattern>),
    OptionSome(Box<Pattern>),
    OptionNone,
    ResultOk(Box<Pattern>),
    ResultErr(Box<Pattern>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<ExprNode>,
    pub body: ExprNode,
}

// *** CHANGED: carries types ***
#[derive(Debug, Clone, PartialEq)]
pub struct ExternFn {
    pub name: Symbol,
    pub name_span: Span,
    pub params: Vec<(Symbol, Span, Option<TypeExpr>)>,
    pub ret: Option<TypeExpr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeArg {
    pub key: String,
    pub value: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    pub name: String,
    pub args: Vec<AttributeArg>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Binding {
        name: Symbol,
        name_span: Span,
        value: ExprNode,
        attrs: Vec<Attribute>,
    },
    FnDef {
        name: Symbol,
        name_span: Span,
        type_params: Vec<Symbol>,
        params: Vec<(Symbol, Span, Option<TypeExpr>)>,
        ret: Option<TypeExpr>,
        body: ExprNode,
        attrs: Vec<Attribute>,
    },
    StructDef {
        name: Symbol,
        name_span: Span,
        type_params: Vec<Symbol>,
        fields: Vec<(Symbol, Span, Option<TypeExpr>)>,
    },
    EnumDef {
        name: Symbol,
        name_span: Span,
        type_params: Vec<Symbol>,
        variants: Vec<EnumVariantRepr>,
    },
    ImplBlock {
        target: Symbol,
        target_span: Span,
        type_params: Vec<Symbol>,
        is_pub: bool,
        methods: Vec<Item>,
        span: Span,
    },
    MacroDef {
        name: Symbol,
        name_span: Span,
        params: Vec<(Symbol, Span)>,
        body: ExprNode,
    },
    Stmt(StmtNode),
    Use(UseItem),
    ExternBlock {
        abi: String,
        span: Span,
        functions: Vec<ExternFn>,
    },
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

#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    Int,
    Float,
    Bool,
    Str,
    Unit,
    Named(Symbol),
    Generic(Symbol, Vec<TypeExpr>),
    Tuple(Vec<TypeExpr>),
    RawPtr { mutable: bool, inner: Box<TypeExpr> },
}
