use glyim_mutant::engine::MutationEngine;
use glyim_mutant::config::{MutationConfig, MutationOperator};
use glyim_hir::{Hir, HirExpr, HirItem, HirFn};
use glyim_hir::types::ExprId;
use glyim_diag::Span;
use glyim_interner::Interner;

fn span() -> Span { Span::new(0,0) }
fn id() -> ExprId { ExprId::new(0) }

#[test]
fn engine_generates_arithmetic_mutation() {
    let mut interner = Interner::new();
    let name = interner.intern("add_fn");
    let hir_fn = HirFn {
        doc: None,
        name,
        type_params: vec![],
        params: vec![],
        param_mutability: vec![],
        ret: None,
        body: HirExpr::Binary {
            id: ExprId::new(0),
            op: glyim_hir::HirBinOp::Add,
            lhs: Box::new(HirExpr::IntLit { id: ExprId::new(1), value: 1, span: span() }),
            rhs: Box::new(HirExpr::IntLit { id: ExprId::new(2), value: 2, span: span() }),
            span: span(),
        },
        span: span(),
        is_pub: false,
        is_macro_generated: false,
        is_extern_backed: false,
        is_test: false,
        test_config: None,
    };
    let hir = Hir { items: vec![HirItem::Fn(hir_fn)] };
    let config = MutationConfig {
        operators: vec![MutationOperator::ArithmeticPlusToMinus],
        skip_tests: false,
        ..Default::default()
    };
    let mut engine = MutationEngine::new(config);
    let mutations = engine.generate_mutations(&hir);
    assert!(!mutations.is_empty());
    assert!(mutations.iter().any(|m| matches!(m.operator, MutationOperator::ArithmeticPlusToMinus)));
}

#[test]
fn engine_respects_skip_tests() {
    let mut interner = Interner::new();
    let name = interner.intern("test_fn");
    let hir_fn = HirFn {
        doc: None,
        name,
        type_params: vec![],
        params: vec![],
        param_mutability: vec![],
        ret: None,
        body: HirExpr::Binary {
            id: ExprId::new(0),
            op: glyim_hir::HirBinOp::Add,
            lhs: Box::new(HirExpr::IntLit { id: ExprId::new(1), value: 1, span: span() }),
            rhs: Box::new(HirExpr::IntLit { id: ExprId::new(2), value: 2, span: span() }),
            span: span(),
        },
        span: span(),
        is_pub: false,
        is_macro_generated: false,
        is_extern_backed: false,
        is_test: true, // this is a test function
        test_config: Some(glyim_hir::node::HirTestConfig {
            should_panic: false, ignored: false, tags: vec![], source_file: String::new()
        }),
    };
    let hir = Hir { items: vec![HirItem::Fn(hir_fn)] };
    let config = MutationConfig::default(); // skip_tests is true by default
    let mut engine = MutationEngine::new(config);
    let mutations = engine.generate_mutations(&hir);
    assert!(mutations.is_empty(), "should skip test functions");
}
