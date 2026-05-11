#![deny(unreachable_patterns)]

pub mod env;
pub mod errors;
pub mod naming;
pub mod solve;
pub mod symbols;
pub mod unify;
pub mod typeck;
pub mod validate;

pub use errors::{TypeError, UnifyError};
pub use naming::format_type_for_error;
pub use symbols::KnownSymbols;
pub use solve::SolveResult;
pub use unify::{UnificationTable, extract_type_substitutions, ExtractResult, ExtractError};
pub use typeck::{FnTypes, TypeCheckResult as NewTypeCheckResult, TypeChecker as NewTypeChecker};
pub use validate::validate_mono_input;

// Backward-compatible TypeChecker wrapper
use glyim_interner::Interner;
use glyim_hir::Hir;
use std::collections::HashMap;

pub struct TypeChecker {
    pub interner: Interner,
    pub errors: Vec<TypeError>,
    pub expr_types: Vec<glyim_hir::types::HirType>,
    pub call_type_args: HashMap<glyim_hir::types::ExprId, Vec<glyim_hir::types::HirType>>,
}

impl TypeChecker {
    pub fn new(interner: Interner) -> Self {
        Self {
            interner,
            errors: Vec::new(),
            expr_types: Vec::new(),
            call_type_args: HashMap::new(),
        }
    }

    pub fn check(&mut self, hir: &Hir) -> Result<TypeCheckOutput, Vec<TypeError>> {
        let mut known = KnownSymbols::intern_all(&mut self.interner);
        let mut tc = NewTypeChecker::new(self.interner.clone(), known.clone());
        let result = tc.check(hir);
        self.errors = result.type_errors.clone();
        if self.errors.is_empty() {
            // Map FnTypes to expr_types (plain vec for backward compat)
            let mut all_expr_types = Vec::new();
            let mut all_call_type_args = HashMap::new();
            for (_, fn_types) in &result.fn_types_map {
                for (id, ty) in &fn_types.expr_types {
                    let idx = id.as_usize();
                    if idx >= all_expr_types.len() {
                        all_expr_types.resize(idx + 1, glyim_hir::types::HirType::Error);
                    }
                    all_expr_types[idx] = ty.clone();
                }
                for (id, args) in &fn_types.call_type_args {
                    all_call_type_args.insert(*id, args.clone());
                }
            }
            self.expr_types = all_expr_types;
            self.call_type_args = all_call_type_args;
            self.interner = tc.interner.clone();
            Ok(TypeCheckOutput {
                expr_types: self.expr_types.clone(),
                call_type_args: self.call_type_args.clone(),
                interner: self.interner.clone(),
            })
        } else {
            Err(self.errors.clone())
        }
    }
}
pub struct TypeCheckOutput {
    pub expr_types: Vec<glyim_hir::types::HirType>,
    pub call_type_args: std::collections::HashMap<glyim_hir::types::ExprId, Vec<glyim_hir::types::HirType>>,
    pub interner: Interner,
}

impl From<TypeError> for glyim_diag::diagnostic::Diagnostic {
    fn from(err: TypeError) -> glyim_diag::diagnostic::Diagnostic {
        let (start, end, msg) = match &err {
            TypeError::UnknownField { span, .. } => (span.0, span.1, err.to_string()),
            TypeError::MissingField { span, .. } => (span.0, span.1, err.to_string()),
            TypeError::NonExhaustiveMatch { span, .. } => (span.0, span.1, err.to_string()),
            TypeError::AssignToImmutable { span, .. } => (span.0, span.1, err.to_string()),
            TypeError::AssignThroughNonPointer { span, .. } => (span.0, span.1, err.to_string()),
            TypeError::DerefNonPointer { span, .. } => (span.0, span.1, err.to_string()),
            TypeError::UnresolvedName { span, name } => (span.start, span.end, format!("unresolved name `{}`", name)),
            TypeError::MismatchedTypes { expected_span, .. } => (expected_span.start, expected_span.end, err.to_string()),
            _ => (0, 0, err.to_string()),
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

#[cfg(test)]
mod tests;
