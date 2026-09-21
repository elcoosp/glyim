//! Coherence checker: orphan rules, overlap detection, and negative impls.

use std::collections::HashMap;

use glyim_core::def_id::TraitDefId;
use glyim_core::interner::Name;
use glyim_diag::{DiagSeverity, GlyimDiagnostic, SubDiagnostic};
use glyim_span::Span;
use glyim_type::{ImplPolarity, Substitution, Ty, TyCtxMut};

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ResolvedImplHeader {
    pub trait_def_id: Option<TraitDefId>,
    pub trait_name: Option<Name>,
    pub trait_substs: Substitution,
    pub self_ty: Ty,
    pub self_type_name: Option<Name>,
    pub generic_param_names: Vec<Name>,
    pub polarity: ImplPolarity,
    pub span: Span,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
struct RegisteredImpl {
    trait_def_id: TraitDefId,
    self_ty: Ty,
    self_type_name: Option<Name>,
    is_blanket: bool,
    polarity: ImplPolarity,
    span: Span,
}

pub struct CoherenceChecker<'a> {
    def_map: &'a glyim_def_map::CrateDefMap,
    registered: HashMap<TraitDefId, Vec<RegisteredImpl>>,
    negative_impls: Vec<(TraitDefId, Ty)>,
}

impl<'a> CoherenceChecker<'a> {
    pub fn new(def_map: &'a glyim_def_map::CrateDefMap) -> Self {
        Self {
            def_map,
            registered: HashMap::new(),
            negative_impls: Vec::new(),
        }
    }

    /// Structural type matching with generic parameters.
    /// Returns true if the two types can be made equal by substituting generic args.
    fn structural_tys_match(&self, ctx: &TyCtxMut, a: Ty, b: Ty) -> bool {
        // Check for type parameters.
        let a_is_param = matches!(ctx.ty_kind(a), glyim_type::TyKind::Param(_));
        let b_is_param = matches!(ctx.ty_kind(b), glyim_type::TyKind::Param(_));

        if a_is_param && b_is_param {
            // Both are params: they overlap only if they have the same name.
            if let (glyim_type::TyKind::Param(p1), glyim_type::TyKind::Param(p2)) =
                (ctx.ty_kind(a), ctx.ty_kind(b))
            {
                return p1.name == p2.name;
            }
            return false;
        }

        if a_is_param || b_is_param {
            // One is a param, the other is concrete: they overlap.
            return true;
        }

        // Otherwise, structural comparison.
        match (ctx.ty_kind(a), ctx.ty_kind(b)) {
            (glyim_type::TyKind::Adt(id_a, subs_a), glyim_type::TyKind::Adt(id_b, subs_b)) => {
                if id_a != id_b {
                    return false;
                }
                let args_a = ctx.substitution_args(*subs_a);
                let args_b = ctx.substitution_args(*subs_b);
                if args_a.len() != args_b.len() {
                    return false;
                }
                args_a
                    .iter()
                    .zip(args_b.iter())
                    .all(|(ga, gb)| match (ga, gb) {
                        (glyim_type::GenericArg::Ty(ta), glyim_type::GenericArg::Ty(tb)) => {
                            self.structural_tys_match(ctx, *ta, *tb)
                        }
                        // Lifetime/const generics: treat as always-compatible for
                        // overlap purposes (this crate does not yet model const
                        // generic values precisely enough to compare them).
                        _ => true,
                    })
            }
            (
                glyim_type::TyKind::Ref(_, inner_a, mut_a),
                glyim_type::TyKind::Ref(_, inner_b, mut_b),
            ) => mut_a == mut_b && self.structural_tys_match(ctx, *inner_a, *inner_b),
            (
                glyim_type::TyKind::RawPtr(inner_a, mut_a),
                glyim_type::TyKind::RawPtr(inner_b, mut_b),
            ) => mut_a == mut_b && self.structural_tys_match(ctx, *inner_a, *inner_b),
            (glyim_type::TyKind::Slice(inner_a), glyim_type::TyKind::Slice(inner_b)) => {
                self.structural_tys_match(ctx, *inner_a, *inner_b)
            }
            (glyim_type::TyKind::Array(inner_a, _), glyim_type::TyKind::Array(inner_b, _)) => {
                self.structural_tys_match(ctx, *inner_a, *inner_b)
            }
            (glyim_type::TyKind::Tuple(sub_a), glyim_type::TyKind::Tuple(sub_b)) => {
                let args_a = ctx.substitution_args(*sub_a);
                let args_b = ctx.substitution_args(*sub_b);
                if args_a.len() != args_b.len() {
                    return false;
                }
                args_a
                    .iter()
                    .zip(args_b.iter())
                    .all(|(ga, gb)| match (ga, gb) {
                        (glyim_type::GenericArg::Ty(ta), glyim_type::GenericArg::Ty(tb)) => {
                            self.structural_tys_match(ctx, *ta, *tb)
                        }
                        _ => false,
                    })
            }
            (glyim_type::TyKind::Never, glyim_type::TyKind::Never)
            | (glyim_type::TyKind::Unit, glyim_type::TyKind::Unit)
            | (glyim_type::TyKind::Bool, glyim_type::TyKind::Bool)
            | (glyim_type::TyKind::Char, glyim_type::TyKind::Char)
            | (glyim_type::TyKind::String, glyim_type::TyKind::String) => true,
            (glyim_type::TyKind::Int(ia), glyim_type::TyKind::Int(ib)) => ia == ib,
            (glyim_type::TyKind::Uint(ua), glyim_type::TyKind::Uint(ub)) => ua == ub,
            (glyim_type::TyKind::Float(fa), glyim_type::TyKind::Float(fb)) => fa == fb,
            _ => false,
        }
    }

    /// Checks if a type is a blanket impl (contains a type parameter).

    /// Resolve a name in any module of the crate, recursively.
    fn resolve_name_in_any_module(&self, name: Name) -> Option<glyim_core::def_id::LocalDefId> {
        let mut stack = vec![self.def_map.root];
        while let Some(module) = stack.pop() {
            let module_data = &self.def_map.modules[module];
            if let Some((id, _)) = module_data.scope.resolve(name) {
                return Some(id);
            }
            for (_, child) in &module_data.children {
                stack.push(*child);
            }
        }
        None
    }

    /// Returns true if there is a negative impl for the given trait and self type.
    #[allow(dead_code)]
    fn has_negative_impl(&self, trait_def_id: TraitDefId, self_ty: Ty) -> bool {
        self.negative_impls
            .iter()
            .any(|(tid, ty)| *tid == trait_def_id && *ty == self_ty)
    }

    pub fn check_and_register(
        &mut self,
        header: ResolvedImplHeader,
        ctx: &mut TyCtxMut,
        infer: &mut glyim_solve::InferenceTable,
    ) -> Result<(), Vec<GlyimDiagnostic>> {
        if header.trait_def_id.is_some() || header.trait_name.is_some() {
            self.check_orphan_rule(&header)?;
        }

        if let Some(trait_def_id) = header.trait_def_id
            && let Some(errors) = self.check_overlap(trait_def_id, &header, ctx, infer)
        {
            return Err(errors);
        }

        self.register(header);
        Ok(())
    }

    pub(crate) fn check_orphan_rule(
        &self,
        header: &ResolvedImplHeader,
    ) -> Result<(), Vec<GlyimDiagnostic>> {
        let trait_is_local = header
            .trait_name
            .and_then(|n| self.resolve_name_in_any_module(n))
            .is_some()
            || (header.trait_name.is_none() && header.trait_def_id.is_none());

        let self_type_is_local = header
            .self_type_name
            .and_then(|n| self.resolve_name_in_any_module(n))
            .is_some();

        if trait_is_local || self_type_is_local {
            return Ok(());
        }

        let trait_str = header
            .trait_def_id
            .map(|id| format!("trait #{}", id.to_raw()))
            .unwrap_or_else(|| "<unresolved>".to_string());
        let self_str = format!("{:?}", header.self_ty);

        let msg = format!(
            "orphan rule violation: cannot implement foreign {} for foreign type {}",
            trait_str, self_str,
        );
        Err(vec![GlyimDiagnostic::type_error(header.span, msg)])
    }

    fn self_tys_overlap(
        &self,
        old: &RegisteredImpl,
        new: &ResolvedImplHeader,
        ctx: &mut TyCtxMut,
        _infer: &mut glyim_solve::InferenceTable,
    ) -> bool {
        self.structural_tys_match(ctx, old.self_ty, new.self_ty)
    }

    fn check_overlap(
        &self,
        trait_def_id: TraitDefId,
        new_header: &ResolvedImplHeader,
        ctx: &mut TyCtxMut,
        infer: &mut glyim_solve::InferenceTable,
    ) -> Option<Vec<GlyimDiagnostic>> {
        let existing = self.registered.get(&trait_def_id)?;

        for old in existing {
            // Negative impls: same polarity negative doesn't conflict.
            if new_header.polarity == ImplPolarity::Negative
                && old.polarity == ImplPolarity::Negative
            {
                continue;
            }
            if old.polarity == ImplPolarity::Negative
                && new_header.polarity == ImplPolarity::Positive
            {
                continue;
            }

            if self.self_tys_overlap(old, new_header, ctx, infer) {
                return Some(self.make_overlap_diag(new_header, old));
            }
        }
        None
    }

    fn make_overlap_diag(
        &self,
        new: &ResolvedImplHeader,
        old: &RegisteredImpl,
    ) -> Vec<GlyimDiagnostic> {
        let trait_str = new
            .trait_def_id
            .map(|id| format!("trait #{}", id.to_raw()))
            .unwrap_or_else(|| "<inherent>".to_string());

        let msg = format!("conflicting implementations of {}", trait_str);
        let mut diag = GlyimDiagnostic::type_error(new.span, msg);
        diag = diag.with_sub(SubDiagnostic {
            severity: DiagSeverity::Note,
            message: "previous impl here".to_string(),
            span: Some(old.span.into()),
        });
        vec![diag]
    }

    /// Compatibility helper for tests — uses the polarity parameter.
    #[allow(dead_code)]
    pub(crate) fn check_and_register_impl_compat(
        &mut self,
        header: &ResolvedImplHeader,
        polarity: ImplPolarity,
        ctx: &mut TyCtxMut,
        infer: &mut glyim_solve::InferenceTable,
    ) -> Result<(), Vec<GlyimDiagnostic>> {
        let mut header = header.clone();
        header.polarity = polarity;
        self.check_and_register(header, ctx, infer)
    }

    fn register(&mut self, header: ResolvedImplHeader) {
        let trait_def_id = match header.trait_def_id {
            Some(id) => id,
            None => return,
        };

        let is_blanket = !header.generic_param_names.is_empty();

        let polarity = header.polarity;

        if polarity == ImplPolarity::Negative {
            self.negative_impls.push((trait_def_id, header.self_ty));
        }

        self.registered
            .entry(trait_def_id)
            .or_default()
            .push(RegisteredImpl {
                trait_def_id,
                self_ty: header.self_ty,
                self_type_name: header.self_type_name,
                is_blanket,
                polarity,
                span: header.span,
            });
    }
}
