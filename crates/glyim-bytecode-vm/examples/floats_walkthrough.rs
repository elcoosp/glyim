//! Floats walkthrough: run modules with the float-operation seam
//! attached, then read what every executed float operation computed —
//! the constant pushes, the `(Float, Float)` binops, the negations,
//! and the IEEE-754 specials traffic.
//!
//! Every other opcode class carries an observation seam; the float
//! stream was the last one without one. It is also the only arithmetic
//! the machine runs that *never faults*: integer division by zero
//! traps, but `1.0 / 0.0` is the documented convention (`+inf`,
//! matching LLVM's `fdiv` and the MIR interpreter), and `inf - inf` or
//! `0.0 * inf` land as NaN — a silently-poisoned value that flows into
//! comparisons (every NaN compare is false) and looks like a lowering
//! bug rather than the arithmetic it came from. The floats seam
//! (`FloatCollector` + `render_floats`) is that stream's counterpart:
//! the VM hands it one `FloatConst` per executed `OP_LOAD_CONST_F64`
//! push (the site and the constant), one `FloatBinop` per executed
//! `(Float, Float)` binop (the site, the opcode, the operands in full,
//! and the machine's answer — a `Float` for arithmetic, the machine's
//! `Int` bool for comparisons), and one `FloatNeg` per executed
//! `OP_NEG` over a float, and its `nan_results()` / `inf_results()`
//! folds keep the specials path visible instead of silently consumed.
//! A *refused* float op — a mixed `(Int, Float)` binop, a bitwise op
//! over floats — faults and reports nothing (the fault stream owns
//! it), while the constant pushes that executed before the refusal
//! still report. The hooks observe exactly like the other seams:
//! read-only (the answers are unchanged), total over *executed* float
//! operations, bounded capture with a displayed overflow flag, and a
//! deterministic render. This walkthrough runs a float kernel, lands
//! in both specials, reads the comparison and negation faces, counts
//! a float-heavy loop's traffic, reads the bounded capture, proves
//! the seam is read-only, and shows a refused pairing reporting
//! nothing. Run with:
//!
//! ```text
//! cargo run -p glyim-bytecode-vm --example floats_walkthrough
//! ```

use glyim_bytecode_vm::{FloatCollector, Function, Module, Opcode, Vm, render_floats};

/// main (fn 0) pushes 1.5 and 2.25 and multiplies them: the canonical
/// one-binop kernel.
fn kernel_module() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConstF64 as u8); // the left operand
    main.extend_from_slice(&1.5f64.to_bits().to_le_bytes());
    main.push(Opcode::LoadConstF64 as u8); // the right operand
    main.extend_from_slice(&2.25f64.to_bits().to_le_bytes());
    main.push(Opcode::Mul as u8);
    main.push(Opcode::Return as u8);
    Module::new(vec![Function::new(main, 0, 0)], 0)
}

/// main (fn 0) divides 1.0 by 0.0 (the `+inf` convention) and then
/// multiplies the inf by 0.0 (NaN): the specials shape.
fn specials_module() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConstF64 as u8); // the dividend
    main.extend_from_slice(&1.0f64.to_bits().to_le_bytes());
    main.push(Opcode::LoadConstF64 as u8); // the zero divisor
    main.extend_from_slice(&0.0f64.to_bits().to_le_bytes());
    main.push(Opcode::Div as u8); // +inf by convention
    main.push(Opcode::LoadConstF64 as u8); // the multiplier
    main.extend_from_slice(&0.0f64.to_bits().to_le_bytes());
    main.push(Opcode::Mul as u8); // NaN by convention
    main.push(Opcode::Return as u8);
    Module::new(vec![Function::new(main, 0, 0)], 0)
}

/// main (fn 0) compares 1.5 < 2.5: the comparison face — the machine
/// answers with its `Int` bool encoding.
fn comparison_module() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConstF64 as u8); // the left operand
    main.extend_from_slice(&1.5f64.to_bits().to_le_bytes());
    main.push(Opcode::LoadConstF64 as u8); // the right operand
    main.extend_from_slice(&2.5f64.to_bits().to_le_bytes());
    main.push(Opcode::Lt as u8);
    main.push(Opcode::Return as u8);
    Module::new(vec![Function::new(main, 0, 0)], 0)
}

/// main (fn 0) negates 1.5: the unary face — the IEEE negation.
fn neg_module() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConstF64 as u8); // the operand
    main.extend_from_slice(&1.5f64.to_bits().to_le_bytes());
    main.push(Opcode::Neg as u8);
    main.push(Opcode::Return as u8);
    Module::new(vec![Function::new(main, 0, 0)], 0)
}

/// main (fn 0) adds an `Int` to a `Float`: the *refused* shape — the
/// machine faults with `BinOpTypeMismatch`.
fn mixed_kind_module() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8); // the Int
    main.extend_from_slice(&7i64.to_le_bytes());
    main.push(Opcode::LoadConstF64 as u8); // the Float
    main.extend_from_slice(&2.5f64.to_bits().to_le_bytes());
    main.push(Opcode::Add as u8); // refused
    main.push(Opcode::Return as u8);
    Module::new(vec![Function::new(main, 0, 0)], 0)
}

/// main (fn 0) runs a float kernel in a countdown loop (`count`
/// iterations): the float-heavy shape — four float events per
/// iteration plus the accumulator's initial push.
fn float_loop_module(count: i64) -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8); // the counter
    main.extend_from_slice(&count.to_le_bytes());
    main.push(Opcode::StoreLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadConstF64 as u8); // the accumulator
    main.extend_from_slice(&0.0f64.to_bits().to_le_bytes());
    main.push(Opcode::StoreLocal as u8);
    main.extend_from_slice(&1u32.to_le_bytes());
    // Loop head (pc 28): the float kernel into local 1, decrement,
    // loop while the counter is still positive.
    main.push(Opcode::LoadConstF64 as u8);
    main.extend_from_slice(&1.5f64.to_bits().to_le_bytes());
    main.push(Opcode::LoadConstF64 as u8);
    main.extend_from_slice(&2.25f64.to_bits().to_le_bytes());
    main.push(Opcode::Mul as u8);
    main.push(Opcode::LoadLocal as u8);
    main.extend_from_slice(&1u32.to_le_bytes());
    main.push(Opcode::Add as u8);
    main.push(Opcode::StoreLocal as u8);
    main.extend_from_slice(&1u32.to_le_bytes());
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
    main.push(Opcode::Gt as u8);
    main.push(Opcode::JumpIf as u8);
    main.extend_from_slice(&28u32.to_le_bytes()); // loop head
    main.push(Opcode::LoadLocal as u8); // the accumulator
    main.extend_from_slice(&1u32.to_le_bytes());
    main.push(Opcode::Return as u8);
    Module::new(vec![Function::new(main, 2, 0)], 0)
}

/// Run a module with a fresh float collector attached and return the
/// typed collector.
fn floats_run(module: &Module) -> FloatCollector {
    let mut vm = Vm::new().with_floats(Box::new(FloatCollector::new()));
    let _ = vm.run_module(module);
    let mut boxed = vm.take_floats().expect("sink still attached");
    boxed
        .as_any()
        .downcast_ref::<FloatCollector>()
        .expect("the collector came back")
        .clone()
}

fn main() {
    // 1. The one-binop kernel: two constant pushes and a multiply,
    //    each line spelling its site, its operands, and the bit-exact
    //    answer.
    let module = kernel_module();
    let floats = floats_run(&module);
    println!("one float kernel:");
    println!("{}", render_floats(&floats));

    // 2. The specials shape: `1.0 / 0.0` is `+inf` by convention and
    //    `inf * 0.0` is NaN — never faults, and the specials folds
    //    keep the path visible.
    let specials = specials_module();
    let floats = floats_run(&specials);
    println!("the IEEE-754 specials (never faults):");
    println!("{}", render_floats(&floats));
    println!(
        "specials folds: {} NaN results, {} infinite results\n",
        floats.nan_results(),
        floats.inf_results()
    );

    // 3. The comparison face: the `(Float, Float)` arm answers with
    //    the machine's `Int` bool — reported with the operands, and
    //    the specials folds stay at zero.
    let comparison = comparison_module();
    let floats = floats_run(&comparison);
    println!("the comparison face (an Int answer):");
    println!("{}", render_floats(&floats));

    // 4. The unary face: the IEEE negation, bit-exact (`-0.0` is
    //    distinct from `0.0`).
    let neg = neg_module();
    let floats = floats_run(&neg);
    println!("the negation face:");
    println!("{}", render_floats(&floats));

    // 5. The float-heavy shape: a countdown loop runs the kernel per
    //    iteration — the tallies count every float operation.
    let loop_module = float_loop_module(3);
    let floats = floats_run(&loop_module);
    println!(
        "a 3-iteration float-heavy loop: {} events ({} pushes, {} binops, {} negs)\n",
        floats.events().len(),
        floats.const_count(),
        floats.binop_count(),
        floats.neg_count()
    );

    // 6. The bounded capture: a capped collector keeps the first
    //    events and flags the overflow — displayed, never a silent
    //    drop.
    let mut vm = Vm::new().with_floats(Box::new(FloatCollector::bounded(6)));
    let _ = vm.run_module(&loop_module).expect("module runs");
    let mut boxed = vm.take_floats().expect("sink still attached");
    let bounded = boxed
        .as_any()
        .downcast_ref::<FloatCollector>()
        .expect("the collector came back")
        .clone();
    println!(
        "bounded(6) over thirteen events: {} events kept, overflowed = {}\n",
        bounded.events().len(),
        bounded.overflowed()
    );

    // 7. The refused shape: a mixed `(Int, Float)` binop faults — no
    //    binop event (the fault stream owns the refusal), but the
    //    constant push that executed before it still reports.
    let mixed = mixed_kind_module();
    let outcome = Vm::new().run_module(&mixed);
    let floats = floats_run(&mixed);
    println!("the refused shape: {outcome:?}");
    println!(
        "captured {} events, {} binops — the refusal itself reports nothing\n",
        floats.events().len(),
        floats.binop_count()
    );

    // 8. The read-only proof: the traced run's result equals the
    //    untraced one exactly (the seam observes — it never changes
    //    execution).
    let untraced = Vm::new().run_module(&module).expect("untraced run");
    let traced = {
        let mut vm = Vm::new().with_floats(Box::new(FloatCollector::new()));
        let value = vm.run_module(&module).expect("traced run");
        let _ = vm.take_floats().expect("sink still attached");
        value
    };
    println!(
        "read-only: traced {traced:?} == untraced {untraced:?} — the seam observes, execution unchanged"
    );
}
