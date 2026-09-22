use glyim_diag::GlyimDiagnostic;
use glyim_type::*;
use std::collections::VecDeque;

#[derive(Clone, Debug)]
/// Obligation.
pub struct Obligation {
    /// Struct.
    pub predicate: Predicate,
    /// Struct.
    pub cause: ObligationCause,
}

#[derive(Clone, Debug)]
/// ObligationCause.
pub struct ObligationCause {
    /// Struct.
    pub span: glyim_span::Span,
    /// Struct.
    pub code: ObligationCauseCode,
}

#[derive(Clone, Debug)]
/// ObligationCauseCode.
pub enum ObligationCauseCode {
    /// Variant.
    WellFormed,
    /// Variant.
    TypeConstruction,
    /// Variant.
    MatchArm,
    /// Variant.
    IfThenElse,
}

/// FulfillmentCtx.
pub struct FulfillmentCtx<'a> {
    /// Struct.
    pub solver: &'a mut dyn crate::solver::TraitSolver,
    /// Struct.
    pub ctx: &'a TyCtx,
    obligations: VecDeque<Obligation>,
    processed_count: usize,
    diagnostics: Vec<GlyimDiagnostic>,
}

#[derive(Clone, Debug)]
/// OverflowError.
pub struct OverflowError {
    /// Struct.
    pub predicate: Predicate,
    /// Struct.
    pub depth: usize,
    /// Source location of the obligation that exceeded the limit. Carried so
    /// the caller can emit the overflow diagnostic at a real span rather than
    /// `Span::DUMMY`.
    pub span: glyim_span::Span,
}

/// can_coerce.
pub fn can_coerce(ctx: &TyCtx, a: Ty, b: Ty) -> bool {
    if a == b {
        return true;
    }
    match (ctx.ty_kind(a), ctx.ty_kind(b)) {
        (TyKind::Array(elem_a, _), TyKind::Slice(elem_b)) if elem_a == elem_b => true,
        (TyKind::Ref(_, inner_a, mut_a), TyKind::Ref(_, inner_b, mut_b)) => {
            // Allow &mut T -> &T as well
            (mut_a == mut_b)
                || (*mut_a == glyim_core::primitives::Mutability::Mut
                    && *mut_b == glyim_core::primitives::Mutability::Not)
                    && can_coerce(ctx, *inner_a, *inner_b)
        }
        (TyKind::RawPtr(inner_a, mut_a), TyKind::RawPtr(inner_b, mut_b)) => {
            (mut_a == mut_b)
                || (*mut_a == glyim_core::primitives::Mutability::Mut
                    && *mut_b == glyim_core::primitives::Mutability::Not)
                    && can_coerce(ctx, *inner_a, *inner_b)
        }
        // §6.2: fn-item coercion to fn pointer. A zero-sized function item
        // coerces to `fn(Args) -> Ret` when its signature matches the pointer's.
        (TyKind::FnDef(fn_def_id, _), TyKind::FnPtr(target_sig)) => {
            ctx.fn_sig(*fn_def_id).is_some_and(|sig| sig == target_sig)
        }
        // §6.2: closure coercion to fn pointer. A *non-capturing* closure
        // coerces to `fn(Args) -> Ret` when its (parameter, return) signature
        // matches the pointer's. Capturing closures cannot be represented as a
        // bare code pointer, so only capture-free closures are coercible.
        (TyKind::Closure(closure_id, _), TyKind::FnPtr(target_sig)) => {
            let Some(sig) = ctx.closure_sig(*closure_id) else {
                return false;
            };
            let capture_count = ctx
                .closure_adt(*closure_id)
                .and_then(|adt_id| ctx.adt_def(adt_id))
                .and_then(|adt| adt.variants.first())
                .map(|v| v.fields.len())
                .unwrap_or(0);
            if capture_count != 0 {
                return false;
            }
            let inputs = ctx.substitution_args(sig.inputs);
            let target_inputs = ctx.substitution_args(target_sig.inputs);
            if inputs.len() != target_inputs.len() {
                return false;
            }
            let params_match = inputs
                .iter()
                .zip(target_inputs.iter())
                .all(|(a, b)| match (a, b) {
                    (GenericArg::Ty(ta), GenericArg::Ty(tb)) => ta == tb,
                    _ => false,
                });
            params_match && sig.output == target_sig.output
        }
        _ => false,
    }
}

impl<'a> FulfillmentCtx<'a> {
    /// new.
    pub fn new(ctx: &'a TyCtx, solver: &'a mut dyn crate::solver::TraitSolver) -> Self {
        Self {
            solver,
            ctx,
            obligations: VecDeque::new(),
            processed_count: 0,
            diagnostics: Vec::new(),
        }
    }

    /// register_obligation.
    pub fn register_obligation(&mut self, obligation: Obligation) {
        self.obligations.push_back(obligation);
    }

    /// process_obligations.
    pub fn process_obligations(&mut self, limit: usize) -> Result<(), OverflowError> {
        while let Some(obligation) = self.obligations.pop_front() {
            self.processed_count += 1;
            if self.processed_count > limit {
                return Err(OverflowError {
                    predicate: obligation.predicate.clone(),
                    depth: self.processed_count,
                    span: obligation.cause.span,
                });
            }
            match &obligation.predicate {
                Predicate::Trait(trait_pred) => match self.solver.can_prove(self.ctx, trait_pred) {
                    crate::solver::SolverResult::Proven => {}
                    crate::solver::SolverResult::Ambiguous => {
                        self.diagnostics.push(GlyimDiagnostic::type_error(
                            obligation.cause.span,
                            format!("ambiguous trait bound: {:?}", trait_pred),
                        ));
                    }
                    crate::solver::SolverResult::DefiniteNo => {
                        self.diagnostics.push(GlyimDiagnostic::type_error(
                            obligation.cause.span,
                            format!("trait bound not satisfied: {:?}", trait_pred),
                        ));
                    }
                },
                Predicate::WellFormed(_)
                | Predicate::TypeOutlives(_)
                | Predicate::RegionOutlives(_)
                | Predicate::Coerce(_, _) => {}
            }
        }
        Ok(())
    }

    /// into_diagnostics.
    pub fn into_diagnostics(self) -> Vec<GlyimDiagnostic> {
        self.diagnostics
    }
}

impl<'a> Extend<Obligation> for FulfillmentCtx<'a> {
    fn extend<T: IntoIterator<Item = Obligation>>(&mut self, iter: T) {
        for ob in iter {
            self.register_obligation(ob);
        }
    }
}
