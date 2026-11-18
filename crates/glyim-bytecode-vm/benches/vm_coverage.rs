//! Instruction-coverage timing harness.
//!
//! Dependency-free (`harness = false`), mirroring the `vm_trace` /
//! `vm_profile` / `vm_backtrace` benches in this crate: only
//! `std::time::Instant` + `std::hint::black_box`. The coverage seam (the
//! `coverage` module) folds the same per-dispatch stream the trace hook
//! produces, so its headline numbers are the **hot-path overhead** of
//! folding a live run (the sink rides the identical `with_trace` hook — the
//! extra work per dispatch is one map update), the **post-hoc fold** of a
//! captured transcript through `from_steps`, and the deterministic render
//! (which walks the module's site inventory). Run with:
//!
//! ```text
//! cargo bench -p glyim-bytecode-vm --bench vm_coverage
//! ```

use glyim_bytecode_vm::{CoverageCollector, Function, Module, Opcode, TraceCollector, Vm};
use std::collections::BTreeMap;
use std::hint::black_box;
use std::time::Instant;

/// A countdown loop: a preamble seeds entry local 0 with `SEED`, then the
/// loop decrements it to zero — ~`10 * SEED` dispatches per run over a
/// handful of sites (the aggregate-friendly shape: bounded map, hot loop).
fn countdown_loop() -> Module {
    let mut code = Vec::new();
    // Preamble: seed local 0 with the loop count.
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&10_000i64.to_le_bytes());
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    // Loop head (block 0): local 0 -= 1; JumpIf local 0 > 0 back to the
    // head; return local 0.
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
    const ROUNDS: u32 = 20;
    let module = countdown_loop();

    let start = Instant::now();
    for _ in 0..ROUNDS {
        let mut vm = Vm::new();
        let _ = black_box(vm.run_module(&module).expect("untraced run"));
    }
    let untraced = start.elapsed();

    let start = Instant::now();
    let mut steps = 0u64;
    for _ in 0..ROUNDS {
        let mut vm = Vm::new().with_trace(Box::new(CoverageCollector::new()));
        let _ = black_box(vm.run_module(&module).expect("covered run"));
        let mut boxed = vm.take_trace().expect("attached");
        steps += boxed
            .as_any()
            .downcast_ref::<CoverageCollector>()
            .map(|c| c.steps())
            .unwrap_or(0);
    }
    let covered = start.elapsed();
    println!(
        "countdown runs (20 x ~100k dispatches): untraced {untraced:?} | coverage-fold {covered:?} ({steps} dispatches folded)"
    );

    // Post-hoc fold: capture the transcript first, then re-aggregate it
    // through `from_steps` (the consumer that keeps a trace *and* wants the
    // hit map pays the fold once, off the hot path).
    let mut fold_us = 0u128;
    {
        let collector = TraceCollector::bounded(1 << 18);
        let mut vm = Vm::new().with_trace(Box::new(collector));
        let _ = vm.run_module(&module).expect("capture run");
        let mut boxed = vm.take_trace().expect("attached");
        if let Some(c) = boxed.as_any().downcast_ref::<TraceCollector>() {
            let steps: Vec<_> = c.steps().to_vec();
            let start = Instant::now();
            let coverage = CoverageCollector::from_steps(black_box(&steps));
            fold_us = start.elapsed().as_micros();
            let _ = black_box(coverage.steps());
        }
    }
    println!("from_steps fold over the captured transcript: {fold_us} us");

    // The render walks the module's site inventory once per call; build the
    // hit map from a real covered run (no width duplication here — the
    // collector owns the fold), then render it repeatedly.
    let hits: BTreeMap<(usize, usize), u64> = {
        let collector = CoverageCollector::new();
        let mut vm = Vm::new().with_trace(Box::new(collector));
        let _ = vm.run_module(&module).expect("covered run");
        let mut boxed = vm.take_trace().expect("attached");
        boxed
            .as_any()
            .downcast_ref::<CoverageCollector>()
            .expect("the collector came back")
            .hits()
            .clone()
    };
    let start = Instant::now();
    let mut bytes = 0usize;
    for _ in 0..1_000 {
        bytes += black_box(glyim_bytecode_vm::render_coverage(&hits, &module)).len();
    }
    let render = start.elapsed();
    println!("render (1000 x {bytes}-byte total): {render:?}");
}
