use crate::mock::lower_ctx::SpanOp;
use glyim_span::Span;

/// `assert_span_pushed`.
pub fn assert_span_pushed(ops: &[SpanOp], expected: Span) {
    let found = ops
        .iter()
        .any(|op| matches!(op, SpanOp::Push(s) if *s == expected));
    assert!(found, "expected span {expected:?} to have been pushed");
}
/// `assert_spans_balanced`.
pub fn assert_spans_balanced(ops: &[SpanOp]) {
    let depth: usize = ops.iter().fold(0, |acc: usize, op| match op {
        SpanOp::Push(_) => acc + 1,
        SpanOp::Pop => acc.saturating_sub(1),
    });
    assert_eq!(depth, 0, "unbalanced span operations");
}
