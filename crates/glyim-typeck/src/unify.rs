use crate::errors::UnifyError;
use glyim_diag::Span;
use glyim_hir::types::{HirType, TypeVar};
use glyim_interner::Symbol;
use std::collections::{HashMap, HashSet};

const MAX_INFER_DEPTH: u32 = 256;

pub struct UnificationTable {
    parents: Vec<u32>,
    ranks: Vec<u8>,
    spans: Vec<Span>,
    bindings: Vec<Option<HirType>>,
}

impl Default for UnificationTable {
    fn default() -> Self {
        Self::new()
    }
}

impl UnificationTable {
    pub fn debug_binding(&mut self, var: TypeVar) -> Option<HirType> {
        let root = self.find(var);
        self.bindings
            .get(root.raw_index() as usize)
            .and_then(|b| b.clone())
    }
    pub fn new() -> Self {
        Self {
            parents: Vec::new(),
            ranks: Vec::new(),
            spans: Vec::new(),
            bindings: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.parents.clear();
        self.ranks.clear();
        self.spans.clear();
        self.bindings.clear();
    }

    pub fn fresh_var(&mut self, span: Span) -> TypeVar {
        let idx = self.parents.len() as u32;
        self.parents.push(idx);
        self.ranks.push(0);
        self.spans.push(span);
        self.bindings.push(None);
        TypeVar::from_raw_unchecked(idx)
    }

    pub fn var_span(&self, var: TypeVar) -> Option<&Span> {
        self.spans.get(var.raw_index() as usize)
    }

    pub fn find(&mut self, var: TypeVar) -> TypeVar {
        let mut root = var.raw_index();
        while self.parents[root as usize] != root {
            let parent = self.parents[root as usize];
            self.parents[root as usize] = self.parents[parent as usize];
            root = parent;
        }
        TypeVar::from_raw_unchecked(root)
    }

    pub fn resolve(&mut self, ty: &HirType) -> Result<HirType, UnifyError> {
        self.resolve_infer_depth(ty, 0)
    }

    fn resolve_infer_depth(&mut self, ty: &HirType, depth: u32) -> Result<HirType, UnifyError> {
        match ty {
            HirType::Infer(var) => {
                if depth > MAX_INFER_DEPTH {
                    return Err(UnifyError::ResolveDepthExceeded {
                        type_var: *var,
                        span: self
                            .var_span(*var)
                            .copied()
                            .unwrap_or_else(|| Span::new(0, 0)),
                    });
                }
                let root = self.find(*var);
                let bound: Option<HirType> = self
                    .bindings
                    .get(root.raw_index() as usize)
                    .and_then(|b| b.clone());
                match bound {
                    Some(bound) => self.resolve_infer_depth(&bound, depth + 1),
                    None => Ok(HirType::Infer(root)),
                }
            }
            HirType::Generic(sym, args) => {
                let args: Vec<HirType> = args
                    .iter()
                    .map(|a| self.resolve_infer_depth(a, depth))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(HirType::Generic(*sym, args))
            }
            HirType::Tuple(elems) => {
                let elems: Vec<HirType> = elems
                    .iter()
                    .map(|e| self.resolve_infer_depth(e, depth))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(HirType::Tuple(elems))
            }
            HirType::RawPtr(inner) => Ok(HirType::RawPtr(Box::new(
                self.resolve_infer_depth(inner, depth)?,
            ))),
            HirType::Func(params, ret) => {
                let params: Vec<HirType> = params
                    .iter()
                    .map(|p| self.resolve_infer_depth(p, depth))
                    .collect::<Result<Vec<_>, _>>()?;
                let ret = self.resolve_infer_depth(ret, depth)?;
                Ok(HirType::Func(params, Box::new(ret)))
            }
            _ => Ok(ty.clone()),
        }
    }

    pub fn unify(
        &mut self,
        a: &HirType,
        b: &HirType,
        expected_span: Span,
        found_span: Span,
    ) -> Result<(), UnifyError> {
        let a_resolved = self.resolve(a)?;
        let b_resolved = self.resolve(b)?;
        if a_resolved == b_resolved {
            return Ok(());
        }
        if matches!(a_resolved, HirType::Error | HirType::Never)
            || matches!(b_resolved, HirType::Error | HirType::Never)
        {
            return Ok(());
        }

        if let (HirType::Infer(var_a), HirType::Infer(var_b)) = (&a_resolved, &b_resolved) {
            self.union(*var_a, *var_b);
            return Ok(());
        }
        if let HirType::Infer(var) = &a_resolved {
            if self.occurs(*var, &b_resolved)? {
                return Err(UnifyError::InfiniteType {
                    span: expected_span,
                });
            }
            self.bind(*var, &b_resolved);
            return Ok(());
        }
        if let HirType::Infer(var) = &b_resolved {
            if self.occurs(*var, &a_resolved)? {
                return Err(UnifyError::InfiniteType { span: found_span });
            }
            self.bind(*var, &a_resolved);
            return Ok(());
        }
        self.unify_structural(&a_resolved, &b_resolved, expected_span, found_span)
    }

    fn bind(&mut self, var: TypeVar, ty: &HirType) {
        let root = self.find(var);
        let idx = root.raw_index() as usize;
        if idx >= self.bindings.len() {
            self.bindings.resize(idx + 1, None);
        }
        self.bindings[idx] = Some(ty.clone());
    }

    fn union(&mut self, a: TypeVar, b: TypeVar) {
        let root_a = self.find(a);
        let root_b = self.find(b);
        if root_a == root_b {
            return;
        }
        let (new_root, old_root) = match self.ranks[root_a.raw_index() as usize]
            .cmp(&self.ranks[root_b.raw_index() as usize])
        {
            std::cmp::Ordering::Less => (root_b, root_a),
            std::cmp::Ordering::Greater => (root_a, root_b),
            std::cmp::Ordering::Equal => {
                self.ranks[root_a.raw_index() as usize] += 1;
                (root_a, root_b)
            }
        };
        self.parents[old_root.raw_index() as usize] = new_root.raw_index();
    }

    fn occurs(&mut self, var: TypeVar, ty: &HirType) -> Result<bool, UnifyError> {
        match ty {
            HirType::Infer(v) => Ok(self.find(var) == self.find(*v)),
            HirType::Generic(_, args) | HirType::Tuple(args) => {
                for a in args {
                    if self.occurs(var, a)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            HirType::RawPtr(inner) => self.occurs(var, inner),
            HirType::Func(params, ret) => {
                for p in params {
                    if self.occurs(var, p)? {
                        return Ok(true);
                    }
                }
                self.occurs(var, ret)
            }
            _ => Ok(false),
        }
    }

    fn unify_structural(
        &mut self,
        a: &HirType,
        b: &HirType,
        expected_span: Span,
        found_span: Span,
    ) -> Result<(), UnifyError> {
        match (a, b) {
            (HirType::Unit, HirType::Unit) => Ok(()),
            (HirType::Named(s1), HirType::Named(s2)) if s1 == s2 => Ok(()),
            (HirType::Int, HirType::Int)
            | (HirType::Bool, HirType::Bool)
            | (HirType::Float, HirType::Float)
            | (HirType::Str, HirType::Str) => Ok(()),
            (HirType::Generic(s1, a1), HirType::Generic(s2, a2))
                if s1 == s2 && a1.len() == a2.len() =>
            {
                for (aa, ab) in a1.iter().zip(a2.iter()) {
                    self.unify(aa, ab, expected_span, found_span)?;
                }
                Ok(())
            }
            (HirType::RawPtr(i1), HirType::RawPtr(i2)) => {
                self.unify(i1, i2, expected_span, found_span)
            }
            (HirType::Tuple(e1), HirType::Tuple(e2)) if e1.len() == e2.len() => {
                for (ea, eb) in e1.iter().zip(e2.iter()) {
                    self.unify(ea, eb, expected_span, found_span)?;
                }
                Ok(())
            }
            (HirType::Func(p1, r1), HirType::Func(p2, r2)) if p1.len() == p2.len() => {
                for (pa, pb) in p1.iter().zip(p2.iter()) {
                    self.unify(pa, pb, expected_span, found_span)?;
                }
                self.unify(r1, r2, expected_span, found_span)
            }
            _ => Err(UnifyError::Mismatch {
                expected: a.clone(),
                found: b.clone(),
                expected_span,
                found_span,
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExtractError {
    ShapeMismatch {
        expected: HirType,
        found: HirType,
        expected_span: Span,
        found_span: Span,
    },
}

pub struct ExtractResult {
    pub sub: HashMap<Symbol, HirType>,
    pub errors: Vec<ExtractError>,
}

pub fn extract_type_substitutions(
    schema: &HirType,
    concrete: &HirType,
    type_params: &HashSet<Symbol>,
    expected_span: Span,
    found_span: Span,
) -> ExtractResult {
    let mut sub = HashMap::new();
    let mut errors = Vec::new();
    extract_recursive(
        schema,
        concrete,
        type_params,
        &mut sub,
        &mut errors,
        expected_span,
        found_span,
    );
    ExtractResult { sub, errors }
}

fn extract_recursive(
    schema: &HirType,
    concrete: &HirType,
    type_params: &HashSet<Symbol>,
    sub: &mut HashMap<Symbol, HirType>,
    errors: &mut Vec<ExtractError>,
    expected_span: Span,
    found_span: Span,
) {
    match (schema, concrete) {
        (HirType::Param(sym), _) if type_params.contains(sym) => {
            if let Some(existing) = sub.get(sym) {
                if existing != concrete {
                    errors.push(ExtractError::ShapeMismatch {
                        expected: existing.clone() as HirType,
                        found: concrete.clone(),
                        expected_span,
                        found_span,
                    });
                }
            } else {
                sub.insert(*sym, concrete.clone());
            }
        }
        (HirType::Generic(sa, aa), HirType::Generic(sb, ab))
            if sa == sb && aa.len() == ab.len() =>
        {
            for (a, b) in aa.iter().zip(ab.iter()) {
                extract_recursive(a, b, type_params, sub, errors, expected_span, found_span);
            }
        }
        (HirType::Named(a), HirType::Named(b)) if a == b => {}
        (HirType::RawPtr(is), HirType::RawPtr(cs)) => {
            extract_recursive(is, cs, type_params, sub, errors, expected_span, found_span);
        }
        (HirType::Tuple(es), HirType::Tuple(ec)) if es.len() == ec.len() => {
            for (s, c) in es.iter().zip(ec.iter()) {
                extract_recursive(s, c, type_params, sub, errors, expected_span, found_span);
            }
        }
        (HirType::Func(ps, rs), HirType::Func(pc, rc)) if ps.len() == pc.len() => {
            for (s, c) in ps.iter().zip(pc.iter()) {
                extract_recursive(s, c, type_params, sub, errors, expected_span, found_span);
            }
            extract_recursive(rs, rc, type_params, sub, errors, expected_span, found_span);
        }
        _ => {
            errors.push(ExtractError::ShapeMismatch {
                expected: schema.clone(),
                found: concrete.clone(),
                expected_span,
                found_span,
            });
        }
    }
}
