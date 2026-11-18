//! Local-watch timing harness.
//!
//! Dependency-free (`harness = false`), mirroring the `vm_trace` /
//! `vm_profile` / `vm_backtrace` / `vm_coverage` benches in this crate:
//! only `std::time::Instant` + `std::hint::black_box`. The watch seam (the
//! `watch` module) hooks the *write* paths rather than the dispatch loop,
//! so its headline numbers are the **per-write overhead** of a live
//! capture over a store-heavy program (the seed store plus one store per
//! loop iteration — the hook is one `None` check per write when no sink is
//! attached), and the deterministic render over the captured events. Run
//! with:
//!
//! ```text
//! cargo bench -p glyim-bytecode-vm --bench vm_watch
//! ```

use glyim_bytecode_vm::{Function, Module, Opcode, Vm, WatchCollector, render_watch};
use std::hint::black_box;
use std::time::Instant;

/// A countdown loop: a preamble seeds entry local 0 with `SEED`, then the
/// loop decrements it to zero — one write per iteration over a handful of
/// dispatches (the watch-friendly shape: a hot slot, a bounded event list
/// when captured bounded).
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
        let _ = black_box(vm.run_module(&module).expect("unwatched run"));
    }
    let unwatched = start.elapsed();

    let start = Instant::now();
    let mut writes = 0u64;
    for _ in 0..ROUNDS {
        let mut vm = Vm::new().with_watch(Box::new(WatchCollector::bounded(1 << 16)));
        let _ = black_box(vm.run_module(&module).expect("watched run"));
        let mut boxed = vm.take_watch().expect("attached");
        writes += boxed
            .as_any()
            .downcast_ref::<WatchCollector>()
            .map(|c| c.len() as u64)
            .unwrap_or(0);
    }
    let watched = start.elapsed();
    println!(
        "countdown runs (20 x ~100k dispatches, 10k writes each): unwatched {unwatched:?} | watch-capture {watched:?} ({writes} writes captured)"
    );

    // The render walks the captured event list once per call; build the
    // event list from a real watched run (the collector owns the capture),
    // then render it repeatedly.
    let events = {
        let collector = WatchCollector::new();
        let mut vm = Vm::new().with_watch(Box::new(collector));
        let _ = vm.run_module(&module).expect("watched run");
        let mut boxed = vm.take_watch().expect("attached");
        boxed
            .as_any()
            .downcast_ref::<WatchCollector>()
            .expect("the collector came back")
            .clone()
            .into_events()
    };
    let start = Instant::now();
    let mut bytes = 0usize;
    for _ in 0..1_000 {
        bytes += black_box(render_watch(&events, &module)).len();
    }
    let render = start.elapsed();
    println!("render (1000 x {bytes}-byte total): {render:?}");
}
