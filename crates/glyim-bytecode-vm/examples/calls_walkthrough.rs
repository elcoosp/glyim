//! Calls walkthrough: run modules with the calls seam attached, then
//! read what every executed call and return did — the call sites, the
//! arguments, the destinations, and the values that came back.
//!
//! The call-graph seam (`CallGraphCollector` + `render_call_graph`) folds
//! the execution trace into per-`(caller, callee)` edges, and the
//! call-state seam snapshots a frame's locals at its boundaries — but the
//! trace's one-line step entries fold the call's operands away, and the
//! state event carries the whole local array, not the value the callee
//! produced. The calls seam (`CallCollector` + `render_calls`) is the
//! call-ABI counterpart: the VM hands it one `CallStarted` per executed
//! call — the call site, the callee, the argument values in full, the
//! destination local, and the resume target — and one `CallReturned` per
//! executed return — the returning function, its frame depth, and the
//! value it handed back — so a run's call traffic is readable after the
//! fact, including the open calls a fault or fuel budget leaves behind.
//! The hooks observe exactly like the other seams: read-only (the
//! arguments land in the callee's locals and the return value reaches the
//! caller unchanged), total over *executed* calls (a refused call reports
//! nothing — the fault stream owns it), bounded capture with a displayed
//! overflow flag, and a deterministic render. This walkthrough pairs a
//! call with its return, walks a bounded recursion's nesting, displays a
//! fault's open call, counts a call-heavy loop's traffic, reads the
//! bounded capture, and proves the seam is read-only. Run with:
//!
//! ```text
//! cargo run -p glyim-bytecode-vm --example calls_walkthrough
//! ```

use glyim_bytecode_vm::{CallCollector, Function, Module, Opcode, Vm, render_calls};

/// main (fn 1) calls helper (fn 0) once with argument 21; helper doubles
/// its argument and pushes it for `OP_RETURN`. Emitter order: the callee
/// index goes on the stack first, then the arguments, then
/// `OP_CALL argc dest resume`.
fn call_double_module() -> Module {
    let mut helper = Vec::new();
    helper.push(Opcode::LoadLocal as u8);
    helper.extend_from_slice(&1u32.to_le_bytes());
    helper.push(Opcode::LoadLocal as u8);
    helper.extend_from_slice(&1u32.to_le_bytes());
    helper.push(Opcode::Add as u8);
    helper.push(Opcode::StoreLocal as u8);
    helper.extend_from_slice(&0u32.to_le_bytes());
    helper.push(Opcode::LoadLocal as u8); // push the result
    helper.extend_from_slice(&0u32.to_le_bytes());
    helper.push(Opcode::Return as u8);
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8); // the callee (fn 0)
    main.extend_from_slice(&0i64.to_le_bytes());
    main.push(Opcode::LoadConst as u8); // the argument
    main.extend_from_slice(&21i64.to_le_bytes());
    main.push(Opcode::Call as u8);
    main.extend_from_slice(&1u32.to_le_bytes()); // argc
    main.extend_from_slice(&2u32.to_le_bytes()); // dest_local
    main.extend_from_slice(&31u32.to_le_bytes()); // resume
    main.push(Opcode::LoadLocal as u8); // the result
    main.extend_from_slice(&2u32.to_le_bytes());
    main.push(Opcode::Return as u8);
    Module::new(
        vec![Function::new(helper, 2, 1), Function::new(main, 3, 0)],
        1,
    )
}

/// main (fn 1) calls `count_down` (fn 0) with `3`; `count_down` recurses on
/// `n - 1` until it reaches zero and returns the countdown depth.
fn recursion_module() -> Module {
    let mut count_down = Vec::new();
    count_down.push(Opcode::LoadLocal as u8); // n
    count_down.extend_from_slice(&1u32.to_le_bytes());
    count_down.push(Opcode::LoadConst as u8); // zero
    count_down.extend_from_slice(&0i64.to_le_bytes());
    count_down.push(Opcode::Eq as u8); // n == 0
    count_down.push(Opcode::JumpIf as u8); // base case (pc 73)
    count_down.extend_from_slice(&73u32.to_le_bytes());
    count_down.push(Opcode::LoadConst as u8); // the callee (self)
    count_down.extend_from_slice(&0i64.to_le_bytes());
    count_down.push(Opcode::LoadLocal as u8); // n
    count_down.extend_from_slice(&1u32.to_le_bytes());
    count_down.push(Opcode::LoadConst as u8); // one
    count_down.extend_from_slice(&1i64.to_le_bytes());
    count_down.push(Opcode::Sub as u8); // n - 1
    count_down.push(Opcode::Call as u8); // count_down(n-1)
    count_down.extend_from_slice(&1u32.to_le_bytes()); // argc
    count_down.extend_from_slice(&2u32.to_le_bytes()); // dest_local
    count_down.extend_from_slice(&57u32.to_le_bytes()); // resume
    count_down.push(Opcode::LoadLocal as u8); // recursion result
    count_down.extend_from_slice(&2u32.to_le_bytes());
    count_down.push(Opcode::LoadConst as u8); // one
    count_down.extend_from_slice(&1i64.to_le_bytes());
    count_down.push(Opcode::Add as u8); // result + 1
    count_down.push(Opcode::Return as u8);
    count_down.push(Opcode::LoadConst as u8); // zero (base case)
    count_down.extend_from_slice(&0i64.to_le_bytes());
    count_down.push(Opcode::Return as u8);
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8); // the callee (fn 0)
    main.extend_from_slice(&0i64.to_le_bytes());
    main.push(Opcode::LoadConst as u8); // the seed (3)
    main.extend_from_slice(&3i64.to_le_bytes());
    main.push(Opcode::Call as u8);
    main.extend_from_slice(&1u32.to_le_bytes()); // argc
    main.extend_from_slice(&2u32.to_le_bytes()); // dest_local
    main.extend_from_slice(&31u32.to_le_bytes()); // resume
    main.push(Opcode::LoadLocal as u8); // the result
    main.extend_from_slice(&2u32.to_le_bytes());
    main.push(Opcode::Return as u8);
    Module::new(
        vec![Function::new(count_down, 3, 1), Function::new(main, 3, 0)],
        1,
    )
}

/// main (fn 1) calls helper (fn 0), which traps: the run faults after
/// the call started, before any return — the open call the render
/// displays.
fn faulting_callee_module() -> Module {
    let helper = vec![Opcode::Trap as u8];
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8); // the callee (fn 0)
    main.extend_from_slice(&0i64.to_le_bytes());
    main.push(Opcode::LoadConst as u8); // the argument
    main.extend_from_slice(&7i64.to_le_bytes());
    main.push(Opcode::Call as u8);
    main.extend_from_slice(&1u32.to_le_bytes());
    main.extend_from_slice(&2u32.to_le_bytes());
    main.extend_from_slice(&31u32.to_le_bytes());
    main.push(Opcode::Return as u8); // never reached
    Module::new(
        vec![Function::new(helper, 1, 1), Function::new(main, 3, 0)],
        1,
    )
}

/// A call-heavy countdown: main calls helper once per loop iteration,
/// seeding local 0 with `count` and looping while it is positive.
fn call_loop_module(count: i64) -> Module {
    let mut helper = Vec::new();
    helper.push(Opcode::LoadLocal as u8);
    helper.extend_from_slice(&1u32.to_le_bytes());
    helper.push(Opcode::Return as u8);
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8); // the seed
    main.extend_from_slice(&count.to_le_bytes());
    main.push(Opcode::StoreLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    // Loop head (pc 14): call helper(local 0), discard the result,
    // decrement, loop while still positive.
    main.push(Opcode::LoadConst as u8); // the callee (fn 0)
    main.extend_from_slice(&0i64.to_le_bytes());
    main.push(Opcode::LoadLocal as u8); // the argument
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Call as u8);
    main.extend_from_slice(&1u32.to_le_bytes()); // argc
    main.extend_from_slice(&2u32.to_le_bytes()); // dest_local
    main.extend_from_slice(&41u32.to_le_bytes()); // resume
    main.push(Opcode::LoadLocal as u8); // the result
    main.extend_from_slice(&2u32.to_le_bytes());
    main.push(Opcode::Drop as u8);
    main.push(Opcode::LoadLocal as u8); // the counter
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadConst as u8); // one
    main.extend_from_slice(&1i64.to_le_bytes());
    main.push(Opcode::Sub as u8);
    main.push(Opcode::StoreLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadLocal as u8); // the counter
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadConst as u8); // zero
    main.extend_from_slice(&0i64.to_le_bytes());
    main.push(Opcode::Gt as u8); // loop condition
    main.push(Opcode::JumpIf as u8); // back to the loop head
    main.extend_from_slice(&14u32.to_le_bytes());
    main.push(Opcode::LoadLocal as u8); // the counter
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Return as u8);
    Module::new(
        vec![Function::new(helper, 1, 1), Function::new(main, 3, 0)],
        1,
    )
}

/// Run a module with a fresh call collector attached and return the
/// typed collector.
fn calls_run(module: &Module) -> CallCollector {
    let mut vm = Vm::new().with_calls(Box::new(CallCollector::new()));
    let _ = vm.run_module(module);
    let mut boxed = vm.take_calls().expect("sink still attached");
    boxed
        .as_any()
        .downcast_ref::<CallCollector>()
        .expect("the collector came back")
        .clone()
}

fn main() {
    // 1. The call-ABI shape: one call with its argument spellings, its
    //    return, and the entry frame's return with the module result.
    let module = call_double_module();
    let calls = calls_run(&module);
    println!("one call, its return, and the module result:");
    println!("{}", render_calls(&calls));

    // 2. The recursion shape: a bounded countdown drives the depth folds
    //    — the high-water mark and the LIFO unwind are readable.
    let recursion = calls_run(&recursion_module());
    println!(
        "count_down(3): {} calls, {} returns, depth high-water {}\n",
        recursion.starts().len(),
        recursion.returns().len(),
        recursion.max_depth()
    );

    // 3. The fault shape: a callee that traps leaves its call open — the
    //    unreturned tail is displayed, never silently paired away.
    let faulted = calls_run(&faulting_callee_module());
    println!(
        "a faulting callee: {} calls started, {} returned, {} open\n",
        faulted.starts().len(),
        faulted.returns().len(),
        faulted.open_calls()
    );

    // 4. The call-heavy shape: a countdown loop calls once per iteration
    //    — the stream counts both faces.
    let loop_module = call_loop_module(3);
    let calls = calls_run(&loop_module);
    println!(
        "a 3-iteration call-heavy loop: {} calls, {} returns\n",
        calls.starts().len(),
        calls.returns().len()
    );

    // 5. The bounded capture: a capped collector keeps the first events
    //    and flags the overflow — displayed, never a silent drop.
    let mut vm = Vm::new().with_calls(Box::new(CallCollector::bounded(4)));
    let _ = vm.run_module(&loop_module).expect("module runs");
    let mut boxed = vm.take_calls().expect("sink still attached");
    let bounded = boxed
        .as_any()
        .downcast_ref::<CallCollector>()
        .expect("the collector came back")
        .clone();
    println!(
        "bounded(4) over seven events: {} events kept, overflowed = {}\n",
        bounded.events().len(),
        bounded.overflowed()
    );

    // 6. The read-only proof: the traced run's result equals the untraced
    //    one exactly (the seam observes — it never changes execution).
    let untraced = Vm::new().run_module(&module).expect("untraced run");
    let traced = {
        let mut vm = Vm::new().with_calls(Box::new(CallCollector::new()));
        let value = vm.run_module(&module).expect("traced run");
        let _ = vm.take_calls().expect("sink still attached");
        value
    };
    println!(
        "read-only: traced {traced:?} == untraced {untraced:?} — the seam observes, execution unchanged"
    );
}
