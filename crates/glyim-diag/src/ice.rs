//! `ice!` + repro bundles + panic hook (plan §6 M3.3, fixes P-07).
//!
//! P-07's two guarantees — a *single* ICE report per process and the
//! *correct* repo URL — are realized as pure, unit-tested pieces plus a thin
//! global panic-hook wrapper:
//!
//! - [`format_ice_report`] formats exactly one report (the repo URL appears
//!   once, the message, the command, and the source are attached). The hook
//!   and any future ICE entry point route through it, so the report is never
//!   duplicated.
//! - [`first_report`] is the single-report decision: the first panic in a
//!   process emits, every later one is suppressed.
//! - [`install_ice_handler`] installs the global panic hook that calls the
//!   above two and exits 101 (the panic=abort model, ADR-0005). It is
//!   idempotent (`Once`), so the CLI and a test harness both calling it never
//!   double-register.
//!
//! The hook is deliberately not installed in the test binary: the pure
//! pieces are tested directly, and [`ice!`] is tested via `catch_unwind`
//! (the panic propagates instead of `exit(101)` when no hook is installed).
//!
//! Wiring [`install_ice_handler`] into `glyim-cli`'s entry point is a
//! one-line follow-up that this crate cannot verify locally: `glyim-cli`
//! hard-depends on `glyim-codegen-llvm` (inkwell/llvm-sys 231) and so builds
//! only on hosts with system LLVM 23 (CI's `test-linux-runtime` job). The
//! capability, tests, and example live here, in the LLVM-free leaf.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Once, OnceLock, RwLock};

use std::sync::Arc;

/// The canonical glyim repository URL. Every ICE report names exactly this
/// URL (P-07: "correct repo URL"), so a user hitting an ICE files an issue
/// against the right project regardless of how the compiler was invoked.
pub const REPO_URL: &str = "https://github.com/elcoosp/glyim";

/// A reproduction bundle captured at an ICE and attached to the report so a
/// user can file a reproducible issue without re-deriving the failing input.
///
/// `source` is the source text the compiler was processing; `command` is the
/// command line that reproduces the ICE (e.g. `glyim build foo.g`). Both are
/// attached to the report by [`format_ice_report`].
#[derive(Clone, Debug)]
pub struct ReproBundle {
    /// The source text the compiler was processing when it ICE'd.
    pub source: Arc<str>,
    /// The command line that reproduces the ICE.
    pub command: String,
}

/// Format a single ICE report (P-07).
///
/// Pure: the repo URL appears exactly once, the `message` is carried, and
/// when `bundle` is `Some` the command and source are attached as a
/// reproduction block. Both [`install_ice_handler`]'s panic hook and any
/// future ICE entry point call this, so the report is never duplicated.
#[must_use]
pub fn format_ice_report(repo_url: &str, bundle: Option<&ReproBundle>, message: &str) -> String {
    let mut report = String::new();
    report.push_str("internal compiler error: ");
    report.push_str(message);
    report.push_str("\n\nnote: this is a bug in glyim, not in your code.\n");
    report.push_str("note: please file an issue at ");
    report.push_str(repo_url);
    report.push_str("/issues\n");
    if let Some(bundle) = bundle {
        report.push_str("\n--- reproduction ---\n");
        report.push_str("command: ");
        report.push_str(&bundle.command);
        report.push_str("\nsource:\n");
        report.push_str(&bundle.source);
        if !bundle.source.ends_with('\n') {
            report.push('\n');
        }
        report.push_str("--- end reproduction ---\n");
    }
    report
}

/// P-07 single-report decision. The panic hook calls this with the
/// process-global [`ICE_REPORTED`] flag: the first panic returns `true`
/// (emit the report), every later panic returns `false` (suppress the
/// duplicate). Extracted as a free function so the guarantee is unit-testable
/// on a local flag without mutating process-global state.
pub fn first_report(flag: &AtomicBool) -> bool {
    !flag.swap(true, Ordering::SeqCst)
}

/// Process-global "has an ICE report already been emitted?" flag. Set by the
/// panic hook via [`first_report`]; never reset (a process that ICE'd once
/// exits 101, so a second panic is always a duplicate to suppress).
static ICE_REPORTED: AtomicBool = AtomicBool::new(false);

/// Process-global last reproduction bundle, populated by [`ice!`] so the
/// panic hook can attach it to the report. `RwLock<Option<…>>` because the
/// hook reads while `ice!` writes; the lock is held only for a clone, never
/// across a panic.
static LAST_BUNDLE: OnceLock<RwLock<Option<ReproBundle>>> = OnceLock::new();

fn bundle_slot() -> &'static RwLock<Option<ReproBundle>> {
    LAST_BUNDLE.get_or_init(|| RwLock::new(None))
}

/// Record `bundle` as the last reproduction context. Called by [`ice!`]
/// before it panics, so the panic hook can attach the bundle to the report.
/// Exposed publicly because the macro expands at the call site, outside this
/// module.
pub fn set_last_repro_bundle(bundle: ReproBundle) {
    if let Ok(mut slot) = bundle_slot().write() {
        *slot = Some(bundle);
    }
}

/// Test-only snapshot of the last bundle (a clone, so the global is
/// untouched). Used by the M3.3 tests to assert `set_last_repro_bundle`
/// round-trips.
#[cfg(test)]
pub(crate) fn last_repro_bundle_clone() -> Option<ReproBundle> {
    bundle_slot().read().ok().and_then(|slot| slot.clone())
}

/// Install the global ICE panic hook (P-07). On any panic the hook formats
/// exactly one ICE report — the repo URL from [`REPO_URL`], the last
/// [`ReproBundle`] if [`ice!`] captured one — prints it to stderr, and exits
/// 101 (the panic=abort model, ADR-0005).
///
/// Idempotent: the first call wins, later calls are no-ops, so the CLI and a
/// test harness both calling it never double-register `set_hook` (which
/// would itself panic). Intended to be called once at compiler entry.
pub fn install_ice_handler() {
    install_ice_handler_with(REPO_URL);
}

/// Install the ICE panic hook with a custom repo URL. The URL is captured by
/// the hook as `&'static str`, so the override is intended for forks or
/// embedded users whose issue tracker differs from the upstream glyim repo.
pub fn install_ice_handler_with(repo_url: &'static str) {
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        std::panic::set_hook(Box::new(move |info| {
            if !first_report(&ICE_REPORTED) {
                return; // P-07: never emit a second ICE report.
            }
            let payload = info.payload();
            let message = payload
                .downcast_ref::<&'static str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("panic");
            let bundle = bundle_slot().read().ok().and_then(|slot| slot.clone());
            let report = format_ice_report(repo_url, bundle.as_ref(), message);
            eprintln!("{report}");
            std::process::exit(101);
        }));
    });
}

/// ICE the compiler (plan §6 M3.3 / P-07).
///
/// Records `bundle` as the last reproduction context (so the panic hook can
/// attach it), then panics with `message`. If [`install_ice_handler`] has
/// been called, the global hook formats exactly one ICE report and exits
/// 101; otherwise the panic propagates as a normal `panic!` (used by
/// `catch_unwind` in tests).
///
/// # When to use this
///
/// Only for genuine internal-compiler-error invariants — a violated
/// assumption that means the compiler cannot continue correctly. User-facing
/// errors use [`GlyimDiagnostic`](crate::GlyimDiagnostic) constructors; this
/// macro is the "we have a bug" exit.
///
/// # Example
///
/// ```no_run
/// use glyim_diag::ice::{ReproBundle, install_ice_handler};
/// use std::sync::Arc;
///
/// install_ice_handler(); // once, at compiler entry
/// let bundle = ReproBundle {
///     source: Arc::from("fn main() {}"),
///     command: "glyim build foo.g".into(),
/// };
/// glyim_diag::ice!(bundle, "monomorphization invariant violated");
/// ```
#[macro_export]
macro_rules! ice {
    ($bundle:expr, $message:expr $(,)?) => {{
        $crate::ice::set_last_repro_bundle($bundle);
        panic!("ice: {}", $message);
    }};
}
