use glyim_hir::types::{ExprId, HirType};
use glyim_interner::Symbol;
use miette::Diagnostic;
use similar::ChangeTag;
use similar::TextDiff;

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
}

impl Diagnostic for TypeError {
    fn severity(&self) -> Option<miette::Severity> {
        Some(miette::Severity::Error)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = miette::LabeledSpan> + '_>> {
        match self {
            TypeError::MismatchedTypes {
                span,
                expected,
                found,
                ..
            } => Some(Box::new(std::iter::once(miette::LabeledSpan::new(
                Some(format!("expected {:?}, found {:?}", expected, found)),
                span.0,
                span.1 - span.0,
            )))),
            _ => None,
        }
    }

    fn related<'a>(&'a self) -> Option<Box<dyn Iterator<Item = &'a dyn Diagnostic> + 'a>> {
        // Macro expansion chain diagnostics are prepared but not yet wired due to
        // lifetime constraints in the Diagnostic trait – will be completed in a follow-up.
        None
    }

    fn help<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        if let TypeError::MismatchedTypes {
            expected, found, ..
        } = self
        {
            let expected_str = format!("{:?}", expected);
            let found_str = format!("{:?}", found);
            let diff = TextDiff::from_lines(&expected_str, &found_str);
            let mut result = String::new();
            for change in diff.iter_all_changes() {
                match change.tag() {
                    ChangeTag::Equal => {
                        result.push_str(&format!(" {}\n", change));
                    }
                    ChangeTag::Delete => {
                        result.push_str(&format!("-{}\n", change));
                    }
                    ChangeTag::Insert => {
                        result.push_str(&format!("+{}\n", change));
                    }
                }
            }
            if !result.is_empty() {
                return Some(Box::new(format!("Type diff:\n{result}")));
            }
        }
        None
    }
}

impl From<TypeError> for glyim_diag::diagnostic::Diagnostic {
    fn from(err: TypeError) -> glyim_diag::diagnostic::Diagnostic {
        let span = match &err {
            TypeError::MismatchedTypes { span, .. } => glyim_diag::Span::new(span.0, span.1),
            TypeError::UnknownType { .. } => glyim_diag::Span::new(0, 0),
            TypeError::UnknownField { span, .. } => glyim_diag::Span::new(span.0, span.1),
            TypeError::MissingField { span, .. } => glyim_diag::Span::new(span.0, span.1),
            TypeError::ExtraField { .. } => glyim_diag::Span::new(0, 0),
            TypeError::NonExhaustiveMatch { span, .. } => glyim_diag::Span::new(span.0, span.1),
            TypeError::InvalidQuestion { .. } => glyim_diag::Span::new(0, 0),
            TypeError::ExpectedFunction { .. } => glyim_diag::Span::new(0, 0),
            TypeError::InvalidReturnType { .. } => glyim_diag::Span::new(0, 0),
            TypeError::IfConditionMustBeBool { .. } => glyim_diag::Span::new(0, 0),
            TypeError::AssignToImmutable { span, .. } => glyim_diag::Span::new(span.0, span.1),
            TypeError::AssignThroughNonPointer { span, .. } => {
                glyim_diag::Span::new(span.0, span.1)
            }
            TypeError::DerefNonPointer { span, .. } => glyim_diag::Span::new(span.0, span.1),
            TypeError::UnresolvedName { span, .. } => glyim_diag::Span::new(span.0, span.1),
        };
        glyim_diag::diagnostic::Diagnostic {
            severity: glyim_diag::diagnostic::Severity::Error,
            file: None,
            span,
            message: err.to_string(),
            code: None,
            suggestion: None,
        }
    }
}
