use crate::diagnostics::TypeError;
use crate::diagnostics::biabduction::bi_abductive_synthesis;
use crate::diagnostics::zippering::zip_diff;
use crate::ty::{Ty, TyArena, TyKind};
use glyim_diag::Span;
use glyim_hir::types::HirType;
use glyim_interner::Interner;
use glyim_interner::Symbol;
use std::collections::HashMap;

/// A token proving an error was emitted.
#[derive(Clone, Copy, Debug)]
pub struct ErrorGuaranteed(pub(crate) ());

pub struct UnificationTable {
    parents: Vec<Ty>,
    ranks: Vec<u8>,
    /// Optional interner for diagnostics (autofix suggestions).
    interner: Option<Interner>,
}

impl UnificationTable {
    pub fn new() -> Self {
        Self {
            parents: Vec::new(),
            ranks: Vec::new(),
            interner: None,
        }
    }

    pub fn with_interner(interner: Interner) -> Self {
        Self {
            parents: Vec::new(),
            ranks: Vec::new(),
            interner: Some(interner),
        }
    }

    pub fn new_var(&mut self, arena: &mut TyArena, span: Span) -> Ty {
        let ty = arena.fresh_infer(span);
        self.parents.push(ty);
        self.ranks.push(0);
        ty
    }

    pub fn find(&self, arena: &TyArena, ty: Ty) -> Ty {
        if ty.0 >= self.parents.len() {
            return ty;
        }
        match arena.get(ty) {
            TyKind::Infer | TyKind::Error if self.parents[ty.0] == ty => ty,
            TyKind::Infer => self.find(arena, self.parents[ty.0]),
            _ => ty,
        }
    }

    pub fn unify(
        &mut self,
        arena: &mut TyArena,
        a: Ty,
        b: Ty,
        span: Span,
        emit_err: &mut dyn FnMut(TypeError),
    ) -> Result<(), ErrorGuaranteed> {
        let a = self.find(arena, a);
        let b = self.find(arena, b);
        if a == b {
            return Ok(());
        }

        if matches!(arena.get(a), TyKind::Error) || matches!(arena.get(b), TyKind::Error) {
            return Ok(());
        }

        if self.occurs(arena, a, b) || self.occurs(arena, b, a) {
            let origin = arena
                .get_infer_span(a)
                .or_else(|| arena.get_infer_span(b))
                .unwrap_or(span);
            emit_err(TypeError::InfiniteType {
                span: crate::diagnostics::span_to_src(origin),
            });
            arena.poison(a);
            return Err(ErrorGuaranteed(()));
        }

        if matches!(arena.get(a), TyKind::Infer) {
            self.union(a, b);
            return Ok(());
        }
        if matches!(arena.get(b), TyKind::Infer) {
            self.union(b, a);
            return Ok(());
        }

        self.unify_structural(arena, a, b, span, emit_err)
    }

    fn union(&mut self, a: Ty, b: Ty) {
        let max = a.0.max(b.0) + 1;
        self.parents.resize(self.parents.len().max(max), a); // self-reference for new slots
        self.ranks.resize(self.ranks.len().max(max), 0);
        self.parents[a.0] = b;
    }

    fn occurs(&self, arena: &TyArena, var: Ty, ty: Ty) -> bool {
        if var == ty {
            return true;
        }
        match arena.get(ty) {
            TyKind::App(_, args) => args.iter().any(|&arg| self.occurs(arena, var, arg)),
            TyKind::Fn(params, ret) => {
                params.iter().any(|&p| self.occurs(arena, var, p)) || self.occurs(arena, var, *ret)
            }
            TyKind::RawPtr(inner) => self.occurs(arena, var, *inner),
            _ => false,
        }
    }

    fn unify_structural(
        &mut self,
        arena: &mut TyArena,
        a: Ty,
        b: Ty,
        span: Span,
        emit_err: &mut dyn FnMut(TypeError),
    ) -> Result<(), ErrorGuaranteed> {
        match (arena.get(a), arena.get(b)) {
            (TyKind::Int, TyKind::Int) => Ok(()),
            (TyKind::Bool, TyKind::Bool) => Ok(()),
            (TyKind::Float, TyKind::Float) => Ok(()),
            (TyKind::Str, TyKind::Str) => Ok(()),
            (TyKind::Unit, TyKind::Unit) => Ok(()),
            (TyKind::Never, TyKind::Never) => Ok(()),
            (TyKind::Named(s1), TyKind::Named(s2)) if s1 == s2 => Ok(()),
            (TyKind::App(s1, a1), TyKind::App(s2, a2)) if s1 == s2 && a1.len() == a2.len() => {
                let args1 = a1.clone();
                let args2 = a2.clone();
                for (&arg_a, &arg_b) in args1.iter().zip(args2.iter()) {
                    self.unify(arena, arg_a, arg_b, span, emit_err)?;
                }
                Ok(())
            }
            (TyKind::Fn(p1, r1), TyKind::Fn(p2, r2)) if p1.len() == p2.len() => {
                let params1 = p1.clone();
                let params2 = p2.clone();
                let ret1 = *r1;
                let ret2 = *r2;
                for (&pa, &pb) in params1.iter().zip(params2.iter()) {
                    self.unify(arena, pa, pb, span, emit_err)?;
                }
                self.unify(arena, ret1, ret2, span, emit_err)
            }
            (TyKind::RawPtr(i1), TyKind::RawPtr(i2)) => self.unify(arena, *i1, *i2, span, emit_err),
            _ => {
                let diff_path = zip_diff(arena, a, b, "root".to_string());
                let autofix = self
                    .interner
                    .as_ref()
                    .and_then(|i| bi_abductive_synthesis(arena, i, a, b));
                emit_err(TypeError::MismatchedTypes {
                    expected_span: crate::diagnostics::span_to_src(
                        arena.get_infer_span(a).unwrap_or(span),
                    ),
                    found_span: crate::diagnostics::span_to_src(
                        arena.get_infer_span(b).unwrap_or(span),
                    ),
                    expected: format!("{:?}", arena.get(a)),
                    found: format!("{:?}", arena.get(b)),
                    diff_path,
                    autofix,
                });
                Err(ErrorGuaranteed(()))
            }
        }
    }
}

impl Default for UnificationTable {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract type parameter bindings by matching a schema against a concrete type.
///
/// `schema` is the type from the generic definition (e.g., `T`, `Vec<T>`, `(T, U)`).
/// `concrete` is the actual type at the call site (e.g., `Int`, `Vec<Int>`, `(Int, Bool)`).
/// `type_params` is the set of symbols that are type parameters (not type names).
///
/// Results are accumulated into `sub`. If a parameter is already bound and the
/// new binding conflicts, the conflict is silently ignored (the first binding wins).
pub fn extract_type_substitutions(
    schema: &HirType,
    concrete: &HirType,
    type_params: &[Symbol],
    sub: &mut HashMap<Symbol, HirType>,
) {
    match (schema, concrete) {
        (HirType::Named(sym), _) if type_params.contains(sym) => {
            sub.entry(*sym).or_insert_with(|| concrete.clone());
        }
        (HirType::Generic(sym_a, args_a), HirType::Generic(sym_b, args_b))
            if sym_a == sym_b && args_a.len() == args_b.len() =>
        {
            for (a, b) in args_a.iter().zip(args_b.iter()) {
                extract_type_substitutions(a, b, type_params, sub);
            }
        }
        (HirType::Named(a), HirType::Named(b)) if a == b => {}
        (HirType::RawPtr(inner_s), HirType::RawPtr(inner_c)) => {
            extract_type_substitutions(inner_s, inner_c, type_params, sub);
        }
        (HirType::Option(inner_s), HirType::Option(inner_c)) => {
            extract_type_substitutions(inner_s, inner_c, type_params, sub);
        }
        (HirType::Result(ok_s, err_s), HirType::Result(ok_c, err_c)) => {
            extract_type_substitutions(ok_s, ok_c, type_params, sub);
            extract_type_substitutions(err_s, err_c, type_params, sub);
        }
        (HirType::Tuple(elems_s), HirType::Tuple(elems_c)) if elems_s.len() == elems_c.len() => {
            for (s, c) in elems_s.iter().zip(elems_c.iter()) {
                extract_type_substitutions(s, c, type_params, sub);
            }
        }
        (HirType::Func(params_s, ret_s), HirType::Func(params_c, ret_c))
            if params_s.len() == params_c.len() =>
        {
            for (s, c) in params_s.iter().zip(params_c.iter()) {
                extract_type_substitutions(s, c, type_params, sub);
            }
            extract_type_substitutions(ret_s, ret_c, type_params, sub);
        }
        (HirType::Named(sym), HirType::Generic(_, _)) if type_params.contains(sym) => {
            sub.entry(*sym).or_insert_with(|| concrete.clone());
        }
        _ => {}
    }
}

/// Build a substitution map by unifying function parameters with argument types.
pub fn unify_fn_call(
    fn_params: &[(Symbol, HirType)],
    arg_types: &[HirType],
    type_params: &[Symbol],
) -> Result<HashMap<Symbol, HirType>, UnifyError> {
    let mut sub = HashMap::new();
    for ((_, param_ty), arg_ty) in fn_params.iter().zip(arg_types.iter()) {
        extract_type_substitutions(param_ty, arg_ty, type_params, &mut sub);
    }
    // Check for conflicts
    let mut conflict_sub = HashMap::new();
    for ((_, param_ty), arg_ty) in fn_params.iter().zip(arg_types.iter()) {
        check_for_conflicts(param_ty, arg_ty, type_params, &sub, &mut conflict_sub)?;
    }
    Ok(sub)
}

/// Check for conflicting type parameter bindings.
fn check_for_conflicts(
    schema: &HirType,
    concrete: &HirType,
    type_params: &[Symbol],
    sub: &HashMap<Symbol, HirType>,
    _conflict_sub: &mut HashMap<Symbol, (HirType, HirType)>,
) -> Result<(), UnifyError> {
    match (schema, concrete) {
        (HirType::Named(sym), _) if type_params.contains(sym) => {
            if let Some(existing) = sub.get(sym) {
                if !types_structurally_equal(existing, concrete) {
                    return Err(UnifyError::Conflict {
                        param: *sym,
                        existing: existing.clone(),
                        new: concrete.clone(),
                    });
                }
            }
            Ok(())
        }
        (HirType::Generic(sym_a, args_a), HirType::Generic(sym_b, args_b))
            if sym_a == sym_b && args_a.len() == args_b.len() =>
        {
            for (a, b) in args_a.iter().zip(args_b.iter()) {
                check_for_conflicts(a, b, type_params, sub, _conflict_sub)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Simplified structural equality check for types.
fn types_structurally_equal(a: &HirType, b: &HirType) -> bool {
    match (a, b) {
        (HirType::Int, HirType::Int) => true,
        (HirType::Bool, HirType::Bool) => true,
        (HirType::Float, HirType::Float) => true,
        (HirType::Str, HirType::Str) => true,
        (HirType::Unit, HirType::Unit) => true,
        (HirType::Named(a), HirType::Named(b)) => a == b,
        (HirType::Generic(sa, aa), HirType::Generic(sb, ab)) => {
            sa == sb
                && aa.len() == ab.len()
                && aa
                    .iter()
                    .zip(ab.iter())
                    .all(|(a, b)| types_structurally_equal(a, b))
        }
        (HirType::RawPtr(a), HirType::RawPtr(b)) => types_structurally_equal(a, b),
        (HirType::Option(a), HirType::Option(b)) => types_structurally_equal(a, b),
        (HirType::Result(a1, a2), HirType::Result(b1, b2)) => {
            types_structurally_equal(a1, b1) && types_structurally_equal(a2, b2)
        }
        (HirType::Tuple(a), HirType::Tuple(b)) if a.len() == b.len() => a
            .iter()
            .zip(b.iter())
            .all(|(x, y)| types_structurally_equal(x, y)),
        _ => false,
    }
}

/// Errors from unification.
#[derive(Debug, Clone)]
pub enum UnifyError {
    Conflict {
        param: Symbol,
        existing: HirType,
        new: HirType,
    },
}

#[cfg(test)]
mod extract_subst_tests {
    use super::*;
    use glyim_interner::Interner;

    #[test]
    fn extract_simple_param() {
        let mut interner = Interner::new();
        let t = interner.intern("T");
        let type_params = vec![t];
        let mut sub = HashMap::new();
        extract_type_substitutions(&HirType::Named(t), &HirType::Int, &type_params, &mut sub);
        assert_eq!(sub.get(&t), Some(&HirType::Int));
    }

    #[test]
    fn extract_nested_generic() {
        let mut interner = Interner::new();
        let t = interner.intern("T");
        let vec_sym = interner.intern("Vec");
        let type_params = vec![t];
        let schema = HirType::Generic(vec_sym, vec![HirType::Named(t)]);
        let concrete = HirType::Generic(vec_sym, vec![HirType::Int]);
        let mut sub = HashMap::new();
        extract_type_substitutions(&schema, &concrete, &type_params, &mut sub);
        assert_eq!(sub.get(&t), Some(&HirType::Int));
    }

    #[test]
    fn extract_no_match() {
        let mut interner = Interner::new();
        let t = interner.intern("T");
        let u = interner.intern("U");
        let type_params = vec![t];
        let mut sub = HashMap::new();
        extract_type_substitutions(&HirType::Named(u), &HirType::Int, &type_params, &mut sub);
        assert!(sub.is_empty());
    }

    #[test]
    fn unify_fn_call_simple() {
        let mut interner = Interner::new();
        let t = interner.intern("T");
        let x = interner.intern("x");
        let fn_params = vec![(x, HirType::Named(t))];
        let arg_types = vec![HirType::Int];
        let result = unify_fn_call(&fn_params, &arg_types, &[t]);
        assert!(result.is_ok());
        let sub = result.unwrap();
        assert_eq!(sub.get(&t), Some(&HirType::Int));
    }
}
