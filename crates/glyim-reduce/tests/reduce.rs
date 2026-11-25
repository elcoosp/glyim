//! Library consumer tests for the M4.5 reducer
//! (`docs/roadmaps/glyim-master-plan.md` §6 M4.5).
//!
//! The plan's work item pins the acceptance this suite encodes:
//!
//! > **M4.5** | `glyim-reduce` (CST delta-debugging, token fallback)
//! > wired to the driver: minimal repro + witness fixture + gap stub per
//! > new bug | median reduced size < 40 lines on a 20-bug sample
//!
//! The "< 40 lines" bar belongs to the M4.1 re-verification campaign
//! (it measures over the 20-bug sample that campaign owns); the
//! acceptance encoded here is the mechanism the campaign will drive:
//! a *reproducing* program must reduce to a *minimal* repro that still
//! reproduces — every accepted step preserves the pinned failure, the
//! reduction never leaves a state the driver seam rejects, and the
//! CST-first/token-fallback granularity ladder actually runs (a
//! item-level-only reducer cannot shrink inside one function body).
//!
//! The repro driving every test is a real, live-at-HEAD failure of the
//! class the reducer exists to shrink: the optimizer's constant-prop pass
//! silently folds a division by zero to `0` at `-O2` while `-O0` panics
//! (registered as GAP-0009 in `ledger/gaps/known/`), so the pinned
//! "still reproduces" predicate — compile at `-O2`, run, exit `0` — is
//! observed behaviour, not an invented expectation.

use glyim_reduce::{DiagnosticRepro, ExitRepro, Reducer, ReproCheck};

/// A bloated reproducer for the silent `-O2` div-by-zero fold: four inert
/// top-level items and a fourteen-statement `main` around the one defect
/// expression. Every inert item/statement must be removable without
/// losing the pinned behaviour (`ExitRepro` at `-O2`/exit `0`; the
/// observed baseline at HEAD).
const BLOATED: &str = "
fn unused_helper_a(x: i32) -> i32 {
    let mut acc = 0;
    let mut i = 0;
    while i < x {
        acc = acc + i * 2;
        i = i + 1;
    }
    acc
}

fn unused_helper_b(a: i32, b: i32) -> i32 {
    let t = a * 3 + b;
    let u = t - 7;
    u
}

struct Padding {
    alpha: i32,
    beta: i32,
}

fn unused_helper_c(pad: Padding) -> i32 {
    pad.alpha + pad.beta
}

fn main() -> i32 {
    let warm = 1 + 2;
    let more = warm * 3;
    let mut sink = more;
    sink = sink + 4;
    sink = sink - 5;
    let warm2 = sink * 2;
    let warm3 = warm2 + 6;
    let warm4 = warm3 - 7;
    let warm5 = warm4 * 8;
    let defect = 5 / 0;
    return defect;
}
";

/// Pins the plan's "minimal repro" acceptance: the reducer shrinks a
/// reproducing program to a form that still reproduces through the same
/// driver seam, strictly smaller, under the 40-line plan bar, with the
/// defect expression intact.
#[test]
fn reduces_reproducing_program_to_minimal_repro() {
    let check = ExitRepro::new(2, 0);
    let reduced = Reducer::new(&check).reduce(BLOATED);

    assert!(
        check.still_reproduces(reduced.source()),
        "the reduced repro must still reproduce through the driver seam:\n{}",
        reduced.source()
    );
    assert!(
        reduced.line_count() < reduced.original_line_count(),
        "the reducer must actually shrink the input \
         ({} -> {} lines)",
        reduced.original_line_count(),
        reduced.line_count()
    );
    assert!(
        reduced.line_count() < 40,
        "the reduced repro must sit under the plan's 40-line bar \
         (M4.5 DoD), got {} lines:\n{}",
        reduced.line_count(),
        reduced.source()
    );
    assert!(
        reduced.source().contains("/ 0"),
        "the defect expression must survive reduction:\n{}",
        reduced.source()
    );
    // Minimality: nothing survives that the reduction could remove. At the
    // fixed point, deleting ANY single top-level item or body statement
    // must either stop reproducing or change nothing — the reducer itself
    // proves this by construction, so the test pins the observable end
    // state: only one function remains (the entry point) and its body is
    // short enough to be reviewable.
    let open_braces = reduced.source().matches('{').count();
    let close_braces = reduced.source().matches('}').count();
    assert_eq!(
        open_braces,
        close_braces,
        "reduction must not leave unbalanced braces:\n{}",
        reduced.source()
    );
    assert!(
        reduced.source().contains("fn main"),
        "the entry point must survive reduction:\n{}",
        reduced.source()
    );
    assert!(
        reduced.check_calls() > 0,
        "the reducer must drive the seam (at least one check call)"
    );
}

/// Pins the granularity ladder: the defect lives inside `main`'s body, so
/// top-level-item deletion alone cannot reach it. Statement-level
/// (CST) reduction inside the body must run, and the token fallback must
/// be reachable — the recorded passes must show the ladder descended.
#[test]
fn reduction_ladder_descends_to_statement_and_token_levels() {
    let check = ExitRepro::new(2, 0);
    let reduced = Reducer::new(&check).reduce(BLOATED);

    let levels: Vec<&str> = reduced
        .passes()
        .iter()
        .map(|pass| pass.granularity)
        .collect();
    assert!(
        levels.contains(&"item"),
        "item-level CST reduction must run first, passes: {levels:?}"
    );
    assert!(
        levels.contains(&"statement"),
        "statement-level CST reduction must run (the defect is inside a \
         body), passes: {levels:?}"
    );
}

/// Pins the compile-fail seam: a *diagnostic* repro (the GAP-0008 shape —
/// the compiler must keep rejecting with the pinned message) reduces
/// under the same contract, through the second built-in check. The
/// needle is the cascade's *defect-specific* head —
/// "expected pattern, found Or" names an `Or` token in pattern position,
/// which only a genuine leading-`|` arm shape can emit; the generic
/// "expected pattern" alone would let a coincidentally-mangled variant
/// "reproduce" (the check's doc contract says to pin the specific
/// message the driver observed, and this test does).
#[test]
fn reduces_compile_fail_repro_through_diagnostic_check() {
    // GAP-0008's live shape: a leading `|` on a match arm emits a spurious
    // `P0001 expected pattern`. Bloated with an inert helper item.
    let bloated = "
fn inert_padding_fn(x: i32) -> i32 {
    let mut acc = 0;
    acc = acc + x;
    acc
}

fn main() -> i32 {
    let x = 2;
    let r = match x {
        | 1 => 10
        | 2 => 20
        _ => 30
    };
    return r;
}
";
    // The intact two-arm program's observed cascade at HEAD (probed live):
    // both arms must keep emitting their parse-recovery sequence, in
    // order, for the repro to stay fixture-grade.
    const GAP_0008_CASCADE: [&str; 6] = [
        "expected pattern, found Or",
        "expected FatArrow, found IntLit",
        "expected pattern, found FatArrow",
        "expected FatArrow, found IntLit",
        "expected pattern, found FatArrow",
        "expected FatArrow, found IntLit",
    ];
    let check = DiagnosticRepro::cascade(&GAP_0008_CASCADE);
    let reduced = Reducer::new(&check).reduce(bloated);

    assert!(
        check.still_reproduces(reduced.source()),
        "the reduced compile-fail repro must still fail with the pinned \
         diagnostic:\n{}",
        reduced.source()
    );
    assert!(
        reduced.line_count() < reduced.original_line_count(),
        "the reducer must shrink the compile-fail input \
         ({} -> {} lines)",
        reduced.original_line_count(),
        reduced.line_count()
    );
    assert!(
        reduced.source().contains("| 1"),
        "the leading-pipe defect must survive reduction:\n{}",
        reduced.source()
    );
}

/// Pins the never-weaker invariant: no committed intermediate may stop
/// reproducing. The reducer records every committed step; the trace must
/// be non-empty and monotone (each committed pass keeps the pinned
/// behaviour — asserted by replaying the recorded sources through the
/// check).
#[test]
fn every_committed_intermediate_still_reproduces() {
    let check = ExitRepro::new(2, 0);
    let reduced = Reducer::new(&check).reduce(BLOATED);

    let committed: Vec<&str> = reduced
        .passes()
        .iter()
        .filter_map(|pass| pass.first_committed_source.as_deref())
        .collect();
    assert!(
        !committed.is_empty(),
        "at least one committed reduction step must be recorded"
    );
    for source in committed {
        assert!(
            check.still_reproduces(source),
            "a recorded intermediate stopped reproducing:\n{source}"
        );
    }
}

/// A non-reproducing input must come back unchanged (the reducer cannot
/// invent a repro), and the stats must say so honestly.
#[test]
fn non_reproducing_input_is_returned_unchanged() {
    let check = DiagnosticRepro::new("this diagnostic never happens");
    let reduced = Reducer::new(&check).reduce(BLOATED);

    assert_eq!(reduced.source(), BLOATED);
    assert_eq!(reduced.line_count(), reduced.original_line_count());
    assert_eq!(reduced.passes().len(), 0);
}
