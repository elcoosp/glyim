use glyim_solve::{SolverIteratorNextInfo, SolverResult, TraitSolver};
use glyim_type::{Predicate, TraitPredicate, Ty, TyCtx};

/// MockSolver.
pub struct MockSolver {
    responses: Vec<(PredicateMatcher, SolverResult)>,
    calls: Vec<TraitPredicate>,
    default: SolverResult,
    /// Optional override for `iterator_next_info` so tests can simulate
    /// "solver resolved Iterator::next" vs "solver didn't" in isolation.
    iterator_next_override: Option<Box<dyn Fn(Ty, Ty) -> Option<SolverIteratorNextInfo>>>,
}

enum PredicateMatcher {
    TraitId(glyim_core::def_id::TraitDefId),
    Any,
}

impl MockSolver {
    /// new.
    pub fn new() -> Self {
        Self {
            responses: Vec::new(),
            calls: Vec::new(),
            default: SolverResult::Ambiguous,
            iterator_next_override: None,
        }
    }
    /// default_result.
    pub fn default_result(mut self, result: SolverResult) -> Self {
        self.default = result;
        self
    }
    /// respond_for_trait.
    pub fn respond_for_trait(
        mut self,
        id: glyim_core::def_id::TraitDefId,
        result: SolverResult,
    ) -> Self {
        self.responses.push((PredicateMatcher::TraitId(id), result));
        self
    }
    /// respond_for_any.
    pub fn respond_for_any(mut self, result: SolverResult) -> Self {
        self.responses.push((PredicateMatcher::Any, result));
        self
    }
    /// Attach an `Iterator::next` resolver. When set, `iterator_next_info`
    /// consults this closure, letting a test exercise both the "solver found
    /// it" (`Some(info)`) and "solver didn't" (`None`) branches of the Tier
    /// 1.3 fallback code in isolation.
    pub fn with_iterator_next(
        mut self,
        f: impl Fn(Ty, Ty) -> Option<SolverIteratorNextInfo> + 'static,
    ) -> Self {
        self.iterator_next_override = Some(Box::new(f));
        self
    }
    /// call_count.
    pub fn call_count(&self) -> usize {
        self.calls.len()
    }
    /// calls.
    pub fn calls(&self) -> &[TraitPredicate] {
        &self.calls
    }

    fn find_response(&self, predicate: &TraitPredicate) -> Option<SolverResult> {
        self.responses.iter().find_map(|(m, r)| match m {
            PredicateMatcher::TraitId(id) if predicate.trait_ref.def_id == *id => Some(r.clone()),
            PredicateMatcher::Any => Some(r.clone()),
            _ => None,
        })
    }
}

impl Default for MockSolver {
    fn default() -> Self {
        Self::new()
    }
}

impl TraitSolver for MockSolver {
    fn can_prove(&mut self, _ctx: &TyCtx, predicate: &TraitPredicate) -> SolverResult {
        self.calls.push(predicate.clone());
        self.find_response(predicate)
            .unwrap_or_else(|| self.default.clone())
    }
    fn evaluate_predicate(&mut self, ctx: &TyCtx, predicate: &Predicate) -> SolverResult {
        match predicate {
            Predicate::Trait(tp) => self.can_prove(ctx, tp),
            _ => self.default.clone(),
        }
    }

    fn iterator_next_info(
        &self,
        _ctx_mut: &mut glyim_type::TyCtxMut,
        iter_ty: glyim_type::Ty,
        elem_ty: glyim_type::Ty,
    ) -> Option<glyim_solve::SolverIteratorNextInfo> {
        self.iterator_next_override
            .as_ref()
            .and_then(|f| f(iter_ty, elem_ty))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_core::def_id::FnDefId;

    fn sample_info() -> SolverIteratorNextInfo {
        SolverIteratorNextInfo {
            fn_def_id: FnDefId::from_raw(0),
            fn_substs: glyim_type::Substitution::empty(),
            fn_ty: Ty::ERROR,
            option_ty: Ty::UNIT,
            discr_ty: Ty::UNIT,
            ref_iter_ty: Ty::ERROR,
        }
    }

    #[test]
    fn test_iterator_next_override_resolved() {
        // Tier 7.4: with_iterator_next wires the closure into iterator_next_info,
        // so a test can exercise the "solver resolved Iterator::next" branch.
        let info = sample_info();
        let solver = MockSolver::new().with_iterator_next(move |_iter, _elem| Some(info.clone()));
        let mut ctx = glyim_type::TyCtxMut::new(glyim_core::interner::Interner::new());
        let got = TraitSolver::iterator_next_info(&solver, &mut ctx, Ty::UNIT, Ty::UNIT);
        assert!(
            got.is_some(),
            "iterator_next_info should return the override's Some(info)"
        );
        assert_eq!(got.unwrap().fn_def_id, FnDefId::from_raw(0));
    }

    #[test]
    fn test_iterator_next_no_override_is_none() {
        let solver = MockSolver::new();
        let mut ctx = glyim_type::TyCtxMut::new(glyim_core::interner::Interner::new());
        assert!(TraitSolver::iterator_next_info(&solver, &mut ctx, Ty::UNIT, Ty::UNIT).is_none());
    }

    #[test]
    fn test_iterator_next_override_can_return_none() {
        let solver = MockSolver::new().with_iterator_next(|_iter, _elem| None);
        let mut ctx = glyim_type::TyCtxMut::new(glyim_core::interner::Interner::new());
        assert!(TraitSolver::iterator_next_info(&solver, &mut ctx, Ty::UNIT, Ty::UNIT).is_none());
    }
}
