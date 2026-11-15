//! Policy-level tests for the signature-help engine (LSP-SIGNATURE-HELP).
//!
//! Both halves of the request — *which call is the cursor inside* and
//! *what signature does the callee have* — are resolved through in-memory
//! doubles (`MockCallSites` / `MockSignatures`) implementing
//! [`CallSiteSource`] and [`SignatureSource`], not the live
//! `AnalysisDatabase` — the point of the trait seams: the *policy*
//! (innermost-call choice, active-parameter counting, honest misses) is
//! verified decoupled from any graph, index, lock, or source-map
//! machinery. The graph/index-backed path (real HIR call records, real
//! symbol signatures, wire mapping) is covered in `signature_help_tests`.

use crate::signature_help::{
    CallSiteSource, CalledFunction, SignatureData, SignatureSource, compute_signature_help,
};
use crate::tests::helpers::make_span;
use glyim_span::{FileId, Span};

/// The document the tests scan: two call forms in comment lines, the
/// same shapes the reference graph records on real code. Deliberately
/// free of item declarations, so a name's first occurrence IS its call.
const SRC: &str = "// add(1, 2)\n// outer(inner(x, y), z)\n";

/// The whole call form `callee(...)` in `src`: from the callee's first
/// byte through the matching close paren (depth-counted), the same
/// extent the reference graph records for `ReferenceKind::Call`.
fn call_span(src: &str, callee: &str, file: u32) -> Span {
    let form = format!("{callee}(");
    let lo = src
        .find(&form)
        .unwrap_or_else(|| panic!("signature-help fixture drifted: `{callee}(` not found"));
    let open = lo + callee.len();
    let mut depth = 0usize;
    for (i, ch) in src[open..].char_indices() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    return make_span(FileId::from_raw(file), lo, open + i + 1);
                }
            }
            _ => {}
        }
    }
    panic!("signature-help fixture drifted: `{callee}(` never closes");
}

/// In-memory call-site store: returns the *innermost* recorded call
/// whose span contains the cursor (the same choice the production
/// `ReferenceGraphCallSiteSource` makes over the graph's records).
#[derive(Default)]
struct MockCallSites {
    sites: Vec<CalledFunction>,
}

impl MockCallSites {
    fn add(&mut self, src: &str, callee: &str, file: u32) {
        self.sites.push(CalledFunction {
            callee: callee.to_string(),
            span: call_span(src, callee, file),
        });
    }
}

impl CallSiteSource for MockCallSites {
    fn call_site_at(&self, file_id: FileId, offset: usize) -> Option<CalledFunction> {
        self.sites
            .iter()
            .filter(|s| s.span.file == file_id)
            .filter(|s| s.span.lo.to_usize() <= offset && offset < s.span.hi.to_usize())
            .min_by_key(|s| s.span.hi.to_usize() - s.span.lo.to_usize())
            .cloned()
    }
}

/// In-memory signature table.
#[derive(Default)]
struct MockSignatures {
    signatures: Vec<(String, SignatureData)>,
}

impl MockSignatures {
    fn add(&mut self, name: &str, label: &str, params: usize) {
        self.signatures.push((
            name.to_string(),
            SignatureData {
                label: label.to_string(),
                parameters: (0..params).map(|i| format!("p{i}: T{i}")).collect(),
            },
        ));
    }
}

impl SignatureSource for MockSignatures {
    fn signature_of(&self, callee: &str) -> Option<SignatureData> {
        self.signatures
            .iter()
            .find(|(name, _)| name == callee)
            .map(|(_, data)| data.clone())
    }
}

/// The mock store over [`SRC`]: the two call forms with all three
/// callees signed. `outer` deliberately takes ONE parameter while its
/// fixture call passes two arguments — the over-call clamp is pinned
/// below.
fn populated() -> (MockCallSites, MockSignatures) {
    let mut calls = MockCallSites::default();
    calls.add(SRC, "add", 1);
    calls.add(SRC, "outer", 1);
    calls.add(SRC, "inner", 1);
    let mut sigs = MockSignatures::default();
    sigs.add("add", "fn add(a: i32, b: i32) -> i32", 2);
    sigs.add("outer", "fn outer(v: i32) -> i32", 1);
    sigs.add("inner", "fn inner(x: i32, y: i32) -> i32", 2);
    (calls, sigs)
}

/// Byte offset of the first `needle` in [`SRC`].
fn offset_of(needle: &str) -> usize {
    SRC.find(needle).expect("fixture carries the needle")
}

/// Byte offset of the first `needle` in a standalone fixture.
fn find_in(src: &str, needle: &str) -> usize {
    src.find(needle).expect("fixture carries the needle")
}

#[test]
fn the_innermost_call_wins_on_nested_calls() {
    // The cursor sits inside BOTH `outer(inner(x, y), z)` and
    // `inner(x, y)`. Signature help is about the call the argument
    // belongs to, so the innermost span answers.
    let (calls, sigs) = populated();
    let cursor = offset_of("y)");

    let plan = compute_signature_help(&calls, &sigs, SRC, FileId::from_raw(1), cursor)
        .expect("the cursor is inside a call");

    assert_eq!(plan.label, "fn inner(x: i32, y: i32) -> i32");
}

#[test]
fn the_cursor_on_the_callee_name_resolves_the_call() {
    // Signature help also fires on the callee token itself (the trigger
    // for typing `(`), so the whole call span — name included — counts.
    let (calls, sigs) = populated();
    let cursor = offset_of("add("); // the `add` token of line 0's call

    let plan = compute_signature_help(&calls, &sigs, SRC, FileId::from_raw(1), cursor)
        .expect("the cursor is on the callee name");

    assert_eq!(plan.label, "fn add(a: i32, b: i32) -> i32");
    assert_eq!(plan.parameters.len(), 2);
}

#[test]
fn active_parameter_counts_top_level_commas_before_the_cursor() {
    // Cursor on the `2` of `add(1, 2)`: exactly one top-level comma sits
    // before it inside the argument list.
    let (calls, sigs) = populated();
    let cursor = offset_of("2)");

    let plan = compute_signature_help(&calls, &sigs, SRC, FileId::from_raw(1), cursor)
        .expect("the cursor is inside add(1, 2)");

    assert_eq!(plan.label, "fn add(a: i32, b: i32) -> i32");
    assert_eq!(plan.parameters.len(), 2);
    assert_eq!(plan.active_parameter, Some(1));
}

#[test]
fn the_first_argument_activates_parameter_zero() {
    let (calls, sigs) = populated();
    let cursor = offset_of("1,"); // the `1` of `add(1, 2)`

    let plan = compute_signature_help(&calls, &sigs, SRC, FileId::from_raw(1), cursor)
        .expect("the cursor is inside add(1, 2)");

    assert_eq!(plan.active_parameter, Some(0));
}

#[test]
fn nested_call_commas_do_not_count_for_the_outer_call() {
    // `outer(inner(x, y), z)` — the comma inside `inner(...)` sits at
    // paren depth 1 from outer's perspective, so when the cursor lands
    // on `z` (after the nested call closed) only the top-level comma
    // counts. `outer` takes one parameter and its fixture call passes
    // two arguments, so the counted index clamps to the recorded arity
    // (an over-called site still highlights a real parameter). And while
    // the cursor is on `y`, the innermost-hit rule routes the request to
    // `inner`, whose own comma count sees the same comma at depth 0.
    let (calls, sigs) = populated();

    let plan = compute_signature_help(&calls, &sigs, SRC, FileId::from_raw(1), offset_of("z)"))
        .expect("the cursor is inside outer(...)");
    assert_eq!(plan.label, "fn outer(v: i32) -> i32");
    assert_eq!(plan.parameters.len(), 1);
    assert_eq!(plan.active_parameter, Some(0), "clamped to outer's arity");

    let plan = compute_signature_help(&calls, &sigs, SRC, FileId::from_raw(1), offset_of("y)"))
        .expect("the cursor is inside inner(...)");
    assert_eq!(plan.label, "fn inner(x: i32, y: i32) -> i32");
    assert_eq!(plan.active_parameter, Some(1), "inner's second argument");
}

#[test]
fn zero_parameter_calls_activate_nothing() {
    // A resolved call with no parameters must answer an active_parameter
    // of `None`, not `Some(0)`: there is nothing to highlight.
    let mut calls = MockCallSites::default();
    let src = "// tick()";
    calls.add(src, "tick", 1);
    let mut sigs = MockSignatures::default();
    sigs.add("tick", "fn tick()", 0);

    let plan = compute_signature_help(&calls, &sigs, src, FileId::from_raw(1), find_in(src, "("))
        .expect("the cursor is inside tick()");

    assert!(plan.parameters.is_empty(), "tick takes no parameters");
    assert_eq!(plan.active_parameter, None);
}

#[test]
fn none_off_any_recorded_call() {
    let (calls, sigs) = populated();
    assert!(
        compute_signature_help(&calls, &sigs, SRC, FileId::from_raw(1), 0).is_none(),
        "a cursor outside every call (on the leading slashes) has no signature to show"
    );
}

#[test]
fn none_when_the_callee_has_no_recorded_signature() {
    // Call edges are recorded for ANY called name — including
    // function-typed locals (`callback(...)`) whose signature the symbol
    // index does not carry. An unknown callee answers `null` rather
    // than fabricating a signature.
    let mut calls = MockCallSites::default();
    let src = "// callback(arg)";
    calls.add(src, "callback", 1);
    let sigs = MockSignatures::default();

    assert!(
        compute_signature_help(&calls, &sigs, src, FileId::from_raw(1), find_in(src, "arg"))
            .is_none(),
        "an unrecorded callee has no signature to show"
    );
}

#[test]
fn none_for_a_foreign_file_call() {
    // The recorded calls live in file 1; the request runs for file 2.
    let (calls, sigs) = populated();
    let cursor = offset_of("2)");
    assert!(compute_signature_help(&calls, &sigs, SRC, FileId::from_raw(2), cursor).is_none());
}
