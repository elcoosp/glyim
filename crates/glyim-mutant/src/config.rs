use serde::{Deserialize, Serialize};

/// The set of mutation operators supported by the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MutationOperator {
    // Arithmetic
    ArithmeticPlusToMinus,
    ArithmeticMinusToPlus,
    ArithmeticMulToDiv,
    ArithmeticDivToMul,
    // Comparison
    CompareLessToLessEqual,
    CompareGreaterToGreaterEqual,
    CompareEqualToNotEqual,
    CompareNotEqualToEqual,
    CompareLessEqualToLess,
    CompareGreaterEqualToGreater,
    // Boolean
    BooleanAndToOr,
    BooleanOrToAnd,
    BooleanNotElimination,
    // Constants
    ConstantZero,
    ConstantOne,
    ConstantBoundary,
    // Statements
    StatementDeletion,
    // Conditionals
    ConditionalFlip,
    // Return values
    ReturnValueZero,
    ReturnValueNegate,
}

#[derive(Debug, Clone)]
pub struct MutationConfig {
    pub operators: Vec<MutationOperator>,
    pub skip_pure: bool,
    pub skip_tests: bool,
    pub max_mutations_per_fn: usize,
    pub detect_equivalents: bool,
}

impl Default for MutationConfig {
    fn default() -> Self {
        Self {
            operators: vec![
                MutationOperator::ArithmeticPlusToMinus,
                MutationOperator::ArithmeticMinusToPlus,
                MutationOperator::ArithmeticMulToDiv,
                MutationOperator::ArithmeticDivToMul,
                MutationOperator::CompareEqualToNotEqual,
                MutationOperator::CompareNotEqualToEqual,
                MutationOperator::BooleanAndToOr,
                MutationOperator::BooleanOrToAnd,
                MutationOperator::BooleanNotElimination,
                MutationOperator::ConstantZero,
                MutationOperator::StatementDeletion,
                MutationOperator::ConditionalFlip,
            ],
            skip_pure: true,
            skip_tests: true,
            max_mutations_per_fn: 50,
            detect_equivalents: true,
        }
    }
}
