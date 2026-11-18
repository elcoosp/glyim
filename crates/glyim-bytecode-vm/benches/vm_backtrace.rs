//! Fault-backtrace timing harness.
//!
//! Dependency-free (`harness = false`), mirroring the `vm_trace` / `vm_profile`
//! benches in this crate: only `std::time::Instant` + `std::hint::black_box`.
//! The backtrace seam (the `backtrace` module) hooks the *error* path, not the
//! dispatch loop, so its headline numbers are the **per-dispatch site-tracking
//! cost** (the two integer stores that keep the machine's current site visible
//! — included in *both* arms of the baseline comparison, so the baseline pair
//! isolates everything else), the **fault-path cost** of capturing a report
//! with the collector attached on a failing run (hook call + report build —
//! paid once per failed run), and the deterministic render. Run with:
//!
//! ```text
//! cargo bench -p glyim-bytecode-vm --bench vm_backtrace
//! ```

use glyim_bytecode_vm::{BacktraceCollector, Function, Module, Opcode, Vm, VmError};
use std::hint::black_box;
use std::time::Instant;

/// A countdown loop: a preamble seeds entry local 0 with `SEED`, then the
/// loop decrements it to zero — ~`10 * SEED` dispatches per run, the same
/// shape the trace/profile benches' overhead claims rest on.
fn countdown_loop() -> Module {
    let mut code = Vec::new();
    // Preamble: seed local 0 with the loop count (the entry frame binds no
    // args, and locals are zero-initialised, so the count travels as a
    // constant before the loop).
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
    // Block-index mode: the target names the block table's index (0), not a
    // raw byte offset.
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::Return as u8);
    Module::new(vec![Function::with_blocks(code, 1, 1, vec![head])], 0)
}

/// A module whose callee traps: `main` calls `helper` (the golden call
/// shape), the helper executes `OP_TRAP` at its first dispatch. The
/// fault-path arm measures the seam's cost over this run.
fn trapping_module() -> Module {
    let helper = vec![Opcode::Trap as u8];
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&0i64.to_le_bytes()); // fn index 0: helper
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&21i64.to_le_bytes());
    main.push(Opcode::Call as u8);
    main.extend_from_slice(&1u32.to_le_bytes()); // argc
    main.extend_from_slice(&0u32.to_le_bytes()); // dest local
    main.extend_from_slice(&31u32.to_le_bytes()); // resume byte offset
    main.push(Opcode::LoadLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Return as u8);
    Module::new(
        vec![Function::new(helper, 2, 1), Function::new(main, 1, 0)],
        1,
    )
}

/// A self-calling function that overflows the call-depth guard: the deep
/// chain arm measures the report build + render over a 1023-caller chain.
fn self_recursion_module() -> Module {
    let mut code = Vec::new();
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&0i64.to_le_bytes()); // fn index 0: self
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&1i64.to_le_bytes());
    code.push(Opcode::Call as u8);
    code.extend_from_slice(&1u32.to_le_bytes()); // argc
    code.extend_from_slice(&0u32.to_le_bytes()); // dest local
    code.extend_from_slice(&31u32.to_le_bytes()); // resume byte offset
    code.push(Opcode::Return as u8);
    Module::new(vec![Function::new(code, 2, 1)], 0)
}

fn main() {
    const ROUNDS: u32 = 20;
    let module = countdown_loop();

    // Baseline pair: untraced runs with a BacktraceCollector attached versus
    // not. Both arms pay the per-dispatch site tracking (it is unconditional
    // machine state), so the delta isolates the hook path — which a clean
    // (non-faulting) run never touches beyond the attachment's `None`-ness.
    let start = Instant::now();
    for _ in 0..ROUNDS {
        let mut vm = Vm::new();
        let _ = black_box(vm.run_module(&module).expect("untraced run"));
    }
    let untraced = start.elapsed();

    let start = Instant::now();
    for _ in 0..ROUNDS {
        let mut vm = Vm::new().with_backtrace(Box::new(BacktraceCollector::new()));
        let _ = black_box(vm.run_module(&module).expect("backtraced run"));
    }
    let backtraced_clean = start.elapsed();
    println!(
        "clean runs (20 x ~100k dispatches): untraced {untraced:?} | backtrace-attached {backtraced_clean:?}"
    );

    // Fault path: a failing run with the collector attached versus without.
    // The seam fires once here (hook call + report build + capture), so this
    // is the per-fault cost, paid once per failed run.
    let trap_module = trapping_module();
    let start = Instant::now();
    let mut errors = 0u32;
    for _ in 0..1_000 {
        let mut vm = Vm::new();
        let _ = black_box(vm.run_module(&trap_module).unwrap_err());
        errors += 1;
    }
    let fault_unsinked = start.elapsed();

    let start = Instant::now();
    let mut reports = 0u32;
    for _ in 0..1_000 {
        let collector = BacktraceCollector::new();
        let mut vm = Vm::new().with_backtrace(Box::new(collector));
        let err = black_box(vm.run_module(&trap_module).unwrap_err());
        debug_assert_eq!(err, VmError::AbnormalTermination);
        reports += 1;
    }
    let fault_sinked = start.elapsed();
    println!(
        "faulting runs (1000): unsinked {fault_unsinked:?} | sinked {fault_sinked:?} ({errors} faults, {reports} reports)"
    );

    // The render: build one report (a fresh fault) and render it repeatedly.
    let collector = BacktraceCollector::new();
    let mut vm = Vm::new().with_backtrace(Box::new(collector));
    let _ = vm.run_module(&trap_module).unwrap_err();
    let mut boxed = vm.take_backtrace().expect("attached");
    let report = boxed
        .as_any()
        .downcast_ref::<BacktraceCollector>()
        .and_then(|c| c.report())
        .cloned()
        .expect("the fault was captured");
    let start = Instant::now();
    let mut bytes = 0usize;
    for _ in 0..1_000 {
        bytes += black_box(glyim_bytecode_vm::render_backtrace(&report, &trap_module)).len();
    }
    let render = start.elapsed();
    println!("render (1000 x {bytes}-byte total): {render:?}");

    // Deep chain: the call-depth overflow builds a 1023-caller report; build
    // + render once to size the worst case.
    let rec_module = self_recursion_module();
    let collector = BacktraceCollector::new();
    let mut vm = Vm::new().with_backtrace(Box::new(collector));
    let _ = vm.run_module(&rec_module).unwrap_err();
    let mut boxed = vm.take_backtrace().expect("attached");
    let report = boxed
        .as_any()
        .downcast_ref::<BacktraceCollector>()
        .and_then(|c| c.report().cloned())
        .expect("the overflow fault was captured");
    let start = Instant::now();
    let deep = black_box(glyim_bytecode_vm::render_backtrace(&report, &rec_module));
    println!(
        "deep-chain render ({} callers, {}B listing): {:?}",
        report.callers.len(),
        deep.len(),
        start.elapsed(),
    );
}
