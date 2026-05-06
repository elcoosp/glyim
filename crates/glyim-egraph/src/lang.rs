use egg::{Language, Id};

/// Operator tags for GlyimExpr.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GlyimOp {
    Num,
    FNum,
    BoolLit,
    StrLit,
    Var,
    BinOp(glyim_hir::node::HirBinOp),
    UnOp(glyim_hir::node::HirUnOp),
    Call,
    If,
    MethodCall,
    FieldAccess,
    StructLit,
    EnumVariant,
}

/// A Glyim expression node in the e-graph.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GlyimExpr {
    pub op: GlyimOp,
    pub data: String, // stores the immediate value (number as string, name, etc.)
    pub children: Vec<Id>,
}

impl GlyimExpr {
    pub fn num(n: i64) -> Self {
        GlyimExpr { op: GlyimOp::Num, data: n.to_string(), children: vec![] }
    }
    pub fn fnum(bits: u64) -> Self {
        GlyimExpr { op: GlyimOp::FNum, data: bits.to_string(), children: vec![] }
    }
    pub fn bool_lit(b: bool) -> Self {
        GlyimExpr { op: GlyimOp::BoolLit, data: b.to_string(), children: vec![] }
    }
    pub fn str_lit(s: &str) -> Self {
        GlyimExpr { op: GlyimOp::StrLit, data: s.to_string(), children: vec![] }
    }
    pub fn var(name: &str) -> Self {
        GlyimExpr { op: GlyimOp::Var, data: name.to_string(), children: vec![] }
    }
    pub fn bin_op(op: glyim_hir::node::HirBinOp, lhs: Id, rhs: Id) -> Self {
        GlyimExpr { op: GlyimOp::BinOp(op), data: String::new(), children: vec![lhs, rhs] }
    }
    pub fn un_op(op: glyim_hir::node::HirUnOp, inner: Id) -> Self {
        GlyimExpr { op: GlyimOp::UnOp(op), data: String::new(), children: vec![inner] }
    }
    pub fn call(name: &str, args: Vec<Id>) -> Self {
        GlyimExpr { op: GlyimOp::Call, data: name.to_string(), children: args }
    }
    pub fn if_expr(cond: Id, then: Id, else_: Id) -> Self {
        GlyimExpr { op: GlyimOp::If, data: String::new(), children: vec![cond, then, else_] }
    }
    pub fn method_call(name: &str, recv: Id, args: Vec<Id>) -> Self {
        let mut children = vec![recv];
        children.extend(args);
        GlyimExpr { op: GlyimOp::MethodCall, data: name.to_string(), children }
    }
    pub fn field_access(obj: Id, field: &str) -> Self {
        GlyimExpr { op: GlyimOp::FieldAccess, data: field.to_string(), children: vec![obj] }
    }
    pub fn struct_lit(name: &str, fields: Vec<(String, Id)>) -> Self {
        let children: Vec<Id> = fields.iter().map(|(_, id)| *id).collect();
        GlyimExpr { op: GlyimOp::StructLit, data: name.to_string(), children }
    }
    pub fn enum_variant(enum_name: &str, variant_name: &str, args: Vec<Id>) -> Self {
        GlyimExpr { op: GlyimOp::EnumVariant, data: format!("{}::{}", enum_name, variant_name), children: args }
    }
}

impl Language for GlyimExpr {
    type Discriminant = GlyimOp;
    fn discriminant(&self) -> Self::Discriminant { self.op }
    fn matches(&self, other: &Self) -> bool { self.op == other.op && self.children.len() == other.children.len() }
    fn children(&self) -> &[Id] { &self.children }
    fn children_mut(&mut self) -> &mut [Id] { &mut self.children }
}

impl std::fmt::Display for GlyimExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.op {
            GlyimOp::Num => write!(f, "{}", self.data),
            GlyimOp::FNum => write!(f, "{}f", self.data),
            GlyimOp::BoolLit => write!(f, "{}", self.data),
            GlyimOp::StrLit => write!(f, "\"{}\"", self.data),
            GlyimOp::Var => write!(f, "{}", self.data),
            GlyimOp::BinOp(op) => write!(f, "{:?}", op),
            GlyimOp::UnOp(op) => write!(f, "{:?}", op),
            GlyimOp::Call => write!(f, "call {}", self.data),
            GlyimOp::If => write!(f, "if"),
            GlyimOp::MethodCall => write!(f, "method {}", self.data),
            GlyimOp::FieldAccess => write!(f, ".{}", self.data),
            GlyimOp::StructLit => write!(f, "struct {}", self.data),
            GlyimOp::EnumVariant => write!(f, "enum {}", self.data),
        }
    }
}
