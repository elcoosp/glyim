use crate::analysis::GlyimAnalysis;
use crate::lang::GlyimLang;
use egg::{Extractor, AstSize};

pub fn extract_best(egraph: &egg::EGraph<GlyimLang, GlyimAnalysis>, root: egg::Id) -> egg::RecExpr<GlyimLang> {
    let cost_fn = AstSize;
    let extractor = Extractor::new(egraph, cost_fn);
    let (_, best) = extractor.find_best(root);
    best
}
