use crate::analysis::GlyimAnalysis;
use crate::convert::hir_expr_to_egraph;
use crate::lang::GlyimLang;
use crate::rules::core_rewrites;
use egg::{EGraph, Runner};
use glyim_hir::node::HirExpr;
use glyim_hir::types::HirType;
use glyim_interner::Interner;
use std::collections::HashMap;
use std::time::Duration;

/// Result of an equivalence check.
#[derive(Debug)]
pub struct EquivalenceResult {
    pub equivalent: bool,
    pub iterations: usize,
    pub egraph_size: usize,
    pub elapsed: Duration,
}

/// Check if two HIR expressions are algebraically equivalent.
/// This is the foundation for equivalent mutant pruning in Phase 7.
pub fn are_equivalent(
    expr_a: &HirExpr,
    expr_b: &HirExpr,
    types: &[HirType],
    interner: &Interner,
) -> EquivalenceResult {
    let start = std::time::Instant::now();
    let mut egraph = EGraph::new(GlyimAnalysis::default());
    let mut type_map = HashMap::new();
    let id_a = hir_expr_to_egraph(&mut egraph, expr_a, interner, types, &mut type_map);
    let id_b = hir_expr_to_egraph(&mut egraph, expr_b, interner, types, &mut type_map);

    let rules = core_rewrites();
    let runner = Runner::<GlyimLang, GlyimAnalysis, ()>::new(GlyimAnalysis::default())
        .with_iter_limit(10)
        .with_node_limit(10_000)
        .with_time_limit(Duration::from_millis(50))
        .with_egraph(egraph)
        .run(&rules);

    let equivalent = runner.egraph.find(id_a) == runner.egraph.find(id_b);
    EquivalenceResult {
        equivalent,
        iterations: runner.iterations.len(),
        egraph_size: runner.egraph.total_number_of_nodes(),
        elapsed: start.elapsed(),
    }
}
