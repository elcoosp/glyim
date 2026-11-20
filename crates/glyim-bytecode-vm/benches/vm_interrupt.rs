//! Host-interrupt timing harness.
//!
//! Dependency-free (`harness = false`), mirroring the `vm_fuel` /
//! `vm_stack` / `vm_trace` benches in this crate: only
//! `std::time::Instant` + `std::hint::black_box`. The host-interrupt
//! gate (the `interrupt` module) rides the dispatch loop just after the
//! fuel gate — one consultation per pending dispatch — so its headline
//! numbers are the **per-dispatch veto-check overhead** of an armed
//! gate over a never-terminating countdown program: a closure veto
//! (`InterruptFn`) that always answers `None` (one call + one `None`
//! per dispatch) and a `Deadline` veto far in the future (one
//! `Instant` comparison per dispatch), against the unattached baseline
//! (one `None` check per dispatch). The refusal path itself is a
//! single `Err` return that ends the run, so it is not a hot path;
//! what matters is the steady-state cost when the veto is armed and
//! silent. Run with:
//!
//! ```text
//! cargo bench -p glyim-bytecode-vm --bench vm_interrupt
//! ```

use glyim_bytecode_vm::{Deadline, Function, InterruptFn, Module, Opcode, Vm};
use std::hint::black_box;
use std::time::{Duration, Instant};

/// A countdown loop: a preamble seeds entry local 0 with the count, then
/// the loop decrements it to zero — the never-terminating shape (~100k
/// dispatches per run, one veto consultation each).
fn countdown_loop() -> Module {
    let mut code = Vec::new();
    // Preamble: seed local 0 with the loop count.
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&10_000i64.to_le_bytes());
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    // Loop head (block 0): local 0 -= 1; JumpIf local 0 > 0 back to the
    // head; return local 0.
    let head = code.len();
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&1i64.to_le_bytes());
    code.push(Opcode::Sub as u8);
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&0i64.to_le_bytes());
    code.push(Opcode::Gt as u8);
    code.push(Opcode::JumpIf as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::Return as u8);
    Module::new(vec![Function::with_blocks(code, 1, 1, vec![head])], 0)
}

fn main() {
    const ROUNDS: u32 = 20;
    let module = countdown_loop();

    let start = Instant::now();
    for _ in 0..ROUNDS {
        let mut vm = Vm::new();
        let _ = black_box(vm.run_module(&module).expect("unattached run"));
    }
    let unattached = start.elapsed();

    let start = Instant::now();
    for _ in 0..ROUNDS {
        let mut vm = Vm::new().with_interrupt(Box::new(InterruptFn::new(|_| None)));
        let _ = black_box(vm.run_module(&module).expect("closure-armed run"));
    }
    let closure_armed = start.elapsed();

    let start = Instant::now();
    for _ in 0..ROUNDS {
        let mut vm =
            Vm::new().with_interrupt(Box::new(Deadline::from_now(Duration::from_secs(3600))));
        let _ = black_box(vm.run_module(&module).expect("deadline-armed run"));
    }
    let deadline_armed = start.elapsed();

    println!(
        "countdown runs (20 x ~100k dispatches): unattached {unattached:?} | closure-veto {closure_armed:?} | deadline-veto {deadline_armed:?}"
    );

    // The refusal path: an armed veto that fires at the first dispatch
    // ends the run immediately — the gate's stop story, not a hot path.
    let mut vm = Vm::new().with_interrupt(Box::new(Deadline::after(Instant::now())));
    let err = black_box(
        vm.run_module(&module)
            .expect_err("the deadline already passed"),
    );
    println!("already-expired deadline: {err:?}");
}
