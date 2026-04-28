//! Typed Abstract Syntax Tree — the compiler pipeline's primary data structure.
use glyim_diag::Span;
use glyim_interner::Symbol;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BinOp { Add, Sub, Mul, Div, Mod, Eq, Neq, Lt, Gt, Lte, Gte, And, Or }

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum UnOp { Neg, Not }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExprNode { pub kind: ExprKind, pub span: Span }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExprKind {
    IntLit(i64), Ident(Symbol),
    Binary { op: BinOp, lhs: Box<ExprNode>, rhs: Box<ExprNode> },
    Unary { op: UnOp, operand: Box<ExprNode> },
    Lambda { params: Vec<Symbol>, body: Box<ExprNode> },
    Block(Vec<ExprNode>),
    Call { callee: Box<ExprNode>, args: Vec<ExprNode> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Binding { name: Symbol, name_span: Span, value: ExprNode },
    FnDef { name: Symbol, name_span: Span, params: Vec<(Symbol, Span)>, body: ExprNode },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ast { pub items: Vec<Item> }

impl ExprNode {
    #[cfg(test)] pub fn dummy(kind: ExprKind) -> Self { Self { kind, span: Span::new(0,0) } }
    pub fn int_lit(value: i64, span: Span) -> Self { Self { kind: ExprKind::IntLit(value), span } }
    pub fn ident(sym: Symbol, span: Span) -> Self { Self { kind: ExprKind::Ident(sym), span } }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_interner::Interner;

    #[test] fn expr_node_int_lit_match() {
        let node = ExprNode { kind: ExprKind::IntLit(42), span: Span::new(0,2) };
        assert!(matches!(&node.kind, ExprKind::IntLit(42)));
    }
    #[test] fn int_lit_convenience() {
        let node = ExprNode::int_lit(42, Span::new(5,7));
        assert_eq!(node.kind, ExprKind::IntLit(42));
        assert_eq!(node.span.start, 5);
    }
    #[test] fn ident_convenience() {
        let mut i = Interner::new(); let s = i.intern("x");
        let node = ExprNode::ident(s, Span::new(0,1));
        assert_eq!(node.kind, ExprKind::Ident(s));
    }
    #[test] fn binary_expr_shape() {
        let lhs = ExprNode::int_lit(1, Span::new(0,1));
        let rhs = ExprNode::int_lit(2, Span::new(4,5));
        let node = ExprNode { kind: ExprKind::Binary { op: BinOp::Add, lhs: Box::new(lhs), rhs: Box::new(rhs) }, span: Span::new(0,5) };
        assert!(matches!(&node.kind, ExprKind::Binary { op: BinOp::Add, .. }));
    }
    #[test] fn lambda_expr_shape() {
        let mut i = Interner::new(); let p = i.intern("x");
        let body = ExprNode::ident(p, Span::new(8,9));
        let node = ExprNode { kind: ExprKind::Lambda { params: vec![p], body: Box::new(body) }, span: Span::new(0,9) };
        assert!(matches!(&node.kind, ExprKind::Lambda { params, .. } if params.len() == 1));
    }
    #[test] fn item_binding_shape() {
        let mut i = Interner::new(); let name = i.intern("main");
        let item = Item::Binding { name, name_span: Span::new(0,4), value: ExprNode::int_lit(42, Span::new(7,9)) };
        assert!(matches!(item, Item::Binding { .. }));
    }
    #[test] fn item_fn_def_shape() {
        let mut i = Interner::new(); let name = i.intern("f"); let a = i.intern("a");
        let item = Item::FnDef { name, name_span: Span::new(3,4), params: vec![(a, Span::new(5,6))], body: ExprNode::ident(a, Span::new(9,10)) };
        assert!(matches!(item, Item::FnDef { params, .. } if params.len() == 1));
    }
    #[test] fn ast_holds_items() { let ast = Ast { items: vec![] }; assert!(ast.items.is_empty()); }
}
