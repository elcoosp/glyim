use glyim_core::arena::IndexVec;
use glyim_diag::GlyimDiagnostic;
use glyim_span::Span;
use glyim_type::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// VariableKind.
pub enum VariableKind {
/// Variant.
    General,
/// Variant.
    Integer,
/// Variant.
    Float,
}

#[derive(Clone, Debug)]
/// TypeVariable.
pub struct TypeVariable {
/// Struct.
    pub universe: UniverseIndex,
/// Struct.
    pub value: Option<Ty>,
/// Struct.
    pub kind: VariableKind,
}

#[derive(Clone, Debug)]
/// RegionVariable.
pub struct RegionVariable {
/// Struct.
    pub universe: UniverseIndex,
/// Struct.
    pub value: Option<Region>,
}

#[derive(Clone, Debug)]
/// InferenceSnapshot.
pub struct InferenceSnapshot {
    ty_vars: IndexVec<TyVar, TypeVariable>,
    int_vars: IndexVec<IntVar, TypeVariable>,
    float_vars: IndexVec<FloatVar, TypeVariable>,
    region_vars: IndexVec<RegionVid, RegionVariable>,
    universe: UniverseIndex,
}

/// InferenceTable.
pub struct InferenceTable {
    ty_vars: IndexVec<TyVar, TypeVariable>,
    int_vars: IndexVec<IntVar, TypeVariable>,
    float_vars: IndexVec<FloatVar, TypeVariable>,
    region_vars: IndexVec<RegionVid, RegionVariable>,
    universe: UniverseIndex,
    diagnostics: std::cell::RefCell<Vec<GlyimDiagnostic>>,
}

const MAX_RESOLVE_DEPTH: u32 = 256;

impl InferenceTable {
/// new.
    pub fn new() -> Self {
        Self {
            ty_vars: IndexVec::new(),
            int_vars: IndexVec::new(),
            float_vars: IndexVec::new(),
            region_vars: IndexVec::new(),
            universe: UniverseIndex(0),
            diagnostics: std::cell::RefCell::new(Vec::new()),
        }
    }

/// take_diagnostics.
    pub fn take_diagnostics(&mut self) -> Vec<GlyimDiagnostic> {
        std::mem::take(&mut *self.diagnostics.borrow_mut())
    }

/// snapshot.
    pub fn snapshot(&self) -> InferenceSnapshot {
        InferenceSnapshot {
            ty_vars: self.ty_vars.clone(),
            int_vars: self.int_vars.clone(),
            float_vars: self.float_vars.clone(),
            region_vars: self.region_vars.clone(),
            universe: self.universe,
        }
    }

/// rollback_to.
    pub fn rollback_to(&mut self, snapshot: InferenceSnapshot) {
        self.ty_vars = snapshot.ty_vars;
        self.int_vars = snapshot.int_vars;
        self.float_vars = snapshot.float_vars;
        self.region_vars = snapshot.region_vars;
        self.universe = snapshot.universe;
    }

/// commit.
    pub fn commit(&mut self, _snapshot: InferenceSnapshot) {}

/// new_ty_var.
    pub fn new_ty_var(&mut self, _ctx: &mut TyCtxMut) -> TyVar {
        self.ty_vars.push(TypeVariable {
            universe: self.universe,
            value: None,
            kind: VariableKind::General,
        })
    }

/// new_int_var.
    pub fn new_int_var(&mut self, _ctx: &mut TyCtxMut) -> IntVar {
        self.int_vars.push(TypeVariable {
            universe: self.universe,
            value: None,
            kind: VariableKind::Integer,
        })
    }

/// new_float_var.
    pub fn new_float_var(&mut self, _ctx: &mut TyCtxMut) -> FloatVar {
        self.float_vars.push(TypeVariable {
            universe: self.universe,
            value: None,
            kind: VariableKind::Float,
        })
    }

/// new_region_var.
    pub fn new_region_var(&mut self, _ctx: &mut TyCtxMut) -> RegionVid {
        self.region_vars.push(RegionVariable {
            universe: self.universe,
            value: None,
        })
    }

    fn occurs(&self, ctx: &dyn TypeLookup, var: TyVar, ty: Ty) -> bool {
        let ty = self.resolve_ty_shallow(ctx, ty);
        match ctx.ty_kind(ty) {
            TyKind::Infer(InferVar::Ty(v)) if *v == var => true,
            TyKind::Ref(_, inner, _) => self.occurs(ctx, var, *inner),
            TyKind::RawPtr(inner, _) => self.occurs(ctx, var, *inner),
            TyKind::Slice(inner) => self.occurs(ctx, var, *inner),
            TyKind::Array(inner, _) => self.occurs(ctx, var, *inner),
            TyKind::Adt(_, substs)
            | TyKind::FnDef(_, substs)
            | TyKind::Closure(_, substs)
            | TyKind::Tuple(substs)
            | TyKind::Opaque(_, substs) => {
                for arg in ctx.substitution_args(*substs) {
                    if let GenericArg::Ty(t) = arg
                        && self.occurs(ctx, var, *t)
                    {
                        return true;
                    }
                }
                false
            }
            TyKind::FnPtr(sig) => {
                for arg in ctx.substitution_args(sig.inputs) {
                    if let GenericArg::Ty(t) = arg
                        && self.occurs(ctx, var, *t)
                    {
                        return true;
                    }
                }
                self.occurs(ctx, var, sig.output)
            }
            _ => false,
        }
    }

/// universe.
    pub fn universe(&self) -> UniverseIndex {
        self.universe
    }
/// create_universe.
    pub fn create_universe(&mut self) -> UniverseIndex {
        self.universe = UniverseIndex(self.universe.0 + 1);
        self.universe
    }

/// probe_ty_var.
    pub fn probe_ty_var(&self, var: TyVar) -> Option<Ty> {
        self.ty_vars.get(var).and_then(|v| v.value)
    }

/// probe_int_var.
    pub fn probe_int_var(&self, var: IntVar) -> Option<Ty> {
        self.int_vars.get(var).and_then(|v| v.value)
    }

/// probe_float_var.
    pub fn probe_float_var(&self, var: FloatVar) -> Option<Ty> {
        self.float_vars.get(var).and_then(|v| v.value)
    }

/// unify.
    pub fn unify(
        &mut self,
        ctx: &mut TyCtxMut,
        a: Ty,
        b: Ty,
        span: Span,
    ) -> Result<Vec<Constraint>, Vec<GlyimDiagnostic>> {
        let a = self.resolve_ty_shallow_preserve_int(ctx, a);
        let b = self.resolve_ty_shallow_preserve_int(ctx, b);
        self.unify_tys(ctx, a, b, span)
    }

    #[allow(unreachable_patterns, unused_variables)]
    fn unify_tys(
        &mut self,
        ctx: &mut TyCtxMut,
        a: Ty,
        b: Ty,
        span: Span,
    ) -> Result<Vec<Constraint>, Vec<GlyimDiagnostic>> {
        if a == b {
            return Ok(Vec::new());
        }
        let a_kind = ctx.ty_kind(a).clone();
        let b_kind = ctx.ty_kind(b).clone();

        let a_is_int = matches!(a_kind, TyKind::Infer(InferVar::Int(_)));
        let a_is_float = matches!(a_kind, TyKind::Infer(InferVar::Float(_)));
        match (a_kind, b_kind) {
            (TyKind::Error, _) | (_, TyKind::Error) => Ok(Vec::new()),
            (TyKind::Never, _) | (_, TyKind::Never) => Ok(Vec::new()),
            (TyKind::Infer(InferVar::Int(var)), other)
            | (other, TyKind::Infer(InferVar::Int(var))) => {
                let int_ty = if a_is_int { a } else { b };
                match &other {
                    TyKind::Int(_)
                    | TyKind::Uint(_)
                    | TyKind::Infer(InferVar::Int(_))
                    | TyKind::Error => {
                        self.int_vars[var].value = Some(b);
                        Ok(Vec::new())
                    }
                    TyKind::Infer(InferVar::Ty(general)) => {
                        self.ty_vars[*general].value = Some(int_ty);
                        Ok(Vec::new())
                    }
                    _ => Err(vec![GlyimDiagnostic::type_error(
                        span,
                        format!(
                            "mismatched types: expected integer type, found {}",
                            PrintTy::new(b, ctx)
                        ),
                    )]),
                }
            }
            (TyKind::Infer(InferVar::Float(var)), other)
            | (other, TyKind::Infer(InferVar::Float(var))) => {
                let float_ty = if a_is_float { a } else { b };
                match &other {
                    TyKind::Float(_) | TyKind::Infer(InferVar::Float(_)) | TyKind::Error => {
                        self.float_vars[var].value = Some(b);
                        Ok(Vec::new())
                    }
                    TyKind::Infer(InferVar::Ty(general)) => {
                        self.ty_vars[*general].value = Some(float_ty);
                        Ok(Vec::new())
                    }
                    _ => Err(vec![GlyimDiagnostic::type_error(
                        span,
                        format!(
                            "mismatched types: expected float type, found {}",
                            PrintTy::new(b, ctx)
                        ),
                    )]),
                }
            }
            (TyKind::Infer(InferVar::Ty(var)), other)
            | (other, TyKind::Infer(InferVar::Ty(var))) => {
                // The value being bound to `var` is the *non-infer* side. When
                // the infer var is the second argument (`b`), the trivial
                // `occurs(var, Infer(var))` check would otherwise misfire and
                // report a spurious infinite-type error.
                let other_ty = if matches!(ctx.ty_kind(a), TyKind::Infer(_)) { b } else { a };
                let tv = &self.ty_vars[var];
                match tv.kind {
                    VariableKind::General => {
                        if self.occurs(ctx, var, other_ty) {
                            return Err(vec![GlyimDiagnostic::type_error(
                                span,
                                format!(
                                    "cannot construct infinite type: {} = {}",
                                    PrintTy::new(a, ctx),
                                    PrintTy::new(b, ctx)
                                ),
                            )]);
                        }
                        self.ty_vars[var].value = Some(other_ty);
                        Ok(Vec::new())
                    }
                    VariableKind::Integer => match &other {
                        TyKind::Int(_) | TyKind::Error => {
                            if self.occurs(ctx, var, other_ty) {
                                return Err(vec![GlyimDiagnostic::type_error(
                                    span,
                                    format!(
                                        "cannot construct infinite type: {} = {}",
                                        PrintTy::new(a, ctx),
                                        PrintTy::new(b, ctx)
                                    ),
                                )]);
                            }
                            self.ty_vars[var].value = Some(other_ty);
                            Ok(Vec::new())
                        }
                        TyKind::Infer(InferVar::Int(_)) | TyKind::Infer(InferVar::Ty(_)) => {
                            if self.occurs(ctx, var, other_ty) {
                                return Err(vec![GlyimDiagnostic::type_error(
                                    span,
                                    format!(
                                        "cannot construct infinite type: {} = {}",
                                        PrintTy::new(a, ctx),
                                        PrintTy::new(b, ctx)
                                    ),
                                )]);
                            }
                            self.ty_vars[var].value = Some(other_ty);
                            Ok(Vec::new())
                        }
                        _ => Err(vec![GlyimDiagnostic::type_error(
                            span,
                            format!(
                                "mismatched types: expected integer type, found {}",
                                PrintTy::new(b, ctx)
                            ),
                        )]),
                    },
                    VariableKind::Float => match &other {
                        TyKind::Float(_) | TyKind::Error => {
                            if self.occurs(ctx, var, other_ty) {
                                return Err(vec![GlyimDiagnostic::type_error(
                                    span,
                                    format!(
                                        "cannot construct infinite type: {} = {}",
                                        PrintTy::new(a, ctx),
                                        PrintTy::new(b, ctx)
                                    ),
                                )]);
                            }
                            self.ty_vars[var].value = Some(other_ty);
                            Ok(Vec::new())
                        }
                        TyKind::Infer(InferVar::Float(_)) | TyKind::Infer(InferVar::Ty(_)) => {
                            if self.occurs(ctx, var, other_ty) {
                                return Err(vec![GlyimDiagnostic::type_error(
                                    span,
                                    format!(
                                        "cannot construct infinite type: {} = {}",
                                        PrintTy::new(a, ctx),
                                        PrintTy::new(b, ctx)
                                    ),
                                )]);
                            }
                            self.ty_vars[var].value = Some(other_ty);
                            Ok(Vec::new())
                        }
                        _ => Err(vec![GlyimDiagnostic::type_error(
                            span,
                            format!(
                                "mismatched types: expected float type, found {}",
                                PrintTy::new(b, ctx)
                            ),
                        )]),
                    },
                }
            }
            (TyKind::Param(param_a), TyKind::Param(param_b)) => {
                if param_a.index == param_b.index {
                    Ok(Vec::new())
                } else {
                    Err(vec![GlyimDiagnostic::type_error(
                        span,
                        format!(
                            "parameter index mismatch: {} vs {}",
                            param_a.index, param_b.index
                        ),
                    )])
                }
            }
            (TyKind::Param(param), TyKind::Infer(InferVar::Ty(var)))
            | (TyKind::Infer(InferVar::Ty(var)), TyKind::Param(param)) => {
                let param_ty = if let TyKind::Param(_) = ctx.ty_kind(a) {
                    a
                } else {
                    b
                };
                if self.occurs(ctx, var, param_ty) {
                    return Err(vec![GlyimDiagnostic::type_error(
                        span,
                        "cannot construct infinite type".to_string(),
                    )]);
                }
                self.ty_vars[var].value = Some(param_ty);
                Ok(Vec::new())
            }
            (TyKind::Param(_), TyKind::Infer(InferVar::Int(_var)))
            | (TyKind::Infer(InferVar::Int(_var)), TyKind::Param(_)) => {
                Err(vec![GlyimDiagnostic::type_error(
                    span,
                    "cannot unify integer variable with type parameter".to_string(),
                )])
            }
            (TyKind::Param(_), TyKind::Infer(InferVar::Float(_var)))
            | (TyKind::Infer(InferVar::Float(_var)), TyKind::Param(_)) => {
                Err(vec![GlyimDiagnostic::type_error(
                    span,
                    "cannot unify float variable with type parameter".to_string(),
                )])
            }
            (TyKind::Ref(r_a, ty_a, mut_a), TyKind::Ref(r_b, ty_b, mut_b)) => {
                if mut_a != mut_b {
                    return Err(vec![GlyimDiagnostic::type_error(
                        span,
                        format!("mismatched mutability: {:?} vs {:?}", mut_a, mut_b),
                    )]);
                }
                let mut constraints = vec![Constraint::RegionEq { a: r_a, b: r_b }];
                constraints.extend(self.unify_tys(ctx, ty_a, ty_b, span)?);
                Ok(constraints)
            }
            (TyKind::Int(int_a), TyKind::Int(int_b)) if int_a == int_b => Ok(Vec::new()),
            (TyKind::Uint(uint_a), TyKind::Uint(uint_b)) if uint_a == uint_b => Ok(Vec::new()),
            (TyKind::Float(float_a), TyKind::Float(float_b)) if float_a == float_b => {
                Ok(Vec::new())
            }
            (TyKind::Bool, TyKind::Bool) => Ok(Vec::new()),
            (TyKind::Char, TyKind::Char) => Ok(Vec::new()),
            (TyKind::String, TyKind::String) => Ok(Vec::new()),
            (TyKind::Unit, TyKind::Unit) => Ok(Vec::new()),
            (TyKind::Tuple(substs_a), TyKind::Tuple(substs_b)) => {
                let args_a = ctx.substitution_args(substs_a);
                let args_b = ctx.substitution_args(substs_b);
                if args_a.len() != args_b.len() {
                    return Err(vec![GlyimDiagnostic::type_error(
                        span,
                        format!(
                            "mismatched types: {} vs {}",
                            PrintTy::new(a, ctx),
                            PrintTy::new(b, ctx)
                        ),
                    )]);
                }
                let pairs: Vec<(Ty, Ty)> = args_a
                    .iter()
                    .zip(args_b.iter())
                    .filter_map(|(ga, gb)| match (ga, gb) {
                        (GenericArg::Ty(ta), GenericArg::Ty(tb)) => Some((*ta, *tb)),
                        _ => None,
                    })
                    .collect();
                if pairs.len() != args_a.len() {
                    return Err(vec![GlyimDiagnostic::type_error(
                        span,
                        "mismatched generic arguments in tuple".to_string(),
                    )]);
                }
                let mut constraints = Vec::new();
                for (ta, tb) in pairs {
                    constraints.extend(self.unify_tys(ctx, ta, tb, span)?);
                }
                Ok(constraints)
            }
            (TyKind::Array(elem_a, const_a), TyKind::Array(elem_b, const_b)) => {
                if const_a.kind != const_b.kind || const_a.ty != const_b.ty {
                    return Err(vec![GlyimDiagnostic::type_error(
                        span,
                        "mismatched array lengths".to_string(),
                    )]);
                }
                self.unify_tys(ctx, elem_a, elem_b, span)
            }
            (TyKind::Slice(elem_a), TyKind::Slice(elem_b)) => {
                self.unify_tys(ctx, elem_a, elem_b, span)
            }
            (TyKind::RawPtr(inner_a, mut_a), TyKind::RawPtr(inner_b, mut_b)) => {
                if mut_a != mut_b {
                    return Err(vec![GlyimDiagnostic::type_error(
                        span,
                        format!("mismatched mutability: {:?} vs {:?}", mut_a, mut_b),
                    )]);
                }
                self.unify_tys(ctx, inner_a, inner_b, span)
            }
            (TyKind::FnPtr(sig_a), TyKind::FnPtr(sig_b)) => {
                if sig_a.unsafety != sig_b.unsafety
                    || sig_a.abi != sig_b.abi
                    || sig_a.c_variadic != sig_b.c_variadic
                {
                    return Err(vec![GlyimDiagnostic::type_error(
                        span,
                        "mismatched function signatures".to_string(),
                    )]);
                }
                let inputs_a = ctx.substitution_args(sig_a.inputs);
                let inputs_b = ctx.substitution_args(sig_b.inputs);
                if inputs_a.len() != inputs_b.len() {
                    return Err(vec![GlyimDiagnostic::type_error(
                        span,
                        "mismatched function argument counts".to_string(),
                    )]);
                }
                let pairs: Vec<(Ty, Ty)> = inputs_a
                    .iter()
                    .zip(inputs_b.iter())
                    .filter_map(|(ga, gb)| match (ga, gb) {
                        (GenericArg::Ty(ta), GenericArg::Ty(tb)) => Some((*ta, *tb)),
                        _ => None,
                    })
                    .collect();
                if pairs.len() != inputs_a.len() {
                    return Err(vec![GlyimDiagnostic::type_error(
                        span,
                        "mismatched function argument kinds".to_string(),
                    )]);
                }
                let mut constraints = Vec::new();
                for (ta, tb) in pairs {
                    constraints.extend(self.unify_tys(ctx, ta, tb, span)?);
                }
                constraints.extend(self.unify_tys(ctx, sig_a.output, sig_b.output, span)?);
                Ok(constraints)
            }
            (TyKind::Adt(id_a, substs_a), TyKind::Adt(id_b, substs_b)) => {
                if id_a != id_b {
                    return Err(vec![GlyimDiagnostic::type_error(
                        span,
                        format!(
                            "mismatched types: {} vs {}",
                            PrintTy::new(a, ctx),
                            PrintTy::new(b, ctx)
                        ),
                    )]);
                }
                let args_a = ctx.substitution_args(substs_a);
                let args_b = ctx.substitution_args(substs_b);
                if args_a.len() != args_b.len() {
                    return Err(vec![GlyimDiagnostic::type_error(
                        span,
                        "mismatched type argument counts".to_string(),
                    )]);
                }
                let pairs: Vec<(Ty, Ty)> = args_a
                    .iter()
                    .zip(args_b.iter())
                    .filter_map(|(ga, gb)| match (ga, gb) {
                        (GenericArg::Ty(ta), GenericArg::Ty(tb)) => Some((*ta, *tb)),
                        _ => None,
                    })
                    .collect();
                if pairs.len() != args_a.len() {
                    return Err(vec![GlyimDiagnostic::type_error(
                        span,
                        "mismatched generic argument kinds in Adt".to_string(),
                    )]);
                }
                let mut constraints = Vec::new();
                for (ta, tb) in pairs {
                    constraints.extend(self.unify_tys(ctx, ta, tb, span)?);
                }
                Ok(constraints)
            }
            (TyKind::FnDef(id_a, substs_a), TyKind::FnDef(id_b, substs_b)) => {
                if id_a != id_b {
                    return Err(vec![GlyimDiagnostic::type_error(
                        span,
                        format!(
                            "mismatched types: {} vs {}",
                            PrintTy::new(a, ctx),
                            PrintTy::new(b, ctx)
                        ),
                    )]);
                }
                let args_a = ctx.substitution_args(substs_a);
                let args_b = ctx.substitution_args(substs_b);
                if args_a.len() != args_b.len() {
                    return Err(vec![GlyimDiagnostic::type_error(
                        span,
                        "mismatched type argument counts".to_string(),
                    )]);
                }
                let pairs: Vec<(Ty, Ty)> = args_a
                    .iter()
                    .zip(args_b.iter())
                    .filter_map(|(ga, gb)| match (ga, gb) {
                        (GenericArg::Ty(ta), GenericArg::Ty(tb)) => Some((*ta, *tb)),
                        _ => None,
                    })
                    .collect();
                if pairs.len() != args_a.len() {
                    return Err(vec![GlyimDiagnostic::type_error(
                        span,
                        "mismatched generic argument kinds in FnDef".to_string(),
                    )]);
                }
                let mut constraints = Vec::new();
                for (ta, tb) in pairs {
                    constraints.extend(self.unify_tys(ctx, ta, tb, span)?);
                }
                Ok(constraints)
            }
            (TyKind::Closure(id_a, substs_a), TyKind::Closure(id_b, substs_b)) => {
                if id_a != id_b {
                    return Err(vec![GlyimDiagnostic::type_error(
                        span,
                        format!(
                            "mismatched types: {} vs {}",
                            PrintTy::new(a, ctx),
                            PrintTy::new(b, ctx)
                        ),
                    )]);
                }
                let args_a = ctx.substitution_args(substs_a);
                let args_b = ctx.substitution_args(substs_b);
                if args_a.len() != args_b.len() {
                    return Err(vec![GlyimDiagnostic::type_error(
                        span,
                        "mismatched type argument counts".to_string(),
                    )]);
                }
                let pairs: Vec<(Ty, Ty)> = args_a
                    .iter()
                    .zip(args_b.iter())
                    .filter_map(|(ga, gb)| match (ga, gb) {
                        (GenericArg::Ty(ta), GenericArg::Ty(tb)) => Some((*ta, *tb)),
                        _ => None,
                    })
                    .collect();
                if pairs.len() != args_a.len() {
                    return Err(vec![GlyimDiagnostic::type_error(
                        span,
                        "mismatched generic argument kinds in Closure".to_string(),
                    )]);
                }
                let mut constraints = Vec::new();
                for (ta, tb) in pairs {
                    constraints.extend(self.unify_tys(ctx, ta, tb, span)?);
                }
                Ok(constraints)
            }
            (TyKind::Dynamic(preds_a, r_a), TyKind::Dynamic(preds_b, r_b)) => {
                if r_a != r_b {
                    return Err(vec![GlyimDiagnostic::type_error(
                        span,
                        format!("region mismatch: {:?} vs {:?}", r_a, r_b),
                    )]);
                }
                let preds_a = preds_a.as_ref().skip_binder();
                let preds_b = preds_b.as_ref().skip_binder();
                if preds_a.len() != preds_b.len() {
                    return Err(vec![GlyimDiagnostic::type_error(
                        span,
                        format!(
                            "dynamic type predicate count mismatch: {} vs {}",
                            preds_a.len(),
                            preds_b.len()
                        ),
                    )]);
                }
                let mut constraints = Vec::new();
                for (pa, pb) in preds_a.iter().zip(preds_b.iter()) {
                    match (pa, pb) {
                        (Predicate::Trait(tp_a), Predicate::Trait(tp_b)) => {
                            if tp_a.trait_ref.def_id != tp_b.trait_ref.def_id {
                                return Err(vec![GlyimDiagnostic::type_error(
                                    span,
                                    format!(
                                        "trait mismatch: {:?} vs {:?}",
                                        tp_a.trait_ref.def_id, tp_b.trait_ref.def_id
                                    ),
                                )]);
                            }
                            // Unify the substitutions - clone args to release borrow on ctx
                            let args_a: Vec<GenericArg> =
                                ctx.substitution_args(tp_a.trait_ref.substs).to_vec();
                            let args_b: Vec<GenericArg> =
                                ctx.substitution_args(tp_b.trait_ref.substs).to_vec();
                            if args_a.len() != args_b.len() {
                                return Err(vec![GlyimDiagnostic::type_error(
                                    span,
                                    "trait substitution count mismatch",
                                )]);
                            }
                            for (arg_a, arg_b) in args_a.iter().zip(args_b.iter()) {
                                match (arg_a, arg_b) {
                                    (GenericArg::Ty(ta), GenericArg::Ty(tb)) => {
                                        constraints.extend(self.unify_tys(ctx, *ta, *tb, span)?);
                                    }
                                    (GenericArg::Lifetime(la), GenericArg::Lifetime(lb)) => {
                                        if la != lb {
                                            constraints.push(Constraint::RegionEq {
                                                a: la.clone(),
                                                b: lb.clone(),
                                            });
                                        }
                                    }
                                    (GenericArg::Const(ca), GenericArg::Const(cb)) => {
                                        if ca != cb {
                                            return Err(vec![GlyimDiagnostic::type_error(
                                                span,
                                                format!("const mismatch: {:?} vs {:?}", ca, cb),
                                            )]);
                                        }
                                    }
                                    _ => {
                                        return Err(vec![GlyimDiagnostic::type_error(
                                            span,
                                            "mismatched generic argument kinds in dynamic trait",
                                        )]);
                                    }
                                }
                            }
                        }
                        (Predicate::RegionOutlives(rp_a), Predicate::RegionOutlives(rp_b)) => {
                            if rp_a.a != rp_b.a || rp_a.b != rp_b.b {
                                constraints.push(Constraint::RegionEq {
                                    a: rp_a.a.clone(),
                                    b: rp_b.a.clone(),
                                });
                                constraints.push(Constraint::RegionEq {
                                    a: rp_a.b.clone(),
                                    b: rp_b.b.clone(),
                                });
                            }
                        }
                        (Predicate::TypeOutlives(tp_a), Predicate::TypeOutlives(tp_b)) => {
                            if tp_a.ty != tp_b.ty || tp_a.region != tp_b.region {
                                constraints.push(Constraint::TypeEq {
                                    a: tp_a.ty,
                                    b: tp_b.ty,
                                });
                                constraints.push(Constraint::RegionEq {
                                    a: tp_a.region.clone(),
                                    b: tp_b.region.clone(),
                                });
                            }
                        }
                        (Predicate::WellFormed(ty_a), Predicate::WellFormed(ty_b)) => {
                            if ty_a != ty_b {
                                constraints.extend(self.unify_tys(ctx, *ty_a, *ty_b, span)?);
                            }
                        }
                        (Predicate::Coerce(a, b), Predicate::Coerce(c, d)) => {
                            constraints.extend(self.unify_tys(ctx, *a, *c, span)?);
                            constraints.extend(self.unify_tys(ctx, *b, *d, span)?);
                        }
                        _ => {
                            return Err(vec![GlyimDiagnostic::type_error(
                                span,
                                format!("predicate kind mismatch: {:?} vs {:?}", pa, pb),
                            )]);
                        }
                    }
                }
                Ok(constraints)
            }
            (TyKind::Opaque(id_a, substs_a), TyKind::Opaque(id_b, substs_b)) => {
                if id_a != id_b {
                    return Err(vec![GlyimDiagnostic::type_error(
                        span,
                        format!(
                            "mismatched types: {} vs {}",
                            PrintTy::new(a, ctx),
                            PrintTy::new(b, ctx)
                        ),
                    )]);
                }
                let args_a = ctx.substitution_args(substs_a);
                let args_b = ctx.substitution_args(substs_b);
                if args_a.len() != args_b.len() {
                    return Err(vec![GlyimDiagnostic::type_error(
                        span,
                        "mismatched type argument counts".to_string(),
                    )]);
                }
                let pairs: Vec<(Ty, Ty)> = args_a
                    .iter()
                    .zip(args_b.iter())
                    .filter_map(|(ga, gb)| match (ga, gb) {
                        (GenericArg::Ty(ta), GenericArg::Ty(tb)) => Some((*ta, *tb)),
                        _ => None,
                    })
                    .collect();
                if pairs.len() != args_a.len() {
                    return Err(vec![GlyimDiagnostic::type_error(
                        span,
                        "mismatched generic argument kinds in Opaque".to_string(),
                    )]);
                }
                let mut constraints = Vec::new();
                for (ta, tb) in pairs {
                    constraints.extend(self.unify_tys(ctx, ta, tb, span)?);
                }
                Ok(constraints)
            }
            (TyKind::Projection(proj_a), TyKind::Projection(proj_b)) => {
                if proj_a.trait_ref.def_id != proj_b.trait_ref.def_id
                    || proj_a.item_name != proj_b.item_name
                {
                    return Err(vec![GlyimDiagnostic::type_error(
                        span,
                        format!(
                            "mismatched types: {} vs {}",
                            PrintTy::new(a, ctx),
                            PrintTy::new(b, ctx)
                        ),
                    )]);
                }
                let args_a = ctx.substitution_args(proj_a.trait_ref.substs);
                let args_b = ctx.substitution_args(proj_b.trait_ref.substs);
                if args_a.len() != args_b.len() {
                    return Err(vec![GlyimDiagnostic::type_error(
                        span,
                        "mismatched type argument counts in projection".to_string(),
                    )]);
                }
                let pairs: Vec<(Ty, Ty)> = args_a
                    .iter()
                    .zip(args_b.iter())
                    .filter_map(|(ga, gb)| match (ga, gb) {
                        (GenericArg::Ty(ta), GenericArg::Ty(tb)) => Some((*ta, *tb)),
                        _ => None,
                    })
                    .collect();
                if pairs.len() != args_a.len() {
                    return Err(vec![GlyimDiagnostic::type_error(
                        span,
                        "mismatched generic argument kinds in projection".to_string(),
                    )]);
                }
                let mut constraints = Vec::new();
                for (ta, tb) in pairs {
                    constraints.extend(self.unify_tys(ctx, ta, tb, span)?);
                }
                Ok(constraints)
            }
            (_a_k, _b_k) => {
                Err(vec![GlyimDiagnostic::type_error(
                    span,
                    format!(
                        "mismatched types: {} vs {}",
                        PrintTy::new(a, ctx),
                        PrintTy::new(b, ctx)
                    ),
                )])
            }
        }
    }

/// resolve_ty_shallow.
    pub fn resolve_ty_shallow(&self, ctx: &dyn TypeLookup, ty: Ty) -> Ty {
        self.resolve_ty_shallow_depth(ctx, ty, 0, &mut std::collections::HashSet::new())
    }

    /// Like `resolve_ty_shallow`, but leaves an unresolved integer-literal
    /// inference var (`InferVar::Int`) as the var itself instead of defaulting
    /// it to `i32`. Used by `unify_tys` so a literal operand (`2`) can still be
    /// bound to its peer type (`u32`, `usize`, …) — defaulting it to `i32`
    /// first would collapse `2 == self.raw` (raw: u32) into `i32 == u32` and
    /// spuriously fail. Final reporting uses `resolve_ty_shallow` (which keeps
    /// the `i32` fallback).
    fn resolve_ty_shallow_preserve_int(
        &self,
        ctx: &dyn TypeLookup,
        ty: Ty,
    ) -> Ty {
        match ctx.ty_kind(ty) {
            TyKind::Infer(InferVar::Int(var)) => {
                if let Some(value) = self.int_vars.get(*var).and_then(|v| v.value) {
                    self.resolve_ty_shallow_preserve_int(ctx, value)
                } else {
                    ty
                }
            }
            TyKind::Infer(InferVar::Ty(var)) => {
                if let Some(value) = self.ty_vars.get(*var).and_then(|v| v.value) {
                    self.resolve_ty_shallow_preserve_int(ctx, value)
                } else {
                    ty
                }
            }
            TyKind::Infer(InferVar::Float(var)) => {
                if let Some(value) = self.float_vars.get(*var).and_then(|v| v.value) {
                    self.resolve_ty_shallow_preserve_int(ctx, value)
                } else {
                    ty
                }
            }
            _ => ty,
        }
    }

    fn resolve_ty_shallow_depth(
        &self,
        ctx: &dyn TypeLookup,
        ty: Ty,
        depth: u32,
        visited: &mut std::collections::HashSet<TyVar>,
    ) -> Ty {
        if depth > MAX_RESOLVE_DEPTH {
            let diag = GlyimDiagnostic::type_error(
                glyim_span::Span::DUMMY,
                "resolution depth limit exceeded".to_string(),
            );
            self.diagnostics.borrow_mut().push(diag);
            return Ty::ERROR;
        }
        match ctx.ty_kind(ty) {
            TyKind::Infer(InferVar::Ty(var)) => {
                if visited.contains(var) {
                    let diag = GlyimDiagnostic::type_error(
                        glyim_span::Span::DUMMY,
                        "infinite type cycle detected while resolving inference variables"
                            .to_string(),
                    );
                    self.diagnostics.borrow_mut().push(diag);
                    return Ty::ERROR;
                }
                visited.insert(*var);
                if let Some(value) = self.ty_vars.get(*var).and_then(|v| v.value) {
                    return self.resolve_ty_shallow_depth(ctx, value, depth + 1, visited);
                }
                ty
            }
            TyKind::Infer(InferVar::Int(var)) => {
                if let Some(value) = self.int_vars.get(*var).and_then(|v| v.value) {
                    self.resolve_ty_shallow_depth(ctx, value, depth + 1, visited)
                } else {
                    // Unsuffixed integer literal left unconstrained: default to
                    // `i32` (Rust fallback semantics). This is the *reporting*
                    // default; `unify_tys` uses a preserve-var variant so a
                    // literal can still be bound to its peer type (`u32`,
                    // `usize`, …) before final resolution.
                    Ty::I32
                }
            }
            TyKind::Infer(InferVar::Float(var)) => {
                if let Some(value) = self.float_vars.get(*var).and_then(|v| v.value) {
                    self.resolve_ty_shallow_depth(ctx, value, depth + 1, visited)
                } else {
                    ty
                }
            }
            _ => ty,
        }
    }

/// fully_resolve.
    pub fn fully_resolve(&self, ctx: &dyn TypeLookup, ty: Ty) -> Result<Ty, Vec<TyVar>> {
        let resolved = self.resolve_ty_shallow(ctx, ty);
        if self.has_unresolved_non_ty_infer(ctx, resolved) {
            return Err(Vec::new());
        }
        if ctx.ty_flags(resolved).contains(TypeFlags::HAS_TY_INFER) {
            let mut unresolved = Vec::new();
            self.collect_unresolved_vars(ctx, resolved, &mut unresolved);
            if unresolved.is_empty() {
                Ok(resolved)
            } else {
                Err(unresolved)
            }
        } else {
            Ok(resolved)
        }
    }

    fn has_unresolved_non_ty_infer(&self, ctx: &dyn TypeLookup, ty: Ty) -> bool {
        match ctx.ty_kind(ty) {
            TyKind::Infer(InferVar::Int(var)) => {
                self.int_vars.get(*var).is_none_or(|v| v.value.is_none())
            }
            TyKind::Infer(InferVar::Float(var)) => {
                self.float_vars.get(*var).is_none_or(|v| v.value.is_none())
            }
            TyKind::Infer(InferVar::Ty(_)) => false,
            TyKind::Ref(_, inner, _) => self.has_unresolved_non_ty_infer(ctx, *inner),
            TyKind::RawPtr(inner, _) => self.has_unresolved_non_ty_infer(ctx, *inner),
            TyKind::Slice(inner) => self.has_unresolved_non_ty_infer(ctx, *inner),
            TyKind::Array(inner, _) => self.has_unresolved_non_ty_infer(ctx, *inner),
            TyKind::Adt(_, substs)
            | TyKind::FnDef(_, substs)
            | TyKind::Closure(_, substs)
            | TyKind::Opaque(_, substs)
            | TyKind::Tuple(substs) => {
                for arg in ctx.substitution_args(*substs) {
                    if let GenericArg::Ty(t) = arg
                        && self.has_unresolved_non_ty_infer(ctx, *t)
                    {
                        return true;
                    }
                }
                false
            }
            TyKind::FnPtr(sig) => {
                for arg in ctx.substitution_args(sig.inputs) {
                    if let GenericArg::Ty(t) = arg
                        && self.has_unresolved_non_ty_infer(ctx, *t)
                    {
                        return true;
                    }
                }
                self.has_unresolved_non_ty_infer(ctx, sig.output)
            }
            _ => false,
        }
    }

    fn collect_unresolved_vars(&self, ctx: &dyn TypeLookup, ty: Ty, vars: &mut Vec<TyVar>) {
        match ctx.ty_kind(ty) {
            TyKind::Infer(InferVar::Ty(var)) => {
                if let Some(tv) = self.ty_vars.get(*var) {
                    if tv.value.is_none() {
                        vars.push(*var);
                    } else if let Some(resolved) = tv.value {
                        self.collect_unresolved_vars(ctx, resolved, vars);
                    }
                }
            }
            TyKind::Infer(InferVar::Int(_)) | TyKind::Infer(InferVar::Float(_)) => {}
            TyKind::Ref(_, inner, _) => self.collect_unresolved_vars(ctx, *inner, vars),
            TyKind::RawPtr(inner, _) => self.collect_unresolved_vars(ctx, *inner, vars),
            TyKind::Slice(inner) => self.collect_unresolved_vars(ctx, *inner, vars),
            TyKind::Array(inner, _) => self.collect_unresolved_vars(ctx, *inner, vars),
            TyKind::Adt(_, substs)
            | TyKind::FnDef(_, substs)
            | TyKind::Closure(_, substs)
            | TyKind::Opaque(_, substs)
            | TyKind::Tuple(substs) => {
                for arg in ctx.substitution_args(*substs) {
                    if let GenericArg::Ty(t) = arg {
                        self.collect_unresolved_vars(ctx, *t, vars);
                    }
                }
            }
            TyKind::FnPtr(sig) => {
                for arg in ctx.substitution_args(sig.inputs) {
                    if let GenericArg::Ty(t) = arg {
                        self.collect_unresolved_vars(ctx, *t, vars);
                    }
                }
                self.collect_unresolved_vars(ctx, sig.output, vars);
            }
            _ => {}
        }
    }

    // Test helpers
    #[allow(dead_code)]
    #[cfg(test)]
    pub(crate) fn ty_var_kind(&self, var: TyVar) -> Option<VariableKind> {
        self.ty_vars.get(var).map(|tv| tv.kind)
    }
    #[cfg(test)]
    #[allow(dead_code)]
    #[allow(dead_code)]
    #[allow(dead_code)]
    pub(crate) fn set_ty_var_value(&mut self, var: TyVar, value: Ty) {
        if let Some(tv) = self.ty_vars.get_mut(var) {
            tv.value = Some(value);
        }
    }
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn set_int_var_value(&mut self, var: IntVar, value: Ty) {
        if let Some(iv) = self.int_vars.get_mut(var) {
            iv.value = Some(value);
        }
    }
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn set_float_var_value(&mut self, var: FloatVar, value: Ty) {
        if let Some(fv) = self.float_vars.get_mut(var) {
            fv.value = Some(value);
        }
    }
}

impl Default for InferenceTable {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
/// Constraint.
pub enum Constraint {
/// Variant.
    TypeEq {
        /// a field.
        a: Ty,
        /// b field.
        b: Ty,
    },
/// Variant.
    RegionEq {
        /// a field.
        a: Region,
        /// b field.
        b: Region,
    },
/// Variant.
    RegionOutlives {
        /// a field.
        a: Region,
        /// b field.
        b: Region,
    },
/// Variant.
    TypeOutlives {
        /// ty field.
        ty: Ty,
        /// region field.
        region: Region,
    },
}

#[test]
fn test_occurs_check_prevents_infinite_type() {
    use glyim_core::interner::Interner;
    use glyim_type::{InferVar, TyCtxMut, TyKind};

    let mut ctx = TyCtxMut::new(Interner::new());
    let mut infer = InferenceTable::new();

    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));

    let list_ty = ctx.mk_ty(TyKind::Slice(var_ty));

    let result = infer.unify(&mut ctx, var_ty, list_ty, glyim_span::Span::DUMMY);
    assert!(
        result.is_err(),
        "Unifying ?T with List<?T> should fail occurs check"
    );
}
