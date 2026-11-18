//! Execution-profile timing harness.
//!
//! Dependency-free (`harness = false`), mirroring the `vm_trace` bench in
//! this crate: only `std::time::Instant` + `std::hint::black_box`. The
//! profile seam (the `profile` module) folds the same per-dispatch stream
//! the trace hook produces, so its headline numbers are the **hot-path
//! overhead** of folding a live run (the sink rides the identical
//! `with_trace` hook — the extra work per dispatch is the two map updates),
//! the **post-hoc fold** of a captured transcript through `from_steps`, and
//! the deterministic render of the aggregate. Run with:
//!
//! ```text
//! cargo bench -p glyim-bytecode-vm --bench vm_profile
//! ```

use glyim_bytecode_vm::render_profile;
use glyim_bytecode_vm::{Function, Module, Opcode, ProfileCollector, TraceCollector, Vm};
use std::hint::black_box;
use std::time::Instant;

/// A countdown loop: a preamble seeds entry local 0 with `SEED`, then the
/// loop decrements it to zero — ~`10 * SEED` dispatches per run, the same
/// shape the trace seam's overhead claim rests on (an op-count fold over a
/// loop-heavy workload: 4 distinct opcodes, one hot).
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
        let _ = black_box(vm.run_module(&module).expect("unprofiled run"));
    }
    let unprofiled = start.elapsed();

    let start = Instant::now();
    let mut steps = 0u64;
    for _ in 0..ROUNDS {
        let mut vm = Vm::new().with_trace(Box::new(ProfileCollector::new()));
        let _ = black_box(vm.run_module(&module).expect("profiled run"));
        let mut boxed = vm.take_trace().expect("attached");
        steps += boxed
            .as_any()
            .downcast_ref::<ProfileCollector>()
            .map(|p| p.steps())
            .unwrap_or(0);
    }
    let profiled = start.elapsed();

    // Post-hoc fold: capture the transcript first, then re-aggregate it
    // through `from_steps` (the consumer that keeps a trace *and* wants the
    // aggregate pays the fold once, off the hot path).
    let mut fold_us = 0u128;
    {
        let collector = TraceCollector::bounded(1 << 18);
        let mut vm = Vm::new().with_trace(Box::new(collector));
        let _ = vm.run_module(&module).expect("capture run");
        let mut boxed = vm.take_trace().expect("attached");
        if let Some(c) = boxed.as_any().downcast_ref::<TraceCollector>() {
            let steps: Vec<_> = c.steps().to_vec();
            let start = Instant::now();
            let profile = ProfileCollector::from_steps(black_box(&steps));
            fold_us = start.elapsed().as_micros();
            let _ = black_box(profile.steps());
        }
    }

    // Render cost over one profiled run (the diagnostic listing the sink's
    // consumer would print).
    let mut render_us = 0u128;
    let mut rendered_len = 0usize;
    {
        let mut vm = Vm::new().with_trace(Box::new(ProfileCollector::new()));
        let _ = vm.run_module(&module).expect("profile run");
        let mut boxed = vm.take_trace().expect("attached");
        if let Some(p) = boxed.as_any().downcast_ref::<ProfileCollector>() {
            let profile = p.clone();
            let start = Instant::now();
            let text = render_profile(black_box(&profile), &module);
            rendered_len = text.len();
            render_us = start.elapsed().as_micros();
        }
    }

    let _ = steps;
    println!("unprofiled {ROUNDS} runs : {unprofiled:?}");
    println!("profiled  {ROUNDS} runs : {profiled:?}");
    println!("from_steps fold of captured transcript: {fold_us}us");
    println!("render of aggregate: {render_us}us ({rendered_len}B)");
}
