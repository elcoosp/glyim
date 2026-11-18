//! Tail-window timing harness.
//!
//! Dependency-free (`harness = false`), mirroring the `vm_trace` /
//! `vm_watch` / `vm_cost` benches in this crate: only
//! `std::time::Instant` + `std::hint::black_box`. The window seam (the
//! `window` module) folds the same per-dispatch stream the trace hook
//! produces, so its headline numbers are the **hot-path overhead** of
//! keeping the flight-recorder ring live (the sink rides the identical
//! `with_trace` hook — the extra work per dispatch is one step clone
//! into the ring plus, once the window is full, one eviction counted in
//! `dropped`), the **post-hoc fold** of a captured transcript through
//! `from_tail`, and the deterministic render. Run with:
//!
//! ```text
//! cargo bench -p glyim-bytecode-vm --bench vm_window
//! ```

use glyim_bytecode_vm::{Function, Module, Opcode, TraceCollector, Vm, WindowCollector};
use std::hint::black_box;
use std::time::Instant;

/// A countdown loop: seeds local 0 with `n` and counts down to 0 —
/// ~`8 * n` dispatches over one function (the scroll-friendly shape: the
/// window fills after `tail` dispatches and then evicts every step).
fn countdown_loop() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&12_500i64.to_le_bytes());
    main.push(Opcode::StoreLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    // Loop head (pc 14): decrement, then branch back while > 0.
    main.push(Opcode::LoadLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&1i64.to_le_bytes());
    main.push(Opcode::Sub as u8);
    main.push(Opcode::StoreLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&0i64.to_le_bytes());
    main.push(Opcode::Gt as u8);
    main.push(Opcode::JumpIf as u8);
    main.extend_from_slice(&14u32.to_le_bytes()); // loop head
    main.push(Opcode::LoadLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Return as u8);
    Module::new(vec![Function::new(main, 1, 0)], 0)
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
    let mut dropped = 0u64;
    for _ in 0..ROUNDS {
        let mut vm = Vm::new().with_trace(Box::new(WindowCollector::tail(64)));
        let _ = black_box(vm.run_module(&module).expect("windowed run"));
        let mut boxed = vm.take_trace().expect("attached");
        dropped += boxed
            .as_any()
            .downcast_ref::<WindowCollector>()
            .map(|w| w.dropped())
            .unwrap_or(0);
    }
    let windowed = start.elapsed();

    let transcript = TraceCollector::new();
    let mut vm = Vm::new().with_trace(Box::new(transcript));
    let _ = vm.run_module(&module).expect("transcript run");
    let mut boxed = vm.take_trace().expect("attached");
    let steps = boxed
        .as_any()
        .downcast_ref::<TraceCollector>()
        .map(|t| t.steps().to_vec())
        .unwrap_or_default();

    let start = Instant::now();
    for _ in 0..ROUNDS {
        let window = WindowCollector::from_tail(black_box(64), black_box(&steps));
        black_box(window.dropped());
    }
    let refold = start.elapsed();

    let window = WindowCollector::from_tail(64, &steps);
    let start = Instant::now();
    let rendered = render_len(&window, &module);
    let render = start.elapsed();

    println!(
        "countdown runs (20 x ~100k dispatches): untraced {untraced:?} | window-fold {windowed:?} ({dropped} dispatches evicted) | from_tail {refold:?} ({} steps) | render {render:?} ({rendered} bytes)",
        steps.len(),
    );
}

fn render_len(window: &WindowCollector, module: &Module) -> usize {
    glyim_bytecode_vm::render_window(window, module).len()
}
