#![deny(unreachable_patterns)]

pub mod env;
pub mod errors;
pub mod naming;
pub mod solve;
pub mod symbols;
pub mod unify;

pub use errors::{TypeError, UnifyError};
pub use naming::format_type_for_error;
pub use symbols::KnownSymbols;
pub use solve::SolveResult;
pub use unify::{UnificationTable, extract_type_substitutions, ExtractResult, ExtractError};

// Temporary stub for existing compiler pipeline (will be replaced in later chunks)
pub struct TypeChecker {
    pub interner: glyim_interner::Interner,
    pub errors: Vec<TypeError>,
}

impl TypeChecker {
    pub fn new(interner: glyim_interner::Interner) -> Self {
        Self { interner, errors: Vec::new() }
    }

    pub fn check(&mut self, _hir: &glyim_hir::Hir) -> Result<TypeCheckOutput, Vec<TypeError>> {
        // This stub will be replaced by the real implementation in Chunk 6
        Err(Vec::new())
    }
}

pub struct TypeCheckOutput {
    pub expr_types: Vec<glyim_hir::types::HirType>,
    pub call_type_args: std::collections::HashMap<glyim_hir::types::ExprId, Vec<glyim_hir::types::HirType>>,
    pub interner: glyim_interner::Interner,
}

impl From<TypeError> for glyim_diag::diagnostic::Diagnostic {
    fn from(err: TypeError) -> glyim_diag::diagnostic::Diagnostic {
        glyim_diag::diagnostic::Diagnostic {
            severity: glyim_diag::diagnostic::Severity::Error,
            file: None,
            span: glyim_diag::Span::new(0, 0),
            message: err.to_string(),
            code: None,
            suggestion: None,
        }
    }
}
