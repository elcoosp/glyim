use crate::typeck::FnTypes;
use glyim_hir::types::HirType;
use glyim_diag::Span;
use glyim_interner::Symbol;
use std::collections::HashMap;

#[derive(Debug)]
pub struct ValidationError {
    pub kind: ValidationErrorKind,
    pub fn_name: Symbol,
    pub ty: HirType,
    pub span: Span,
}

#[derive(Debug)]
pub enum ValidationErrorKind {
    InferInOutput,
    ParamInNonGenericOutput,
}

pub fn validate_mono_input(fn_types_map: &HashMap<Symbol, FnTypes>) -> Result<(), Vec<ValidationError>> {
    let mut errors = Vec::new();
    for (fn_name, ft) in fn_types_map {
        for ty in ft.expr_types.values() {
            if ty.has_infer() {
                errors.push(ValidationError {
                    kind: ValidationErrorKind::InferInOutput,
                    fn_name: *fn_name,
                    ty: ty.clone(),
                    span: ft.span,
                });
            }
            if ty.has_param() && !ft.is_generic {
                errors.push(ValidationError {
                    kind: ValidationErrorKind::ParamInNonGenericOutput,
                    fn_name: *fn_name,
                    ty: ty.clone(),
                    span: ft.span,
                });
            }
        }
    }
    if errors.is_empty() { Ok(()) } else { Err(errors) }
}
