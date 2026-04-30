use glyim_hir::types::{ExprId, HirType};
use glyim_interner::Symbol;
use miette::Diagnostic;
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
}

impl Diagnostic for TypeError {
    fn severity(&self) -> Option<miette::Severity> {
        Some(miette::Severity::Error)
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
                    similar::ChangeTag::Equal => {
                        result.push_str(&format!(" {}\n", change));
                    }
                    similar::ChangeTag::Delete => {
                        result.push_str(&format!("-{}\n", change));
                    }
                    similar::ChangeTag::Insert => {
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

    fn labels(&self) -> Option<Box<dyn Iterator<Item = miette::LabeledSpan>>> {
        // Type errors don't have byte spans yet, so no labels.
        None
    }
}
