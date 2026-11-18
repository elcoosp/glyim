//! Fault-backtrace walkthrough: run a decoded module that faults, then read
//! where it failed and how it got there.
//!
//! The trace seam (`TraceCollector` + `render_trace`) answers "what did this
//! module *do*, step by step?"; the profile seam (`ProfileCollector` +
//! `render_profile`) answers "where did the dispatches *go*?". This
//! walkthrough exercises the failure-time counterpart (`BacktraceCollector` +
//! `render_backtrace`): when a run ends in a `VmError`, the VM hands one
//! `FaultReport` to the attached sink — the fault site (function, dispatch
//! pc, decoded opcode), the suspended caller chain (innermost first), the
//! dispatch count, and the fuel state — because the bare `VmError` carries
//! none of that. The walkthrough shows the seam's honesty rules: a
//! pre-execution fault reports the entry request with zero dispatches, the
//! fuel gate's refused dispatch is the reported site, and the fault site is
//! exactly the trace's last recorded step when both seams ride one run. Run
//! with:
//!
//! ```text
//! cargo run -p glyim-bytecode-vm --example fault_backtrace_walkthrough
//! ```

use glyim_bytecode_vm::{
    BacktraceCollector, Function, Module, Opcode, TraceCollector, Vm, decode_module, encode_module,
    render_backtrace, render_trace,
};

/// helper(n): traps. No jumps, so it carries no block-offset table (the
/// VM's raw byte-offset target mode).
fn helper() -> Function {
    let code = vec![Opcode::Trap as u8];
    Function::new(code, 2, 1)
}

/// `main()`: calls `helper(21)` — the cross-function `OP_CALL` shape, with
/// the caller suspended at byte 31 (after the call's operands) while the
/// callee faults.
fn main_fn() -> Function {
    let mut code = Vec::new();
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&0i64.to_le_bytes()); // fn index 0: helper
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&21i64.to_le_bytes());
    code.push(Opcode::Call as u8);
    code.extend_from_slice(&1u32.to_le_bytes()); // argc
    code.extend_from_slice(&0u32.to_le_bytes()); // dest local
    code.extend_from_slice(&31u32.to_le_bytes()); // resume byte offset
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::Return as u8);
    Function::new(code, 1, 0)
}

/// A self-calling function that overflows the call-depth guard — the deep
/// chain the renderer lists one caller per line.
fn self_recursion() -> Function {
    let mut code = Vec::new();
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&0i64.to_le_bytes()); // fn index 0: self
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&1i64.to_le_bytes());
    code.push(Opcode::Call as u8);
    code.extend_from_slice(&1u32.to_le_bytes()); // argc
    code.extend_from_slice(&0u32.to_le_bytes()); // dest local
    code.extend_from_slice(&31u32.to_le_bytes()); // resume byte offset
    code.push(Opcode::Return as u8);
    Function::new(code, 2, 1)
}

/// Run `module` with a backtrace collector attached and print the render +
/// the report's structured facts (the seam is read-only: the error the host
/// receives is the machine's own).
fn fault_and_report(module: &Module, label: &str) {
    let collector = BacktraceCollector::new();
    let mut vm = Vm::new().with_backtrace(Box::new(collector));
    let err = vm.run_module(module).expect_err(label);
    println!("--- {label} ---");
    println!("the caller received: {err}");
    let mut boxed = vm.take_backtrace().expect("the sink is still attached");
    let sink = boxed.as_any();
    let collector = sink
        .downcast_ref::<BacktraceCollector>()
        .expect("the collector came back");
    let report = collector.report().expect("the fault was captured");
    println!("{}", render_backtrace(report, module));
    println!(
        "structured: site = fn {} pc {} ({:?}), {} caller(s), {} dispatch(es), fuel {:?}",
        report.site.func,
        report.site.pc,
        report.site.opcode,
        report.callers.len(),
        report.executed,
        report.fuel_left,
    );
    println!();
}

fn main() {
    // The image round trip composes with the seam: ship the bytes, decode
    // them back, then fault the loaded module.
    let module = Module::with_strings(vec![helper(), main_fn()], vec!["abc".to_string()], 1);
    let bytes = encode_module(&module);
    let loaded = decode_module(&bytes).expect("the image decodes");
    fault_and_report(&loaded, "a trapped callee under a suspended caller");

    // A pre-execution fault: the entry index is out of range, so nothing
    // dispatched — the report says so instead of inventing an instruction.
    let bad_entry = Module::new(vec![helper(), main_fn()], 7);
    fault_and_report(&bad_entry, "a pre-execution fault (bad entry index)");

    // The fuel gate's refused dispatch is the reported site, and the header
    // shows the spent budget.
    let mut loop_code = Vec::new();
    loop_code.push(Opcode::Jump as u8);
    loop_code.extend_from_slice(&0u32.to_le_bytes());
    let looping = Module::new(vec![Function::with_blocks(loop_code, 0, 0, vec![0])], 0);
    let collector = BacktraceCollector::new();
    let mut vm = Vm::new().with_fuel(5).with_backtrace(Box::new(collector));
    let err = vm
        .run_module(&looping)
        .expect_err("the loop burns the budget");
    println!("--- the fuel gate stops an infinite loop ---");
    println!("the caller received: {err}");
    let mut boxed = vm.take_backtrace().expect("the sink is still attached");
    let sink = boxed.as_any();
    let collector = sink
        .downcast_ref::<BacktraceCollector>()
        .expect("the collector came back");
    println!(
        "{}",
        render_backtrace(collector.report().expect("captured"), &looping)
    );
    println!();

    // The deep chain: the call-depth overflow lists every suspended caller,
    // innermost first (truncated here to keep the walkthrough readable).
    let recursion = Module::new(vec![self_recursion()], 0);
    let collector = BacktraceCollector::new();
    let mut vm = Vm::new().with_backtrace(Box::new(collector));
    let err = vm
        .run_module(&recursion)
        .expect_err("the recursion overflows the call-depth guard");
    let mut boxed = vm.take_backtrace().expect("the sink is still attached");
    let sink = boxed.as_any();
    let collector = sink
        .downcast_ref::<BacktraceCollector>()
        .expect("the collector came back");
    let report = collector.report().expect("the fault was captured");
    println!(
        "--- the deep chain ({} suspended callers) ---",
        report.callers.len()
    );
    println!("the caller received: {err}");
    let listing = render_backtrace(report, &recursion);
    let head: Vec<&str> = listing.lines().take(5).collect();
    println!(
        "{}\n  ... ({} caller lines total)\n",
        head.join("\n"),
        report.callers.len()
    );

    // Cross-seam consistency: with a trace collector riding the same run,
    // the backtrace's fault site is the trace's last recorded step (the
    // faulting dispatch) — two views of one machine, no drift.
    let trace = TraceCollector::new();
    let backtrace = BacktraceCollector::new();
    let mut vm = Vm::new()
        .with_trace(Box::new(trace))
        .with_backtrace(Box::new(backtrace));
    let _ = vm.run_module(&loaded).unwrap_err();
    let mut boxed = vm.take_trace().expect("the trace sink is still attached");
    let last = boxed
        .as_any()
        .downcast_ref::<TraceCollector>()
        .expect("the trace collector came back")
        .steps()
        .last()
        .expect("the faulting dispatch is traced")
        .clone();
    let transcript = render_trace(
        boxed
            .as_any()
            .downcast_ref::<TraceCollector>()
            .expect("the trace collector came back")
            .steps(),
        &loaded,
    );
    let mut boxed = vm
        .take_backtrace()
        .expect("the backtrace sink is still attached");
    let sink = boxed.as_any();
    let backtrace = sink
        .downcast_ref::<BacktraceCollector>()
        .expect("the collector came back");
    let report = backtrace.report().expect("the fault was captured");
    println!("--- one run, two seams ---");
    println!(
        "trace's last step: fn {} pc {} | backtrace site: fn {} pc {} (same dispatch)",
        last.func, last.pc, report.site.func, report.site.pc
    );
    println!("the trace's tail:");
    println!("{}", transcript.lines().last().unwrap_or(""));
    println!(
        "the backtrace's fault line:\n  {}",
        render_backtrace(report, &loaded)
            .lines()
            .nth(1)
            .unwrap_or("")
    );
}
