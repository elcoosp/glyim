//! Assertion-stream timing harness.
//!
//! Dependency-free (`harness = false`), mirroring the `vm_state` /
//! `vm_watch` benches in this crate: only `std::time::Instant` +
//! `std::hint::black_box`. The assertion seam (the `asserts` module) rides
//! the `OP_ASSERT` arm — one event per evaluated assert, passing or
//! failing, with the site, the polarity, and the observed condition — so
//! its headline numbers are the **per-assert overhead** of a live run
//! (one `Option` check per assert dispatch plus, per event, one small
//! struct clone), the deterministic render, and the capture-only shape
//! the watch bench mirrors. Run with:
//!
//! ```text
//! cargo bench -p glyim-bytecode-vm --bench vm_asserts
//! ```

use glyim_bytecode_vm::{AssertCollector, Function, Module, Opcode, Vm};
use std::hint::black_box;
use std::time::Instant;

/// An assert-heavy loop: a countdown whose body asserts the counter is
/// non-negative every iteration — every iteration pays exactly one
/// `OP_ASSERT` dispatch (one event per iteration).
fn assert_loop() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&10_000i64.to_le_bytes());
    main.push(Opcode::StoreLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    // Loop head (pc 14): assert the counter is non-negative, then
    // decrement it, then loop while it is still positive.
    main.push(Opcode::LoadLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&0i64.to_le_bytes());
    main.push(Opcode::Ge as u8);
    main.push(Opcode::Assert as u8); // pc 29 — one event per iteration
    main.push(1u8);
    main.extend_from_slice(&35u32.to_le_bytes()); // fall-through
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
    Module::new(vec![Function::new(main, 1, 0)], 0)
}

fn main() {
    const ROUNDS: u32 = 20;
    let module = assert_loop();

    let start = Instant::now();
    for _ in 0..ROUNDS {
        let mut vm = Vm::new();
        let _ = black_box(vm.run_module(&module).expect("untraced run"));
    }
    let untraced = start.elapsed();

    let start = Instant::now();
    let mut evaluated = 0u64;
    for _ in 0..ROUNDS {
        let mut vm = Vm::new().with_asserts(Box::new(AssertCollector::new()));
        let _ = black_box(vm.run_module(&module).expect("assert run"));
        let mut boxed = vm.take_asserts().expect("attached");
        evaluated += boxed
            .as_any()
            .downcast_ref::<AssertCollector>()
            .map(|c| c.events().len() as u64)
            .unwrap_or(0);
    }
    let captured = start.elapsed();

    let mut vm = Vm::new().with_asserts(Box::new(AssertCollector::new()));
    let _ = vm.run_module(&module).expect("render run");
    let mut boxed = vm.take_asserts().expect("attached");
    let events = boxed
        .as_any()
        .downcast_ref::<AssertCollector>()
        .map(|c| c.events().to_vec())
        .unwrap_or_default();

    let start = Instant::now();
    let rendered = glyim_bytecode_vm::render_asserts(black_box(&events)).len();
    let render = start.elapsed();

    println!(
        "assert-loop runs (20 x 10k asserts): untraced {untraced:?} | per-assert hook {captured:?} ({evaluated} assertions captured) | render {render:?} ({rendered} bytes over {} events)",
        events.len(),
    );
}
