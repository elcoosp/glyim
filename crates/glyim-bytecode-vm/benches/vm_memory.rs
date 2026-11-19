//! Memory-stream timing harness.
//!
//! Dependency-free (`harness = false`), mirroring the `vm_casts` /
//! `vm_asserts` benches in this crate: only `std::time::Instant` +
//! `std::hint::black_box`. The memory seam (the `memory` module) rides
//! the `OP_STORE_FIELD` and `OP_DEREf` arms — one write event per
//! executed store and one read event per executed deref, with the site,
//! the address, and a sink-side copy of the value — so its headline
//! numbers are the **per-access overhead** of a live run (one `Option`
//! check per access plus, per event, one value clone), the deterministic
//! render, and the capture-only shape the assert bench mirrors. Run
//! with:
//!
//! ```text
//! cargo bench -p glyim-bytecode-vm --bench vm_memory
//! ```

use glyim_bytecode_vm::{Function, MemCollector, Module, Opcode, Vm};
use std::hint::black_box;
use std::time::Instant;

/// A memory-heavy loop: a countdown whose body stores the counter
/// through address 0 and reads it back every iteration — every iteration
/// pays exactly one `OP_STORE_FIELD` dispatch and one `OP_DEREf` dispatch
/// (one write event and one read event per iteration).
fn store_loop() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&10_000i64.to_le_bytes());
    main.push(Opcode::StoreLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    // Loop head (pc 14): store the counter through address 0, read it
    // back, drop the read, decrement, loop while still positive.
    main.push(Opcode::LoadLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadLocalAddr as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::StoreField as u8); // one write event per iteration
    main.push(Opcode::LoadLocalAddr as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Deref as u8); // one read event per iteration
    main.push(Opcode::Drop as u8);
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
    let module = store_loop();

    let start = Instant::now();
    for _ in 0..ROUNDS {
        let mut vm = Vm::new();
        let _ = black_box(vm.run_module(&module).expect("untraced run"));
    }
    let untraced = start.elapsed();

    let start = Instant::now();
    let mut executed = 0u64;
    for _ in 0..ROUNDS {
        let mut vm = Vm::new().with_memory(Box::new(MemCollector::new()));
        let _ = black_box(vm.run_module(&module).expect("memory run"));
        let mut boxed = vm.take_memory().expect("attached");
        executed += boxed
            .as_any()
            .downcast_ref::<MemCollector>()
            .map(|c| c.events().len() as u64)
            .unwrap_or(0);
    }
    let captured = start.elapsed();

    let mut vm = Vm::new().with_memory(Box::new(MemCollector::new()));
    let _ = vm.run_module(&module).expect("render run");
    let mut boxed = vm.take_memory().expect("attached");
    let collector = boxed
        .as_any()
        .downcast_ref::<MemCollector>()
        .cloned()
        .unwrap_or_default();

    let start = Instant::now();
    let rendered = glyim_bytecode_vm::render_memory(black_box(&collector)).len();
    let render = start.elapsed();

    println!(
        "store-loop runs (20 x 10k store+deref): untraced {untraced:?} | per-access hooks {captured:?} ({executed} accesses captured) | render {render:?} ({rendered} bytes over {} events)",
        collector.events().len(),
    );
}
