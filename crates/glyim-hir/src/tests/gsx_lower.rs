//! GSX view expressions survive HIR lowering with their structure intact
//! (GSX-in-Glyim §4.1, finding F7: "Add `Expr::Gsx(...)`. Do **not** desugar
//! to `Call` early — the `createElement` mistake").

use crate::pipeline_api::lower_crate_for_pipeline;
use crate::{BodyId, CrateHir, Expr, ItemKind};
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
    assert!(
        diags.is_empty(),
        "Lowering produced diagnostics: {:?}",
        diags
    );
    TestContext { hir, interner }
}

fn fn_body_expr(ctx: &TestContext, fn_name: &str) -> Option<(BodyId, crate::ExprId)> {
    let name = ctx.interner.intern(fn_name);
    for item in ctx.hir.items.iter() {
        if let ItemKind::Fn(f) = &item.kind
            && item.name == name
            && let Some(b) = f.body
        {
            // The body's last expr is the fn's Block; the GSX node is its
            // tail (or a statement inside).
            let blk = ctx.hir.bodies[b].exprs.len() - 1;
            let body = &ctx.hir.bodies[b];
            let inner = match &body.exprs[crate::ExprId::from_raw(blk as u32)] {
                Expr::Block { stmts, tail } => {
                    tail.or_else(|| stmts.last().copied()).expect("gsx expr")
                }
                _ => crate::ExprId::from_raw(blk as u32),
            };
            return Some((b, inner));
        }
    }
    None
}

/// F7: an element stays `Expr::Gsx` — not a call tree.
#[test]
fn gsx_element_survives_hir() {
    let ctx = lower_source("fn v() { <div/> }");
    let (_, e) = fn_body_expr(&ctx, "v").expect("body");
    let body = &ctx.hir.bodies.iter_enumerated().next().unwrap().1;
    match &body.exprs[e] {
        Expr::Gsx(g) => {
            assert!(g.children.is_empty());
            assert!(g.self_closing);
            match &g.tag {
                crate::GsxTag::Intrinsic(n) => {
                    assert_eq!(ctx.interner.resolve(*n), "div");
                }
                other => panic!("unexpected tag: {other:?}"),
            }
        }
        other => panic!("GSX was desugared away: {other:?}"),
    }
}

/// Attrs keep their value kinds: lit / bare / expr hole.
#[test]
fn gsx_attr_values_lower() {
    let ctx = lower_source("fn v() { <input value={x} disabled type=\"text\"/> }");
    let (_, e) = fn_body_expr(&ctx, "v").expect("body");
    let body = &ctx.hir.bodies.iter_enumerated().next().unwrap().1;
    match &body.exprs[e] {
        Expr::Gsx(g) => {
            assert_eq!(g.attrs.len(), 3);
            match &g.attrs[0] {
                crate::GsxAttrHIR::Plain { name, value } => {
                    assert_eq!(ctx.interner.resolve(*name), "value");
                    assert!(matches!(value, crate::GsxAttrValue::Expr(_)));
                }
                other => panic!("{other:?}"),
            }
            assert!(matches!(
                &g.attrs[1],
                crate::GsxAttrHIR::Plain {
                    value: crate::GsxAttrValue::Bare,
                    ..
                }
            ));
            assert!(matches!(
                &g.attrs[2],
                crate::GsxAttrHIR::Plain {
                    value: crate::GsxAttrValue::Lit(_),
                    ..
                }
            ));
        }
        other => panic!("{other:?}"),
    }
}

/// Text children stay string literals (§3.2); dynamic children lower their
/// hole expressions into the body.
#[test]
fn gsx_children_lower() {
    let ctx = lower_source("fn v() { <h1>\"Hi {s.name}\"{s.count}</h1> }");
    let (_, e) = fn_body_expr(&ctx, "v").expect("body");
    let body = &ctx.hir.bodies.iter_enumerated().next().unwrap().1;
    match &body.exprs[e] {
        Expr::Gsx(g) => {
            assert_eq!(g.children.len(), 2);
            assert!(matches!(&g.children[0], crate::GsxChildHIR::Text(_)));
            assert!(matches!(&g.children[1], crate::GsxChildHIR::Expr(_)));
        }
        other => panic!("{other:?}"),
    }
}

/// Directive attrs (`on:click`) and dialect tags (`web:canvas`) keep their
/// namespace structure.
#[test]
fn gsx_directives_and_dialect_tags() {
    let ctx = lower_source("fn v() { <web:canvas on:click={h}/>; }");
    let (_, e) = fn_body_expr(&ctx, "v").expect("body");
    let body = &ctx.hir.bodies.iter_enumerated().next().unwrap().1;
    match &body.exprs[e] {
        Expr::Gsx(g) => {
            match &g.tag {
                crate::GsxTag::Dialect(ns, tag) => {
                    assert_eq!(ctx.interner.resolve(*ns), "web");
                    assert_eq!(ctx.interner.resolve(*tag), "canvas");
                }
                other => panic!("{other:?}"),
            }
            match &g.attrs[0] {
                crate::GsxAttrHIR::Directive { ns, name, .. } => {
                    assert_eq!(ctx.interner.resolve(*ns), "on");
                    assert_eq!(ctx.interner.resolve(*name), "click");
                }
                other => panic!("{other:?}"),
            }
        }
        other => panic!("{other:?}"),
    }
}

/// Nested elements nest in the HIR too (§3.1 shapes).
#[test]
fn gsx_nesting_lower() {
    let ctx = lower_source("fn v() { <ul><li>\"a\"</li></ul> }");
    let (_, e) = fn_body_expr(&ctx, "v").expect("body");
    let body = &ctx.hir.bodies.iter_enumerated().next().unwrap().1;
    match &body.exprs[e] {
        Expr::Gsx(outer) => match &outer.children[0] {
            crate::GsxChildHIR::Node(inner) => match &inner.tag {
                crate::GsxTag::Intrinsic(_) => {}
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}
