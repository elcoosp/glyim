//! Interpreter runner – executes MIR bodies using glyim-mir-interp.
//!
//! This is the harness's **portable** run path. Unlike the native
//! (`run-pass` → link → exec) path, it needs neither an LLVM toolchain nor a
//! system linker, so it exercises run-pass/run-fail fixtures on *any* host.
//! The harness prefers a native executable when one was produced and falls
//! back to this runner otherwise (see `strategy::RunPassStrategy`).
//!
//! The runner selects the crate's `main` body by the `entry_main` local id
//! (the same id the CLI's `--emit=exec` path uses to emit the C-ABI `main`
//! wrapper) and interprets it, taking the returned `i32` — if any — as the
//! process exit code. A `main` that returns `()` exits `0`. This mirrors what
//! the linker + OS would do for a real binary, so a `// check-stdout:` /
//! `// exit-code:` fixture exercises the same contract on both paths.
//!
//! Since M2.4 the *process contract* itself (entry resolution, exit-code
//! masking, panic → 101 + canonical stderr line, FFI-buffer capture on both
//! paths) lives in [`glyim_mir_interp::run_program`], where the CLI's
//! `--emit=run` and any future consumer share it. This runner adds only what
//! is harness-specific: the `GLYIM_INTERP_TRACE` debug env var, the
//! wall-clock timeout wrapper, and the mapping onto [`RunResult`].

use glyim_mir_interp::run_program;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

/// `InterpRunner`.
pub struct InterpRunner {
    bodies: Vec<Arc<glyim_mir::Body>>,
    ty_ctx: Arc<glyim_type::TyCtx>,
    /// `LocalDefId` (raw) of the crate's `main`, if the pipeline found one.
    entry_main: Option<u32>,
}

impl InterpRunner {
    /// new.
    #[must_use]
    pub fn new(
        bodies: Vec<Arc<glyim_mir::Body>>,
        ty_ctx: Arc<glyim_type::TyCtx>,
        entry_main: Option<u32>,
    ) -> Self {
        Self {
            bodies,
            ty_ctx,
            entry_main,
        }
    }

    /// run.
    #[must_use]
    pub fn run(&self, timeout: Duration) -> super::runner::RunResult {
        if std::env::var("GLYIM_INTERP_TRACE").is_ok() {
            eprintln!(
                "[TRACE-HARNESS-ENTRY] bodies={} entry_main={:?}",
                self.bodies.len(),
                self.entry_main
            );
        }
        let start = std::time::Instant::now();
        let (tx, rx) = std::sync::mpsc::channel();
        // T140-FIX-LEAK: retain the JoinHandle so the worker thread is
        // joined on the completion path. Without this, nextest flags the
        // test as "leaky" (a detached thread outlives the test).
        let bodies = self.bodies.clone();
        let ty_ctx = Arc::clone(&self.ty_ctx);
        let entry_main = self.entry_main;
        let handle = std::thread::spawn(move || {
            let outcome = run_program(&bodies, ty_ctx.as_ref(), entry_main);
            let _ = tx.send(outcome);
        });
        if let Ok(outcome) = rx.recv_timeout(timeout) {
            // Worker finished; join it to reclaim the OS thread.
            let _ = handle.join();
            let duration = start.elapsed();
            super::runner::RunResult {
                exit_code: Some(outcome.exit_code),
                stdout: String::from_utf8_lossy(&outcome.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&outcome.stderr).into_owned(),
                timed_out: false,
                duration,
            }
        } else {
            // Worker is still running (timeout). It will eventually
            // finish and exit; joining here would block forever, so we
            // intentionally detach it. This is the one intentional
            // leak, and it only occurs on the timeout path.
            drop(handle);
            let duration = start.elapsed();
            super::runner::RunResult {
                exit_code: None,
                stdout: String::new(),
                stderr: format!("interpreter timed out after {}s", timeout.as_secs()),
                timed_out: true,
                duration,
            }
        }
    }
}

impl fmt::Debug for InterpRunner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InterpRunner")
            .field("bodies", &self.bodies.len())
            .field("entry_main", &self.entry_main)
            .finish_non_exhaustive()
    }
}
