//! Module-image walkthrough: encode a module to bytes, load it back, run it.
//!
//! The `glyim.module-image/1` seam is the persistence boundary for the
//! bytecode backend: the emitter's raw instruction stream cannot cross a
//! process boundary (no function table, no entry point), while an *image*
//! can — ship it, hash it, cache it, then decode it into a validated,
//! executable [`Module`]. This walkthrough exercises exactly that path with
//! a two-function module (`helper(n) = n * 2`, `main` returning
//! `helper(21)`), including one decode rejection so the failure contract is
//! visible. Run with:
//!
//! ```text
//! cargo run -p glyim-bytecode-vm --example module_image_walkthrough
//! ```

use glyim_bytecode_vm::{
    Function, ImageError, Module, Opcode, Value, decode_module, encode_module,
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
    let module = Module::new(vec![helper(), main_fn()], 1);

    // Serialize: the artifact a producer ships.
    let image = encode_module(&module);
    println!(
        "encoded {} function(s) into {} bytes (magic {:?}, version {})",
        module.functions.len(),
        image.len(),
        glyim_bytecode_vm::IMAGE_MAGIC,
        glyim_bytecode_vm::IMAGE_VERSION,
    );

    // Deserialize and run: the consumer's path over untrusted bytes.
    let loaded = decode_module(&image).expect("a freshly encoded image must decode");
    let value = loaded.run().expect("decoded module must execute");
    println!("entry returned {value:?} (expected Int(42))");

    // The failure contract: structural damage is rejected explicitly, never
    // mis-executed.
    let mut corrupted = image.clone();
    corrupted[4] = 0x63; // unsupported format version
    match decode_module(&corrupted) {
        Err(ImageError::UnsupportedVersion { found, supported }) => {
            println!("corrupt image rejected: version {found} (supported {supported})");
        }
        other => panic!("expected UnsupportedVersion, got {other:?}"),
    }

    // The version-2 string pool: string constants travel under
    // `OP_LOAD_CONST_STR` (a pool index), and the pool itself rides inside
    // the image, so string payloads — and content equality over them —
    // survive the process boundary. main compares pool slots 0 and 1 by
    // content and returns 0 (they differ).
    let mut code = Vec::new();
    code.push(Opcode::LoadConstStr as u8);
    code.extend_from_slice(&0u32.to_le_bytes()); // "abc"
    code.push(Opcode::LoadConstStr as u8);
    code.extend_from_slice(&1u32.to_le_bytes()); // "abd"
    code.push(Opcode::Eq as u8);
    code.push(Opcode::Return as u8);
    let stringy = Module::with_strings(
        vec![Function::new(code, 1, 0)],
        vec!["abc".to_string(), "abd".to_string()],
        0,
    );
    let stringy_image = encode_module(&stringy);
    let loaded_strings = decode_module(&stringy_image).expect("pooled image must decode");
    println!(
        "pooled image: {} string(s) rode along ({} bytes)",
        loaded_strings.strings.len(),
        stringy_image.len()
    );
    let eq = loaded_strings.run().expect("pooled module must execute");
    println!("\"abc\" == \"abd\" -> {eq:?} (content equality, not interner-index luck)");
    assert_eq!(eq, Value::Int(0));
}
