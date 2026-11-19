//! Call-stream timing harness.
//!
//! Dependency-free (`harness = false`), mirroring the `vm_memory` /
//! `vm_casts` benches in this crate: only `std::time::Instant` +
//! `std::hint::black_box`. The calls seam (the `calls` module) rides the
//! `OP_CALL` / `OP_CALL_INDIRECT` arms and the `OP_RETURN` arm — one
//! start event per executed call (with sink-side copies of the
//! arguments) and one return event per executed return — so its headline
//! numbers are the **per-call overhead** of a live run (one `Option`
//! check per call and per return plus, per event, one argument-vector
//! or value clone), the deterministic render, and the capture shape the
//! memory bench mirrors. Run with:
//!
//! ```text
//! cargo bench -p glyim-bytecode-vm --bench vm_calls
//! ```

use glyim_bytecode_vm::{CallCollector, Function, Module, Opcode, Vm};
use std::hint::black_box;
use std::time::Instant;

/// A call-heavy loop: main calls helper once per countdown iteration —
/// every iteration pays exactly one `OP_CALL` dispatch and one
/// `OP_RETURN` dispatch (one start event and one return event per
/// iteration), plus the entry frame's final return.
fn call_loop() -> Module {
    let mut helper = Vec::new();
    helper.push(Opcode::LoadLocal as u8);
    helper.extend_from_slice(&1u32.to_le_bytes());
    helper.push(Opcode::Return as u8);
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&10_000i64.to_le_bytes());
    main.push(Opcode::StoreLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    // Loop head (pc 14): call helper(local 0), discard the result,
    // decrement, loop while still positive.
    main.push(Opcode::LoadConst as u8); // the callee (fn 0)
    main.extend_from_slice(&0i64.to_le_bytes());
    main.push(Opcode::LoadLocal as u8); // the argument
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Call as u8);
    main.extend_from_slice(&1u32.to_le_bytes()); // argc
    main.extend_from_slice(&2u32.to_le_bytes()); // dest_local
    main.extend_from_slice(&41u32.to_le_bytes()); // resume
    main.push(Opcode::LoadLocal as u8); // the result
    main.extend_from_slice(&2u32.to_le_bytes());
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
    main.push(Opcode::LoadLocal as u8); // the counter
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Return as u8);
    Module::new(
        vec![Function::new(helper, 1, 1), Function::new(main, 3, 0)],
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
    let mut executed = 0u64;
    for _ in 0..ROUNDS {
        let mut vm = Vm::new().with_calls(Box::new(CallCollector::new()));
        let _ = black_box(vm.run_module(&module).expect("call run"));
        let mut boxed = vm.take_calls().expect("attached");
        executed += boxed
            .as_any()
            .downcast_ref::<CallCollector>()
            .map(|c| c.events().len() as u64)
            .unwrap_or(0);
    }
    let captured = start.elapsed();

    let mut vm = Vm::new().with_calls(Box::new(CallCollector::new()));
    let _ = vm.run_module(&module).expect("render run");
    let mut boxed = vm.take_calls().expect("attached");
    let collector = boxed
        .as_any()
        .downcast_ref::<CallCollector>()
        .cloned()
        .unwrap_or_default();

    let start = Instant::now();
    let rendered = glyim_bytecode_vm::render_calls(black_box(&collector)).len();
    let render = start.elapsed();

    println!(
        "call-loop runs (20 x 10k call+return): untraced {untraced:?} | per-call hooks {captured:?} ({executed} events captured) | render {render:?} ({rendered} bytes over {} events)",
        collector.events().len(),
    );
}
