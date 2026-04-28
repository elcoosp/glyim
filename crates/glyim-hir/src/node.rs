use glyim_interner::Symbol;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HirBinOp {
    Add, Sub, Mul, Div, Mod,
    Eq, Neq, Lt, Gt, Lte, Gte,
    And, Or,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HirUnOp { Neg, Not }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HirStmt {
    Let { name: Symbol, mutable: bool, value: HirExpr },
    Assign { target: Symbol, value: HirExpr },
    Expr(HirExpr),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HirExpr {
    IntLit(i64),
    StrLit(String),
    Ident(Symbol),
    Binary { op: HirBinOp, lhs: Box<HirExpr>, rhs: Box<HirExpr> },
    Unary { op: HirUnOp, operand: Box<HirExpr> },
    Block(Vec<HirStmt>),
    If {
        condition: Box<HirExpr>,
        then_branch: Box<HirExpr>,
        else_branch: Option<Box<HirExpr>>,
    },
    Println(Box<HirExpr>),
    Assert {
        condition: Box<HirExpr>,
        message: Option<Box<HirExpr>>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirFn {
    pub name: Symbol,
    pub params: Vec<Symbol>,
    pub body: HirExpr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hir { pub fns: Vec<HirFn> }
