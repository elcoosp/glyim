//! M3.3 `ice!` + repro bundle + panic hook — acceptance tests (plan §6 M3.3).
//!
//! The plan's work row for M3.3 includes
//! (docs/roadmaps/glyim-master-plan.md §6, M3.3 row, Work column):
//!
//! "ice! + repro bundles + panic hook (fixes P-07: single ICE report,
//! correct repo URL)"
//!
//! P-07's two guarantees — exactly one ICE report per process, and the
//! correct repo URL — are pinned here against the pure report formatter
//! [`format_ice_report`](crate::ice::format_ice_report) and the
//! single-report decision helper. The global panic hook itself is a thin
//! `Once`+`set_hook` wrapper around these pure pieces (installed at
//! compiler entry, not in tests, so the test process never `exit(101)`'s).

use std::sync::atomic::AtomicBool;

use crate::ice::{ReproBundle, first_report, format_ice_report};

const REPO_URL: &str = "https://github.com/elcoosp/glyim";

#[test]
fn report_contains_the_repo_url_exactly_once() {
    // P-07: "correct repo URL" — present, and "single ICE report" — the URL
    // is not repeated (a duplicated report would duplicate the URL).
    let bundle = ReproBundle {
        source: "fn main() {}".into(),
        command: "glyim build foo.g".into(),
    };
    let report = format_ice_report(REPO_URL, Some(&bundle), "lowering invariant violated");
    let occurrences = report.matches(REPO_URL).count();
    assert_eq!(
        occurrences, 1,
        "ICE report must name the repo URL exactly once (P-07), got {occurrences}",
    );
}

#[test]
fn report_contains_message_command_and_source() {
    let bundle = ReproBundle {
        source: "let x: i32 = \"s\";".into(),
        command: "glyim build bad.g".into(),
    };
    let report = format_ice_report(REPO_URL, Some(&bundle), "typeck blew up");
    assert!(
        report.contains("typeck blew up"),
        "report must carry the ICE message"
    );
    assert!(
        report.contains("glyim build bad.g"),
        "report must carry the repro command"
    );
    assert!(
        report.contains("let x: i32 = \"s\";"),
        "report must carry the repro source",
    );
}

#[test]
fn report_works_without_a_bundle() {
    // An unexpected panic (not via `ice!`) has no captured bundle: the hook
    // must still emit a single report with the repo URL.
    let report = format_ice_report(REPO_URL, None, "unexpected");
    assert!(report.contains("unexpected"));
    assert_eq!(report.matches(REPO_URL).count(), 1);
}

#[test]
fn first_report_emits_once_then_suppresses() {
    // P-07: "single ICE report". The panic hook calls `first_report(&flag)`:
    // the first panic returns `true` (emit), every later panic `false`
    // (suppress the duplicate). Verified on a local flag so the test does
    // not mutate process-global state.
    let flag = AtomicBool::new(false);
    assert!(
        first_report(&flag),
        "the first ICE panic must emit a report"
    );
    assert!(
        !first_report(&flag),
        "a second ICE panic must NOT emit a duplicate report"
    );
    assert!(!first_report(&flag), "and neither must a third");
}

#[test]
fn ice_macro_panics_with_the_message() {
    // `ice!` records a repro bundle then panics with the message (the hook
    // formats the report if installed; without the hook the panic propagates
    // and `catch_unwind` observes it). The hook is deliberately NOT installed
    // in this test binary, so the panic unwinds instead of `exit(101)`.
    let bundle = ReproBundle {
        source: "fn main() {}".into(),
        command: "glyim build foo.g".into(),
    };
    let result = std::panic::catch_unwind(|| {
        crate::ice!(bundle, "monomorphization invariant violated");
    });
    let err = result.expect_err("ice! must panic");
    let msg = err
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| err.downcast_ref::<&'static str>().copied())
        .unwrap_or_default();
    assert!(
        msg.contains("monomorphization invariant violated"),
        "ice! panic payload must carry the message, got {msg:?}",
    );
}

#[test]
fn set_last_repro_bundle_round_trips() {
    // `ice!` stores its bundle so the panic hook can attach the repro to the
    // report. The store + read is a pure round-trip (no hook installed).
    let bundle = ReproBundle {
        source: "source line".into(),
        command: "glyim build x.g".into(),
    };
    crate::ice::set_last_repro_bundle(bundle.clone());
    let read = crate::ice::last_repro_bundle_clone();
    assert_eq!(
        read.as_ref().map(|b| b.command.as_str()),
        Some("glyim build x.g")
    );
}
