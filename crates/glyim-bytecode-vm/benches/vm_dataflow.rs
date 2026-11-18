//! Data-flow timing harness.
//!
//! Dependency-free (`harness = false`), mirroring the `vm_watch` /
//! `vm_trace` / `vm_cost` benches in this crate: only `std::time::Instant`
//! and `std::hint::black_box`. The data-flow seam (the `dataflow` module)
//! hooks the *read* path (`OP_LOAD_LOCAL`) and pairs those reads with the
//! writes the watch hook reports, so its headline numbers are the
//! **per-read overhead** of a live paired fold over a read-heavy program
//! (the hook is one `None` check per read when no sink is attached), the
//! interleaved-transcript capture, and the deterministic render. Run with:
//!
//! ```text
//! cargo bench -p glyim-bytecode-vm --bench vm_dataflow
//! ```

use glyim_bytecode_vm::{
    DataFlowCollector, FlowRecorder, Function, Module, Opcode, Vm, render_data_flow,
};
use std::hint::black_box;
use std::time::Instant;

/// A countdown loop: a preamble seeds entry local 0 with the iteration
/// count, then the loop reads it three times per iteration (the
/// read-heavy shape: read-compute, read-store-back, read-test) and
/// decrements it once per iteration — the data-flow-friendly shape: a hot
/// slot whose every read pairs with the newest def.
fn countdown_loop(iterations: i64) -> Module {
    let mut code = Vec::new();
    // Preamble: seed local 0 with the loop count.
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&iterations.to_le_bytes());
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    // Loop head (block 0): read-compute-store, read-test-jump, and the
    // post-loop read for the return value.
    let head = code.len();
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&1i64.to_le_bytes());
    code.push(Opcode::Sub as u8);
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&0i64.to_le_bytes());
    code.push(Opcode::Gt as u8);
    code.push(Opcode::JumpIf as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::Return as u8);
    Module::new(vec![Function::with_blocks(code, 1, 1, vec![head])], 0)
}

fn main() {
    let module = countdown_loop(2_000);

    // Correctness + determinism pins before the timing (plan C6 style):
    // the paired result equals the unpaired result, and the fold's totals
    // match the program's static read/write counts.
    let unpaired = Vm::new().run_module(&module).expect("unpaired runs");
    let collector = DataFlowCollector::new();
    let mut paired_vm = Vm::new()
        .with_watch(Box::new(collector.clone()))
        .with_reads(Box::new(collector));
    let paired = paired_vm.run_module(&module).expect("paired runs");
    assert_eq!(paired, unpaired);
    let mut boxed = paired_vm.take_reads().expect("attached");
    let collector = boxed
        .as_any()
        .downcast_ref::<DataFlowCollector>()
        .expect("collector")
        .clone();
    assert_eq!(collector.total_writes(), 2_001, "seed + one per iteration");
    assert_eq!(
        collector.total_reads(),
        4_001,
        "two per iteration plus the post-loop return read"
    );
    assert_eq!(collector.uninitialized_read_total(), 0);
    let report = render_data_flow(&collector);
    assert_eq!(report, render_data_flow(&collector));

    // Paired fold overhead: the same run with both hooks live versus the
    // bare machine (the delta is the seam's per-dispatch cost on a
    // read-heavy program).
    let started = Instant::now();
    let bare_value = Vm::new().run_module(black_box(&module)).expect("bare runs");
    let bare_elapsed = started.elapsed();

    let collector = DataFlowCollector::new();
    let started = Instant::now();
    let mut vm = Vm::new()
        .with_watch(Box::new(collector.clone()))
        .with_reads(Box::new(collector.clone()));
    let watched_value = vm.run_module(black_box(&module)).expect("paired runs");
    let paired_elapsed = started.elapsed();
    assert_eq!(bare_value, watched_value);

    // Transcript capture: the interleaved recorder over the same run.
    let recorder = FlowRecorder::new();
    let started = Instant::now();
    let mut vm = Vm::new()
        .with_watch(Box::new(recorder.clone()))
        .with_reads(Box::new(recorder));
    vm.run_module(black_box(&module)).expect("recorded runs");
    let record_elapsed = started.elapsed();

    // Render cost: the deterministic listing over the fold.
    let started = Instant::now();
    for _ in 0..1_000 {
        black_box(render_data_flow(black_box(&collector)));
    }
    let render_elapsed = started.elapsed();

    println!(
        "data-flow bench: 2,000-iteration countdown (5,001 writes+reads)\n  bare run  : {bare_elapsed:?}\n  paired run: {paired_elapsed:?} (delta = the seam's per-dispatch cost)\n  record    : {record_elapsed:?} (interleaved transcript)\n  render    : {render_elapsed:?} (1,000 renders, {} bytes each)",
        report.len(),
    );
}
