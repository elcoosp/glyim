//! Local-watch walkthrough: run a decoded module with the watch seam
//! attached, then read what values its local slots took.
//!
//! The trace seam answers *where* a run went; this seam answers the data
//! question — what did the locals *hold*? This walkthrough attaches a
//! `WatchCollector` to a `Vm`, runs the two-function module the other VM
//! walkthroughs use, renders the captured writes (`render_watch`), reads
//! one watched slot's history with `slot_history`, and then shows the
//! seam's honesty rules in action: a bounded collector marks overflow
//! instead of growing without limit, a failed (out-of-bounds) store
//! reports nothing, and `TracingWatchSink` emits one `tracing::debug!`
//! event per write for the workspace-standard observability layer. Run
//! with:
//!
//! ```text
//! cargo run -p glyim-bytecode-vm --example local_watch_walkthrough
//! ```

use glyim_bytecode_vm::{
    Function, Module, Opcode, TracingWatchSink, Value, Vm, WatchCollector, WatchVia, decode_module,
    encode_module, render_watch,
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
/// whose ABI writes (the argument bind and the callee-return dest write)
/// the watch records.
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
    // The image round trip composes with the watch: ship the bytes, decode
    // them back, then watch the loaded module.
    let module = Module::with_strings(vec![helper(), main_fn()], vec!["abc".to_string()], 1);
    let bytes = encode_module(&module);
    let loaded = decode_module(&bytes).expect("the artifact we just encoded decodes");

    let collector = WatchCollector::new();
    let mut vm = Vm::new().with_watch(Box::new(collector));
    let value = vm.run_module(&loaded).expect("the watched run succeeds");
    let mut boxed = vm.take_watch().expect("the sink is still attached");
    let events: Vec<_> = boxed
        .as_any()
        .downcast_ref::<WatchCollector>()
        .expect("the collector came back through as_any")
        .clone()
        .into_events();

    println!("result: {value:?} (helper(21) — the watch is read-only)");
    println!();
    print!("{}", render_watch(&events, &loaded));

    // The data counterpart of the trace listing: the ABI writes stand out —
    // the call binds 21 into the callee's local 1, and the return lands 42
    // in the caller's dest local 0. Each event names the producing
    // dispatch, so the write pairs with the trace's step of the same
    // number.
    println!();
    for event in &events {
        println!(
            "via {} -> fn {} local {} (frame depth {}): {:?}",
            event.via.name(),
            event.func,
            event.local,
            event.frame_depth,
            event.value
        );
    }

    // A store-heavy program: the watched slot's descending history reads
    // back in one call.
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
    let collector = WatchCollector::new();
    let mut vm = Vm::new().with_watch(Box::new(collector));
    let value = vm.run_module(&countdown).expect("the countdown runs");
    let mut boxed = vm.take_watch().expect("attached");
    let c = boxed
        .as_any()
        .downcast_ref::<WatchCollector>()
        .expect("collector");
    println!();
    println!(
        "countdown result {value:?}; local 0's history: {:?} ({} writes)",
        c.slot_history(0, 0)
            .into_iter()
            .map(|v| match v {
                Value::Int(n) => n.to_string(),
                other => format!("{other:?}"),
            })
            .collect::<Vec<_>>(),
        c.len()
    );

    // A bounded collector marks overflow instead of growing without limit —
    // an infinite loop writing a slot per dispatch must not OOM the
    // embedding host. The flag is a displayed fact, never a silent drop.
    let bounded = WatchCollector::bounded(2);
    let mut vm = Vm::new().with_watch(Box::new(bounded));
    let _ = vm.run_module(&countdown).expect("runs");
    let mut boxed = vm.take_watch().expect("attached");
    let c = boxed
        .as_any()
        .downcast_ref::<WatchCollector>()
        .expect("collector");
    println!(
        "bounded(2): captured {} of 4 writes, overflowed={}",
        c.len(),
        c.overflowed()
    );

    // A failed (out-of-bounds) store writes nothing and reports nothing:
    // the seam is total over the *successful* writes.
    let bad = vec![Opcode::LoadConst as u8, 1, 0, 0, 0, 0, 0, 0, 0];
    let mut code = Vec::new();
    code.extend_from_slice(&bad);
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&99u32.to_le_bytes());
    let module = Module::new(vec![Function::new(code, 0, 0)], 0);
    let mut vm = Vm::new().with_watch(Box::new(WatchCollector::new()));
    let err = vm
        .run_module(&module)
        .expect_err("local 99 is out of bounds");
    let mut boxed = vm.take_watch().expect("attached");
    let c = boxed
        .as_any()
        .downcast_ref::<WatchCollector>()
        .expect("collector");
    println!(
        "out-of-bounds store: {err:?}, captured {} writes (the failed write is silent)",
        c.len()
    );

    // `TracingWatchSink` rides the workspace-standard `tracing` layer: one
    // `debug!` event per local write, visible to any subscriber that
    // enables it. The run itself is unchanged.
    let mut vm = Vm::new().with_watch(Box::new(TracingWatchSink));
    let value = vm.run_module(&loaded).expect("runs");
    assert_eq!(value, Value::Int(42));
    println!(
        "TracingWatchSink run: {value:?} (via {} writes)",
        WatchVia::CalleeReturn.name()
    );
}
