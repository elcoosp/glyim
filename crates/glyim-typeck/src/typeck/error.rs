use glyim_hir::types::{ExprId, HirType};
use glyim_interner::Symbol;

#[derive(Debug, Clone, PartialEq)]
pub enum TypeError {
    MismatchedTypes {
        expected: HirType,
        found: HirType,
        expr_id: ExprId,
    },
    UnknownType {
        name: Symbol,
    },
    UnknownField {
        struct_name: Symbol,
        field: Symbol,
    },
    MissingField {
        struct_name: Symbol,
        field: Symbol,
    },
    ExtraField {
        struct_name: Symbol,
        field: Symbol,
    },
    NonExhaustiveMatch {
        missing: Vec<String>,
    },
    InvalidQuestion {
        expr_id: ExprId,
    },
    ExpectedFunction {
        expr_id: ExprId,
    },
    InvalidReturnType {
        expected: HirType,
        found: HirType,
    },
}

impl std::fmt::Display for TypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypeError::MismatchedTypes { expected, found, .. } => {
                write!(f, "type mismatch: expected {:?}, found {:?}", expected, found)
            }
            TypeError::UnknownType { name } => write!(f, "unknown type: {:?}", name),
            TypeError::UnknownField { struct_name, field } => {
                write!(f, "unknown field '{:?}' on struct '{:?}'", field, struct_name)
            }
            TypeError::MissingField { struct_name, field } => {
                write!(f, "missing field '{:?}' in struct '{:?}'", field, struct_name)
            }
            TypeError::ExtraField { struct_name, field } => {
                write!(f, "extra field '{:?}' in struct '{:?}'", field, struct_name)
            }
            TypeError::NonExhaustiveMatch { missing } => {
                write!(f, "non-exhaustive match, missing variants: {:?}", missing)
            }
            TypeError::InvalidQuestion { .. } => {
                write!(f, "? operator used outside of Result-returning function")
            }
            TypeError::ExpectedFunction { .. } => write!(f, "expected function call"),
            TypeError::InvalidReturnType { expected, found } => {
                write!(f, "invalid return type: expected {:?}, found {:?}", expected, found)
            }
        }
    }
}

impl std::error::Error for TypeError {}
