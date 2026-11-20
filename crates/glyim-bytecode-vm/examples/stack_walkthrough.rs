//! Operand-stack walkthrough: run a decoded module with the stack seam
//! attached, then read what values moved through the machine's operand
//! stack.
//!
//! The trace seam answers *where* a run went and the watch seam what its
//! local slots *took*; this seam answers the movement question — what did
//! the operand stack actually *carry*? This walkthrough attaches a
//! `StackCollector` to a `Vm`, runs the two-function module the other VM
//! walkthroughs use, renders the captured movements (`render_stack`),
//! reads the two faces back with `pushed_events`/`popped_events` and the
//! folds (`pushed_count`/`popped_count`/`net_growth`/`max_depth_seen`),
//! and then shows the seam's honesty rules in action: a bounded collector
//! marks overflow instead of growing without limit, a failed pop (a binop
//! reaching for a missing operand) reports nothing, a unit return moves
//! nothing, and `TracingStackSink` emits one `tracing::debug!` event per
//! movement for the workspace-standard observability layer. Run with:
//!
//! ```text
//! cargo run -p glyim-bytecode-vm --example stack_walkthrough
//! ```

use glyim_bytecode_vm::{
    Function, Module, Opcode, Popped, Pushed, StackCollector, StackEvent, TracingStackSink, Value,
    Vm, decode_module, encode_module, render_stack,
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

/// `main()`: returns `helper(21)` — the cross-function `OP_CALL` shape
/// whose ABI movements (the argument pop and the call-target pop, the
/// retval pop) the stack seam records alongside the callee's arithmetic.
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
    // The image round trip composes with the seam: ship the bytes, decode
    // them back, then watch the loaded module's operand stream.
    let module = Module::with_strings(vec![helper(), main_fn()], vec!["abc".to_string()], 1);
    let bytes = encode_module(&module);
    let loaded = decode_module(&bytes).expect("the artifact we just encoded decodes");

    let collector = StackCollector::new();
    let mut vm = Vm::new().with_stack(Box::new(collector));
    let value = vm.run_module(&loaded).expect("the watched run succeeds");
    let mut boxed = vm.take_stack().expect("the sink is still attached");
    let events: Vec<_> = boxed
        .as_any()
        .downcast_ref::<StackCollector>()
        .expect("the collector came back through as_any")
        .clone()
        .into_events();

    println!("result: {value:?} (helper(21) — the stack seam is read-only)");
    println!();
    print!("{}", render_stack(&events, &loaded));

    // The movement counterpart of the trace listing: the call's two pops
    // stand out (the argument 21 and the call target 0), as does the
    // callee's mul (pop 21, pop 2, push 42). Each movement names the
    // producing dispatch, so it pairs with the trace's step of the same
    // number — and the stream folds back: 6 pushes, 6 pops, net zero, a
    // high-water mark of 2.
    println!();
    let collector = StackCollector::new();
    let mut vm = Vm::new().with_stack(Box::new(collector));
    let _ = vm.run_module(&loaded).expect("run");
    let mut boxed = vm.take_stack().expect("attached");
    let c = boxed
        .as_any()
        .downcast_ref::<StackCollector>()
        .expect("collector");
    println!(
        "folds: pushed {} / popped {} / net growth {} / max depth {}",
        c.pushed_count(),
        c.popped_count(),
        c.net_growth(),
        c.max_depth_seen()
    );

    // A movement-heavy program: every iteration moves twelve operands.
    let mut code = Vec::new();
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&3i64.to_le_bytes());
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
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
    let countdown = Module::new(vec![Function::with_blocks(code, 1, 1, vec![head])], 0);
    let collector = StackCollector::new();
    let mut vm = Vm::new().with_stack(Box::new(collector));
    let value = vm.run_module(&countdown).expect("the countdown runs");
    let mut boxed = vm.take_stack().expect("attached");
    let c = boxed
        .as_any()
        .downcast_ref::<StackCollector>()
        .expect("collector");
    println!(
        "countdown result {value:?}; {} movements captured (max depth {})",
        c.len(),
        c.max_depth_seen()
    );

    // A bounded collector marks overflow instead of growing without limit
    // — a loop moving operands every dispatch must not OOM the embedding
    // host. The flag is a displayed fact, never a silent drop.
    let bounded = StackCollector::bounded(2);
    let mut vm = Vm::new().with_stack(Box::new(bounded));
    let _ = vm.run_module(&countdown).expect("runs");
    let mut boxed = vm.take_stack().expect("attached");
    let c = boxed
        .as_any()
        .downcast_ref::<StackCollector>()
        .expect("collector");
    println!(
        "bounded(2): captured {} movements, overflowed={}",
        c.len(),
        c.overflowed()
    );

    // A pop that fails — a binop reaching for a second operand that is
    // not there — pops nothing and reports nothing: the seam is total
    // over the *executed* movements, and the fault stream owns the
    // underflow.
    let mut code = Vec::new();
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&1i64.to_le_bytes());
    code.push(Opcode::Add as u8); // one operand on the stack, two needed
    let module = Module::new(vec![Function::new(code, 0, 0)], 0);
    let mut vm = Vm::new().with_stack(Box::new(StackCollector::new()));
    let err = vm
        .run_module(&module)
        .expect_err("the second pop underflows");
    let mut boxed = vm.take_stack().expect("attached");
    let c = boxed
        .as_any()
        .downcast_ref::<StackCollector>()
        .expect("collector");
    println!(
        "underflow: {err:?}, captured {} movements (the failed pop is silent)",
        c.len()
    );

    // A unit-returning function's `Return` finds an empty stack and
    // substitutes the machine's `Unit` encoding without a movement — the
    // stream stays silent there.
    let module = Module::new(vec![Function::new(vec![Opcode::Return as u8], 0, 0)], 0);
    let mut vm = Vm::new().with_stack(Box::new(StackCollector::new()));
    let value = vm.run_module(&module).expect("unit return runs");
    let mut boxed = vm.take_stack().expect("attached");
    let c = boxed
        .as_any()
        .downcast_ref::<StackCollector>()
        .expect("collector");
    println!(
        "unit return: {value:?}, {} movements (the substituted Unit never rides the stack)",
        c.len()
    );

    // `TracingStackSink` rides the workspace-standard `tracing` layer: one
    // `debug!` event per movement, visible to any subscriber that enables
    // it. The run itself is unchanged.
    let mut vm = Vm::new().with_stack(Box::new(TracingStackSink));
    let value = vm.run_module(&loaded).expect("runs");
    assert_eq!(value, Value::Int(42));
    println!("TracingStackSink run: {value:?} (one debug! event per movement)");

    // The two faces can also be built by hand and delivered directly —
    // the event types are plain data, so a custom sink owns its shape.
    let _ = (
        Pushed {
            func: 0,
            pc: 0,
            dispatch: 1,
            frame_depth: 1,
            depth: 1,
            value: Value::Int(1),
        },
        Popped {
            func: 0,
            pc: 0,
            dispatch: 2,
            frame_depth: 1,
            depth: 0,
            value: Value::Int(1),
        },
        StackEvent::Pushed(Pushed {
            func: 0,
            pc: 0,
            dispatch: 3,
            frame_depth: 1,
            depth: 1,
            value: Value::Int(2),
        }),
    );
    println!("done.");
}
