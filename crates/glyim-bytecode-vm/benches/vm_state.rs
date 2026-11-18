//! Call-state timing harness.
//!
//! Dependency-free (`harness = false`), mirroring the `vm_trace` /
//! `vm_watch` / `vm_branches` benches in this crate: only
//! `std::time::Instant` + `std::hint::black_box`. The state seam (the
//! `state` module) rides a NEW hook site — the frame lifecycle (one event
//! per frame enter and exit, each with a full locals snapshot) — so its
//! headline numbers are the **boundary overhead** of a live run (one
//! `Option` check per frame boundary plus, per boundary, one locals
//! clone), the deterministic render, and — because the seam has no
//! step-stream to re-fold — the capture-only shape the watch bench
//! mirrors. Run with:
//!
//! ```text
//! cargo bench -p glyim-bytecode-vm --bench vm_state
//! ```

use glyim_bytecode_vm::{Function, Module, Opcode, StateCollector, Vm};
use std::hint::black_box;
use std::time::Instant;

/// A call-heavy loop: fn 1 calls fn 0 (a double-the-argument helper) once
/// per iteration over a countdown — the boundary-friendly shape: every
/// iteration pays one frame enter + one frame exit.
fn call_loop() -> Module {
    let mut helper = Vec::new();
    helper.push(Opcode::LoadConst as u8);
    helper.extend_from_slice(&2i64.to_le_bytes());
    helper.push(Opcode::LoadLocal as u8);
    helper.extend_from_slice(&1u32.to_le_bytes()); // MIR ABI: arg at local 1
    helper.push(Opcode::Mul as u8);
    helper.push(Opcode::Return as u8);
    let mut main = Vec::new();
    // Preamble: seed local 0 with the iteration count.
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&10_000i64.to_le_bytes());
    main.push(Opcode::StoreLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    // Loop head (pc 14): call fn 0 once per iteration, then decrement.
    main.push(Opcode::LoadLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&0i64.to_le_bytes()); // fn index 0: helper
    main.push(Opcode::LoadLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes()); // arg
    main.push(Opcode::Call as u8);
    main.extend_from_slice(&1u32.to_le_bytes()); // argc
    main.extend_from_slice(&1u32.to_le_bytes()); // dest local (scratch)
    main.extend_from_slice(&46u32.to_le_bytes()); // resume byte offset
    main.push(Opcode::LoadLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&1i64.to_le_bytes());
    main.push(Opcode::Sub as u8);
    main.push(Opcode::StoreLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&0i64.to_le_bytes());
    main.push(Opcode::Gt as u8);
    main.push(Opcode::JumpIf as u8);
    main.extend_from_slice(&14u32.to_le_bytes()); // loop head
    main.push(Opcode::LoadLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Return as u8);
    Module::new(
        vec![Function::new(helper, 2, 1), Function::new(main, 1, 0)],
        1,
    )
}

fn main() {
    const ROUNDS: u32 = 20;
    let module = call_loop();

    let start = Instant::now();
    for _ in 0..ROUNDS {
        let mut vm = Vm::new();
        let _ = black_box(vm.run_module(&module).expect("untraced run"));
    }
    let untraced = start.elapsed();

    let start = Instant::now();
    let mut boundaries = 0u64;
    for _ in 0..ROUNDS {
        let mut vm = Vm::new().with_state(Box::new(StateCollector::new()));
        let _ = black_box(vm.run_module(&module).expect("state run"));
        let mut boxed = vm.take_state().expect("attached");
        boundaries += boxed
            .as_any()
            .downcast_ref::<StateCollector>()
            .map(|c| c.events().len() as u64)
            .unwrap_or(0);
    }
    let captured = start.elapsed();

    let collector = StateCollector::new();
    let mut vm = Vm::new().with_state(Box::new(collector));
    let _ = vm.run_module(&module).expect("render run");
    let mut boxed = vm.take_state().expect("attached");
    let events = boxed
        .as_any()
        .downcast_ref::<StateCollector>()
        .map(|c| c.events().to_vec())
        .unwrap_or_default();

    let start = Instant::now();
    let rendered = glyim_bytecode_vm::render_state(black_box(&events)).len();
    let render = start.elapsed();

    println!(
        "call-loop runs (20 x 10k calls): untraced {untraced:?} | state-boundaries {captured:?} ({boundaries} boundaries captured) | render {render:?} ({rendered} bytes over {} events)",
        events.len(),
    );
}
