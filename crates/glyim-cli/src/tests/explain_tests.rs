//! Unit tests for the `--explain` argv pre-scan and the pure renderer
//! (plan §6 M3.3, CT-5). The end-to-end contract — real binary, real
//! stdout/stderr, real exit codes — is pinned by `tests/explain_cli.rs`;
//! these tests pin the parse and render semantics that surface is built
//! from, without spawning a process.

use crate::{explain_render, explain_requested_from_argv};

fn argv(args: &[&str]) -> Vec<String> {
    std::iter::once("glyim".to_string())
        .chain(args.iter().map(|s| (*s).to_string()))
        .collect()
}

#[test]
fn absent_explain_is_not_requested() {
    assert_eq!(explain_requested_from_argv(&argv(&["prog.g"])), None);
    assert_eq!(explain_requested_from_argv(&argv(&["--emit", "obj"])), None);
    assert_eq!(
        explain_requested_from_argv(&argv(&["--explain-rebuild", "prog.g"])),
        None,
        "--explain-rebuild is the M6.9 surface and must not trip the --explain scan",
    );
}

#[test]
fn bare_explain_requests_the_listing() {
    assert_eq!(
        explain_requested_from_argv(&argv(&["--explain"])),
        Some(None)
    );
    assert_eq!(
        explain_requested_from_argv(&argv(&["--emit", "mir", "--explain"])),
        Some(None),
        "the scan runs wherever --explain appears in argv"
    );
}

#[test]
fn explain_with_a_space_value_takes_the_code() {
    assert_eq!(
        explain_requested_from_argv(&argv(&["--explain", "T0050"])),
        Some(Some("T0050".to_string())),
    );
    assert_eq!(
        explain_requested_from_argv(&argv(&["--explain", "T0050", "prog.g"])),
        Some(Some("T0050".to_string())),
        "the code wins; no <INPUT> is required or parsed after it"
    );
}

#[test]
fn explain_equals_form_takes_the_code() {
    assert_eq!(
        explain_requested_from_argv(&argv(&["--explain=T0060"])),
        Some(Some("T0060".to_string())),
    );
    assert_eq!(
        explain_requested_from_argv(&argv(&["--explain="])),
        Some(None)
    );
}

#[test]
fn explain_does_not_swallow_dash_values() {
    assert_eq!(
        explain_requested_from_argv(&argv(&["--explain", "--emit", "obj"])),
        Some(None),
        "a `-`-prefixed token is another flag, not a code"
    );
}

#[test]
fn render_explains_a_registered_code_with_title_and_text() {
    let text = explain_render(Some("T0050")).expect("T0050 must be registered");
    assert!(text.starts_with("T0050 — non-exhaustive match"));
    assert!(text.contains("did not cover every variant"));
    assert!(text.ends_with('\n'));
}

#[test]
fn render_listing_names_every_registered_code() {
    let text = explain_render(None).expect("the listing always renders");
    let count = glyim_diag::error_registry::registry_entries().len();
    assert!(
        text.contains(&format!("Glyim error codes ({count})")),
        "the listing header carries the registry size, got: {text}"
    );
    for entry in glyim_diag::error_registry::registry_entries() {
        assert!(
            text.contains(&entry.code.to_string()),
            "listing must include {}",
            entry.code
        );
    }
}

#[test]
fn render_rejects_an_unregistered_code_with_a_one_liner() {
    let err = explain_render(Some("Z9999")).expect_err("Z9999 is not registered");
    assert!(err.contains("unknown error code `Z9999`"), "got: {err}");
    assert!(err.contains("glyim --explain"), "got: {err}");
    assert!(!err.contains("panicked"), "got: {err}");
}
