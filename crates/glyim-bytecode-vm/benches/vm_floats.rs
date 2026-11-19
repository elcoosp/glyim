//! Float-stream timing harness.
//!
//! Dependency-free (`harness = false`), mirroring the `vm_projections` /
//! `vm_strings` benches in this crate: only `std::time::Instant` +
//! `std::hint::black_box`. The float seam (the `floats` module) rides
//! the `OP_LOAD_CONST_F64` arm, the `(Float, Float)` binop arm, and the
//! `OP_NEG` unary arm — one event per executed float operation, with
//! the site and sink-side copies of the operands and the answer — so
//! its headline numbers are the **per-float-operation overhead** of a
//! live run (one `Option` check per float op plus, per event, the
//! sink-side operand copies), the deterministic render, and the
//! capture-only shape the projection bench mirrors. Run with:
//!
//! ```text
//! cargo bench -p glyim-bytecode-vm --bench vm_floats
//! ```

use glyim_bytecode_vm::{FloatCollector, Function, Module, Opcode, Vm};
use std::hint::black_box;
use std::time::Instant;

/// A float-heavy loop: a countdown whose body runs the float kernel —
/// two constant pushes, a multiply, and an accumulate into a local —
/// every iteration (four float events per iteration, plus the
/// accumulator's initial push before the loop).
fn float_loop() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&10_000i64.to_le_bytes());
    main.push(Opcode::StoreLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadConstF64 as u8); // the accumulator
    main.extend_from_slice(&0.0f64.to_le_bytes());
    main.push(Opcode::StoreLocal as u8);
    main.extend_from_slice(&1u32.to_le_bytes());
    // Loop head (pc 28): the float kernel into local 1, decrement,
    // loop while the counter is still positive.
    main.push(Opcode::LoadConstF64 as u8);
    main.extend_from_slice(&1.5f64.to_le_bytes());
    main.push(Opcode::LoadConstF64 as u8);
    main.extend_from_slice(&2.25f64.to_le_bytes());
    main.push(Opcode::Mul as u8);
    main.push(Opcode::LoadLocal as u8);
    main.extend_from_slice(&1u32.to_le_bytes());
    main.push(Opcode::Add as u8);
    main.push(Opcode::StoreLocal as u8);
    main.extend_from_slice(&1u32.to_le_bytes());
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
    main.extend_from_slice(&28u32.to_le_bytes()); // loop head
    main.push(Opcode::LoadLocal as u8);
    main.extend_from_slice(&1u32.to_le_bytes());
    main.push(Opcode::Return as u8);
    Module::new(vec![Function::new(main, 2, 0)], 0)
}

fn main() {
    const ROUNDS: u32 = 20;
    let module = float_loop();

    let start = Instant::now();
    for _ in 0..ROUNDS {
        let mut vm = Vm::new();
        let _ = black_box(vm.run_module(&module).expect("untraced run"));
    }
    let untraced = start.elapsed();

    let start = Instant::now();
    let mut executed = 0u64;
    for _ in 0..ROUNDS {
        let mut vm = Vm::new().with_floats(Box::new(FloatCollector::new()));
        let _ = black_box(vm.run_module(&module).expect("float run"));
        let mut boxed = vm.take_floats().expect("attached");
        executed += boxed
            .as_any()
            .downcast_ref::<FloatCollector>()
            .map(|c| c.events().len() as u64)
            .unwrap_or(0);
    }
    let captured = start.elapsed();

    let mut vm = Vm::new().with_floats(Box::new(FloatCollector::new()));
    let _ = vm.run_module(&module).expect("render run");
    let mut boxed = vm.take_floats().expect("attached");
    let collector = boxed
        .as_any()
        .downcast_ref::<FloatCollector>()
        .cloned()
        .unwrap_or_default();

    let start = Instant::now();
    let rendered = glyim_bytecode_vm::render_floats(black_box(&collector)).len();
    let render = start.elapsed();

    println!(
        "float-loop runs (20 x 10k iterations): untraced {untraced:?} | per-float-op hooks {captured:?} ({executed} float events captured) | render {render:?} ({rendered} bytes over {} events)",
        collector.events().len(),
    );
}
