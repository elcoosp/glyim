//! Fuel timing harness.
//!
//! Dependency-free (`harness = false`), mirroring the `vm_stack` /
//! `vm_watch` / `vm_trace` benches in this crate: only
//! `std::time::Instant` + `std::hint::black_box`. The fuel seam (the
//! `fuel` module) rides the dispatch loop at the same cadence as the
//! trace hook, so its headline numbers are the **per-dispatch
//! checkpoint overhead** of a live capture over a budget-burning
//! program (the countdown loop — one `FuelTick` per dispatch; the hook
//! is one `None` check per dispatch when no sink is attached), and the
//! deterministic render over the captured diary. The captured arm
//! attaches the sink without a budget (`remaining` is `None` per tick)
//! so the pair isolates the hook cost; the budget decrement itself is
//! the gate's own work, paid identically in both arms. Run with:
//!
//! ```text
//! cargo bench -p glyim-bytecode-vm --bench vm_fuel
//! ```

use glyim_bytecode_vm::{FuelCollector, Function, Module, Opcode, Vm, render_fuel};
use std::hint::black_box;
use std::time::Instant;

/// A countdown loop: a preamble seeds entry local 0 with the count, then
/// the loop decrements it to zero — the budget-burning shape (~100k
/// dispatches per run, one checkpoint each).
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
    let mut ticks = 0u64;
    for _ in 0..ROUNDS {
        let mut vm = Vm::new().with_fuel_sink(Box::new(FuelCollector::bounded(1 << 17)));
        let _ = black_box(vm.run_module(&module).expect("captured run"));
        let mut boxed = vm.take_fuel_sink().expect("attached");
        ticks += boxed
            .as_any()
            .downcast_ref::<FuelCollector>()
            .map(|c| c.len() as u64)
            .unwrap_or(0);
    }
    let captured = start.elapsed();
    println!(
        "countdown runs (20 x ~100k dispatches): unattached {unattached:?} | fuel-capture {captured:?} ({ticks} ticks captured)"
    );

    // The render walks the captured diary once per call; build it from a
    // real captured run, then render it repeatedly.
    let collector = {
        let collector = FuelCollector::new();
        let mut vm = Vm::new().with_fuel_sink(Box::new(collector));
        let _ = vm.run_module(&module).expect("captured run");
        let mut boxed = vm.take_fuel_sink().expect("attached");
        boxed
            .as_any()
            .downcast_ref::<FuelCollector>()
            .expect("the collector came back")
            .clone()
    };
    let start = Instant::now();
    let mut bytes = 0usize;
    for _ in 0..1_000 {
        bytes += black_box(render_fuel(&collector)).len();
    }
    let render = start.elapsed();
    println!("render (1000 x {bytes}-byte total): {render:?}");
}
