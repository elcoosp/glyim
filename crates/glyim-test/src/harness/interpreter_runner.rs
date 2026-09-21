//! Interpreter runner – executes MIR bodies using glyim-mir-interp.

use glyim_mir_interp::Interpreter;
use std::sync::Arc;
use std::time::Duration;

/// InterpRunner.
pub struct InterpRunner {
    bodies: Vec<Arc<glyim_mir::Body>>,
    ty_ctx: Arc<glyim_type::TyCtx>,
}

impl InterpRunner {
    /// new.
    pub fn new(bodies: Vec<Arc<glyim_mir::Body>>, ty_ctx: Arc<glyim_type::TyCtx>) -> Self {
        Self { bodies, ty_ctx }
    }

    /// run.
    pub fn run(self, timeout: Duration) -> super::runner::RunResult {
        let start = std::time::Instant::now();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let output = interpret_bodies(&self.bodies, self.ty_ctx.as_ref());
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

fn interpret_bodies(bodies: &[Arc<glyim_mir::Body>], ty_ctx: &glyim_type::TyCtx) -> InterpOutput {
    let stdout = String::new();
    let mut stderr = String::new();

    if bodies.is_empty() {
        return InterpOutput {
            exit_code: 0,
            stdout,
            stderr,
        };
    }

    // Set up the interpreter.
    let mut interpreter = Interpreter::new(ty_ctx);
    // Register all bodies with the interpreter.
    for body in bodies {
        interpreter.add_function(body.owner, (**body).clone());
    }

    // Run the main body (assume it's the first one).
    let main_body = &bodies[0];
    let result = interpreter.run_body(main_body);

    match result {
        Ok(()) => {
            // If the body returns unit, we consider success.
            // For other return types, we could inspect the return value, but for now we just exit 0.
            InterpOutput {
                exit_code: 0,
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
