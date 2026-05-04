use glyim_typeck::TypeChecker;
use glyim_hir::types::{ExprId, HirType};
use glyim_hir::node::{HirStmt, HirExpr, HirPattern};
use glyim_diag::Span;
use glyim_interner::Interner;

fn make_let_stmt(sym: glyim_interner::Symbol, mutable: bool, val: HirExpr) -> HirStmt {
    HirStmt::LetPat {
        pattern: HirPattern::Var(sym),
        mutable,
        value: val,
        ty: None,
        span: Span::new(0, 0),
    }
}

fn make_int_lit(val: i64) -> HirExpr {
    HirExpr::IntLit {
        id: ExprId::new(0),
        value: val,
        span: Span::new(0, 0),
    }
}

#[test]
fn check_let_binding_int() {
    let mut tc = TypeChecker::new(Interner::new());
    tc.push_scope();
    let x = tc.interner.intern("x");
    let stmt = make_let_stmt(x, false, make_int_lit(42));
    tc.check_stmt(&stmt);
    assert_eq!(tc.lookup_binding(&x), Some(HirType::Int));
}

#[test]
fn check_let_mutable_binding() {
    let mut tc = TypeChecker::new(Interner::new());
    tc.push_scope();
    let x = tc.interner.intern("x");
    let stmt = make_let_stmt(x, true, make_int_lit(10));
    tc.check_stmt(&stmt);
    assert_eq!(tc.lookup_binding(&x), Some(HirType::Int));
}

#[test]
fn check_let_with_type_annotation_mismatch_error() {
    let mut tc = TypeChecker::new(Interner::new());
    tc.push_scope();
    let x = tc.interner.intern("x");
    let stmt = HirStmt::LetPat {
        pattern: HirPattern::Var(x),
        mutable: false,
        value: make_int_lit(42),
        ty: Some(HirType::Float),
        span: Span::new(0, 0),
    };
    tc.check_stmt(&stmt);
    assert!(
        tc.errors.iter().any(|e| matches!(e, glyim_typeck::TypeError::MismatchedTypes { .. })),
        "Expected MismatchedTypes error, got {:?}",
        tc.errors
    );
}

#[test]
fn check_assign_to_immutable_error() {
    let mut tc = TypeChecker::new(Interner::new());
    tc.push_scope();
    let x = tc.interner.intern("x");
    tc.insert_binding(x, HirType::Int, false); // immutable
    let assign = HirStmt::Assign {
        target: x,
        value: make_int_lit(99),
        span: Span::new(0, 0),
    };
    tc.check_stmt(&assign);
    assert!(
        tc.errors.iter().any(|e| matches!(e, glyim_typeck::TypeError::AssignToImmutable { .. })),
        "Expected AssignToImmutable error, got {:?}",
        tc.errors
    );
}

#[test]
fn check_assign_deref_non_pointer_error() {
    let mut tc = TypeChecker::new(Interner::new());
    tc.push_scope();
    let x = tc.interner.intern("x");
    tc.insert_binding(x, HirType::Int, true); // mutable but Int not a pointer
    let assign_deref = HirStmt::AssignDeref {
        target: Box::new(HirExpr::Ident {
            id: ExprId::new(1), name: x, span: Span::new(0,0)
        }),
        value: make_int_lit(0),
        span: Span::new(0, 0),
    };
    tc.check_stmt(&assign_deref);
    assert!(
        tc.errors.iter().any(|e| matches!(e, glyim_typeck::TypeError::AssignThroughNonPointer { .. })),
        "Expected AssignThroughNonPointer error, got {:?}",
        tc.errors
    );
}

#[test]
fn check_assign_field_existing_struct() {
    let mut tc = TypeChecker::new(Interner::new());
    let point_sym = tc.interner.intern("Point");
    let x_sym = tc.interner.intern("x");
    let y_sym = tc.interner.intern("y");

    tc.structs.insert(point_sym, glyim_typeck::typeck::StructInfo {
        fields: vec![
            glyim_hir::item::StructField { name: x_sym, ty: HirType::Int, doc: None },
            glyim_hir::item::StructField { name: y_sym, ty: HirType::Int, doc: None },
        ],
        field_map: {
            let mut m = std::collections::HashMap::new();
            m.insert(x_sym, 0);
            m.insert(y_sym, 1);
            m
        },
    });
    tc.push_scope();
    let p = tc.interner.intern("p");
    tc.insert_binding(p, HirType::Named(point_sym), true);
    let assign_field = HirStmt::AssignField {
        object: Box::new(HirExpr::Ident { id: ExprId::new(2), name: p, span: Span::new(0,0) }),
        field: x_sym,
        value: make_int_lit(10),
        span: Span::new(0, 0),
    };
    tc.check_stmt(&assign_field);
    assert!(tc.errors.is_empty(), "Unexpected errors: {:?}", tc.errors);
}
