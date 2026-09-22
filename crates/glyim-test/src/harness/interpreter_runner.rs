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

use glyim_mir_interp::Interpreter;
use std::sync::Arc;
use std::time::Duration;

/// InterpRunner.
pub struct InterpRunner {
    bodies: Vec<Arc<glyim_mir::Body>>,
    ty_ctx: Arc<glyim_type::TyCtx>,
    /// `LocalDefId` (raw) of the crate's `main`, if the pipeline found one.
    entry_main: Option<u32>,
}

impl InterpRunner {
    /// new.
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
    pub fn run(self, timeout: Duration) -> super::runner::RunResult {
        let start = std::time::Instant::now();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let output = interpret_bodies(&self.bodies, self.ty_ctx.as_ref(), self.entry_main);
            let _ = tx.send(output);
        });
        match rx.recv_timeout(timeout) {
            Ok(output) => {
                let duration = start.elapsed();
                super::runner::RunResult {
                    exit_code: Some(output.exit_code),
                    stdout: output.stdout,
                    stderr: output.stderr,
                    timed_out: false,
                    duration,
                }
            }
            Err(_) => {
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
}

struct InterpOutput {
    exit_code: i32,
    stdout: String,
    stderr: String,
}

fn interpret_bodies(
    bodies: &[Arc<glyim_mir::Body>],
    ty_ctx: &glyim_type::TyCtx,
    entry_main: Option<u32>,
) -> InterpOutput {
    let stdout = String::new();
    let mut stderr = String::new();

    if bodies.is_empty() {
        return InterpOutput {
            exit_code: 0,
            stdout,
            stderr,
        };
    }

    // Pick the `main` body. The pipeline's `entry_main` is the raw `LocalDefId`
    // that `entry_main_local_id` resolved; look it up among the MIR bodies by
    // owner. If it is absent (no `main`, or a fixture that never defines one)
    // fall back to the first body, preserving the historical behaviour.
    let main_body: &Arc<glyim_mir::Body> = entry_main
        .and_then(|raw| {
            bodies.iter().find(|b| {
                b.owner.local_id.to_raw() == raw
            })
        })
        .unwrap_or_else(|| &bodies[0]);

    // Set up the interpreter and register every body.
    let mut interpreter = Interpreter::new(ty_ctx);
    for body in bodies {
        interpreter.add_function(body.owner, (**body).clone());
    }

    let result = interpreter.run_body(main_body);

    match result {
        Ok(()) => {
            // A `main` that returns `i32` (the common case) surfaces its value
            // through `get_return_value`; the runtime then turns it into the
            // process exit code, mirroring a real executable. A `main` that
            // returns `()`/`!` yields `Unit` (or `None`) and exits `0`.
            let exit_code = match interpreter.get_return_value() {
                Some(glyim_mir_interp::InterpValue::Int(v)) => (v as i64 & 0xff) as i32,
                Some(glyim_mir_interp::InterpValue::Uint(v)) => (v as u64 & 0xff) as i32,
                Some(glyim_mir_interp::InterpValue::Bool(b)) => b as i32,
                _ => 0,
            };
            InterpOutput {
                exit_code,
                stdout,
                stderr,
            }
        }
        Err(e) => {
            stderr.push_str(&format!("interpreter error: {}\n", e));
            InterpOutput {
                exit_code: 101,
                stdout,
                stderr,
            }
        }
    }
}
