use crate::errors::{InferKind, TypeError};
use crate::unify::UnificationTable;
use glyim_hir::types::{substitute_type_with, HirType, TypeVar};
use glyim_diag::Span;
use glyim_interner::Symbol;
use std::collections::HashMap;

pub struct SolveResult {
    pub subst: HashMap<Symbol, HirType>,
    pub concrete_args: Vec<HirType>,
    pub fully_resolved: bool,
    pub had_errors: bool,
}

pub fn solve_generic_params<E>(
    table: &mut UnificationTable,
    type_params: &[Symbol],
    param_types: &[(Symbol, HirType)],
    ret_type: Option<&HirType>,
    arg_types: &[HirType],
    expected_return: Option<&HirType>,
    expected_span: Span,
    found_span: Span,
    emit_err: &mut E,
) -> SolveResult
where E: FnMut(TypeError)
{
    eprintln!("[solve_generic_params] type_params={:?}", type_params);
    eprintln!("[solve_generic_params] param_types={:?}", param_types);
    eprintln!("[solve_generic_params] arg_types={:?}", arg_types);
    let mut param_vars: HashMap<Symbol, TypeVar> = HashMap::new();
    let mut had_errors = false;

    for tp in type_params {
        let var = table.fresh_var(expected_span);
        param_vars.insert(*tp, var);
    }

    if arg_types.len() != param_types.len() {
        emit_err(TypeError::ArgumentCountMismatch {
            expected: param_types.len(),
            actual: arg_types.len(),
            span: expected_span,
        });
        return SolveResult {
            subst: HashMap::new(),
            concrete_args: arg_types.to_vec(),
            fully_resolved: false,
            had_errors: true,
        };
    }

    // Zero-argument generic call: infer from expected return type.
    if param_types.is_empty() && arg_types.is_empty() {
        if let Some(expected) = expected_return {
            if let Some(ret) = ret_type {
                // Substitute param_vars (our local fresh variables) into ret_type,
                // then unify with the expected type.  This causes the fresh variables
                // to be resolved inside the table so that the extraction loop below
                // can read them back.
                let ret_resolved = substitute_type_with(ret, &mut |sym| {
                    param_vars.get(sym).map(|&var| HirType::Infer(var))
                }, 0).unwrap_or(HirType::Error);
                // Also resolve / flatten the expected type through the table
                let expected_flat = table.resolve(expected).unwrap_or_else(|_| expected.clone());
                let res = table.unify(&ret_resolved, &expected_flat, expected_span, found_span);
                if let Err(e) = res {
                    had_errors = true;
                    emit_err(e.into_type_error());
                }
            }
        }
        let mut subst = HashMap::new();
        let mut concrete_args = Vec::new();
        let mut fully_resolved = true;
        for tp in type_params {
            let var = param_vars[tp];
            let resolved = table.resolve(&HirType::Infer(var)).unwrap_or(HirType::Error);
            if matches!(resolved, HirType::Infer(_)) {
                fully_resolved = false;
            } else {
                subst.insert(*tp, resolved.clone());
                concrete_args.push(resolved);
            }
        }
        eprintln!("[solve zero-arg] type_params={:?} expected_return={:?} ret_type={:?} → subst={:?} concrete_args={:?} fully_resolved={}", type_params, expected_return, ret_type, subst, concrete_args, fully_resolved);
        return SolveResult { subst, concrete_args, fully_resolved, had_errors };
    }

    for ((_, formal_ty), actual) in param_types.iter().zip(arg_types.iter()) {
        let formal_resolved = match substitute_type_with(formal_ty, &mut |sym| {
            param_vars.get(sym).map(|&var| HirType::Infer(var))
        }, 0) {
            Ok(ty) => ty,
            Err(_) => { had_errors = true; continue; }
        };
        if let Err(e) = table.unify(&formal_resolved, actual, expected_span, found_span) {
            had_errors = true;
            emit_err(e.into_type_error());
        }
    }

    if let Some(expected) = expected_return {
        if let Some(ret) = ret_type {
            let ret_resolved = substitute_type_with(ret, &mut |sym| {
                param_vars.get(sym).map(|&var| HirType::Infer(var))
            }, 0).unwrap_or(HirType::Error);
            if let Err(e) = table.unify(&ret_resolved, expected, expected_span, found_span) {
                had_errors = true;
                emit_err(e.into_type_error());
            }
        }
    }

    let mut subst = HashMap::new();
    let mut concrete_args = Vec::new();
    let mut fully_resolved = true;

    for tp in type_params {
        let var = param_vars[tp];
        let resolved = table.resolve(&HirType::Infer(var)).unwrap_or(HirType::Error);
        let is_unresolved = matches!(resolved, HirType::Infer(_)) || matches!(&resolved, HirType::Param(s) if type_params.contains(s));
        if is_unresolved {
            fully_resolved = false;
            emit_err(TypeError::CannotInferType {
                kind: InferKind::GenericArg,
                type_var: var,
                span: expected_span,
            });
            subst.insert(*tp, HirType::Error);
            concrete_args.push(HirType::Error);
        } else if resolved == HirType::Error {
            fully_resolved = false;
            had_errors = true;
            subst.insert(*tp, HirType::Error);
            concrete_args.push(HirType::Error);
        } else {
            subst.insert(*tp, resolved.clone());
            concrete_args.push(resolved);
        }
    }

    SolveResult { subst, concrete_args, fully_resolved, had_errors }
}
