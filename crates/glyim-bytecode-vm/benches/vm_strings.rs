//! String-stream timing harness.
//!
//! Dependency-free (`harness = false`), mirroring the `vm_calls` /
//! `vm_memory` benches in this crate: only `std::time::Instant` +
//! `std::hint::black_box`. The strings seam (the `strings` module) rides
//! the `OP_LOAD_CONST_STR` arm — one event per executed pool resolution,
//! with the site, the pool slot, and a sink-side copy of the resolved
//! byte string — so its headline numbers are the **per-resolution
//! overhead** of a live run (one `Option` check per load plus, per
//! event, one string clone), the deterministic render, and the
//! capture-only shape the memory bench mirrors. Run with:
//!
//! ```text
//! cargo bench -p glyim-bytecode-vm --bench vm_strings
//! ```

use glyim_bytecode_vm::{Function, Module, Opcode, StringCollector, Vm};
use std::hint::black_box;
use std::time::Instant;

/// A string-heavy loop: a countdown whose body resolves one shared pool
/// slot every iteration — every iteration pays exactly one
/// `OP_LOAD_CONST_STR` dispatch (one event per iteration).
fn string_loop() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&10_000i64.to_le_bytes());
    main.push(Opcode::StoreLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    // Loop head (pc 14): resolve slot 0, drop it, decrement, loop while
    // still positive.
    main.push(Opcode::LoadConstStr as u8); // one load per iteration
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Drop as u8);
    main.push(Opcode::LoadLocal as u8); // the counter
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&1i64.to_le_bytes());
    main.push(Opcode::Sub as u8);
    main.push(Opcode::StoreLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadLocal as u8); // the counter
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&0i64.to_le_bytes());
    main.push(Opcode::Gt as u8);
    main.push(Opcode::JumpIf as u8);
    main.extend_from_slice(&14u32.to_le_bytes()); // loop head
    main.push(Opcode::LoadConstStr as u8); // the module result
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Return as u8);
    Module::with_strings(vec![Function::new(main, 1, 0)], vec!["payload".into()], 0)
}

fn main() {
    const ROUNDS: u32 = 20;
    let module = string_loop();

    let start = Instant::now();
    for _ in 0..ROUNDS {
        let mut vm = Vm::new();
        let _ = black_box(vm.run_module(&module).expect("untraced run"));
    }
    let untraced = start.elapsed();

    let start = Instant::now();
    let mut executed = 0u64;
    for _ in 0..ROUNDS {
        let mut vm = Vm::new().with_strings(Box::new(StringCollector::new()));
        let _ = black_box(vm.run_module(&module).expect("string run"));
        let mut boxed = vm.take_strings().expect("attached");
        executed += boxed
            .as_any()
            .downcast_ref::<StringCollector>()
            .map(|c| c.events().len() as u64)
            .unwrap_or(0);
    }
    let captured = start.elapsed();

    let mut vm = Vm::new().with_strings(Box::new(StringCollector::new()));
    let _ = vm.run_module(&module).expect("render run");
    let mut boxed = vm.take_strings().expect("attached");
    let collector = boxed
        .as_any()
        .downcast_ref::<StringCollector>()
        .cloned()
        .unwrap_or_default();

    let start = Instant::now();
    let rendered = glyim_bytecode_vm::render_strings(black_box(&collector)).len();
    let render = start.elapsed();

    println!(
        "string-loop runs (20 x 10k resolutions): untraced {untraced:?} | per-load hooks {captured:?} ({executed} loads captured) | render {render:?} ({rendered} bytes over {} events)",
        collector.events().len(),
    );
}
