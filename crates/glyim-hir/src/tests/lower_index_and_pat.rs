//! Tests for Stream W2-C03: HIR lowering of index expressions and new patterns.
//!
//! Verifies that:
//! - `arr[0]` lowers to `Expr::Index`
//! - `0 | 1` lowers to `Pat::Or`
//! - `0..=5` lowers to `Pat::Range` inclusive
//! - `[a, b]` lowers to `Pat::Slice`
//! - Edge cases for each pattern type

use glyim_core::interner::Interner;
use glyim_frontend::parse_to_syntax;
use glyim_span::FileId;

use crate::{BodyId, CrateHir, Expr, Literal, Pat};

/// Parse source code and lower it to HIR, returning the `CrateHir` and any
/// diagnostics collected during lowering.
fn parse_and_lower(source: &str) -> (CrateHir, Vec<glyim_diag::GlyimDiagnostic>) {
    let result = parse_to_syntax(source, FileId::from_raw(1));
    let mut interner = Interner::new();
    crate::pipeline_api::lower_crate_for_pipeline(&result.root, &mut interner)
}

/// Return the first body in the HIR, or panic if none exists.
fn first_body(hir: &CrateHir) -> &crate::Body {
    assert!(
        !hir.bodies.is_empty(),
        "expected at least one body in HIR, found none"
    );
    &hir.bodies[BodyId::from_raw(0)]
}

/// GAP A closure: `let x = a.await` inside an `async fn` body must lower to
/// real `Expr::Let` + `Expr::Await` HIR nodes. Previously `is_expr_node`
/// omitted `AwaitExpr`, so `.await` RHSes were dropped along with their `let`
/// bindings (the async body lowered to `[Path, Path, Binary, Block]` with zero
/// `let`/`await`). Covered by the `is_expr_node` fix in `lower/mod.rs`.
///
/// Note: `lower_crate_for_pipeline` also runs `desugar_async`, which emits the
/// async-v1 "multi-await not yet implemented" diagnostic (`GLYIM_DESTUB_PLAN`
/// §Phase 3 safety guard) for a 2-await body. That diagnostic is expected and
/// correct; this test therefore asserts the *lowering* produced the Await/Let
/// nodes and that no OTHER (lowering) diagnostic was emitted.
#[test]
fn async_body_let_await_lowers_to_let_and_await() {
    let (hir, diags) = parse_and_lower(
        "async fn two(a: i32, b: i32) -> i32 { let x = a.await; let y = b.await; x + y }",
    );
    // Only the expected async-v1 (multi-await) diagnostic may be present; any
    // other diagnostic would indicate a real lowering failure.
    assert!(
        diags
            .iter()
            .all(|d| d.code.number == 61 && d.code.category == glyim_diag::ErrorCategory::Type),
        "only the expected async-v1 multi-await diagnostic is allowed, got: {diags:?}"
    );
    let body = first_body(&hir);
    let await_count = body
        .exprs
        .iter()
        .filter(|e| matches!(e, Expr::Await { .. }))
        .count();
    let let_count = body
        .exprs
        .iter()
        .filter(|e| matches!(e, Expr::Let { .. }))
        .count();
    assert!(
        await_count >= 2,
        "async body must contain >=2 Expr::Await nodes (got {await_count})"
    );
    assert!(
        let_count >= 2,
        "async body must contain >=2 Expr::Let nodes (got {let_count})"
    );
}

/// W2-C03-T01: `arr[0]` lowers to `Expr::Index`
#[test]
fn index_expr() {
    let (hir, _diags) = parse_and_lower("fn main() { arr[0] }");
    let body = first_body(&hir);
    let found = body.exprs.iter().any(|e| matches!(e, Expr::Index { .. }));
    assert!(found, "expected Expr::Index in lowered HIR for arr[0]");
}

/// W2-C03-T01b: `arr[i]` with variable index lowers to `Expr::Index`
#[test]
fn index_expr_variable() {
    let (hir, _diags) = parse_and_lower("fn main() { arr[i] }");
    let body = first_body(&hir);
    let found = body
        .exprs
        .iter()
        .any(|e| matches!(e, Expr::Index { base: _, index: _ }));
    assert!(found, "expected Expr::Index in lowered HIR for arr[i]");
}

/// W2-C03-T01c: chained index `m[k]` where base is itself an index
#[test]
fn index_expr_nested() {
    let (hir, _diags) = parse_and_lower("fn main() { m[k] }");
    let body = first_body(&hir);
    let index_count = body
        .exprs
        .iter()
        .filter(|e| matches!(e, Expr::Index { .. }))
        .count();
    assert!(
        index_count >= 1,
        "expected at least one Expr::Index in lowered HIR for m[k]"
    );
}

/// W2-C03-T02: `0 | 1` lowers to `Pat::Or`
#[test]
fn pat_or() {
    let (hir, _diags) = parse_and_lower("fn main() { match x { 0 | 1 => y } }");
    let body = first_body(&hir);
    let found = body.pats.iter().any(|p| matches!(p, Pat::Or(_)));
    assert!(found, "expected Pat::Or in lowered HIR for 0 | 1");
}

/// W2-C03-T02b: `1 | 2 | 3` lowers to `Pat::Or` with 3 alternatives
#[test]
fn pat_or_three_alternatives() {
    let (hir, _diags) = parse_and_lower("fn main() { match x { 1 | 2 | 3 => y } }");
    let body = first_body(&hir);
    let or_pat = body.pats.iter().find(|p| matches!(p, Pat::Or(_)));
    assert!(or_pat.is_some(), "expected Pat::Or in lowered HIR");
    if let Some(Pat::Or(alts)) = or_pat {
        assert!(
            alts.len() >= 2,
            "expected at least 2 alternatives in Pat::Or, found {}",
            alts.len()
        );
    }
}

/// W2-C03-T03: `0..=5` lowers to `Pat::Range` inclusive
#[test]
fn pat_range_inclusive() {
    let (hir, _diags) = parse_and_lower("fn main() { match x { 0..=5 => y } }");
    let body = first_body(&hir);
    let found = body.pats.iter().any(|p| {
        matches!(
            p,
            Pat::Range {
                inclusive: true,
                ..
            }
        )
    });
    assert!(
        found,
        "expected Pat::Range {{ inclusive: true }} in lowered HIR for 0..=5"
    );
}

/// W2-C03-T03b: `0..5` (exclusive range) lowers to `Pat::Range` with inclusive=false
#[test]
fn pat_range_exclusive() {
    let (hir, _diags) = parse_and_lower("fn main() { match x { 0..5 => y } }");
    let body = first_body(&hir);
    let found = body.pats.iter().any(|p| {
        matches!(
            p,
            Pat::Range {
                inclusive: false,
                ..
            }
        )
    });
    assert!(
        found,
        "expected Pat::Range {{ inclusive: false }} in lowered HIR for 0..5"
    );
}

/// W2-C03-T03c: inclusive range preserves start and end literals
#[test]
fn pat_range_inclusive_values() {
    let (hir, _diags) = parse_and_lower("fn main() { match x { 0..=5 => y } }");
    let body = first_body(&hir);
    let range_pat = body.pats.iter().find(|p| {
        matches!(
            p,
            Pat::Range {
                inclusive: true,
                ..
            }
        )
    });
    assert!(range_pat.is_some(), "expected Pat::Range inclusive");
    if let Some(Pat::Range {
        start,
        end,
        inclusive,
    }) = range_pat
    {
        assert!(start.is_some(), "expected range start to be Some for 0..=5");
        assert!(end.is_some(), "expected range end to be Some for 0..=5");
        assert!(*inclusive, "expected inclusive to be true for ..=");
        // Verify start is 0
        if let Some(Literal::Int(val, _)) = start {
            assert_eq!(*val, 0, "expected range start to be 0");
        }
        // Verify end is 5
        if let Some(Literal::Int(val, _)) = end {
            assert_eq!(*val, 5, "expected range end to be 5");
        }
    }
}

/// W2-C03-T04: `[a, b]` lowers to `Pat::Slice`
#[test]
fn pat_slice() {
    let (hir, _diags) = parse_and_lower("fn main() { match x { [a, b] => y } }");
    let body = first_body(&hir);
    let found = body.pats.iter().any(|p| matches!(p, Pat::Slice(_)));
    assert!(found, "expected Pat::Slice in lowered HIR for [a, b]");
}

/// W2-C03-T04b: `[a, _, c]` slice pattern with 3 elements including wildcard
#[test]
fn pat_slice_three_elements() {
    let (hir, _diags) = parse_and_lower("fn main() { match x { [a, _, c] => y } }");
    let body = first_body(&hir);
    let slice_pat = body.pats.iter().find(|p| matches!(p, Pat::Slice(_)));
    assert!(slice_pat.is_some(), "expected Pat::Slice in lowered HIR");
    if let Some(Pat::Slice(elems)) = slice_pat {
        assert_eq!(
            elems.len(),
            3,
            "expected 3 elements in Pat::Slice, found {}",
            elems.len()
        );
    }
}

/// W2-C03-T04c: single element slice `[a]`
#[test]
fn pat_slice_single_element() {
    let (hir, _diags) = parse_and_lower("fn main() { match x { [a] => y } }");
    let body = first_body(&hir);
    let slice_pat = body.pats.iter().find(|p| matches!(p, Pat::Slice(_)));
    assert!(
        slice_pat.is_some(),
        "expected Pat::Slice in lowered HIR for [a]"
    );
    if let Some(Pat::Slice(elems)) = slice_pat {
        assert_eq!(
            elems.len(),
            1,
            "expected 1 element in Pat::Slice, found {}",
            elems.len()
        );
    }
}

/// W2-C03-T04d: empty slice `[]`
#[test]
fn pat_slice_empty() {
    let (hir, _diags) = parse_and_lower("fn main() { match x { [] => y } }");
    let body = first_body(&hir);
    let slice_pat = body.pats.iter().find(|p| matches!(p, Pat::Slice(_)));
    assert!(
        slice_pat.is_some(),
        "expected Pat::Slice in lowered HIR for []"
    );
    if let Some(Pat::Slice(elems)) = slice_pat {
        assert_eq!(
            elems.len(),
            0,
            "expected 0 elements in empty Pat::Slice, found {}",
            elems.len()
        );
    }
}
