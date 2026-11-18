//! Run-diff timing harness.
//!
//! Dependency-free (`harness = false`), mirroring the `module_diff` /
//! `vm_coverage` benches in this crate: only `std::time::Instant` +
//! `std::hint::black_box`. The rundiff seam (the `rundiff` module) is a
//! pure function over two captured transcripts, so its headline numbers
//! are the **identical fast path** (two equal long transcripts — the
//! no-op-rebuild gate scans every dispatch), the **early-divergence
//! short circuit** (the first differing dispatch stops the scan), and the
//! render. Run with:
//!
//! ```text
//! cargo bench -p glyim-bytecode-vm --bench vm_run_diff
//! ```

use glyim_bytecode_vm::{Function, Module, Opcode, TraceCollector, Vm};
use std::hint::black_box;
use std::time::Instant;

/// A countdown loop (~`10 * N` dispatches per run): the transcript source
/// for the identical pair and the divergent pair.
fn countdown_loop(delta: i64) -> Module {
    let mut code = Vec::new();
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&10_000i64.to_le_bytes());
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
    code.extend_from_slice(&delta.to_le_bytes());
    code.push(Opcode::Gt as u8);
    code.push(Opcode::JumpIf as u8);
    // The function carries a block table, so the target is a block INDEX:
    // block 0 is the loop head.
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::Return as u8);
    Module::new(vec![Function::with_blocks(code, 1, 1, vec![head])], 0)
}

fn transcript(module: &Module) -> Vec<glyim_bytecode_vm::VmStep> {
    let collector = TraceCollector::new();
    let mut vm = Vm::new().with_trace(Box::new(collector));
    let _ = vm.run_module(module);
    let mut boxed = vm.take_trace().expect("attached");
    boxed
        .as_any()
        .downcast_ref::<TraceCollector>()
        .map(|t| t.steps().to_vec())
        .unwrap_or_default()
}

fn main() {
    const ROUNDS: u32 = 50;
    // Two runs of the same module: the identical pair (every dispatch is
    // scanned — the no-op gate's full cost).
    let base = countdown_loop(1);
    let a = transcript(&base);
    let b = transcript(&base);
    assert_eq!(a, b, "the machine is deterministic over the same module");

    let start = Instant::now();
    for _ in 0..ROUNDS {
        let rendered = glyim_bytecode_vm::render_run_diff(black_box(&a), black_box(&b));
        assert!(rendered.contains("no divergence"));
    }
    let identical = start.elapsed();

    // Two modules differing in one loop constant: the divergence sits a
    // handful of dispatches in (the preamble runs identically first), so
    // the scan short-circuits almost immediately.
    let tweaked = countdown_loop(2);
    let c = transcript(&tweaked);
    let start = Instant::now();
    for _ in 0..ROUNDS {
        let rendered = glyim_bytecode_vm::render_run_diff(black_box(&a), black_box(&c));
        assert!(rendered.contains("diverge at dispatch"));
    }
    let early = start.elapsed();

    // The render on its own (re-render the identical pair's listing).
    let start = Instant::now();
    for _ in 0..ROUNDS {
        let rendered = glyim_bytecode_vm::render_run_diff(black_box(&a), black_box(&b));
        black_box(rendered.len());
    }
    let render = start.elapsed();

    println!(
        "run-diff (50 rounds over {}-dispatch transcripts): identical full scan {identical:?} | early-divergence short circuit {early:?} | render {render:?}",
        a.len(),
    );
}
