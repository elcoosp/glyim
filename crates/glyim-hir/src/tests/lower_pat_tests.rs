use crate::pipeline_api::lower_crate_for_pipeline;
use crate::{BodyId, CrateHir, Expr, ItemKind, Pat};
use glyim_core::interner::Interner;
use glyim_test::phase::FrontendTester;

struct TestContext {
    hir: CrateHir,
    interner: Interner,
}

fn lower_source(source: &str) -> TestContext {
    let trace = FrontendTester::new(source).run();
    let root = trace.parse_tree.unwrap();
    let mut interner = Interner::new();
    let (hir, diags) = lower_crate_for_pipeline(&root, &mut interner);
    assert!(diags.is_empty(), "Lowering produced diagnostics: {diags:?}");
    TestContext { hir, interner }
}

fn find_fn_body(ctx: &TestContext, fn_name: &str) -> Option<BodyId> {
    let fn_name_name = ctx.interner.intern(fn_name);
    for item in ctx.hir.items.iter() {
        if let ItemKind::Fn(fn_item) = &item.kind
            && item.name == fn_name_name
        {
            return fn_item.body;
        }
    }
    None
}

fn find_match_arm_pattern(hir: &CrateHir, body_id: BodyId) -> Option<Pat> {
    let body = &hir.bodies[body_id];
    for (_, expr) in body.exprs.iter_enumerated() {
        if let Expr::Match { arms, .. } = expr
            && let Some(first_arm) = arms.first()
        {
            let pat = &body.pats[first_arm.pat];
            return Some(pat.clone());
        }
    }
    None
}

#[test]
fn test_or_pattern_lowering() {
    let source = r"
        fn test(x: Option<i32>) {
            match x {
                Some(y) | None => { }
            }
        }
    ";
    let ctx = lower_source(source);
    let body_id = find_fn_body(&ctx, "test").expect("Function test not found");
    let pat = find_match_arm_pattern(&ctx.hir, body_id).expect("Match arm pattern not found");

    match pat {
        Pat::Or(subpats) => {
            assert_eq!(subpats.len(), 2, "Or pattern should have 2 subpatterns");
            // Ensure none of the subpatterns are Pat::Err
            for &pid in &subpats {
                assert!(!matches!(ctx.hir.bodies[body_id].pats[pid], Pat::Err));
            }
        }
        _ => panic!("Expected Pat::Or, got {pat:?}"),
    }
}
