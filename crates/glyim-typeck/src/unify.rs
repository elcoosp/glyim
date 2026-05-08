use crate::ty::{Ty, TyKind, TyArena};
use crate::diagnostics::TypeError;
use crate::diagnostics::zippering::zip_diff;
use crate::diagnostics::biabduction::bi_abductive_synthesis;
use glyim_diag::Span;
use glyim_interner::Interner;

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
            let origin = arena.get_infer_span(a)
                .or_else(|| arena.get_infer_span(b))
                .unwrap_or(span);
            emit_err(TypeError::InfiniteType { span: crate::diagnostics::span_to_src(origin) });
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
        self.parents.resize(self.parents.len().max(max), a);  // self-reference for new slots
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
            (TyKind::RawPtr(i1), TyKind::RawPtr(i2)) => {
                self.unify(arena, *i1, *i2, span, emit_err)
            }
            _ => {
                let diff_path = zip_diff(arena, a, b, "root".to_string());
                let autofix = self.interner.as_ref()
                    .and_then(|i| bi_abductive_synthesis(arena, i, a, b));
                emit_err(TypeError::MismatchedTypes {
                    expected_span: crate::diagnostics::span_to_src(arena.get_infer_span(a).unwrap_or(span)),
                    found_span: crate::diagnostics::span_to_src(arena.get_infer_span(b).unwrap_or(span)),
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
