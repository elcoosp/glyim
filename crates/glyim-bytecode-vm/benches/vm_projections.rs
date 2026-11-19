//! Projection-stream timing harness.
//!
//! Dependency-free (`harness = false`), mirroring the `vm_aggregates` /
//! `vm_strings` benches in this crate: only `std::time::Instant` +
//! `std::hint::black_box`. The projection seam (the `projections`
//! module) rides the `OP_LEN` and `OP_DISCRIMINANT` arms — one event
//! per executed aggregate projection, with the site and sink-side
//! copies of the operand — so its headline numbers are the
//! **per-projection overhead** of a live run (one `Option` check per
//! read plus, per event, one clone of the projected operand), the
//! deterministic render, and the capture-only shape the aggregate bench
//! mirrors. Run with:
//!
//! ```text
//! cargo bench -p glyim-bytecode-vm --bench vm_projections
//! ```

use glyim_bytecode_vm::{Function, Module, Opcode, ProjectionCollector, Vm};
use std::hint::black_box;
use std::time::Instant;

/// A projection-heavy loop: a countdown whose body builds a fresh pair
/// into a local and reads its length every iteration — every iteration
/// pays exactly one `OP_LEN` dispatch (one event per iteration).
fn len_loop() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&10_000i64.to_le_bytes());
    main.push(Opcode::StoreLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    // Loop head (pc 14): build (1, 2) into local 1, read its length,
    // drop the answer, decrement, loop while still positive.
    main.push(Opcode::LoadConst as u8); // the first element
    main.extend_from_slice(&1i64.to_le_bytes());
    main.push(Opcode::LoadConst as u8); // the second element
    main.extend_from_slice(&2i64.to_le_bytes());
    main.push(Opcode::Aggregate as u8); // build (1, 2)
    main.extend_from_slice(&2u32.to_le_bytes());
    main.push(Opcode::StoreLocal as u8); // bind local 1
    main.extend_from_slice(&1u32.to_le_bytes());
    main.push(Opcode::Len as u8); // read local 1's length
    main.extend_from_slice(&1u32.to_le_bytes());
    main.push(Opcode::Drop as u8); // discard the answer
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
    Module::new(vec![Function::new(main, 2, 0)], 0)
}

fn main() {
    const ROUNDS: u32 = 20;
    let module = len_loop();

    let start = Instant::now();
    for _ in 0..ROUNDS {
        let mut vm = Vm::new();
        let _ = black_box(vm.run_module(&module).expect("untraced run"));
    }
    let untraced = start.elapsed();

    let start = Instant::now();
    let mut executed = 0u64;
    for _ in 0..ROUNDS {
        let mut vm = Vm::new().with_projections(Box::new(ProjectionCollector::new()));
        let _ = black_box(vm.run_module(&module).expect("projection run"));
        let mut boxed = vm.take_projections().expect("attached");
        executed += boxed
            .as_any()
            .downcast_ref::<ProjectionCollector>()
            .map(|c| c.events().len() as u64)
            .unwrap_or(0);
    }
    let captured = start.elapsed();

    let mut vm = Vm::new().with_projections(Box::new(ProjectionCollector::new()));
    let _ = vm.run_module(&module).expect("render run");
    let mut boxed = vm.take_projections().expect("attached");
    let collector = boxed
        .as_any()
        .downcast_ref::<ProjectionCollector>()
        .cloned()
        .unwrap_or_default();

    let start = Instant::now();
    let rendered = glyim_bytecode_vm::render_projections(black_box(&collector)).len();
    let render = start.elapsed();

    println!(
        "len-loop runs (20 x 10k projections): untraced {untraced:?} | per-projection hooks {captured:?} ({executed} projections captured) | render {render:?} ({rendered} bytes over {} events)",
        collector.events().len(),
    );
}
