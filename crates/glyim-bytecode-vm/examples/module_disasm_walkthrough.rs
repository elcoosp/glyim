//! Module-disassembly walkthrough: render a decoded module back to text.
//!
//! The image seam made the bytecode backend's output *loadable*
//! (`encode_module`/`decode_module`), and the disassembler is its
//! observability half: once an artifact is decoded, [`render_module`]
//! lists every instruction with its decoded operands — the debugging
//! counterpart of the assembler, the way `llvm-dis` backs `llvm-as`. This
//! walkthrough assembles the two-function module the image walkthrough
//! uses (`helper(n) = n * 2`, `main` returning `helper(21)`), round-trips
//! it through the image format, prints the deterministic listing, and
//! shows the corrupt-artifact contract: an unknown opcode byte lists as
//! `unknown(0xNN)` and a truncated payload as `<truncated>`, so damaged
//! artifacts stay inspectable instead of crashing the renderer. Run with:
//!
//! ```text
//! cargo run -p glyim-bytecode-vm --example module_disasm_walkthrough
//! ```

use glyim_bytecode_vm::{
    Function, Module, Opcode, decode_module, encode_module, render_function, render_module,
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

/// main: returns helper(21) through a cross-function `OP_CALL` whose resume
/// target is the raw byte offset of the instruction after the call.
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
    let module = Module::with_strings(vec![helper(), main_fn()], vec!["abc".to_string()], 1);

    // Round-trip through the image format first: the disassembler's input
    // is a *decoded* module — exactly what an embedded loader holds.
    let image = encode_module(&module);
    let loaded = decode_module(&image).expect("a freshly encoded image must decode");

    println!("=== render_module ===");
    print!("{}", render_module(&loaded));

    // The corrupt-artifact contract: unknown bytes and truncation render,
    // they never panic or skip. Damage stays visible in the listing.
    let mut corrupt = loaded.functions[1].clone();
    corrupt.code[18] = 0x42; // not an opcode: the OP_CALL byte destroyed
    println!("=== render_module (main with an unknown opcode byte) ===");
    print!("{}", render_module(&Module::new(vec![corrupt], 0)));

    let truncated = vec![Opcode::LoadConst as u8, 0xAA]; // payload cut short
    println!("=== render_function (truncated payload) ===");
    print!("{}", render_function(&Function::new(truncated, 1, 0)));
}
