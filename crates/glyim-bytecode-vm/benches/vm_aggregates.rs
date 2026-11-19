//! Aggregate-stream timing harness.
//!
//! Dependency-free (`harness = false`), mirroring the `vm_calls` /
//! `vm_strings` benches in this crate: only `std::time::Instant` +
//! `std::hint::black_box`. The aggregate seam (the `aggregates` module)
//! rides the `OP_AGGREGATE` and `OP_REPEAT` arms — one event per
//! executed composite construction, with the site and sink-side copies
//! of the operands — so its headline numbers are the **per-construction
//! overhead** of a live run (one `Option` check per construction plus,
//! per event, one clone of the folded elements or the replicated
//! value), the deterministic render, and the capture-only shape the
//! strings bench mirrors. Run with:
//!
//! ```text
//! cargo bench -p glyim-bytecode-vm --bench vm_aggregates
//! ```

use glyim_bytecode_vm::{AggregateCollector, Function, Module, Opcode, Vm};
use std::hint::black_box;
use std::time::Instant;

/// A construction-heavy loop: a countdown whose body repeats one value
/// into a one-element tuple every iteration — every iteration pays
/// exactly one `OP_REPEAT` dispatch (one event per iteration).
fn repeat_loop() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&10_000i64.to_le_bytes());
    main.push(Opcode::StoreLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    // Loop head (pc 14): repeat 7 once, drop the tuple, decrement, loop
    // while still positive.
    main.push(Opcode::LoadConst as u8); // the value
    main.extend_from_slice(&7i64.to_le_bytes());
    main.push(Opcode::LoadConst as u8); // one
    main.extend_from_slice(&1i64.to_le_bytes());
    main.push(Opcode::Repeat as u8); // one repeat per iteration
    main.push(Opcode::Drop as u8); // discard the tuple
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
    main.push(Opcode::LoadLocal as u8); // the module result
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Return as u8);
    Module::new(vec![Function::new(main, 1, 0)], 0)
}

fn main() {
    const ROUNDS: u32 = 20;
    let module = repeat_loop();

    let start = Instant::now();
    for _ in 0..ROUNDS {
        let mut vm = Vm::new();
        let _ = black_box(vm.run_module(&module).expect("untraced run"));
    }
    let untraced = start.elapsed();

    let start = Instant::now();
    let mut executed = 0u64;
    for _ in 0..ROUNDS {
        let mut vm = Vm::new().with_aggregates(Box::new(AggregateCollector::new()));
        let _ = black_box(vm.run_module(&module).expect("aggregate run"));
        let mut boxed = vm.take_aggregates().expect("attached");
        executed += boxed
            .as_any()
            .downcast_ref::<AggregateCollector>()
            .map(|c| c.events().len() as u64)
            .unwrap_or(0);
    }
    let captured = start.elapsed();

    let mut vm = Vm::new().with_aggregates(Box::new(AggregateCollector::new()));
    let _ = vm.run_module(&module).expect("render run");
    let mut boxed = vm.take_aggregates().expect("attached");
    let collector = boxed
        .as_any()
        .downcast_ref::<AggregateCollector>()
        .cloned()
        .unwrap_or_default();

    let start = Instant::now();
    let rendered = glyim_bytecode_vm::render_aggregates(black_box(&collector)).len();
    let render = start.elapsed();

    println!(
        "repeat-loop runs (20 x 10k constructions): untraced {untraced:?} | per-construction hooks {captured:?} ({executed} constructions captured) | render {render:?} ({rendered} bytes over {} events)",
        collector.events().len(),
    );
}
