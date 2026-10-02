//! Regression tests for plan §3.6: pathologically nested source must produce
//! a diagnostic instead of overflowing the parser's recursion stack.

use crate::parser::parse_to_syntax;
use glyim_span::FileId;

#[test]
fn deeply_nested_expr_emits_diagnostic_not_panic() {
    // 1000 levels of parentheses inside a function body: enough to exceed
    // MAX_EXPR_DEPTH (256) once parsing descends into the expression. Before
    // §3.6 this would recurse until the thread stack overflowed.
    let opens = "(".repeat(400);
    let src = format!("fn main() {{ {opens} 1");
    let result = parse_to_syntax(&src, FileId::BOGUS);
    // The guard must have fired: at least one "nested too deeply" diagnostic,
    // and — crucially — we did not panic or stack-overflow.
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("nested too deeply")),
        "expected a 'nested too deeply' diagnostic for pathologically nested expr; got: {:?}",
        result.diagnostics
    );
}

#[test]
fn modestly_nested_expr_still_parses() {
    // Well within MAX_EXPR_DEPTH: must parse cleanly (no spurious depth error).
    let src = format!("fn main() {{ {} 1 }}", "(".repeat(20));
    let result = parse_to_syntax(&src, FileId::BOGUS);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("nested too deeply")),
        "legitimate shallow nesting must not trip the depth guard: {:?}",
        result.diagnostics
    );
}

/// FE-6 regression: deeply nested unary / array / block constructs must be
/// rejected with a diagnostic, not crash the compiler with a stack overflow.
/// The old guard covered only `(...)`.
///
/// Depth is kept just over `MAX_EXPR_DEPTH` (256) — enough to prove the guard
/// fires, without pathological memory/CPU (a 5000-deep input can make the
/// parser append diagnostics in a loop).
const OVER: usize = 300;

#[test]
fn deep_unary_chain_is_bounded_not_a_crash() {
    let src = format!("fn main() {{ let _ = {}1; }}", "- ".repeat(OVER));
    let result = parse_to_syntax(&src, FileId::BOGUS);
    assert!(
        !result.diagnostics.is_empty(),
        "deeply nested unary chain must emit a depth diagnostic"
    );
}

#[test]
fn deep_array_nesting_is_bounded_not_a_crash() {
    let src = format!(
        "fn main() {{ let _ = {}0{}; }}",
        "[".repeat(OVER),
        "]".repeat(OVER)
    );
    let result = parse_to_syntax(&src, FileId::BOGUS);
    assert!(
        !result.diagnostics.is_empty(),
        "deeply nested arrays must emit a depth diagnostic"
    );
}

#[test]
fn deep_block_nesting_is_bounded_not_a_crash() {
    let src = format!("fn main() {} {}", "{".repeat(OVER), "}".repeat(OVER));
    let result = parse_to_syntax(&src, FileId::BOGUS);
    assert!(
        !result.diagnostics.is_empty(),
        "deeply nested blocks must emit a depth diagnostic"
    );
}
