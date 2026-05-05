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
    },
    #[error("unknown type: {name:?}")]
    UnknownType { name: Symbol },
    #[error("unknown field {field:?} on struct {struct_name:?}")]
    UnknownField { struct_name: Symbol, field: Symbol },
    #[error("missing field {field:?} in struct {struct_name:?}")]
    MissingField { struct_name: Symbol, field: Symbol },
    #[error("extra field {field:?} in struct {struct_name:?}")]
    ExtraField { struct_name: Symbol, field: Symbol },
    #[error("non-exhaustive match, missing variants: {missing:?}")]
    NonExhaustiveMatch { missing: Vec<String> },
    #[error("? operator used outside of Result-returning function")]
    InvalidQuestion { expr_id: ExprId },
    #[error("expected function call")]
    ExpectedFunction { expr_id: ExprId },
    #[error("invalid return type: expected {expected:?}, found {found:?}")]
    InvalidReturnType { expected: HirType, found: HirType },
    #[error("if condition must be `bool`, found `{found:?}`")]
    IfConditionMustBeBool { found: HirType, expr_id: ExprId },
    #[error("cannot assign to immutable binding")]
    AssignToImmutable { name: Symbol, expr_id: ExprId },
    #[error("cannot assign through non-pointer type `{found:?}`")]
    AssignThroughNonPointer { found: HirType, expr_id: ExprId },
    #[error("cannot dereference non-pointer type `{found:?}`")]
    DerefNonPointer { found: HirType, expr_id: ExprId },
    #[error("unresolved name: {name:?}")]
    UnresolvedName { name: Symbol },
}

impl Diagnostic for TypeError {
    fn severity(&self) -> Option<miette::Severity> {
        Some(miette::Severity::Error)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = miette::LabeledSpan> + '_>> {
        None
    }

    fn related<'a>(&'a self) -> Option<Box<dyn Iterator<Item = &'a dyn miette::Diagnostic> + 'a>> {
        // TODO: walk the span expansion chain and emit "note: expanded from macro X"
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
