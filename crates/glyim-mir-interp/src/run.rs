//! The canonical "run a compiled program in-process" contract.
//!
//! Three consumers used to re-implement this plumbing independently —
//! `glyim-test`'s `InterpRunner`, `glyip`'s test mode, and (new with
//! `--emit=run`) the `glyim` CLI. This module is the single definition
//! they all share, so a fixture's observable behaviour is identical no
//! matter which consumer executes it.
//!
//! The contract is the **process contract**: it reproduces what the OS
//! + linker do for a real `--emit=exec` binary.
//!
//! * **Entry selection.** The entry body is the body whose owner's raw
//!   `LocalDefId` matches `entry_main` (the id the pipeline's
//!   `entry_main_local_id` pre-pass resolves — the same id the CLI's
//!   `--emit=exec` path hands to the backend for the C-ABI `main`
//!   wrapper). `None` falls back to the first body, preserving the test
//!   harness's historical behaviour for fixture programs without a
//!   `main`. Callers that need a strict "no `main`" diagnostic (the CLI)
//!   resolve the entry themselves and reject `None` before calling.
//! * **Exit code.** A `main` returning an integer exits
//!   `(v & 0xff) as i32` — a real process's exit status is masked to
//!   `0xff` at the OS boundary (`fn main() -> i32 { 300 }` exits 44
//!   natively; verified — see the harness notes). A `main` returning
//!   `()` or `!` exits 0. A `bool` return maps to `0`/`1`.
//! * **Panics.** Any interpreter error (a program `panic!`, division by
//!   zero, step/recursion limit, …) exits **101** — matching
//!   `glyim_panic`'s `exit(101)` on the native path — and appends a
//!   `thread 'main' panicked: <error>` line to stderr, so `check-stderr`
//!   fixtures mean the same thing on both backends. The run is reported
//!   as [`RunKind::Panicked`] so callers that need to distinguish
//!   "interpreter said panic" from "main returned 101" can (e.g. `glyip
//!   test` counts a panicked test as a failure regardless of code).
//! * **I/O.** stdout/stderr bytes captured from the `glyim_stdout_write`
//!   / `glyim_stderr_write` FFI hooks are returned on **both** the
//!   completed and panicked paths — a program that prints and then
//!   panics must not lose its output (the TEST-7 fidelity rule).
//!
//! Callers that need a wall-clock budget run this function on a worker
//! thread and time out on the channel (see `glyim-test`'s
//! `InterpRunner::run`); the interpreter's own `step_limit` also bounds
//! runaway programs at 1,000,000 steps by default.

use crate::{InterpValue, Interpreter};
use glyim_mir::Body;
use glyim_type::TyCtx;
use std::sync::Arc;

/// How the interpreted program terminated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunKind {
    /// The entry body ran to completion and returned normally.
    Completed,
    /// The interpreter surfaced an error (program panic, division by
    /// zero, limit exceeded, …); `exit_code` is 101 and `stderr` carries
    /// the canonical panic line.
    Panicked,
}

/// What a real executable produced for the interpreted program.
///
/// `exit_code` is the value `glyim --emit=run` propagates as its own
/// process exit code and `RunResult::exit_code` carries in the test
/// harness; `stdout`/`stderr` are the FFI-captured byte streams.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    /// Whether the program completed normally or panicked.
    pub kind: RunKind,
    /// The process exit code the OS would report for the equivalent
    /// `--emit=exec` binary.
    pub exit_code: i32,
    /// Bytes the program wrote via the stdout FFI hook.
    pub stdout: Vec<u8>,
    /// Bytes the program wrote via the stderr FFI hook, plus the
    /// canonical `thread 'main' panicked: …` line on the panicked path.
    pub stderr: Vec<u8>,
}

/// Run a compiled crate's MIR bodies in-process as if linked and
/// executed natively.
///
/// `bodies` are the monomorphized, optimized MIR bodies (for pipeline
/// consumers: [`MirCompilation::bodies`] — every value `compile_file_to_mir`
/// returns; for the test harness: the compiled fixture's bodies).
/// `entry_main` is the raw `LocalDefId` of the crate's `main` as
/// resolved by the pipeline's entry pre-pass, or `None` to fall back to
/// the first body (the harness's fixture behaviour).
///
/// [`MirCompilation::bodies`]: glyim_pipeline::MirCompilation::bodies
#[must_use]
pub fn run_program(bodies: &[Arc<Body>], ty_ctx: &TyCtx, entry_main: Option<u32>) -> RunOutcome {
    if bodies.is_empty() {
        // A crate with no bodies has no observable behaviour; exit 0
        // like an empty `main` would.
        return RunOutcome {
            kind: RunKind::Completed,
            exit_code: 0,
            stdout: Vec::new(),
            stderr: Vec::new(),
        };
    }

    // Pick the `main` body by the pre-pass's raw `LocalDefId`; fall back
    // to the first body when absent (no `main`, or a fixture that never
    // defines one) — the harness's historical behaviour.
    let main_body: &Arc<Body> = entry_main
        .and_then(|raw| bodies.iter().find(|b| b.owner.local_id.to_raw() == raw))
        .unwrap_or_else(|| &bodies[0]);

    // Register every body so cross-frame `Call`s resolve, then run the
    // entry body to completion (or to the first interpreter error).
    let mut interpreter = Interpreter::new(ty_ctx);
    for body in bodies {
        interpreter.add_function(body.owner, (**body).clone());
    }
    let result = interpreter.run_body(main_body);

    // Read the FFI buffers *before* branching on the result: a program
    // that printed and then panicked must not lose its output (TEST-7).
    let stdout = interpreter.get_stdout().to_vec();
    let stderr = interpreter.get_stderr().to_vec();

    match result {
        Ok(()) => {
            // A `main` returning `i32` surfaces its value through
            // `get_return_value`; the mask below mirrors the OS's
            // `status & 0xff`. Unit/never returns exit 0.
            let exit_code = match interpreter.get_return_value() {
                Some(InterpValue::Int(v)) => (v as i64 & 0xff) as i32,
                Some(InterpValue::Uint(v)) => (v as u64 & 0xff) as i32,
                Some(InterpValue::Bool(b)) => i32::from(b),
                _ => 0,
            };
            RunOutcome {
                kind: RunKind::Completed,
                exit_code,
                stdout,
                stderr,
            }
        }
        Err(e) => {
            // Emit the same stderr the LLVM backend writes from
            // `glyim_panic`, so check-stderr fixtures (e.g.
            // run-fail/div_by_zero_stderr.g) pass on both backends. The
            // exit code 101 matches `glyim_panic`'s `exit(101)`.
            let mut stderr = stderr;
            stderr.extend_from_slice(format!("thread 'main' panicked: {e}\n").as_bytes());
            RunOutcome {
                kind: RunKind::Panicked,
                exit_code: 101,
                stdout,
                stderr,
            }
        }
    }
}
