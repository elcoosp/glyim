use super::*;
use glyim_hir::lower;
use glyim_interner::Interner;
use glyim_parse::parse;

fn lint_source(source: &str) -> Vec<LintDiagnostic> {
    let parse_out = parse(source);
    assert!(parse_out.errors.is_empty(), "parse errors: {:?}", parse_out.errors);
    let mut interner = parse_out.interner;
    let hir = lower(&parse_out.ast, &mut interner);
    let registry = LintRegistry::new();
    lint(&hir, &interner, &registry)
}

#[test]
fn unused_variable_single_unused() {
    let src = "fn main() { let x = 42; }";
    let diags = lint_source(src);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].lint_id, LintId("unused_variable"));
    assert!(diags[0].message.contains("`x`"));
}

#[test]
fn unused_variable_used_no_warning() {
    let src = "fn main() { let x = 42; x }";
    let diags = lint_source(src);
    let unused_vars: Vec<_> = diags.iter().filter(|d| d.lint_id == LintId("unused_variable")).collect();
    assert!(unused_vars.is_empty(), "should not report unused for used variable");
}

#[test]
fn unnecessary_mut_not_reassigned() {
    let src = "fn main() { let mut x = 42; }";
    let diags = lint_source(src);
    let unnecessary_mut: Vec<_> = diags.iter().filter(|d| d.lint_id == LintId("unnecessary_mut")).collect();
    assert_eq!(unnecessary_mut.len(), 1);
    assert!(unnecessary_mut[0].message.contains("`x`"));
}

#[test]
fn unnecessary_mut_reassigned_no_warning() {
    let src = "fn main() { let mut x = 42; x = 10; x }";
    let diags = lint_source(src);
    let unnecessary_mut: Vec<_> = diags.iter().filter(|d| d.lint_id == LintId("unnecessary_mut")).collect();
    assert!(unnecessary_mut.is_empty(), "should not report unnecessary mut when mutated");
}

#[test]
fn unused_function_report() {
    let src = "fn helper() -> i64 { 42 }\nfn main() {}";
    let diags = lint_source(src);
    let unused_fns: Vec<_> = diags.iter().filter(|d| d.lint_id == LintId("unused_function")).collect();
    assert_eq!(unused_fns.len(), 1);
    assert!(unused_fns[0].message.contains("`helper`"));
}

#[test]
fn unused_function_called_no_warning() {
    let src = "fn helper() -> i64 { 42 }\nfn main() { helper(); }";
    let diags = lint_source(src);
    let unused_fns: Vec<_> = diags.iter().filter(|d| d.lint_id == LintId("unused_function")).collect();
    assert!(unused_fns.is_empty(), "called function should not be reported as unused");
}

#[test]
fn dead_code_after_return() {
    let src = "fn main() { return 1; let x = 2; }";
    let diags = lint_source(src);
    let dead: Vec<_> = diags.iter().filter(|d| d.lint_id == LintId("dead_code")).collect();
    assert_eq!(dead.len(), 1);
    assert!(dead[0].message.contains("unreachable"));
}

#[test]
fn no_dead_code_without_return() {
    let src = "fn main() { let x = 1; x }";
    let diags = lint_source(src);
    let dead: Vec<_> = diags.iter().filter(|d| d.lint_id == LintId("dead_code")).collect();
    assert!(dead.is_empty());
}
