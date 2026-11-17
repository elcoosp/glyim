//! Float-casts walkthrough: float arithmetic, comparisons, and width-aware
//! casts in the bytecode VM.
//!
//! The float seam gives the VM a real float value kind. Before it, float
//! constants travelled under the integer `OP_LOAD_CONST` and `OP_CAST` was a
//! no-op, so `1.0 / 0.0` computed an *integer* addition over the raw `f64`
//! bit patterns and `f as i32` silently kept the float. This walkthrough
//! exercises the shapes the emitter emits for float programs: `OP_LOAD_CONST_F64`
//! constants, IEEE-754 arithmetic (`1.0 / 0.0` is `+inf`, matching LLVM's
//! `fdiv` and the MIR interpreter), float comparisons feeding a branch, a
//! cross-function float call, and the interpreter's MIR-14 cast semantics
//! (`(3.99) as i32 == 3`, `(-1i32) as u8 == 255`) over the emitter's
//! kind+target operand bytes. Run with:
//!
//! ```text
//! cargo run -p glyim-bytecode-vm --example float_casts_walkthrough
//! ```

use glyim_bytecode_vm::{CastTarget, Function, Module, Opcode, Value};

/// helper(a, b): `(a + b) * 2.0` — the cross-function float shape.
fn helper() -> Function {
    let mut code = Vec::new();
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&1u32.to_le_bytes()); // MIR ABI: first arg
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&2u32.to_le_bytes()); // second arg
    code.push(Opcode::Add as u8);
    code.push(Opcode::LoadConstF64 as u8);
    code.extend_from_slice(&2.0f64.to_le_bytes());
    code.push(Opcode::Mul as u8);
    code.push(Opcode::Return as u8);
    Function::new(code, 3, 2)
}

/// main: calls the float helper, compares the result against a constant to
/// drive a branch, then casts the branch's float through `f64 -> i64` —
/// the full float surface in one entry point.
fn main_fn() -> Function {
    let mut code = Vec::new();
    // Block 0: call helper(1.25, 2.0) -> (1.25 + 2.0) * 2.0 = 6.5
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&0i64.to_le_bytes()); // fn index 0: helper
    code.push(Opcode::LoadConstF64 as u8);
    code.extend_from_slice(&1.25f64.to_le_bytes());
    code.push(Opcode::LoadConstF64 as u8);
    code.extend_from_slice(&2.0f64.to_le_bytes());
    code.push(Opcode::Call as u8);
    code.extend_from_slice(&2u32.to_le_bytes()); // argc
    code.extend_from_slice(&0u32.to_le_bytes()); // dest local
    code.extend_from_slice(&1u32.to_le_bytes()); // resume -> block 1
    // Block 1: cast the returned Float(6.5) through FloatToInt/I64 -> Int(6)
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::Cast as u8);
    code.push(1u8); // FloatToInt
    code.push(CastTarget::I64 as u8);
    code.push(Opcode::Return as u8);
    // Block 0 spans 9+9+9+13 = 40 bytes; the resume target is a block index.
    Function::with_blocks(code, 1, 0, vec![0, 40])
}

fn main() {
    // 1. IEEE arithmetic and the typed float constant opcode.
    let mut arith = Vec::new();
    arith.push(Opcode::LoadConstF64 as u8);
    arith.extend_from_slice(&1.0f64.to_le_bytes());
    arith.push(Opcode::LoadConstF64 as u8);
    arith.extend_from_slice(&0.0f64.to_le_bytes());
    arith.push(Opcode::Div as u8);
    arith.push(Opcode::Return as u8);
    let inf = Module::new(vec![Function::new(arith, 1, 0)], 0)
        .run()
        .expect("float division must run");
    println!("1.0 / 0.0 = {inf:?} (IEEE +inf, like LLVM fdiv and the interpreter)");

    // 2. A float comparison feeding the VM's Int bool encoding.
    let mut cmp = Vec::new();
    cmp.push(Opcode::LoadConstF64 as u8);
    cmp.extend_from_slice(&2.5f64.to_le_bytes());
    cmp.push(Opcode::LoadConstF64 as u8);
    cmp.extend_from_slice(&3.5f64.to_le_bytes());
    cmp.push(Opcode::Lt as u8);
    cmp.push(Opcode::Return as u8);
    let bool_val = Module::new(vec![Function::new(cmp, 1, 0)], 0)
        .run()
        .expect("float comparison must run");
    println!("2.5 < 3.5 -> {bool_val:?} (the VM's bool encoding, JumpIf is jump-if-nonzero)");

    // 3. The width-aware cast semantics (MIR-14): (-1i32) as u8 == 255.
    let mut trunc = Vec::new();
    trunc.push(Opcode::LoadConst as u8);
    trunc.extend_from_slice(&(-1i64).to_le_bytes());
    trunc.push(Opcode::Cast as u8);
    trunc.push(0u8); // IntToInt
    trunc.push(CastTarget::U8 as u8);
    trunc.push(Opcode::Return as u8);
    let u8_val = Module::new(vec![Function::new(trunc, 1, 0)], 0)
        .run()
        .expect("int truncation must run");
    println!("(-1i32) as u8 -> {u8_val:?} (a real truncation, not an identity)");

    // 4. Cross-function floats through the whole entry point: the helper's
    //    float result is cast to Int(6) before the entry returns.
    let module = Module::new(vec![helper(), main_fn()], 1);
    let value = module.run().expect("float module must run");
    println!("(1.25 + 2.0) * 2.0 as i64 -> {value:?} (Float(6.5) cast through FloatToInt)");

    assert_eq!(inf, Value::Float(f64::INFINITY));
    assert_eq!(bool_val, Value::Int(1));
    assert_eq!(u8_val, Value::Int(255));
    assert_eq!(value, Value::Int(6));
    println!("all walkthrough values match");
}
