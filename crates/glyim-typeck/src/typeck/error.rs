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
    #[error("unknown field {field:?} on struct {struct_name:?}")]
    UnknownField {
        struct_name: Symbol,
        field: Symbol,
        span: (usize, usize),
    },
    #[error("missing field {field:?} in struct {struct_name:?}")]
    MissingField {
        struct_name: Symbol,
        field: Symbol,
        span: (usize, usize),
    },
    #[error("extra field {field:?} in struct {struct_name:?}")]
    ExtraField { struct_name: Symbol, field: Symbol },
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
    #[error("cannot assign to immutable binding")]
    AssignToImmutable {
        name: Symbol,
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
    #[error("unresolved name: {name:?}")]
    UnresolvedName { name: Symbol, span: (usize, usize) },
}

impl Diagnostic for TypeError {
    fn severity(&self) -> Option<miette::Severity> {
        Some(miette::Severity::Error)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = miette::LabeledSpan> + '_>> {
        match self {
            TypeError::MismatchedTypes { span, expected, found, .. } => {
                Some(Box::new(std::iter::once(miette::LabeledSpan::new(
                    Some(format!("expected {:?}, found {:?}", expected, found)),
                    span.0,
                    span.1 - span.0,
                ))))
            }
            _ => None,
        }
    }

    fn related<'a>(&'a self) -> Option<Box<dyn Iterator<Item = &'a dyn miette::Diagnostic> + 'a>> {
        // Get the primary span from any error variant that carries one
        let primary_span: Option<(usize, usize)> = match self {
            TypeError::MismatchedTypes { span, .. } => Some(*span),
            TypeError::AssignToImmutable { span, .. } => Some(*span),
            TypeError::AssignThroughNonPointer { span, .. } => Some(*span),
            TypeError::DerefNonPointer { span, .. } => Some(*span),
            TypeError::UnresolvedName { span, .. } => Some(*span),
            TypeError::NonExhaustiveMatch { span, .. } => Some(*span),
            TypeError::UnknownField { span, .. } => Some(*span),
            TypeError::MissingField { span, .. } => Some(*span),
            _ => None,
        };

        // Build the expansion chain by walking the interning table
        let mut notes: Vec<Box<dyn miette::Diagnostic>> = Vec::new();

        // We don't have expansion_id on the error's span directly;
        // this will be activated when tokens carry expansion IDs.
        // For now, we'll return an empty iterator.

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
