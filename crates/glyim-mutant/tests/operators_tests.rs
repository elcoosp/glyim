use glyim_mutant::config::MutationOperator;
use glyim_mutant::operators::apply_operator;
use glyim_hir::{HirExpr, HirBinOp, HirUnOp};
use glyim_hir::types::ExprId;
use glyim_diag::Span;

fn span() -> Span { Span::new(0,0) }
fn id() -> ExprId { ExprId::new(0) }

#[test]
fn plus_to_minus() {
    let expr = HirExpr::Binary {
        id: id(), op: HirBinOp::Add,
        lhs: Box::new(HirExpr::IntLit { id: id(), value: 1, span: span() }),
        rhs: Box::new(HirExpr::IntLit { id: id(), value: 2, span: span() }),
        span: span(),
    };
    let mutated = apply_operator(&expr, MutationOperator::ArithmeticPlusToMinus).unwrap();
    assert!(matches!(mutated, HirExpr::Binary { op: HirBinOp::Sub, .. }));
}

#[test]
fn minus_to_plus() {
    let expr = HirExpr::Binary {
        id: id(), op: HirBinOp::Sub,
        lhs: Box::new(HirExpr::IntLit { id: id(), value: 5, span: span() }),
        rhs: Box::new(HirExpr::IntLit { id: id(), value: 3, span: span() }),
        span: span(),
    };
    let mutated = apply_operator(&expr, MutationOperator::ArithmeticMinusToPlus).unwrap();
    assert!(matches!(mutated, HirExpr::Binary { op: HirBinOp::Add, .. }));
}

#[test]
fn eq_to_neq() {
    let expr = HirExpr::Binary {
        id: id(), op: HirBinOp::Eq,
        lhs: Box::new(HirExpr::IntLit { id: id(), value: 1, span: span() }),
        rhs: Box::new(HirExpr::IntLit { id: id(), value: 1, span: span() }),
        span: span(),
    };
    let mutated = apply_operator(&expr, MutationOperator::CompareEqualToNotEqual).unwrap();
    assert!(matches!(mutated, HirExpr::Binary { op: HirBinOp::Neq, .. }));
}

#[test]
fn not_elimination() {
    let expr = HirExpr::Unary {
        id: id(), op: HirUnOp::Not,
        operand: Box::new(HirExpr::BoolLit { id: id(), value: true, span: span() }),
        span: span(),
    };
    let mutated = apply_operator(&expr, MutationOperator::BooleanNotElimination).unwrap();
    assert!(matches!(mutated, HirExpr::BoolLit { value: true, .. }));
}

#[test]
fn constant_zero() {
    let expr = HirExpr::IntLit { id: id(), value: 42, span: span() };
    let mutated = apply_operator(&expr, MutationOperator::ConstantZero).unwrap();
    assert_eq!(mutated, HirExpr::IntLit { id: id(), value: 0, span: span() });
}

#[test]
fn conditional_flip() {
    let condition = Box::new(HirExpr::BoolLit { id: ExprId::new(1), value: true, span: span() });
    let then_branch = Box::new(HirExpr::IntLit { id: ExprId::new(2), value: 1, span: span() });
    let else_branch = Box::new(HirExpr::IntLit { id: ExprId::new(3), value: 2, span: span() });
    let expr = HirExpr::If {
        id: ExprId::new(0),
        condition: condition.clone(),
        then_branch: then_branch.clone(),
        else_branch: Some(else_branch.clone()),
        span: span(),
    };
    let mutated = apply_operator(&expr, MutationOperator::ConditionalFlip).unwrap();
    if let HirExpr::If { condition: new_cond, .. } = mutated {
        assert!(matches!(*new_cond, HirExpr::Unary { op: HirUnOp::Not, .. }));
    } else {
        panic!("Expected If");
    }
}

#[test]
fn operator_not_applicable_returns_none() {
    // Trying to flip conditional on a non-If
    let expr = HirExpr::IntLit { id: id(), value: 5, span: span() };
    assert!(apply_operator(&expr, MutationOperator::ConditionalFlip).is_none());
}
