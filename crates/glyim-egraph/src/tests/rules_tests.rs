use crate::analysis::GlyimAnalysis;
use crate::lang::GlyimLang;
use crate::rules::core_rewrites;
use egg::{RecExpr, Runner};
use std::time::Duration;

type MyRunner = Runner<GlyimLang, GlyimAnalysis, ()>;

#[test]
fn add_zero_identity() {
    let expr: RecExpr<GlyimLang> = "(+ x 0)".parse().unwrap();
    let rules = core_rewrites();
    let runner = MyRunner::new(GlyimAnalysis::default())
        .with_iter_limit(5)
        .with_node_limit(10_000)
        .with_time_limit(Duration::from_millis(50))
        .with_expr(&expr)
        .run(&rules);
    let x_id = runner.egraph.lookup_expr(&"x".parse::<RecExpr<GlyimLang>>().unwrap()).unwrap();
    let add_id = runner.egraph.lookup_expr(&expr).unwrap();
    assert_eq!(runner.egraph.find(x_id), runner.egraph.find(add_id));
}

#[test]
fn mul_one_identity() {
    let expr: RecExpr<GlyimLang> = "(* x 1)".parse().unwrap();
    let rules = core_rewrites();
    let runner = MyRunner::new(GlyimAnalysis::default())
        .with_iter_limit(5)
        .with_node_limit(10_000)
        .with_time_limit(Duration::from_millis(50))
        .with_expr(&expr)
        .run(&rules);
    let x_id = runner.egraph.lookup_expr(&"x".parse::<RecExpr<GlyimLang>>().unwrap()).unwrap();
    let mul_id = runner.egraph.lookup_expr(&expr).unwrap();
    assert_eq!(runner.egraph.find(x_id), runner.egraph.find(mul_id));
}

#[test]
fn double_negation() {
    let expr: RecExpr<GlyimLang> = "(- (- x))".parse().unwrap();
    let rules = core_rewrites();
    let runner = MyRunner::new(GlyimAnalysis::default())
        .with_iter_limit(5)
        .with_node_limit(10_000)
        .with_time_limit(Duration::from_millis(50))
        .with_expr(&expr)
        .run(&rules);
    let x_id = runner.egraph.lookup_expr(&"x".parse::<RecExpr<GlyimLang>>().unwrap()).unwrap();
    let neg2_id = runner.egraph.lookup_expr(&expr).unwrap();
    assert_eq!(runner.egraph.find(x_id), runner.egraph.find(neg2_id));
}
