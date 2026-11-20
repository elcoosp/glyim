//! Integer-arithmetic timing harness.
//!
//! Dependency-free (`harness = false`), mirroring the `vm_floats` /
//! `vm_fuel` / `vm_stack` benches in this crate: only
//! `std::time::Instant` + `std::hint::black_box`. The math seam (the
//! `math` module) rides the binop and unary arms — one event per
//! executed `(Int, Int)` binop with the wrap verdict, one per
//! `OP_NEG`/`OP_NOT` over an integer — so its headline numbers are the
//! **per-binop hook overhead** of a live capture over an
//! arithmetic-heavy program (the countdown loop performs two integer
//! binops per iteration — the `Sub` and the comparison — so a capture
//! costs two sink-side copies per iteration; the hook is one `None`
//! check per executed integer operation when no sink is attached), and
//! the deterministic render over the captured transcript. Run with:
//!
//! ```text
//! cargo bench -p glyim-bytecode-vm --bench vm_math
//! ```

use glyim_bytecode_vm::{Function, MathCollector, Module, Opcode, Vm, render_math};
use std::hint::black_box;
use std::time::Instant;

/// A countdown loop: a preamble seeds entry local 0 with the count, then
/// the loop decrements it to zero — the arithmetic-heavy shape (~100k
/// dispatches per run, two executed integer binops per iteration).
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
    let mut events = 0u64;
    for _ in 0..ROUNDS {
        let mut vm = Vm::new().with_math(Box::new(MathCollector::bounded(1 << 17)));
        let _ = black_box(vm.run_module(&module).expect("captured run"));
        let mut boxed = vm.take_math().expect("attached");
        events += boxed
            .as_any()
            .downcast_ref::<MathCollector>()
            .map(|c| c.len() as u64)
            .unwrap_or(0);
    }
    let captured = start.elapsed();
    println!(
        "countdown runs (20 x ~100k dispatches): unattached {unattached:?} | math-capture {captured:?} ({events} events captured)"
    );

    // The render walks the captured transcript once per call; build it
    // from a real captured run, then render it repeatedly.
    let collector = {
        let collector = MathCollector::new();
        let mut vm = Vm::new().with_math(Box::new(collector));
        let _ = vm.run_module(&module).expect("captured run");
        let mut boxed = vm.take_math().expect("attached");
        boxed
            .as_any()
            .downcast_ref::<MathCollector>()
            .expect("the collector came back")
            .clone()
    };
    let start = Instant::now();
    let mut bytes = 0usize;
    for _ in 0..1_000 {
        bytes += black_box(render_math(&collector)).len();
    }
    let render = start.elapsed();
    println!("render (1000 x {bytes}-byte total): {render:?}");
}
