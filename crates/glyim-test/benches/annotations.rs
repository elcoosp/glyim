//! Annotation grammar + tagging-codemod timing harness (M1.4).
//!
//! Dependency-free (`harness = false`): measures the two hot paths the
//! readiness ledger adds to the harness so CI logs can track their
//! headroom. `parse_directives` runs once per fixture at discovery and
//! once per run (the strategies re-parse for warning directives), and
//! `rewrite_fixture` runs once per fixture whenever the codemod is
//! re-applied. Run with:
//!
//! ```text
//! cargo bench -p glyim-test --bench annotations
//! ```

use std::hint::black_box;
use std::time::Instant;

use glyim_test::annotations::codemod::rewrite_fixture;
use glyim_test::annotations::directives::parse_directives;

/// Mirrors the real corpus shape (375 fixtures) so the totals read as
/// "cost of one full discovery pass".
const FIXTURES: usize = 375;

/// A synthetic header at the size of the heaviest retagged fixture:
/// mode + outcome + 3 compile flags + 6 feature tags + backends + gate.
const RICH_HEADER: &str = "\
//@ mode: run-pass\n\
//@ expect-exit: 42\n\
//@ compile-flags: --with-stdlib -O\n\
//@ feature: LANG-SYN-OPERATORS, LANG-BUILTIN-VEC, STD-ALLOC-VEC\n\
//@ feature: RUNTIME-FFI-ATOMIC, TOOL-TEST-RUN-PASS\n\
//@ backends: gam, llvm\n\
//@ needs-llvm\n";

/// A legacy header of the pre-M1.4 shape (what the codemod converts).
const LEGACY_FIXTURE: &str = "\
// Regression note: this fixture once needed `// needs-llvm`.\n\
// test-mode: run-pass\n\
// exit-code: 42\n\
// compile-flags: --with-stdlib\n\
fn main() -> i32 { 42 }\n";

fn main() {
    let full = format!("{RICH_HEADER}fn main() -> i32 {{ 42 }}\n");

    let t = Instant::now();
    let mut parsed = 0usize;
    for _ in 0..FIXTURES {
        let a = parse_directives(black_box(&full)).expect("synthetic header must parse");
        parsed += a.features.len();
    }
    let parse_total = t.elapsed();
    println!(
        "parse_directives: {FIXTURES} rich-header fixtures in {parse_total:.2?} ({parsed} features seen)"
    );

    let t = Instant::now();
    let mut rewritten = 0usize;
    for _ in 0..FIXTURES {
        let out = rewrite_fixture(
            black_box("run-pass/behavior/atomic_usize_fetch_add.g"),
            black_box("run-pass"),
            black_box(LEGACY_FIXTURE),
        )
        .expect("synthetic legacy fixture must rewrite");
        rewritten += out.matches("//@").count();
    }
    let rewrite_total = t.elapsed();
    println!(
        "rewrite_fixture:  {FIXTURES} legacy fixtures in {rewrite_total:.2?} ({rewritten} directives emitted)"
    );
}
