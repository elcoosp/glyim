//! Call-graph timing harness.
//!
//! Dependency-free (`harness = false`), mirroring the `vm_trace` /
//! `vm_profile` / `vm_coverage` benches in this crate: only
//! `std::time::Instant` + `std::hint::black_box`. The callgraph seam (the
//! `callgraph` module) folds the same per-dispatch stream the trace hook
//! produces, so its headline numbers are the **hot-path overhead** of
//! folding a live run (the sink rides the identical `with_trace` hook —
//! the extra work per dispatch is one depth check plus, per call, one map
//! update), the **post-hoc fold** of a captured transcript through
//! `from_steps`, and the deterministic render. Run with:
//!
//! ```text
//! cargo bench -p glyim-bytecode-vm --bench vm_callgraph
//! ```

use glyim_bytecode_vm::{CallGraphCollector, Function, Module, Opcode, TraceCollector, Vm};
use std::hint::black_box;
use std::time::Instant;

/// A call-heavy loop: fn 1 calls fn 0 (a double-the-argument helper) once
/// per iteration over a countdown — ~`16 * N` dispatches and exactly one
/// edge accumulating `N` dispatches (the aggregate-friendly shape: bounded
/// map, hot call loop).
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
    let mut calls = 0u64;
    for _ in 0..ROUNDS {
        let mut vm = Vm::new().with_trace(Box::new(CallGraphCollector::new()));
        let _ = black_box(vm.run_module(&module).expect("callgraph run"));
        let mut boxed = vm.take_trace().expect("attached");
        calls += boxed
            .as_any()
            .downcast_ref::<CallGraphCollector>()
            .map(|g| g.calls())
            .unwrap_or(0);
    }
    let folded = start.elapsed();

    let transcript = TraceCollector::new();
    let mut vm = Vm::new().with_trace(Box::new(transcript));
    let _ = vm.run_module(&module).expect("transcript run");
    let mut boxed = vm.take_trace().expect("attached");
    let steps = boxed
        .as_any()
        .downcast_ref::<TraceCollector>()
        .map(|t| t.steps().to_vec())
        .unwrap_or_default();

    let start = Instant::now();
    for _ in 0..ROUNDS {
        let graph = CallGraphCollector::from_steps(black_box(&steps));
        black_box(graph.calls());
    }
    let refold = start.elapsed();

    let graph = CallGraphCollector::from_steps(&steps);
    let start = Instant::now();
    let rendered = render_len(&graph);
    let render = start.elapsed();

    println!(
        "call-loop runs (20 x ~160k dispatches): untraced {untraced:?} | callgraph-fold {folded:?} ({calls} calls folded) | from_steps {refold:?} ({} steps) | render {render:?} ({rendered} bytes)",
        steps.len(),
    );
}

fn render_len(graph: &CallGraphCollector) -> usize {
    glyim_bytecode_vm::render_call_graph(graph.edges(), graph.calls()).len()
}
