//! End-to-end tests for the `--explain` CLI surface (plan §6 M3.3, CT-5).
//!
//! The plan's CT-5 acceptance line this pins:
//!
//! > `errors/*.toml` (code, category, summary, long explanation, example
//! > fixtures) generates the `ErrorCode` enum, `glyim --explain E0123`, the
//! > docs page, and the ledger's per-feature `errors` cross-check.
//!
//! (docs/roadmaps/glyim-master-plan.md §5.2, "CT-5 Error-code registry")
//!
//! `glyim --explain T0060` must print the registered explanation for the
//! code — looked up through `glyim_diag::error_registry` — without needing
//! an `<INPUT>` file, and must reject unregistered codes with a nonzero
//! exit and a one-line `error:` message rather than a clap usage dump or a
//! panic. `glyim --explain` with no code lists every registered code.

use std::process::Command;

fn cli_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_glyim-cli"))
}

#[test]
fn explain_prints_the_registered_explanation_for_one_code() {
    let out = Command::new(cli_bin())
        .arg("--explain")
        .arg("T0050")
        .output()
        .expect("glyim-cli binary must be runnable");
    assert!(
        out.status.success(),
        "--explain T0050 must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("T0050"),
        "explanation must name the code, got: {stdout}"
    );
    assert!(
        stdout.contains("non-exhaustive match"),
        "explanation must carry the registered title, got: {stdout}"
    );
    assert!(
        stdout.contains("did not cover every variant"),
        "explanation must carry the registered long-form text, got: {stdout}"
    );
    assert!(
        stdout.contains("Add arms for the listed missing variants"),
        "explanation must end with the registered remedy, got: {stdout}"
    );
}

#[test]
fn explain_without_a_code_lists_every_registered_code() {
    let out = Command::new(cli_bin())
        .arg("--explain")
        .output()
        .expect("glyim-cli binary must be runnable");
    assert!(
        out.status.success(),
        "bare --explain must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Glyim error codes"),
        "list mode must title the listing, got: {stdout}"
    );
    // Every registered code appears; spot-check the registry's boundaries
    // (lowest and highest (category, number) entries).
    assert!(stdout.contains("E0001"), "lex entry must be listed");
    assert!(stdout.contains("T0060"), "async-await entry must be listed");
    assert!(stdout.contains("X0000"), "internal entry must be listed");
}

#[test]
fn explain_rejects_an_unregistered_code_gracefully() {
    let out = Command::new(cli_bin())
        .arg("--explain")
        .arg("Z9999")
        .output()
        .expect("glyim-cli binary must be runnable");
    assert_eq!(
        out.status.code(),
        Some(2),
        "unknown code must exit 2 (CLI misuse, matching clap's usage-error code), not panic or exit 0"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown error code"),
        "rejection must be a one-line user-facing error, got: {stderr}"
    );
    assert!(
        stderr.contains("Z9999"),
        "rejection must name the offending code, got: {stderr}"
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).is_empty(),
        "no explanation is printed for an unknown code"
    );
}

#[test]
fn explain_does_not_require_an_input_file() {
    // The pre-clap contract: `glyim --explain T0060` never asks for <INPUT>
    // (mirrors the `-W help` pre-scan; see `run()` in glyim-cli).
    let out = Command::new(cli_bin())
        .arg("--explain")
        .arg("T0060")
        .output()
        .expect("glyim-cli binary must be runnable");
    assert!(
        out.status.success(),
        "--explain must not demand <INPUT>, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("await"),
        "T0060 explanation must mention the async lowering gap, got: {stdout}"
    );
}
