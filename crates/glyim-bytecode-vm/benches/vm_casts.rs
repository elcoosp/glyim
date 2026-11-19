//! Cast-stream timing harness.
//!
//! Dependency-free (`harness = false`), mirroring the `vm_asserts` /
//! `vm_drops` benches in this crate: only `std::time::Instant` +
//! `std::hint::black_box`. The cast seam (the `casts` module) rides the
//! `OP_CAST` arm — one event per executed cast, with the site, the decoded
//! kind/target pair, and sink-side copies of the operand and the result —
//! so its headline numbers are the **per-cast overhead** of a live run
//! (one `Option` check per cast dispatch plus, per event, one operand
//! clone and one result clone), the deterministic render, and the
//! capture-only shape the assert bench mirrors. Run with:
//!
//! ```text
//! cargo bench -p glyim-bytecode-vm --bench vm_casts
//! ```

use glyim_bytecode_vm::{CastCollector, Function, Module, Opcode, Vm};
use std::hint::black_box;
use std::time::Instant;

/// A cast-heavy loop: a countdown whose body casts the counter through
/// `u8` and discards the result every iteration — every iteration pays
/// exactly one `OP_CAST` dispatch (one event per iteration).
fn cast_loop() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&10_000i64.to_le_bytes());
    main.push(Opcode::StoreLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    // Loop head (pc 14): decrement the counter, cast it through u8, drop
    // the cast result, loop while the counter is still positive.
    main.push(Opcode::LoadLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&1i64.to_le_bytes());
    main.push(Opcode::Sub as u8);
    main.push(Opcode::StoreLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Cast as u8); // pc 34 — one event per iteration
    main.push(0u8); // IntToInt
    main.push(6u8); // U8
    main.push(Opcode::Drop as u8);
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
    let module = cast_loop();

    let start = Instant::now();
    for _ in 0..ROUNDS {
        let mut vm = Vm::new();
        let _ = black_box(vm.run_module(&module).expect("untraced run"));
    }
    let untraced = start.elapsed();

    let start = Instant::now();
    let mut executed = 0u64;
    for _ in 0..ROUNDS {
        let mut vm = Vm::new().with_casts(Box::new(CastCollector::new()));
        let _ = black_box(vm.run_module(&module).expect("cast run"));
        let mut boxed = vm.take_casts().expect("attached");
        executed += boxed
            .as_any()
            .downcast_ref::<CastCollector>()
            .map(|c| c.events().len() as u64)
            .unwrap_or(0);
    }
    let captured = start.elapsed();

    let mut vm = Vm::new().with_casts(Box::new(CastCollector::new()));
    let _ = vm.run_module(&module).expect("render run");
    let mut boxed = vm.take_casts().expect("attached");
    let events = boxed
        .as_any()
        .downcast_ref::<CastCollector>()
        .map(|c| c.events().to_vec())
        .unwrap_or_default();

    let start = Instant::now();
    let rendered = glyim_bytecode_vm::render_casts(black_box(&events)).len();
    let render = start.elapsed();

    println!(
        "cast-loop runs (20 x 10k casts): untraced {untraced:?} | per-cast hook {captured:?} ({executed} casts captured) | render {render:?} ({rendered} bytes over {} events)",
        events.len(),
    );
}
