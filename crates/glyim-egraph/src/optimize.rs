use crate::analysis::GlyimAnalysis;
use crate::convert::{hir_expr_to_egraph, egraph_to_hir_expr};
use crate::lang::GlyimLang;
use crate::rules::core_rewrites;
use egg::Runner;
use glyim_hir::node::HirFn;
use glyim_hir::types::HirType;
use glyim_hir::{Hir, HirItem};
use egg::EGraph;
use glyim_interner::Interner;
use std::collections::HashMap;

pub struct OptimizeConfig {
    pub iter_limit: usize,
    pub node_limit: usize,
    pub time_limit_ms: u64,
}

impl Default for OptimizeConfig {
    fn default() -> Self {
        Self { iter_limit: 10, node_limit: 50_000, time_limit_ms: 50 }
    }
}

pub fn optimize_fn(
    hir_fn: &HirFn,
    types: &[HirType],
    interner: &Interner,
    config: &OptimizeConfig,
) -> HirFn {
    let mut egraph = EGraph::new(GlyimAnalysis::default());
    let mut type_map = HashMap::new();
    let root = hir_expr_to_egraph(&mut egraph, &hir_fn.body, interner, types, &mut type_map);

    let rules = core_rewrites();
    let runner = Runner::<GlyimLang, GlyimAnalysis, ()>::new(GlyimAnalysis::default())
        .with_iter_limit(config.iter_limit)
        .with_node_limit(config.node_limit)
        .with_time_limit(std::time::Duration::from_millis(config.time_limit_ms))
        .with_egraph(egraph)
        .run(&rules);

    let _best = crate::extract::extract_best(&runner.egraph, root);
    let mut next_id = 1000;
    let optimized_body = egraph_to_hir_expr(&runner.egraph, root, &mut Interner::new(), &mut next_id);

    HirFn {
        body: optimized_body,
        ..hir_fn.clone()
    }
}

pub fn optimize_module(hir: &Hir, types: &[HirType], interner: &Interner) -> Hir {
    let config = OptimizeConfig::default();
    let mut items = Vec::new();
    for item in &hir.items {
        match item {
            HirItem::Fn(hir_fn) => {
                let optimized = optimize_fn(hir_fn, types, interner, &config);
                items.push(HirItem::Fn(optimized));
            }
            other => items.push(other.clone()),
        }
    }
    Hir { items }
}
