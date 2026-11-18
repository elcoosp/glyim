//! Call-graph walkthrough: run a module with the call-graph seam attached,
//! then read who called whom, how often, and which calls never landed.
//!
//! The profile seam (`ProfileCollector` + `render_profile`) answers "where
//! did the dispatches *go*?" by opcode and function; the call-graph seam
//! (`CallGraphCollector` + `render_call_graph`) answers the structural
//! question — "which function entered which, and how many times?" — as a
//! per-`(caller, callee)` edge map folded from the same dispatch stream
//! the trace hook produces. Because the trace hook fires *before* an
//! instruction executes and the `Call` payload carries no callee (the
//! callee index sits on the operand stack, invisible to a read-only sink),
//! the fold names a callee by the step that *follows* the call — the
//! callee's entry instruction, exactly one frame deeper. A call whose
//! callee never dispatches (an out-of-range function index, a faulting
//! entry, a fuel budget cut between the call and the entry) stays in
//! `calls()` and reports through `unresolved_calls()` — displayed, never
//! silently dropped. This walkthrough runs a recursive module (the
//! self-edge accumulates once per recursive dispatch), re-folds a captured
//! `TraceCollector` transcript through `from_steps` into the identical
//! graph, faults a call into an out-of-range function index to see the
//! unresolved line, and proves the seam is read-only. Run with:
//!
//! ```text
//! cargo run -p glyim-bytecode-vm --example call_graph_walkthrough
//! ```

use glyim_bytecode_vm::{
    CallGraphCollector, Function, Module, Opcode, TraceCollector, Vm, render_call_graph,
};

/// `rec(n)` recurses `n` times, returning 1 per frame (0 at the base);
/// main (fn 1) seeds it with 3. The self-edge `(0, 0)` accumulates once
/// per recursive dispatch. Table-less functions: branch and call-resume
/// targets are raw byte offsets.
fn recursion_module() -> Module {
    let mut rec = Vec::new();
    rec.push(Opcode::LoadLocal as u8);
    rec.extend_from_slice(&1u32.to_le_bytes()); // arg n at local 1
    rec.push(Opcode::LoadConst as u8);
    rec.extend_from_slice(&0i64.to_le_bytes());
    rec.push(Opcode::Le as u8);
    rec.push(Opcode::JumpIf as u8);
    rec.extend_from_slice(&86u32.to_le_bytes()); // base case at pc 86
    rec.push(Opcode::LoadConst as u8);
    rec.extend_from_slice(&0i64.to_le_bytes()); // fn index 0: self
    rec.push(Opcode::LoadLocal as u8);
    rec.extend_from_slice(&1u32.to_le_bytes());
    rec.push(Opcode::LoadConst as u8);
    rec.extend_from_slice(&1i64.to_le_bytes());
    rec.push(Opcode::Sub as u8);
    rec.push(Opcode::StoreLocal as u8);
    rec.extend_from_slice(&1u32.to_le_bytes()); // n -= 1
    rec.push(Opcode::LoadConst as u8);
    rec.extend_from_slice(&0i64.to_le_bytes()); // fn index 0: self
    rec.push(Opcode::LoadLocal as u8);
    rec.extend_from_slice(&1u32.to_le_bytes()); // arg n-1
    rec.push(Opcode::Call as u8);
    rec.extend_from_slice(&1u32.to_le_bytes()); // argc
    rec.extend_from_slice(&1u32.to_le_bytes()); // dest local (scratch)
    rec.extend_from_slice(&76u32.to_le_bytes()); // resume byte offset
    rec.push(Opcode::LoadConst as u8);
    rec.extend_from_slice(&1i64.to_le_bytes());
    rec.push(Opcode::Return as u8);
    // Base case (pc 86): return 0.
    rec.push(Opcode::LoadConst as u8);
    rec.extend_from_slice(&0i64.to_le_bytes());
    rec.push(Opcode::Return as u8);
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&0i64.to_le_bytes()); // fn index 0: rec
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&3i64.to_le_bytes()); // seed n = 3
    main.push(Opcode::Call as u8);
    main.extend_from_slice(&1u32.to_le_bytes()); // argc
    main.extend_from_slice(&0u32.to_le_bytes()); // dest local
    main.extend_from_slice(&31u32.to_le_bytes()); // resume byte offset
    main.push(Opcode::LoadLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Return as u8);
    Module::new(vec![Function::new(rec, 2, 1), Function::new(main, 1, 0)], 1)
}

/// main (fn 0) calls function index 5, which the module does not carry —
/// the call dispatch faults before any callee step exists, so the call is
/// counted but no edge can name it.
fn unresolved_call_module() -> Module {
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

fn main() {
    // 1. The recursive run: attach the collector through the trace hook
    //    exactly like any other sink, recover it typed afterwards.
    let module = recursion_module();
    let mut vm = Vm::new().with_trace(Box::new(CallGraphCollector::new()));
    let value = vm
        .run_module(&module)
        .expect("the module runs (the seam is read-only)");
    let mut boxed = vm.take_trace().expect("sink still attached");
    let graph = boxed
        .as_any()
        .downcast_ref::<CallGraphCollector>()
        .expect("the collector came back")
        .clone();
    println!("rec(3) returned {value:?} (unwatched result, seam is read-only)");
    println!(
        "calls={} steps={} unresolved={}",
        graph.calls(),
        graph.steps(),
        graph.unresolved_calls(),
    );
    println!("{}", render_call_graph(graph.edges(), graph.calls()));

    // 2. The graph is a pure function of the step stream: re-fold a
    //    captured TraceCollector transcript post-hoc into the identical
    //    map (a shared transcript feeds every aggregate seam).
    let transcript = TraceCollector::new();
    let mut vm = Vm::new().with_trace(Box::new(transcript));
    let _ = vm.run_module(&module).expect("transcript run");
    let mut boxed = vm.take_trace().expect("sink still attached");
    let steps = boxed
        .as_any()
        .downcast_ref::<TraceCollector>()
        .expect("the collector came back")
        .steps()
        .to_vec();
    let refolded = CallGraphCollector::from_steps(&steps);
    println!(
        "from_steps over {} captured steps: edges equal = {} (render equal = {})",
        steps.len(),
        refolded.edges() == graph.edges(),
        render_call_graph(refolded.edges(), refolded.calls())
            == render_call_graph(graph.edges(), graph.calls()),
    );
    println!(
        "readers over the recursion graph: callees_of(0)={:?} callers_of(0)={:?}",
        graph.callees_of(0),
        graph.callers_of(0),
    );

    // 3. An out-of-range callee: the call dispatches, faults, and the run
    //    ends — the call is counted, the edge cannot be, and the render
    //    says `unresolved-calls 1` instead of dropping it silently.
    let faulting = unresolved_call_module();
    let mut vm = Vm::new().with_trace(Box::new(CallGraphCollector::new()));
    let outcome = vm.run_module(&faulting);
    let mut boxed = vm.take_trace().expect("sink still attached");
    let graph = boxed
        .as_any()
        .downcast_ref::<CallGraphCollector>()
        .expect("the collector came back")
        .clone();
    println!("\nmodule calling fn 5 (out of range): {outcome:?}");
    println!("{}", render_call_graph(graph.edges(), graph.calls()));
}
