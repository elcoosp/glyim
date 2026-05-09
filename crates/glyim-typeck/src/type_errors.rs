use glyim_hir::types::{ExprId, HirType};
use glyim_interner::Symbol;
use miette::Diagnostic;

#[derive(thiserror::Error, Debug, Clone, PartialEq)]
pub enum TypeError {
    #[error("type mismatch: expected {expected:?}, found {found:?}")]
    MismatchedTypes {
        expected: HirType,
        found: HirType,
        expr_id: ExprId,
        span: (usize, usize),
    },
    #[error("unknown type: {name:?}")]
    UnknownType { name: Symbol },
    #[error("unknown field `{field}` on struct `{struct_name}`")]
    UnknownField {
        struct_name: String,
        field: String,
        span: (usize, usize),
    },
    #[error("missing field `{field}` in struct `{struct_name}`")]
    MissingField {
        struct_name: String,
        field: String,
        span: (usize, usize),
    },
    #[error("extra field `{field}` in struct `{struct_name}`")]
    ExtraField { struct_name: String, field: String },
    #[error("non-exhaustive match, missing variants: {missing:?}")]
    NonExhaustiveMatch {
        missing: Vec<String>,
        span: (usize, usize),
    },
    #[error("? operator used outside of Result-returning function")]
    InvalidQuestion { expr_id: ExprId },
    #[error("expected function call")]
    ExpectedFunction { expr_id: ExprId },
    #[error("invalid return type: expected {expected:?}, found {found:?}")]
    InvalidReturnType { expected: HirType, found: HirType },
    #[error("if condition must be `bool`, found `{found:?}`")]
    IfConditionMustBeBool { found: HirType, expr_id: ExprId },
    #[error("cannot assign to immutable `{name}`")]
    AssignToImmutable {
        name: String,
        expr_id: ExprId,
        span: (usize, usize),
    },
    #[error("cannot assign through non-pointer type `{found:?}`")]
    AssignThroughNonPointer {
        found: HirType,
        expr_id: ExprId,
        span: (usize, usize),
    },
    #[error("cannot dereference non-pointer type `{found:?}`")]
    DerefNonPointer {
        found: HirType,
        expr_id: ExprId,
        span: (usize, usize),
    },
    #[error("unresolved name `{name}`")]
    UnresolvedName {
        name: String,
        span: (usize, usize),
        suggestions: Vec<String>,
    },
    #[error("unresolved method `{method_name}` on type `{receiver_type}`")]
    UnresolvedMethod {
        method_name: String,
        receiver_type: String,
        span: (usize, usize),
    },
}

impl Diagnostic for TypeError {
    fn severity(&self) -> Option<miette::Severity> {
        Some(miette::Severity::Error)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = miette::LabeledSpan> + '_>> {
        match self {
            TypeError::MismatchedTypes { span, .. } => Some(Box::new(std::iter::once(
                miette::LabeledSpan::new(Some(format!("type mismatch")), span.0, span.1 - span.0),
            ))),
            TypeError::UnknownField { span, .. } => Some(Box::new(std::iter::once(
                miette::LabeledSpan::new(Some("unknown field".into()), span.0, span.1 - span.0),
            ))),
            TypeError::MissingField { span, .. } => Some(Box::new(std::iter::once(
                miette::LabeledSpan::new(Some("missing field".into()), span.0, span.1 - span.0),
            ))),
            TypeError::NonExhaustiveMatch { span, .. } => {
                Some(Box::new(std::iter::once(miette::LabeledSpan::new(
                    Some("non-exhaustive match".into()),
                    span.0,
                    span.1 - span.0,
                ))))
            }
            TypeError::InvalidReturnType { .. } => None,
            TypeError::AssignToImmutable { span, .. } => {
                Some(Box::new(std::iter::once(miette::LabeledSpan::new(
                    Some("cannot assign to immutable".into()),
                    span.0,
                    span.1 - span.0,
                ))))
            }
            TypeError::DerefNonPointer { span, .. } => {
                Some(Box::new(std::iter::once(miette::LabeledSpan::new(
                    Some("cannot dereference non-pointer".into()),
                    span.0,
                    span.1 - span.0,
                ))))
            }
            TypeError::UnresolvedMethod { span, .. } => Some(Box::new(std::iter::once(
                miette::LabeledSpan::new(Some("unresolved method".into()), span.0, span.1 - span.0),
            ))),
            TypeError::UnresolvedName { span, .. } => Some(Box::new(std::iter::once(
                miette::LabeledSpan::new(Some("unresolved name".into()), span.0, span.1 - span.0),
            ))),
            _ => None,
        }
    }
}

impl From<TypeError> for glyim_diag::diagnostic::Diagnostic {
    fn from(err: TypeError) -> glyim_diag::diagnostic::Diagnostic {
        let (start, end, msg) = match &err {
            TypeError::MismatchedTypes { span, .. } => (span.0, span.1, err.to_string()),
            TypeError::UnknownType { .. } => (0, 0, err.to_string()),
            TypeError::UnknownField { span, .. } => (span.0, span.1, err.to_string()),
            TypeError::MissingField { span, .. } => (span.0, span.1, err.to_string()),
            TypeError::ExtraField { .. } => (0, 0, err.to_string()),
            TypeError::NonExhaustiveMatch { span, .. } => (span.0, span.1, err.to_string()),
            TypeError::InvalidQuestion { .. } => (0, 0, err.to_string()),
            TypeError::ExpectedFunction { .. } => (0, 0, err.to_string()),
            TypeError::InvalidReturnType { .. } => (0, 0, err.to_string()),
            TypeError::IfConditionMustBeBool { .. } => (0, 0, err.to_string()),
            TypeError::AssignToImmutable { span, .. } => (span.0, span.1, err.to_string()),
            TypeError::AssignThroughNonPointer { span, .. } => (span.0, span.1, err.to_string()),
            TypeError::DerefNonPointer { span, .. } => (span.0, span.1, err.to_string()),
            TypeError::UnresolvedMethod { span, .. } => (span.0, span.1, err.to_string()),
            TypeError::UnresolvedName { span, .. } => (span.0, span.1, err.to_string()),
        };
        glyim_diag::diagnostic::Diagnostic {
            severity: glyim_diag::diagnostic::Severity::Error,
            file: None,
            span: glyim_diag::Span::new(start, end),
            message: msg,
            code: None,
            suggestion: None,
        }
    }
}
