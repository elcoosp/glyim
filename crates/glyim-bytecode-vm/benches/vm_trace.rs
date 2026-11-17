//! Execution-trace timing harness.
//!
//! Dependency-free (`harness = false`), mirroring the `module_disasm` bench
//! in this crate: only `std::time::Instant` + `std::hint::black_box`. The
//! trace seam (the `trace` module) rides the VM's dispatch loop, so its
//! headline number is the **hot-path overhead**: the same workload run
//! untraced (one `None` check per dispatch — the pre-seam behavior) versus
//! traced through a capturing sink, plus the deterministic render of the
//! captured steps. Run with:
//!
//! ```text
//! cargo bench -p glyim-bytecode-vm --bench vm_trace
//! ```

use glyim_bytecode_vm::{Function, Module, Opcode, TraceCollector, Vm, render_trace};
use std::hint::black_box;
use std::time::Instant;

/// A countdown loop: a preamble seeds entry local 0 with `SEED`, then the
/// loop decrements it to zero — ~`10 * SEED` dispatches per run, the shape
/// whose untraced-vs-traced wall time the seam's overhead claim rests on.
fn countdown_loop() -> Module {
    let mut code = Vec::new();
    // Preamble: seed local 0 with the loop count (the entry frame binds no
    // args, and locals are zero-initialised, so the count travels as a
    // constant before the loop).
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&10_000i64.to_le_bytes());
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    // Loop head (block 0, at the offset recorded in the block table below):
    // local 0 -= 1; JumpIf local 0 > 0 back to the head; return local 0.
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
    // Block-index mode: the target names the block table's index (0), not a
    // raw byte offset.
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
    let mut steps = 0usize;
    for _ in 0..ROUNDS {
        let collector = TraceCollector::bounded(1 << 17);
        let mut vm = Vm::new().with_trace(Box::new(collector));
        let _ = black_box(vm.run_module(&module).expect("traced run"));
        let mut boxed = vm.take_trace().expect("attached");
        steps += boxed
            .as_any()
            .downcast_ref::<TraceCollector>()
            .map(|c| c.len())
            .unwrap_or(0);
    }
    let traced = start.elapsed();

    // Fueled run (the `with_fuel` seam): the budget never exhausts here, so
    // this measures the counter's dispatch-loop overhead against the
    // untraced baseline above.
    let start = Instant::now();
    for _ in 0..ROUNDS {
        let mut vm = Vm::new().with_fuel(1_000_000);
        let _ = black_box(vm.run_module(&module).expect("fueled run"));
    }
    let fueled = start.elapsed();

    // Render cost over one captured run (the diagnostic listing the sink's
    // consumer would print).
    let mut render_us = 0u128;
    let mut rendered_len = 0usize;
    {
        let collector = TraceCollector::bounded(1 << 17);
        let mut vm = Vm::new().with_trace(Box::new(collector));
        let _ = vm.run_module(&module).expect("capture run");
        let mut boxed = vm.take_trace().expect("attached");
        if let Some(c) = boxed.as_any().downcast_ref::<TraceCollector>() {
            let steps: Vec<_> = c.steps().to_vec();
            let start = Instant::now();
            let text = render_trace(black_box(&steps), &module);
            rendered_len = text.len();
            render_us = start.elapsed().as_micros();
        }
    }

    let _ = steps;
    println!("untraced {ROUNDS} runs : {untraced:?}");
    println!("fueled   {ROUNDS} runs : {fueled:?} (budget never exhausts)");
    println!("traced   {ROUNDS} runs : {traced:?}");
    println!("render of captured steps: {render_us}us ({rendered_len}B)");
}
