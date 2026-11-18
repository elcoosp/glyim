//! Tail-window walkthrough: run a module with the flight-recorder window
//! attached, then read the last N dispatches before the run ended — with
//! their absolute dispatch numbers and the honest count of everything the
//! bound scrolled out.
//!
//! The trace seam (`TraceCollector` + `render_trace`) records *every*
//! dispatch, which is the right answer at O(steps) memory; the
//! fault-backtrace seam (`BacktraceCollector` + `render_backtrace`)
//! names the fault site without the dispatches that led to it. The
//! tail-window seam (`WindowCollector` + `render_window`) is the
//! bounded-memory counterpart: a fixed-size ring over the same
//! per-dispatch stream that always holds the run's most recent steps —
//! the failure-shaped question ("what happened immediately before?")
//! answered without holding the transcript. It observes exactly like the
//! other stream folds: read-only, total (the faulting dispatch and a
//! fuel budget's last allowed dispatch are the window's newest entries —
//! the hook fires before the instruction executes), bounded (the memory
//! bound is the window, the scroll-out is *displayed* in `dropped`),
//! and a deterministic render in the trace's exact field shape. This
//! walkthrough faults a call into an out-of-range function to see the
//! fault site in the tail, cuts a run with a fuel budget to see the
//! window end at the executed count, scrolls a long run to see the
//! dropped counter, re-folds a captured transcript into the identical
//! window, and proves the seam is read-only. Run with:
//!
//! ```text
//! cargo run -p glyim-bytecode-vm --example fault_window_walkthrough
//! ```

use glyim_bytecode_vm::{
    Function, Module, Opcode, TraceCollector, Vm, VmError, WindowCollector, render_window,
};

/// main (fn 0) calls fn 5 — out of range: three dispatches, the run ends
/// in `VmError::FunctionOutOfBounds`, and the faulting `Call` is the
/// window's newest entry. Table-less functions: call-resume targets are
/// raw byte offsets.
fn out_of_range_call_module() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&5i64.to_le_bytes()); // fn index 5: out of range
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&1i64.to_le_bytes());
    main.push(Opcode::Call as u8);
    main.extend_from_slice(&1u32.to_le_bytes()); // argc
    main.extend_from_slice(&0u32.to_le_bytes()); // dest local
    main.extend_from_slice(&31u32.to_le_bytes()); // resume byte offset
    main.push(Opcode::LoadLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Return as u8);
    Module::new(vec![Function::new(main, 1, 0)], 0)
}

/// A countdown loop: seeds local 0 with `n` and counts down to 0 — the
/// long-run shape that scrolls the window and fills the dropped counter.
fn countdown_module(n: i64) -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&n.to_le_bytes());
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
    // 1. The faulting run: the window rides the trace hook like any
    //    other sink; the faulting dispatch is the newest entry because
    //    the hook fires before the instruction executes.
    let module = out_of_range_call_module();
    let mut vm = Vm::new().with_trace(Box::new(WindowCollector::tail(3)));
    let outcome = vm.run_module(&module);
    let mut boxed = vm.take_trace().expect("sink still attached");
    let window = boxed
        .as_any()
        .downcast_ref::<WindowCollector>()
        .expect("the collector came back")
        .clone();
    println!("module calling fn 5 (out of range): {outcome:?}");
    println!("{}", render_window(&window, &module));

    // 2. A fuel budget cut: the window ends exactly at the executed
    //    count (the refused dispatch reaches no hook), and the newest
    //    entry is the budget's last allowed dispatch.
    let long = countdown_module(50);
    let mut vm = Vm::new()
        .with_trace(Box::new(WindowCollector::tail(3)))
        .with_fuel(10);
    let outcome = vm.run_module(&long);
    let mut boxed = vm.take_trace().expect("sink still attached");
    let cut = boxed
        .as_any()
        .downcast_ref::<WindowCollector>()
        .expect("the collector came back")
        .clone();
    println!("fuel-budget cut after 10 dispatches: {outcome:?}");
    println!("{}", render_window(&cut, &long));

    // 3. A long run scrolls: the window holds the newest `n` dispatches
    //    and counts everything the bound traded away in `dropped`.
    let mut vm = Vm::new().with_trace(Box::new(WindowCollector::tail(5)));
    let value = vm
        .run_module(&long)
        .expect("the module runs (the seam is read-only)");
    let mut boxed = vm.take_trace().expect("sink still attached");
    let scrolled = boxed
        .as_any()
        .downcast_ref::<WindowCollector>()
        .expect("the collector came back")
        .clone();
    println!("\ncountdown(50) returned {value:?} with a window of 5:");
    println!(
        "captured={} dropped={} (404 executed, the scroll is displayed)",
        scrolled.steps().len(),
        scrolled.dropped(),
    );

    // 4. The tail is a pure function of the step stream: re-fold a
    //    captured TraceCollector transcript post-hoc into the identical
    //    window (a shared transcript feeds every stream fold). The
    //    transcript run faults by design — its steps are captured all
    //    the same.
    let transcript = TraceCollector::new();
    let mut vm = Vm::new().with_trace(Box::new(transcript));
    let outcome = vm.run_module(&module);
    debug_assert!(matches!(outcome, Err(VmError::FunctionOutOfBounds(5))));
    let mut boxed = vm.take_trace().expect("sink still attached");
    let steps = boxed
        .as_any()
        .downcast_ref::<TraceCollector>()
        .expect("the collector came back")
        .steps()
        .to_vec();
    let refolded = WindowCollector::from_tail(3, &steps);
    println!(
        "from_tail over {} captured steps: steps equal = {} (render equal = {})",
        steps.len(),
        refolded.steps() == window.steps(),
        render_window(&refolded, &module) == render_window(&window, &module),
    );
}
