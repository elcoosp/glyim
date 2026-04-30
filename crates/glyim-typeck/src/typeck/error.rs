use miette::Diagnostic;
use glyim_hir::types::{ExprId, HirType};
use glyim_interner::Symbol;

#[derive(thiserror::Error, Debug, Clone, PartialEq)]
pub enum TypeError {
    #[error("type mismatch: expected {expected:?}, found {found:?}")]
    MismatchedTypes {
        expected: HirType,
        found: HirType,
        expr_id: ExprId,
    },
    #[error("unknown type: {name:?}")]
    UnknownType { name: Symbol },
    #[error("unknown field {field:?} on struct {struct_name:?}")]
    UnknownField {
        struct_name: Symbol,
        field: Symbol,
    },
    #[error("missing field {field:?} in struct {struct_name:?}")]
    MissingField {
        struct_name: Symbol,
        field: Symbol,
    },
    #[error("extra field {field:?} in struct {struct_name:?}")]
    ExtraField {
        struct_name: Symbol,
        field: Symbol,
    },
    #[error("non-exhaustive match, missing variants: {missing:?}")]
    NonExhaustiveMatch { missing: Vec<String> },
    #[error("? operator used outside of Result-returning function")]
    InvalidQuestion { expr_id: ExprId },
    #[error("expected function call")]
    ExpectedFunction { expr_id: ExprId },
    #[error("invalid return type: expected {expected:?}, found {found:?}")]
    InvalidReturnType {
        expected: HirType,
        found: HirType,
    },
}

impl Diagnostic for TypeError {


    fn severity(&self) -> Option<miette::Severity> {
        Some(miette::Severity::Error)
    }

    fn help<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        None
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = miette::LabeledSpan>>> {
        // Type errors don't have byte spans yet, so no labels.
        // In the future, we can attach spans when available.
        None
    }
}

