//! Integer-arithmetic walkthrough: run arithmetic-heavy and
//! wrap-shaped modules with the math seam attached, then read what the
//! machine computed, at which sites, and where the wrapping semantics
//! engaged.
//!
//! The machine's integer faces — the `(Int, Int)` binop arm, `OP_NEG`,
//! and `OP_NOT` — run in the documented wrapping semantics:
//! `i64::MAX + 1` answers `i64::MIN` (never a fault), shifts mask
//! their shift amount, and a wrapped answer flows into comparisons
//! and branches exactly like an exact one. This walkthrough attaches a
//! `MathCollector` to a `Vm`, runs the two-function module the other
//! VM walkthroughs use (one exact `Mul`), renders the captured
//! transcript (`render_math`), then shows the wrap verdicts in action:
//! `i64::MAX - (-1)` wrapping to `i64::MIN`, the negation of
//! `i64::MIN` wrapping to itself, a shift amount masked by
//! `wrapping_shl`, the per-opcode and per-function wraps folds, a
//! `(Float, Float)` binop staying on the float seam's face, the
//! trapping division reporting nothing, a bounded capture marking
//! overflow, and `TracingMathSink` emitting one `tracing::debug!`
//! event per integer operation for the workspace-standard
//! observability layer. Run with:
//!
//! ```text
//! cargo run -p glyim-bytecode-vm --example math_walkthrough
//! ```

use glyim_bytecode_vm::{
    Function, MathCollector, Module, Opcode, TracingMathSink, Value, Vm, decode_module,
    encode_module, render_math,
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

/// `main()`: returns `helper(21)` — 9 dispatches total (5 in main, 4 in
/// the helper), the two-function module the other VM walkthroughs use.
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

/// main (fn 0) computes `i64::MAX - (-1)` — the canonical wrapped
/// subtraction — then negates `i64::MIN` (the one wrapped negation)
/// and tests it with `OP_NOT`: the wrap-kernel shape.
fn wrap_kernel() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&i64::MAX.to_le_bytes());
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&(-1i64).to_le_bytes());
    main.push(Opcode::Sub as u8);
    main.push(Opcode::Neg as u8);
    main.push(Opcode::Not as u8);
    main.push(Opcode::Return as u8);
    Module::new(vec![Function::new(main, 0, 0)], 0)
}

/// main (fn 0) shifts 1 by 70 — the shift amount is masked to 6 by
/// `wrapping_shl` — the masked-shift shape.
fn shift_kernel() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&1i64.to_le_bytes());
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&70i64.to_le_bytes());
    main.push(Opcode::Shl as u8);
    main.push(Opcode::Return as u8);
    Module::new(vec![Function::new(main, 0, 0)], 0)
}

/// main (fn 0) divides 1 by 0 — the trapping division, which the fault
/// stream owns: the math stream reports nothing.
fn trap_kernel() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&1i64.to_le_bytes());
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&0i64.to_le_bytes());
    main.push(Opcode::Div as u8);
    main.push(Opcode::Return as u8);
    Module::new(vec![Function::new(main, 0, 0)], 0)
}

fn main() {
    // The image round trip composes with the seam: ship the bytes,
    // decode them back, then capture the loaded module's arithmetic.
    let module = Module::with_strings(vec![helper(), main_fn()], vec!["abc".to_string()], 1);
    let bytes = encode_module(&module);
    let loaded = decode_module(&bytes).expect("the artifact we just encoded decodes");

    // The exact binop: helper(21) runs one `(Int, Int)` Mul — exact, so
    // the wrap tally stays at zero and the per-opcode fold names it.
    let collector = MathCollector::new();
    let mut vm = Vm::new().with_math(Box::new(collector));
    let value = vm.run_module(&loaded).expect("the golden module runs");
    let mut boxed = vm.take_math().expect("the sink is still attached");
    let collector = boxed
        .as_any()
        .downcast_ref::<MathCollector>()
        .expect("the collector came back through as_any")
        .clone();
    println!("result: {value:?} (helper(21) — one exact Mul on the stream)");
    println!();
    print!("{}", render_math(&collector));
    println!();
    let mut counts = std::collections::BTreeMap::new();
    counts.insert("Mul", 1u64);
    assert_eq!(collector.count_by_opcode(), counts);
    assert_eq!(
        collector.wrapped_by_function(),
        std::collections::BTreeMap::new()
    );
    println!(
        "per-opcode: {:?}; wraps: {} (the Mul was exact)",
        collector.count_by_opcode(),
        collector.wraps(),
    );
    println!();

    // The wrap kernel: `i64::MAX - (-1)` answers `i64::MIN` by the
    // wrapping semantics, `-i64::MIN` answers `i64::MIN`, and the
    // `OP_NOT` bools the wrapped answer — two wrapped events, one not.
    let (collector, value) = {
        let collector = MathCollector::new();
        let mut vm = Vm::new().with_math(Box::new(collector));
        let value = vm.run_module(&wrap_kernel()).expect("runs");
        let mut boxed = vm.take_math().expect("attached");
        let c = boxed
            .as_any()
            .downcast_ref::<MathCollector>()
            .expect("collector")
            .clone();
        (c, value)
    };
    println!("wrap kernel result: {value:?} (not(i64::MIN) is 0)");
    println!();
    print!("{}", render_math(&collector));
    println!();
    let mut wraps = std::collections::BTreeMap::new();
    wraps.insert(0usize, 2u64);
    assert_eq!(collector.wrapped_by_function(), wraps);
    println!(
        "wraps: {} (the sub and the neg); per function: {:?}",
        collector.wraps(),
        collector.wrapped_by_function(),
    );
    println!();

    // A masked shift: `1 << 70` shifts by `70 & 63 = 6` — the answer is
    // the masked shift's, and the verdict says so.
    let (collector, value) = {
        let collector = MathCollector::new();
        let mut vm = Vm::new().with_math(Box::new(collector));
        let value = vm.run_module(&shift_kernel()).expect("runs");
        let mut boxed = vm.take_math().expect("attached");
        let c = boxed
            .as_any()
            .downcast_ref::<MathCollector>()
            .expect("collector")
            .clone();
        (c, value)
    };
    println!("masked shift result: {value:?} (1 << (70 & 63))");
    println!();
    print!("{}", render_math(&collector));
    println!();

    // The trapping division is the fault stream's, not the math
    // stream's: the run faults and the capture stays empty.
    let collector = MathCollector::new();
    let mut vm = Vm::new().with_math(Box::new(collector));
    let err = vm.run_module(&trap_kernel()).expect_err("1 / 0 traps");
    let mut boxed = vm.take_math().expect("attached");
    let c = boxed
        .as_any()
        .downcast_ref::<MathCollector>()
        .expect("collector");
    println!("trapping division: {err}; math events: {}", c.len());
    assert!(c.is_empty());
    println!();

    // A bounded capture marks overflow instead of growing without
    // limit — a loop reporting every arithmetic step must not OOM the
    // embedding host.
    let bounded = MathCollector::bounded(2);
    let mut vm = Vm::new().with_math(Box::new(bounded));
    let _ = vm.run_module(&wrap_kernel()).expect("runs");
    let mut boxed = vm.take_math().expect("attached");
    let c = boxed
        .as_any()
        .downcast_ref::<MathCollector>()
        .expect("collector");
    println!(
        "bounded(2): captured {} events, overflowed={}",
        c.len(),
        c.overflowed()
    );
    println!();

    // `TracingMathSink` rides the workspace-standard `tracing` layer:
    // one `debug!` event per integer operation, visible to any
    // subscriber that enables it. The run itself is unchanged.
    let mut vm = Vm::new().with_math(Box::new(TracingMathSink));
    let value = vm.run_module(&loaded).expect("runs");
    assert_eq!(value, Value::Int(42));
    println!("TracingMathSink run: {value:?} (one debug! event per integer operation)");
}
