//! Operand-stack timing harness.
//!
//! Dependency-free (`harness = false`), mirroring the `vm_watch` /
//! `vm_trace` / `vm_profile` benches in this crate: only
//! `std::time::Instant` + `std::hint::black_box`. The stack seam (the
//! `stack` module) rides the machine's operand funnels rather than the
//! dispatch loop, so its headline numbers are the **per-movement
//! overhead** of a live capture over a movement-heavy program (the
//! countdown loop pushes and pops the counter through the binop, the
//! store, the compare, and the branch — ~1.4 movements per dispatch; the
//! hooks are one `None` check per movement when no sink is attached),
//! and the deterministic render over the captured movements. Run with:
//!
//! ```text
//! cargo bench -p glyim-bytecode-vm --bench vm_stack
//! ```

use glyim_bytecode_vm::{Function, Module, Opcode, StackCollector, Vm, render_stack};
use std::hint::black_box;
use std::time::Instant;

/// A countdown loop: a preamble seeds entry local 0 with the count, then
/// the loop decrements it to zero — the movement-heavy shape (every
/// iteration moves twelve operands through the funnel).
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
        let _ = black_box(vm.run_module(&module).expect("unattached run"));
    }
    let unattached = start.elapsed();

    let start = Instant::now();
    let mut movements = 0u64;
    for _ in 0..ROUNDS {
        let mut vm = Vm::new().with_stack(Box::new(StackCollector::bounded(1 << 18)));
        let _ = black_box(vm.run_module(&module).expect("captured run"));
        let mut boxed = vm.take_stack().expect("attached");
        movements += boxed
            .as_any()
            .downcast_ref::<StackCollector>()
            .map(|c| c.len() as u64)
            .unwrap_or(0);
    }
    let captured = start.elapsed();
    println!(
        "countdown runs (20 x ~100k dispatches, ~140k movements each): unattached {unattached:?} | stack-capture {captured:?} ({movements} movements captured)"
    );

    // The render walks the captured movement list once per call; build the
    // list from a real captured run (the collector owns the capture), then
    // render it repeatedly.
    let events = {
        let collector = StackCollector::new();
        let mut vm = Vm::new().with_stack(Box::new(collector));
        let _ = vm.run_module(&module).expect("captured run");
        let mut boxed = vm.take_stack().expect("attached");
        boxed
            .as_any()
            .downcast_ref::<StackCollector>()
            .expect("the collector came back")
            .clone()
            .into_events()
    };
    let start = Instant::now();
    let mut bytes = 0usize;
    for _ in 0..1_000 {
        bytes += black_box(render_stack(&events, &module)).len();
    }
    let render = start.elapsed();
    println!("render (1000 x {bytes}-byte total): {render:?}");
}
