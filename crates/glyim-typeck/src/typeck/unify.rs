//! Pure type unification for Glyim type checking.
//! Returns a substitution map on success, or a TypeError on mismatch.

use glyim_hir::types::HirType;
use glyim_interner::Symbol;
use std::collections::HashMap;

/// Attempt to unify `concrete` with `generic`, where `generic` may reference
/// type parameters listed in `type_params`. Returns Ok(substitutions) if
/// unification succeeds, or Err(TypeError) if it fails.
pub fn unify(
    concrete: &HirType,
    generic: &HirType,
    type_params: &[Symbol],
) -> Result<HashMap<Symbol, HirType>, crate::TypeError> {
    let mut sub = HashMap::new();
    unify_recursive(concrete, generic, type_params, &mut sub)?;
    Ok(sub)
}

fn unify_recursive(
    concrete: &HirType,
    generic: &HirType,
    type_params: &[Symbol],
    sub: &mut HashMap<Symbol, HirType>,
) -> Result<(), crate::TypeError> {
    match (concrete, generic) {
        // Type parameter
        (c, HirType::Named(n)) if type_params.contains(n) => {
            if let Some(existing) = sub.get(n) {
                if existing != c { return Err(crate::TypeError::MismatchedTypes { expected: existing.clone(), found: c.clone(), expr_id: glyim_hir::types::ExprId::new(0) }); }
            } else { sub.insert(*n, c.clone()); }
            Ok(())
        }
        // Same concrete types
        (HirType::Int, HirType::Int)|(HirType::Bool, HirType::Bool)|(HirType::Float, HirType::Float)|(HirType::Str, HirType::Str)|(HirType::Unit, HirType::Unit)|(HirType::Never, HirType::Never) => Ok(()),
        // Named types (non-param) must match exactly
        (HirType::Named(cn), HirType::Named(gn)) if !type_params.contains(gn) => {
            if cn == gn { Ok(()) } else { Err(crate::TypeError::MismatchedTypes { expected: generic.clone(), found: concrete.clone(), expr_id: glyim_hir::types::ExprId::new(0) }) }
        }
        // Generic (instantiated) types
        (HirType::Generic(cn, c_args), HirType::Generic(gn, g_args)) if cn == gn && c_args.len() == g_args.len() => {
            for (ca, ga) in c_args.iter().zip(g_args.iter()) { unify_recursive(ca, ga, type_params, sub)?; }
            Ok(())
        }
        // RawPtr
        (HirType::RawPtr(ci), HirType::RawPtr(gi)) => unify_recursive(ci, gi, type_params, sub),
        // Option
        (HirType::Option(ci), HirType::Option(gi)) => unify_recursive(ci, gi, type_params, sub),
        // Result
        (HirType::Result(co, ce), HirType::Result(go, ge)) => { unify_recursive(co, go, type_params, sub)?; unify_recursive(ce, ge, type_params, sub) }
        // Tuple
        (HirType::Tuple(c_e), HirType::Tuple(g_e)) if c_e.len() == g_e.len() => {
            for (ca, ga) in c_e.iter().zip(g_e.iter()) { unify_recursive(ca, ga, type_params, sub)?; }
            Ok(())
        }
        // RawPtr (concrete) vs type param
        (HirType::RawPtr(_), HirType::Named(n)) if type_params.contains(n) => { sub.insert(*n, concrete.clone()); Ok(()) }
        // Fallback: allow any concrete to match a type param
        (_, HirType::Named(n)) if type_params.contains(n) => {
            if let Some(existing) = sub.get(n) {
                if existing != concrete { return Err(crate::TypeError::MismatchedTypes { expected: existing.clone(), found: concrete.clone(), expr_id: glyim_hir::types::ExprId::new(0) }); }
            } else { sub.insert(*n, concrete.clone()); }
            Ok(())
        }
        // Anything else is a mismatch
        _ => Err(crate::TypeError::MismatchedTypes { expected: generic.clone(), found: concrete.clone(), expr_id: glyim_hir::types::ExprId::new(0) }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_interner::Interner;

    fn sym(interner: &mut Interner, s: &str) -> Symbol { interner.intern(s) }

    #[test]
    fn unify_int_with_type_param() {
        let mut interner = Interner::new();
        let t = sym(&mut interner, "T");
        let result = unify(&HirType::Int, &HirType::Named(t), &[t]).unwrap();
        assert_eq!(result.get(&t), Some(&HirType::Int));
    }

    #[test]
    fn unify_mismatch_fails() {
        let result = unify(&HirType::Int, &HirType::Bool, &[]);
        assert!(result.is_err());
    }

    #[test]
    fn unify_generic_with_args() {
        let mut interner = Interner::new();
        let t = sym(&mut interner, "T");
        let result = unify(
            &HirType::Generic(sym(&mut interner, "Vec"), vec![HirType::Int]),
            &HirType::Generic(sym(&mut interner, "Vec"), vec![HirType::Named(t)]),
            &[t],
        ).unwrap();
        assert_eq!(result.get(&t), Some(&HirType::Int));
    }
}
