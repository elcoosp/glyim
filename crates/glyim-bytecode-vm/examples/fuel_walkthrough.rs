//! Fuel walkthrough: run budgeted and unbounded modules with the fuel
//! seam attached, then read where the execution budget actually went.
//!
//! The fuel gate (`Vm::with_fuel`) stops an infinite loop in a loaded
//! artifact with the explicit `VmError::FuelExhausted` instead of hanging
//! the embedding host — but the budget itself was observable only through
//! its refusal. This walkthrough attaches a `FuelCollector` to a `Vm`,
//! runs the two-function module the other VM walkthroughs use under an
//! exact-fit budget, renders the captured diary (`render_fuel`), reads
//! the burn folds (`burn_by_function`, `last_tick`), and then shows the
//! seam's honesty rules in action: an exhausted budget's diary names
//! exactly how far it got, an unbounded run still ticks (`remaining` is
//! `None`), a bounded collector marks overflow instead of growing without
//! limit, and `TracingFuelSink` emits one `tracing::debug!` event per
//! dispatch for the workspace-standard observability layer. Run with:
//!
//! ```text
//! cargo run -p glyim-bytecode-vm --example fuel_walkthrough
//! ```

use glyim_bytecode_vm::{
    FuelCollector, Function, Module, Opcode, TracingFuelSink, Value, Vm, decode_module,
    encode_module, render_fuel,
};
use std::collections::BTreeMap;

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

/// `main()`: returns `helper(21)` — 9 dispatches total (5 in main, 4 in
/// the helper), the exact-fit budget the walkthrough runs under.
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
    // them back, then tick the loaded module's budget.
    let module = Module::with_strings(vec![helper(), main_fn()], vec!["abc".to_string()], 1);
    let bytes = encode_module(&module);
    let loaded = decode_module(&bytes).expect("the artifact we just encoded decodes");

    // An exact-fit budget: 9 dispatches, 9 fuel — the run completes, the
    // budget descends to exactly zero, and the diary names each
    // function's share.
    let collector = FuelCollector::new();
    let mut vm = Vm::new().with_fuel(9).with_fuel_sink(Box::new(collector));
    let value = vm.run_module(&loaded).expect("9 dispatches fit in 9 fuel");
    let mut boxed = vm.take_fuel_sink().expect("the sink is still attached");
    let collector = boxed
        .as_any()
        .downcast_ref::<FuelCollector>()
        .expect("the collector came back through as_any")
        .clone();

    println!("result: {value:?} (helper(21) under an exact-fit budget of 9)");
    println!();
    print!("{}", render_fuel(&collector));

    // The burn folds answer the where-did-it-go question off the same
    // capture: the callee burned 4 dispatches, the caller 5, and the last
    // allowed dispatch was the return.
    println!();
    let mut burn = BTreeMap::new();
    burn.insert(0usize, 4u64);
    burn.insert(1usize, 5u64);
    assert_eq!(collector.burn_by_function(), burn);
    let last = collector.last_tick().expect("ticks exist");
    println!(
        "burn: {:?}; last allowed dispatch {} at fn {} pc {} (remaining {})",
        collector.burn_by_function(),
        last.dispatch,
        last.func,
        last.pc,
        last.remaining.expect("budgeted run"),
    );

    // A budget that runs out mid-loop: the error names the dispatch count,
    // the diary names the site — the third dispatch, the countdown's
    // first loop load.
    let mut code = Vec::new();
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&10i64.to_le_bytes());
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
    let collector = FuelCollector::new();
    let mut vm = Vm::new().with_fuel(3).with_fuel_sink(Box::new(collector));
    let err = vm
        .run_module(&countdown)
        .expect_err("the loop burns the budget");
    let mut boxed = vm.take_fuel_sink().expect("attached");
    let c = boxed
        .as_any()
        .downcast_ref::<FuelCollector>()
        .expect("collector");
    let last = c.last_tick().expect("ticks exist");
    println!(
        "exhausted budget: {err:?}; the diary's last tick is dispatch {} at fn {} pc {} — the refusal happened at the next dispatch",
        last.dispatch, last.func, last.pc
    );

    // An unbounded run still ticks: `remaining` is `None` per tick, and
    // the burn folds work unchanged — the seam is the budget face whether
    // or not a budget is set.
    let mut vm = Vm::new().with_fuel_sink(Box::new(FuelCollector::new()));
    let value = vm.run_module(&loaded).expect("unbounded runs");
    let mut boxed = vm.take_fuel_sink().expect("attached");
    let c = boxed
        .as_any()
        .downcast_ref::<FuelCollector>()
        .expect("collector");
    println!(
        "unbounded run: {value:?}, {} ticks (remaining = None per tick), burn {:?}",
        c.len(),
        c.burn_by_function()
    );

    // A bounded collector marks overflow instead of growing without limit
    // — a loop ticking every dispatch must not OOM the embedding host.
    let bounded = FuelCollector::bounded(2);
    let mut vm = Vm::new().with_fuel_sink(Box::new(bounded));
    let _ = vm.run_module(&countdown).expect("runs");
    let mut boxed = vm.take_fuel_sink().expect("attached");
    let c = boxed
        .as_any()
        .downcast_ref::<FuelCollector>()
        .expect("collector");
    println!(
        "bounded(2): captured {} ticks, overflowed={}",
        c.len(),
        c.overflowed()
    );

    // `TracingFuelSink` rides the workspace-standard `tracing` layer: one
    // `debug!` event per dispatch, visible to any subscriber that enables
    // it. The run itself is unchanged.
    let mut vm = Vm::new()
        .with_fuel(9)
        .with_fuel_sink(Box::new(TracingFuelSink));
    let value = vm.run_module(&loaded).expect("runs");
    assert_eq!(value, Value::Int(42));
    println!("TracingFuelSink run: {value:?} (one debug! event per dispatch)");
}
