#![deny(unreachable_patterns)]

pub mod env;
pub mod errors;
pub mod naming;
pub mod solve;
pub mod symbols;
pub mod typeck;
pub mod unify;
pub mod validate;

pub use errors::{TypeError, UnifyError};
pub use naming::format_type_for_error;
pub use solve::SolveResult;
pub use symbols::KnownSymbols;
pub use typeck::{FnTypes, TypeCheckOutput};
pub use unify::{ExtractError, ExtractResult, UnificationTable, extract_type_substitutions};
pub use validate::validate_mono_input;

// Backward-compatible TypeChecker wrapper for existing compiler
use glyim_hir::Hir;
use glyim_interner::Interner;
use std::collections::HashMap;
use typeck::TypeChecker as NewTypeChecker;

pub struct TypeChecker {
    pub interner: Interner,
    inner: NewTypeChecker,
}

impl TypeChecker {
    pub fn new(interner: Interner) -> Self {
        let mut interner_mut = interner;
        let known = KnownSymbols::intern_all(&mut interner_mut);
        Self {
            interner: interner_mut.clone(),
            inner: NewTypeChecker::new(interner_mut, known),
        }
    }

    pub fn check(&mut self, hir: &Hir) -> Result<TypeCheckOutput, Vec<TypeError>> {
        let result = self.inner.check(hir);
        if result.has_errors() {
            Err(result.type_errors)
        } else {
            Ok(TypeCheckOutput {
                expr_types: Vec::new(),
                call_type_args: HashMap::new(),
                interner: self.inner.interner.clone(),
            })
        }
    }
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
