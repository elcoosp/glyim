//! High-level Intermediate Representation — the codegen's input.

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
pub enum HirExpr {
    IntLit(i64),
    Ident(Symbol),
    Binary { op: HirBinOp, lhs: Box<HirExpr>, rhs: Box<HirExpr> },
    Unary { op: HirUnOp, operand: Box<HirExpr> },
    Block(Vec<HirExpr>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirFn {
    pub name: Symbol,
    pub params: Vec<Symbol>,
    pub body: HirExpr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hir {
    pub fns: Vec<HirFn>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_interner::Interner;

    #[test] fn hir_int_lit() { assert_eq!(HirExpr::IntLit(42), HirExpr::IntLit(42)); }
    #[test] fn hir_fn_shape() {
        let mut interner = Interner::new();
        let name = interner.intern("main");
        let f = HirFn { name, params: vec![], body: HirExpr::IntLit(42) };
        assert_eq!(f.name, name);
        assert!(f.params.is_empty());
    }
    #[test] fn hir_holds_fns() { assert!(Hir { fns: vec![] }.fns.is_empty()); }
}
