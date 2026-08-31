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
