//! Instruction-coverage walkthrough: run a decoded module with the coverage
//! seam attached, then read which sites ran and which are dead.
//!
//! The profile seam (`ProfileCollector` + `render_profile`) answers "where
//! did the dispatches *go*?" by opcode and function; the coverage seam
//! (`CoverageCollector` + `render_coverage`) answers the spatial question —
//! "which instruction sites of the artifact executed, and which never
//! did?" — as a per-`(function, pc)` hit map aligned against the module's
//! instruction-site inventory (the same walk `render_module` uses, so a
//! site reads identically in the static listing and the coverage map).
//! This walkthrough runs a branchy module with an input that misses one
//! block, renders the hit map (the dead block shows up under
//! `uncovered:`), shows loop hits accumulating, re-folds a captured
//! `TraceCollector` transcript through `from_steps` into the identical
//! map, and proves the fuel gate's refused dispatch never reaches the
//! hook. Run with:
//!
//! ```text
//! cargo run -p glyim-bytecode-vm --example coverage_walkthrough
//! ```

use glyim_bytecode_vm::{
    CoverageCollector, Function, Module, Opcode, TraceCollector, Vm, VmError, decode_module,
    encode_module, render_coverage,
};
use std::collections::BTreeMap;

/// `classify(n)`: `0` when `n < 10`, else `1` — block 0 returns the `0`
/// arm, block 1 the `1` arm. With a small input the second arm is the
/// dead code the coverage map names.
fn classify() -> Function {
    let mut code = Vec::new();
    // Block 0 (entry): local 0 < 10 ? block 1 : block 2.
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&10i64.to_le_bytes());
    code.push(Opcode::Lt as u8);
    code.push(Opcode::JumpIf as u8);
    code.extend_from_slice(&1u32.to_le_bytes()); // -> block 1
    // Block 2 (fall-through): return 1.
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&1i64.to_le_bytes());
    code.push(Opcode::Return as u8);
    // Block 1: return 0.
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&0i64.to_le_bytes());
    code.push(Opcode::Return as u8);
    // Layout: block 0 = entry (pc 0), block 2 = fall-through (pc 20),
    // block 1 = the `n < 10` arm (pc 30). The JumpIf targets block 1.
    Function::with_blocks(code, 2, 1, vec![0, 30, 20])
}

/// A countdown loop: the loop-head sites dispatch once per iteration, so
/// their hit counts accumulate instead of pinning at 1.
fn countdown() -> Module {
    let mut code = Vec::new();
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&10i64.to_le_bytes());
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
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

/// Run `module` with a coverage collector attached and print the render.
fn cover_and_render(module: &Module, label: &str) -> BTreeMap<(usize, usize), u64> {
    let collector = CoverageCollector::new();
    let mut vm = Vm::new().with_trace(Box::new(collector));
    let value = vm.run_module(module).expect("the module runs");
    println!("--- {label} (result {value:?}) ---");
    let mut boxed = vm.take_trace().expect("the sink is still attached");
    let sink = boxed.as_any();
    let collector = sink
        .downcast_ref::<CoverageCollector>()
        .expect("the collector came back");
    let hits = collector.hits().clone();
    println!("{}", render_coverage(&hits, module));
    hits
}

fn main() {
    // The image round trip composes with the seam: ship the bytes, decode
    // them back, then cover the loaded module. The `5` input misses the
    // `>= 10` arm — the coverage map names the dead sites.
    let classify_fn = classify();
    let classify_module = Module::new(vec![classify_fn.clone()], 0);
    let bytes = encode_module(&classify_module);
    let loaded = decode_module(&bytes).expect("the image decodes");
    let _ = cover_and_render(&loaded, "a branchy module with a dead arm");

    // Loop hits accumulate: the loop head dispatches once per iteration.
    let _ = cover_and_render(&countdown(), "a countdown loop (10 iterations)");

    // `from_steps` re-folds a captured transcript into the identical map —
    // a consumer that keeps a trace *and* wants the hit map pays the fold
    // once, off the hot path.
    let trace = TraceCollector::new();
    let mut vm = Vm::new().with_trace(Box::new(trace));
    let _ = vm.run_module(&classify_module).expect("the module runs");
    let mut boxed = vm.take_trace().expect("the sink is still attached");
    let steps: Vec<_> = boxed
        .as_any()
        .downcast_ref::<TraceCollector>()
        .expect("the trace collector came back")
        .steps()
        .to_vec();
    let refolded = CoverageCollector::from_steps(&steps);
    let live = cover_and_render(&classify_module, "the live fold (for comparison)");
    println!("--- from_steps re-fold of the captured transcript ---");
    println!("identical to the live fold: {}", refolded.hits() == &live);
    println!();

    // A fuel-refused dispatch never reaches the hook: the coverage ends
    // exactly at the executed sites (here: the loop head, 5 times).
    let module = countdown();
    let collector = CoverageCollector::new();
    let mut vm = Vm::new().with_fuel(5).with_trace(Box::new(collector));
    let err = vm
        .run_module(&module)
        .expect_err("the loop burns the budget");
    let mut boxed = vm.take_trace().expect("the sink is still attached");
    let sink = boxed.as_any();
    let collector = sink
        .downcast_ref::<CoverageCollector>()
        .expect("the collector came back");
    println!("--- the fuel gate stops an infinite-ish loop ---");
    let VmError::FuelExhausted { executed } = err else {
        panic!("expected FuelExhausted");
    };
    println!(
        "executed={executed}, folded={}: the refused dispatch is not a hit",
        collector.steps()
    );
}
