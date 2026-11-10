//! Library consumer example: the M1.6 strict-xfail witness seam (plan §3.3).
//!
//! A `//@ known-gap: GAP-NNNN` header turns a compile-fail fixture into the
//! reproducible *witness* of an open readiness-ledger gap. The harness
//! inverts the expectation:
//!
//! - **gap open** — the compiler must keep *rejecting* the witness, so the
//!   compile-fail outcome passes through unchanged;
//! - **gap closed** — the compiler starts *accepting* the witness, and the
//!   ordinary `DiagnosticMismatch` would read as "test bug". The witness
//!   seam sharpens that into the loud [`FailureReason::KnownGapClosed`]
//!   failure whose help text tells the author how to retire the witness.
//!
//! The example walks the three contracts a consumer of the seam cares
//! about, without touching the corpus on disk:
//!
//! 1. the **discovery contract**: a `known-gap` tag on a fixture that is
//!    not `compile-fail` is rejected before any compilation happens;
//! 2. the **inversion is a no-op while the gap is open**: a witness the
//!    compiler still rejects keeps plain compile-fail semantics (pass);
//! 3. the **strict-xfail inversion**: a witness the compiler accepts fails
//!    loudly with `KnownGapClosed` (rendered here through its `Display`,
//!    which carries the miette-style `help:` block).
//!
//! Along the way the `tracing` events emitted at the seam (the `witness`
//! span and the gap-closed warning) are printed, so the run doubles as a
//! demo of the observability story.
//!
//! Run with:
//!
//! ```text
//! cargo run -p glyim-test --example known_gap_witness
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use glyim_test::FailureReason;
use glyim_test::harness::collector::{DiscoveredTest, TestCollector};
use glyim_test::harness::compiler::FrontendOnlyCompiler;
use glyim_test::harness::config::{TestConfig, TestMode};
use glyim_test::harness::executor::{TestExecutor, TestOutcome};

/// A witness fixture for `GAP-0001` compiled with the frontend-only
/// compiler: `compile` never runs typeck or lowering, so the only way the
/// compiler can reject the program is a parse error.
///
/// `GAP_OPEN_SOURCE` carries a deliberate parse error. The bare `//~ ERROR`
/// annotations (any message at their line) mirror what a real witness
/// fixture looks like while the gap is open: one on the offending line, and
/// one on the closing brace where the parser's recovery error surfaces.
const GAP_OPEN_SOURCE: &str = "\
fn main() { let x: i32 = ; //~ ERROR
} //~ ERROR
";

/// The same witness after the gap closes: the program compiles cleanly,
/// which is exactly the moment the strict-xfail contract demands the loud
/// `KnownGapClosed` failure.
const GAP_CLOSED_SOURCE: &str = "fn main() {}\n";

/// Builds the in-memory witness fixture the way a corpus fixture would be
/// discovered. `open` selects the still-rejected vs already-accepted body.
fn witness_fixture(open: bool) -> DiscoveredTest {
    DiscoveredTest {
        path: PathBuf::from("tests/compile-fail/async-await-in-loop-known-gap.g"),
        name: "async-await-in-loop-known-gap".to_string(),
        config: TestConfig {
            mode: TestMode::CompileFail,
            known_gap: Some("GAP-0001".to_string()),
            ..TestConfig::default()
        },
        source: Arc::from(if open {
            GAP_OPEN_SOURCE
        } else {
            GAP_CLOSED_SOURCE
        }),
        revisions: vec!["default".to_string()],
    }
}

/// Scenario 1: the discovery contract. Since the M1.7 strict-xfail
/// inversion, a `//@ known-gap:` tag is meaningful on compile-fail **and**
/// the run modes; only compile-pass / ui pin no outcome the inversion can
/// sharpen, so a witness there is annotation drift, rejected at discovery
/// with a `Display` that explains why.
fn scenario_discovery_contract() {
    let dir = tempfile::tempdir().expect("tempdir");
    let fixture = dir.path().join("compile-pass");
    std::fs::create_dir(&fixture).expect("create mode dir");
    let witness = fixture.join("witness.g");
    std::fs::write(
        &witness,
        "//@ mode: compile-pass\n//@ known-gap: GAP-0001\nfn main() {}\n",
    )
    .expect("write fixture");

    let err = TestCollector::new(dir.path())
        .collect(None, None)
        .expect_err("a known-gap tag on a compile-pass fixture is drift");

    println!("scenario 1 — discovery contract");
    println!("{err}");
    println!();
}

/// Scenario 2 + 3: run one witness through the executor and print the
/// outcome, including the miette-style `help:` block on the gap-closed
/// failure.
fn run_witness(open: bool) -> TestOutcome {
    let executor =
        TestExecutor::new(false, false, 1, false).with_compiler(Arc::new(FrontendOnlyCompiler));
    let mut results = executor.run_sequential(&[Arc::new(witness_fixture(open))]);
    results.remove(0).outcome
}

fn outcome_reason(outcome: &TestOutcome) -> Option<&FailureReason> {
    match outcome {
        TestOutcome::Passed => None,
        TestOutcome::Failed { reason } => Some(reason),
        TestOutcome::Ignored => None,
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .without_time()
        .with_target(false)
        .init();

    scenario_discovery_contract();

    println!("scenario 2 — gap open: the compiler still rejects the witness");
    match run_witness(true) {
        TestOutcome::Passed => println!("outcome: Passed (compile-fail semantics unchanged)"),
        other => println!("outcome: {other:?}"),
    }
    println!();

    println!("scenario 3 — gap closed: the compiler accepts the witness");
    let outcome = run_witness(false);
    match outcome_reason(&outcome) {
        Some(FailureReason::KnownGapClosed { gap }) => {
            println!("outcome: Failed — the strict-xfail inversion fired for {gap}");
        }
        Some(reason) => println!("outcome: Failed — unexpected reason:\n{reason}"),
        None => println!("outcome: {outcome:?}"),
    }
    println!();

    // The full Display of the gap-closed failure carries the miette-style
    // help block a contributor needs to retire the witness correctly:
    if let Some(reason) = outcome_reason(&outcome) {
        println!("rendered failure:");
        println!("{reason}");
    }
}
