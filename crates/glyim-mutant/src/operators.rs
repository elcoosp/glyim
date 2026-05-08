use glyim_hir::{HirExpr, HirBinOp, HirUnOp};
use crate::config::MutationOperator;

/// Generate a mutated expression if the operator applies.
pub fn apply_operator(
    expr: &HirExpr,
    op: MutationOperator,
) -> Option<HirExpr> {
    match op {
        MutationOperator::ArithmeticPlusToMinus => {
            if let HirExpr::Binary { id, op: HirBinOp::Add, lhs, rhs, span } = expr {
                Some(HirExpr::Binary { id: *id, op: HirBinOp::Sub, lhs: lhs.clone(), rhs: rhs.clone(), span: *span })
            } else { None }
        },
        MutationOperator::ArithmeticMinusToPlus => {
            if let HirExpr::Binary { id, op: HirBinOp::Sub, lhs, rhs, span } = expr {
                Some(HirExpr::Binary { id: *id, op: HirBinOp::Add, lhs: lhs.clone(), rhs: rhs.clone(), span: *span })
            } else { None }
        },
        MutationOperator::ArithmeticMulToDiv => {
            if let HirExpr::Binary { id, op: HirBinOp::Mul, lhs, rhs, span } = expr {
                Some(HirExpr::Binary { id: *id, op: HirBinOp::Div, lhs: lhs.clone(), rhs: rhs.clone(), span: *span })
            } else { None }
        },
        MutationOperator::ArithmeticDivToMul => {
            if let HirExpr::Binary { id, op: HirBinOp::Div, lhs, rhs, span } = expr {
                Some(HirExpr::Binary { id: *id, op: HirBinOp::Mul, lhs: lhs.clone(), rhs: rhs.clone(), span: *span })
            } else { None }
        },
        MutationOperator::CompareEqualToNotEqual => {
            if let HirExpr::Binary { id, op: HirBinOp::Eq, lhs, rhs, span } = expr {
                Some(HirExpr::Binary { id: *id, op: HirBinOp::Neq, lhs: lhs.clone(), rhs: rhs.clone(), span: *span })
            } else { None }
        },
        MutationOperator::CompareNotEqualToEqual => {
            if let HirExpr::Binary { id, op: HirBinOp::Neq, lhs, rhs, span } = expr {
                Some(HirExpr::Binary { id: *id, op: HirBinOp::Eq, lhs: lhs.clone(), rhs: rhs.clone(), span: *span })
            } else { None }
        },
        MutationOperator::BooleanAndToOr => {
            if let HirExpr::Binary { id, op: HirBinOp::And, lhs, rhs, span } = expr {
                Some(HirExpr::Binary { id: *id, op: HirBinOp::Or, lhs: lhs.clone(), rhs: rhs.clone(), span: *span })
            } else { None }
        },
        MutationOperator::BooleanOrToAnd => {
            if let HirExpr::Binary { id, op: HirBinOp::Or, lhs, rhs, span } = expr {
                Some(HirExpr::Binary { id: *id, op: HirBinOp::And, lhs: lhs.clone(), rhs: rhs.clone(), span: *span })
            } else { None }
        },
        MutationOperator::BooleanNotElimination => {
            if let HirExpr::Unary { id: _, op: HirUnOp::Not, operand, span: _ } = expr {
                Some(*operand.clone())
            } else { None }
        },
        MutationOperator::ConstantZero => {
            match expr {
                HirExpr::IntLit { id, value: _, span } => Some(HirExpr::IntLit { id: *id, value: 0, span: *span }),
                HirExpr::FloatLit { id, value: _, span } => Some(HirExpr::FloatLit { id: *id, value: 0.0, span: *span }),
                HirExpr::BoolLit { id, value: _, span } => Some(HirExpr::BoolLit { id: *id, value: false, span: *span }),
                _ => None,
            }
        },
        MutationOperator::StatementDeletion => {
            // This is handled at the statement level by engine.rs
            None
        },
        MutationOperator::ConditionalFlip => {
            if let HirExpr::If { id, condition, then_branch, else_branch, span } = expr {
                Some(HirExpr::If {
                    id: *id,
                    condition: Box::new(HirExpr::Unary {
                        id: glyim_hir::types::ExprId::new(0),
                        op: HirUnOp::Not,
                        operand: condition.clone(),
                        span: *span,
                    }),
                    then_branch: then_branch.clone(),
                    else_branch: else_branch.clone(),
                    span: *span,
                })
            } else { None }
        },
        // Other operators stubbed for now
        _ => None,
    }
}
