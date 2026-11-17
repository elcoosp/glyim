//! Execution-trace walkthrough: run a decoded module with the trace seam
//! attached, then render what actually executed.
//!
//! The disassembler (`render_module`) lists what a decoded artifact
//! *carries*; the trace seam answers the dynamic question — what did this
//! module *do*? This walkthrough attaches a `TraceCollector` to a `Vm`,
//! runs the two-function module the other VM walkthroughs use, renders the
//! captured steps (`render_trace`), and then shows the seam's two honesty
//! rules in action: a bounded collector marks overflow instead of growing
//! without limit, and `TracingSink` emits one `tracing::debug!` event per
//! dispatch for the workspace-standard observability layer. Run with:
//!
//! ```text
//! cargo run -p glyim-bytecode-vm --example execution_trace_walkthrough
//! ```

use glyim_bytecode_vm::{
    Function, Module, Opcode, StepDetail, TraceCollector, TracingSink, Value, Vm, decode_module,
    encode_module, render_trace,
};
/// helper(n): returns `n * 2`. No jumps, so it carries no block-offset
/// table (the VM's raw byte-offset target mode).
fn helper() -> Function {
    let mut code = Vec::new();
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&2i64.to_le_bytes());
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&1u32.to_le_bytes()); // MIR ABI: arg at local 1
    code.push(Opcode::Mul as u8);
    code.push(Opcode::Return as u8);
    Function::new(code, 2, 1)
}

/// `main()`: returns `helper(21)` — the cross-function `OP_CALL` shape whose
/// frame-depth transition the trace records.
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

fn main() {
    // The image round trip composes with the trace: ship the bytes, decode
    // them back, then trace the loaded module.
    let module = Module::with_strings(vec![helper(), main_fn()], vec!["abc".to_string()], 1);
    let bytes = encode_module(&module);
    let loaded = decode_module(&bytes).expect("the artifact we just encoded decodes");

    let collector = TraceCollector::new();
    let mut vm = Vm::new().with_trace(Box::new(collector));
    let value = vm.run_module(&loaded).expect("the traced run succeeds");
    let mut boxed = vm.take_trace().expect("the sink is still attached");
    let steps: Vec<_> = boxed
        .as_any()
        .downcast_ref::<TraceCollector>()
        .expect("the collector came back through as_any")
        .steps()
        .to_vec();

    println!("result: {value:?} (helper(21) — the trace is read-only)");
    println!();
    print!("{}", render_trace(&steps, &loaded));

    // The dynamic counterpart of the disasm listing: the executed path
    // skips the un-executed instructions entirely, and the cross-function
    // call shows up as the frame-depth transition 1 -> 2 -> 1.
    let depths: Vec<usize> = steps.iter().map(|s| s.frame_depth).collect();
    println!();
    println!("frame-depth transitions: {depths:?}");
    let call = steps
        .iter()
        .find(|s| {
            s.detail
                == StepDetail::Call {
                    argc: 1,
                    dest: 0,
                    target: 31,
                }
        })
        .expect("the call step is recorded");
    println!(
        "call step: fn {} pc {} stack={}",
        call.func, call.pc, call.stack_depth
    );

    // A bounded collector marks overflow instead of growing without limit —
    // an infinite loop must not OOM the embedding host. The flag is a
    // displayed fact, never a silent drop.
    let bounded = TraceCollector::bounded(3);
    let mut vm = Vm::new().with_trace(Box::new(bounded));
    let _ = vm.run_module(&loaded).expect("runs");
    let mut boxed = vm.take_trace().expect("attached");
    let c = boxed
        .as_any()
        .downcast_ref::<TraceCollector>()
        .expect("collector");
    println!();
    println!(
        "bounded(3): captured {} of 9 steps, overflowed={}",
        c.len(),
        c.overflowed()
    );

    // The fuel seam is the runtime complement of the image decode's
    // structural validation: an infinite loop in a loaded artifact stops at
    // the budget with the explicit `FuelExhausted` error instead of hanging
    // the host.
    let mut code = Vec::new();
    code.push(Opcode::Jump as u8);
    code.extend_from_slice(&0u32.to_le_bytes()); // self-jump: b0 -> b0
    let spinner = Module::new(vec![Function::with_blocks(code, 0, 0, vec![0])], 0);
    let mut vm = Vm::new().with_fuel(500);
    match vm.run_module(&spinner) {
        Err(glyim_bytecode_vm::VmError::FuelExhausted { executed }) => {
            println!("infinite loop stopped by fuel: executed={executed}");
        }
        _ => unreachable!("a self-jump loop cannot halt"),
    }

    // `TracingSink` rides the workspace-standard `tracing` layer: one
    // `debug!` event per dispatch, visible to any subscriber that enables
    // it. The run itself is unchanged.
    let mut vm = Vm::new().with_trace(Box::new(TracingSink));
    let value = vm.run_module(&loaded).expect("runs");
    assert_eq!(value, Value::Int(42));
    println!("TracingSink run: {value:?}");
}
