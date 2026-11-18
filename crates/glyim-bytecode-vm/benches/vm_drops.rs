//! Drop-stream timing harness.
//!
//! Dependency-free (`harness = false`), mirroring the `vm_watch` /
//! `vm_asserts` benches in this crate: only `std::time::Instant` +
//! `std::hint::black_box`. The discard seam (the `drops` module) rides
//! the `OP_DROP` arm — one event per executed discard, carrying the site
//! and the popped value — so its headline numbers are the **per-discard
//! overhead** of a live run (one `Option` check per drop dispatch plus,
//! per event, one value clone into the collector), the deterministic
//! render, and the capture-only shape the watch bench mirrors. Run with:
//!
//! ```text
//! cargo bench -p glyim-bytecode-vm --bench vm_drops
//! ```

use glyim_bytecode_vm::{DropCollector, Function, Module, Opcode, Vm};
use std::hint::black_box;
use std::time::Instant;

/// A drop-heavy loop: a countdown whose body loads slot 0's address and
/// drops it every iteration — every iteration pays exactly one `OP_DROP`
/// dispatch (one event per iteration).
fn drop_loop() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&10_000i64.to_le_bytes());
    main.push(Opcode::StoreLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    // Loop head (pc 14): discard slot 0's address, decrement the counter,
    // then loop while it is still positive.
    main.push(Opcode::LoadLocalAddr as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Drop as u8); // pc 19 — one discard per iteration
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
    let module = drop_loop();

    let start = Instant::now();
    for _ in 0..ROUNDS {
        let mut vm = Vm::new();
        let _ = black_box(vm.run_module(&module).expect("untraced run"));
    }
    let untraced = start.elapsed();

    let start = Instant::now();
    let mut discarded = 0u64;
    for _ in 0..ROUNDS {
        let mut vm = Vm::new().with_drops(Box::new(DropCollector::new()));
        let _ = black_box(vm.run_module(&module).expect("drop run"));
        let mut boxed = vm.take_drops().expect("attached");
        discarded += boxed
            .as_any()
            .downcast_ref::<DropCollector>()
            .map(|c| c.events().len() as u64)
            .unwrap_or(0);
    }
    let captured = start.elapsed();

    let mut vm = Vm::new().with_drops(Box::new(DropCollector::new()));
    let _ = vm.run_module(&module).expect("render run");
    let mut boxed = vm.take_drops().expect("attached");
    let events = boxed
        .as_any()
        .downcast_ref::<DropCollector>()
        .map(|c| c.events().to_vec())
        .unwrap_or_default();

    let start = Instant::now();
    let rendered = glyim_bytecode_vm::render_drops(black_box(&events)).len();
    let render = start.elapsed();

    println!(
        "drop-loop runs (20 x 10k discards): untraced {untraced:?} | per-discard hook {captured:?} ({discarded} discards captured) | render {render:?} ({rendered} bytes over {} events)",
        events.len(),
    );
}
