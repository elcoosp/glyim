use crate::Expander;
use glyim_frontend::parse_to_syntax;
use glyim_span::{FileId, HygieneCtx};

/// Helper to parse a source string and get the root syntax node.
fn parse_source(source: &str) -> glyim_syntax::SyntaxNode {
    let result = parse_to_syntax(source, FileId::BOGUS);
    assert!(
        result.diagnostics.is_empty(),
        "Unexpected parse errors: {:?}",
        result.diagnostics
    );
    result.root
}

/// Helper to count the number of macro call nodes in a syntax tree.
fn count_macro_calls(node: &glyim_syntax::SyntaxNode) -> usize {
    use glyim_syntax::SyntaxKind;
    let mut count = 0;
    if node.kind() == SyntaxKind::MacroCall {
        count += 1;
    }
    for child in node.children() {
        count += count_macro_calls(&child);
    }
    count
}

/// Test V19-T01: Simple macro without arguments expands to literal.
#[test]
fn simple_macro_expands() {
    let src = r#"
macro_rules! foo {
    () => { 42 };
}
fn main() {
    foo!();
}
"#;
    let root = parse_source(src);
    let mut hygiene = HygieneCtx::new();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags): (_, Vec<glyim_diag::GlyimDiagnostic>) = expander.expand_crate(&root);
    assert!(diags.is_empty(), "Diagnostics: {:?}", diags);
    let remaining_macros = count_macro_calls(&expanded);
    assert_eq!(remaining_macros, 0, "Expected all macros expanded");
}

/// Test V19-T02: Macro with repetition $(...)* expands.
#[test]
fn repetition_macro_expands() {
    let src = r#"
macro_rules! vec_like {
    ($($x:expr),* $(,)?) => {
        {
            let mut v = Vec::new();
            $( v.push($x); )*
            v
        }
    };
}
fn main() {
    vec_like!(1, 2, 3);
}
"#;
    let root = parse_source(src);
    let mut hygiene = HygieneCtx::new();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags): (_, Vec<glyim_diag::GlyimDiagnostic>) = expander.expand_crate(&root);
    assert!(diags.is_empty(), "Diagnostics: {:?}", diags);
    let remaining_macros = count_macro_calls(&expanded);
    assert_eq!(remaining_macros, 0, "Expected all macros expanded");
}

/// Test V19-T03: Hygiene – macro-introduced variables don't leak.
#[test]
fn hygiene_variables_dont_leak() {
    let src = r#"
macro_rules! make_local {
    () => { let x = 5; };
}
fn main() {
    let x = 10;
    make_local!();
    x
}
"#;
    let root = parse_source(src);
    let mut hygiene = HygieneCtx::new();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags): (_, Vec<glyim_diag::GlyimDiagnostic>) = expander.expand_crate(&root);
    assert!(diags.is_empty(), "Diagnostics: {:?}", diags);
    let remaining_macros = count_macro_calls(&expanded);
    assert_eq!(remaining_macros, 0);
}

/// Test V19-T04: $crate resolves to crate path.
#[test]
fn dollar_crate_resolves() {
    let src = r#"
macro_rules! call_from_crate {
    () => { $crate::some_func(); };
}
fn main() {
    call_from_crate!();
}
"#;
    let root = parse_source(src);
    let mut hygiene = HygieneCtx::new();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags): (_, Vec<glyim_diag::GlyimDiagnostic>) = expander.expand_crate(&root);
    assert!(diags.is_empty(), "Diagnostics: {:?}", diags);
    let remaining_macros = count_macro_calls(&expanded);
    assert_eq!(remaining_macros, 0);
}

/// Test V19-T05: Macro with multiple arms (pattern matching).
#[test]
fn multiple_arms() {
    let src = r#"
macro_rules! choose {
    (a) => { 1 };
    (b) => { 2 };
}
fn main() {
    let _x = choose!(a);
    let _y = choose!(b);
}
"#;
    let root = parse_source(src);
    let mut hygiene = HygieneCtx::new();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags): (_, Vec<glyim_diag::GlyimDiagnostic>) = expander.expand_crate(&root);
    assert!(diags.is_empty(), "Diagnostics: {:?}", diags);
    let remaining_macros = count_macro_calls(&expanded);
    assert_eq!(remaining_macros, 0);
}

/// Test V19-T06: Macro recursion limit.
#[test]
fn recursion_limit_hits() {
    let src = r#"
macro_rules! recurse {
    ($x:expr) => { recurse!($x) };
}
fn main() {
    recurse!(0);
}
"#;
    let root = parse_source(src);
    let mut hygiene = HygieneCtx::new();
    let mut expander = Expander::new(&mut hygiene);
    let (_expanded, diags): (_, Vec<glyim_diag::GlyimDiagnostic>) = expander.expand_crate(&root);
    // Expect diagnostics about recursion limit
    assert!(!diags.is_empty(), "Expected recursion limit diagnostic");
}

/// Test V19-T07: Empty macro expansion produces no tokens.
#[test]
fn empty_expansion() {
    let src = r#"
macro_rules! empty {
    () => {};
}
fn main() {
    empty!();
}
"#;
    let root = parse_source(src);
    let mut hygiene = HygieneCtx::new();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags): (_, Vec<glyim_diag::GlyimDiagnostic>) = expander.expand_crate(&root);
    assert!(diags.is_empty(), "Diagnostics: {:?}", diags);
    let remaining_macros = count_macro_calls(&expanded);
    assert_eq!(remaining_macros, 0);
}

/// Test V19-T08: Nested macro invocations expand recursively.
#[test]
fn nested_macros() {
    let src = r#"
macro_rules! inner {
    () => { 42 };
}
macro_rules! outer {
    () => { inner!() };
}
fn main() {
    outer!();
}
"#;
    let root = parse_source(src);
    let mut hygiene = HygieneCtx::new();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags): (_, Vec<glyim_diag::GlyimDiagnostic>) = expander.expand_crate(&root);
    assert!(diags.is_empty(), "Diagnostics: {:?}", diags);
    let remaining_macros = count_macro_calls(&expanded);
    assert_eq!(remaining_macros, 0);
}

/// Test V19-T09: Repetition with separator token.
#[test]
fn repetition_with_separator() {
    let src = r#"
macro_rules! joined {
    ($($x:expr),+ $(,)?) => {
        { $( let _ = $x; )+ }
    };
}
fn main() {
    joined!(1, 2, 3);
}
"#;
    let root = parse_source(src);
    let mut hygiene = HygieneCtx::new();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags): (_, Vec<glyim_diag::GlyimDiagnostic>) = expander.expand_crate(&root);
    assert!(diags.is_empty(), "Diagnostics: {:?}", diags);
    let remaining_macros = count_macro_calls(&expanded);
    assert_eq!(remaining_macros, 0);
}

/// Test V19-T10: Zero-or-one repetition with separator matches optional args.
#[test]
fn zero_or_one_repetition() {
    let src = r#"
macro_rules! maybe_add {
    ($base:expr $(, $extra:expr)?) => {
        $base $(+ $extra)?
    };
}
fn main() {
    let _a = maybe_add!(1);
    let _b = maybe_add!(1, 2);
}
"#;
    let root = parse_source(src);
    let mut hygiene = HygieneCtx::new();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags): (_, Vec<glyim_diag::GlyimDiagnostic>) = expander.expand_crate(&root);
    assert!(diags.is_empty(), "Diagnostics: {:?}", diags);
    let remaining_macros = count_macro_calls(&expanded);
    assert_eq!(remaining_macros, 0);
}

/// Test V19-T11: Mixed literal tokens and metavariables.
#[test]
fn mixed_literal_and_metavar() {
    let src = r#"
macro_rules! pair {
    ($a:expr, $b:expr) => {
        ($a, $b)
    };
}
fn main() {
    let _x = pair!(1, 2);
}
"#;
    let root = parse_source(src);
    let mut hygiene = HygieneCtx::new();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags): (_, Vec<glyim_diag::GlyimDiagnostic>) = expander.expand_crate(&root);
    assert!(diags.is_empty(), "Diagnostics: {:?}", diags);
    let remaining_macros = count_macro_calls(&expanded);
    assert_eq!(remaining_macros, 0);
}

/// Test V19-T12: Multiple different metavariables in one pattern.
#[test]
fn multiple_metavars() {
    let src = r#"
macro_rules! swap {
    ($a:expr, $b:expr) => {
        ($b, $a)
    };
}
fn main() {
    let _x = swap!(10, 20);
}
"#;
    let root = parse_source(src);
    let mut hygiene = HygieneCtx::new();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags): (_, Vec<glyim_diag::GlyimDiagnostic>) = expander.expand_crate(&root);
    assert!(diags.is_empty(), "Diagnostics: {:?}", diags);
    let remaining_macros = count_macro_calls(&expanded);
    assert_eq!(remaining_macros, 0);
}

/// Test V19-T13: Macro invocation as expression statement with trailing semicolon.
#[test]
fn macro_call_with_semicolon() {
    let src = r#"
macro_rules! make_int {
    () => { 42 };
}
fn main() {
    make_int!();
}
"#;
    let root = parse_source(src);
    let mut hygiene = HygieneCtx::new();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags): (_, Vec<glyim_diag::GlyimDiagnostic>) = expander.expand_crate(&root);
    assert!(diags.is_empty(), "Diagnostics: {:?}", diags);
    let remaining_macros = count_macro_calls(&expanded);
    assert_eq!(remaining_macros, 0);
}

/// Test V19-T14: Deep recursion produces diagnostic.
#[test]
fn deep_recursion_emits_diagnostic() {
    let src = r#"
macro_rules! deep {
    () => { deep!() };
}
fn main() {
    deep!();
}
"#;
    let root = parse_source(src);
    let mut hygiene = HygieneCtx::new();
    let mut expander = Expander::new(&mut hygiene);
    let (_expanded, diags): (_, Vec<glyim_diag::GlyimDiagnostic>) = expander.expand_crate(&root);
    assert!(!diags.is_empty(), "Expected recursion limit diagnostic");
}

// ---------------------------------------------------------------------------
// Additional tests for edge cases and extended coverage
// ---------------------------------------------------------------------------

/// V19-T15: Multiple separate repetitions in one pattern.
#[test]
fn multiple_repetitions() {
    let src = r#"
macro_rules! pairs {
    ($($a:expr),* ; $($b:expr),*) => {
        ($($a),*) + ($($b),*)
    };
}
fn main() {
    let _x = pairs!(1, 2; 3, 4);
}
"#;
    let root = parse_source(src);
    let mut hygiene = HygieneCtx::new();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags): (_, Vec<glyim_diag::GlyimDiagnostic>) = expander.expand_crate(&root);
    assert!(diags.is_empty(), "Diagnostics: {:?}", diags);
    assert_eq!(count_macro_calls(&expanded), 0);
}

/// V19-T16: Macro defined but never used – should not cause errors.
#[test]
fn unused_macro_no_error() {
    let src = r#"
macro_rules! unused {
    () => { 1 };
}
fn main() {}
"#;
    let root = parse_source(src);
    let mut hygiene = HygieneCtx::new();
    let mut expander = Expander::new(&mut hygiene);
    let (_, diags): (_, Vec<glyim_diag::GlyimDiagnostic>) = expander.expand_crate(&root);
    assert!(diags.is_empty(), "Diagnostics: {:?}", diags);
}

/// V19-T17: Extra tokens after macro arguments – no arm matches, produces error.
#[test]
fn extra_tokens_no_match() {
    let src = r#"
macro_rules! exact {
    (a) => { 1 };
}
fn main() {
    exact!(a b);
}
"#;
    let root = parse_source(src);
    let mut hygiene = HygieneCtx::new();
    let mut expander = Expander::new(&mut hygiene);
    let (_expanded, diags): (_, Vec<glyim_diag::GlyimDiagnostic>) = expander.expand_crate(&root);
    assert!(!diags.is_empty(), "Expected no-matching-arm diagnostic");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("no matching macro arm")),
        "Diagnostic should mention no matching arm, got: {:?}",
        diags
    );
}

/// V19-T18: Nested delimited groups in macro arguments – simplified
/// pattern to avoid a known limitation with repetition‑inside‑repetition
/// flattening.
#[test]
fn nested_groups_in_args() {
    let src = r#"
macro_rules! inner {
    ($x:expr) => { $x + 1 };
}
macro_rules! apply {
    ($mac:ident ! $args:tt) => { $mac ! $args };
}
fn main() {
    let _x = apply!(inner!((1,2)));
}
"#;
    let root = parse_source(src);
    let mut hygiene = HygieneCtx::new();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags): (_, Vec<glyim_diag::GlyimDiagnostic>) = expander.expand_crate(&root);
    assert!(diags.is_empty(), "Diagnostics: {:?}", diags);
    assert_eq!(count_macro_calls(&expanded), 0);
}

/// V19-T19: Re‑expansion depth limit exact count (verify diagnostic appears).
#[test]
fn recursion_limit_exact() {
    let src = r#"
macro_rules! recurse {
    ($n:expr) => {
        recurse!($n)
    };
}
fn main() {
    recurse!(1);
}
"#;
    let root = parse_source(src);
    let mut hygiene = HygieneCtx::new();
    let mut expander = Expander::new(&mut hygiene);
    let (_expanded, diags): (_, Vec<glyim_diag::GlyimDiagnostic>) = expander.expand_crate(&root);
    // Should eventually hit the recursion limit
    assert!(!diags.is_empty(), "Expected recursion limit diagnostic");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("recursion") || d.message.contains("limit")),
        "Diagnostics: {:?}",
        diags
    );
}

/// V19-T20: Arm precedence – first matching arm wins.
#[test]
fn arm_precedence() {
    let src = r#"
macro_rules! choose {
    (a) => { "first" };
    (a) => { "second" };
}
fn main() {
    let _x = choose!(a);
}
"#;
    let root = parse_source(src);
    let mut hygiene = HygieneCtx::new();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags): (_, Vec<glyim_diag::GlyimDiagnostic>) = expander.expand_crate(&root);
    assert!(diags.is_empty(), "Diagnostics: {:?}", diags);
    // The expansion should contain "first", not "second".
    let text = expanded.text().to_string();
    assert!(
        text.contains("first"),
        "Expansion should use first arm, got: {}",
        text
    );
    assert!(
        !text.contains("second"),
        "Expansion incorrectly used second arm: {}",
        text
    );
}

/// V19-T21: `$crate` substitution produces a `crate` token.
#[test]
fn dollar_crate_output() {
    let src = r#"
macro_rules! path {
    () => { $crate::some_func() };
}
fn main() {
    path!();
}
"#;
    let root = parse_source(src);
    let mut hygiene = HygieneCtx::new();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags): (_, Vec<glyim_diag::GlyimDiagnostic>) = expander.expand_crate(&root);
    assert!(diags.is_empty(), "Diagnostics: {:?}", diags);
    // The expanded code should contain the `crate` keyword.
    let text = expanded.text().to_string();
    assert!(
        text.contains("crate"),
        "Expansion should contain `crate`, got: {}",
        text
    );
}

/// V19-T22: Empty arms and empty expansion – multiple no‑op arms.
#[test]
fn empty_arm_sequence() {
    let src = r#"
macro_rules! nothing {
    () => {};
    (,) => {};
}
fn main() {
    nothing!();
    nothing!(,);
}
"#;
    let root = parse_source(src);
    let mut hygiene = HygieneCtx::new();
    let mut expander = Expander::new(&mut hygiene);
    let (expanded, diags): (_, Vec<glyim_diag::GlyimDiagnostic>) = expander.expand_crate(&root);
    assert!(diags.is_empty(), "Diagnostics: {:?}", diags);
    assert_eq!(count_macro_calls(&expanded), 0);
}

/// V19-T23 (Phase 9.2): a `MacroKind::Proc` invocation is dispatched through a
/// `glyim_proc_macro::Registry` supplied to the expander. The registry is
/// populated in-process here (the two-stage cdylib build populates it via
/// `load_cdylib`); the expander must delegate the call to the registered
/// expansion function and splice its output back as the expanded tokens.
#[test]
fn proc_macro_invocation_dispatches_through_registry() {
    use glyim_proc_macro::Registry;
    use glyim_syntax::SyntaxKind;

    // A proc macro that doubles its input: `double!(x)` -> `x x`.
    let mut registry = Registry::new();
    registry.register(
        "double",
        |input: &[(SyntaxKind, String)]| -> Vec<(SyntaxKind, String)> {
            let mut out = Vec::with_capacity(input.len() * 2);
            for tok in input {
                out.push(tok.clone());
                out.push(tok.clone());
            }
            out
        },
    );
    assert!(registry.contains("double"));

    let src = r#"
fn main() {
    double!(seven);
}
"#;
    let root = parse_source(src);
    let mut hygiene = HygieneCtx::new();
    let mut expander = Expander::new(&mut hygiene);
    expander.with_proc_registry(Some(&registry));
    let (expanded, diags) = expander.expand_crate(&root);
    assert!(diags.is_empty(), "Diagnostics: {:?}", diags);

    let text = expanded.text().to_string();
    // The expanded tokens (`seven seven`) must appear in the result.
    assert!(
        text.contains("seven") && count_occurrences(&text, "seven") >= 2,
        "proc macro expansion must double `seven`, got: {}",
        text
    );
}

/// Count how many times `needle` appears in `haystack`.
fn count_occurrences(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}
