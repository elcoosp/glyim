//! Float-execution timing harness for the bytecode VM's float seam.
//!
//! Dependency-free (`harness = false`), mirroring the `module_image` bench
//! in this crate: only `std::time::Instant` + `std::hint::black_box`. The
//! float dispatch path (`OP_LOAD_CONST_F64` constants, `(Float, Float)`
//! binop arms, float→int casts) costs a value-kind check per operation that
//! the integer path does not, so the seam's per-op overhead must stay small
//! against the interpreter's native float ops. Run with:
//!
//! ```text
//! cargo bench -p glyim-bytecode-vm --bench float_exec
//! ```

use glyim_bytecode_vm::{CastTarget, Function, Module, Opcode, Value};
use std::hint::black_box;
use std::time::Instant;

/// The measured corpus: a single function whose body loops `iterations`
/// times over a float kernel — an `OP_LOAD_CONST_F64` constant load, a float
/// multiply and add into an accumulator local, an int loop counter, and one
/// `FloatToInt` cast at the end — the shape the emitter emits for float
/// arithmetic in real programs.
fn float_kernel_module(iterations: i64) -> Module {
    let mut code = Vec::new();
    let mut blocks = Vec::new();
    // Prologue: the accumulator must start as a *float* (locals are
    // zero-initialised to Int(0), and `(Float, Int)` Add is a typed error,
    // not a silent coercion).
    code.push(Opcode::LoadConstF64 as u8);
    code.extend_from_slice(&0.0f64.to_le_bytes());
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    // Blocks: 0 = loop head, 1 = loop body, 2 = exit.
    blocks.push(code.len()); // block 0: loop head
    // if counter < iterations -> block 1 else -> block 2
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&1u32.to_le_bytes()); // counter
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&iterations.to_le_bytes());
    code.push(Opcode::Lt as u8);
    code.push(Opcode::JumpIf as u8);
    code.extend_from_slice(&1u32.to_le_bytes()); // body
    code.push(Opcode::Jump as u8);
    code.extend_from_slice(&2u32.to_le_bytes()); // exit

    blocks.push(code.len()); // block 1: float kernel
    // acc = acc + 1.5 * 2.25 (a float const load + two float binops)
    code.push(Opcode::LoadConstF64 as u8);
    code.extend_from_slice(&1.5f64.to_le_bytes());
    code.push(Opcode::LoadConstF64 as u8);
    code.extend_from_slice(&2.25f64.to_le_bytes());
    code.push(Opcode::Mul as u8);
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes()); // acc
    code.push(Opcode::Add as u8);
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    // counter = counter + 1
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&1u32.to_le_bytes());
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&1i64.to_le_bytes());
    code.push(Opcode::Add as u8);
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&1u32.to_le_bytes());
    code.push(Opcode::Jump as u8);
    code.extend_from_slice(&0u32.to_le_bytes()); // back to head

    blocks.push(code.len()); // block 2: exit
    // return (acc as i64): the one float->int cast per run
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::Cast as u8);
    code.push(1u8); // FloatToInt
    code.push(CastTarget::I64 as u8);
    code.push(Opcode::Return as u8);

    // locals: 0 = float accumulator, 1 = int counter
    Module::new(vec![Function::with_blocks(code, 2, 0, blocks)], 0)
}

fn main() {
    let iterations = 512;
    let module = float_kernel_module(iterations);

    // Correctness anchor: 512 * (1.5 * 2.25) == 1728.0, cast to 1728.
    assert_eq!(
        module.run().expect("float kernel must run"),
        Value::Int(1728),
        "float kernel result drifted"
    );

    const RUNS: u32 = 20;
    let mut best_ns_per_float_op = u64::MAX;
    for _ in 0..RUNS {
        let start = Instant::now();
        black_box(module.run().expect("float kernel must run"));
        let elapsed_ns = start.elapsed().as_nanos() as u64;
        // Two float ops (one Mul + one Add) per iteration, plus the final
        // cast; the per-iteration cost is what the seam charges.
        let ns_per_iteration = elapsed_ns / u64::try_from(iterations).expect("positive count");
        best_ns_per_float_op = best_ns_per_float_op.min(ns_per_iteration / 2);
    }

    println!(
        "kernel: {iterations} iterations (LoadConstF64 + Mul + Add per iteration, FloatToInt exit)"
    );
    println!("best: {best_ns_per_float_op} ns/float-op");
}
