//! Module-image encode/decode timing harness.
//!
//! Dependency-free (`harness = false`), mirroring the `glyim-mir-interp`
//! `run_program` bench and the `glyim-lsp` request benches: only
//! `std::time::Instant` + `std::hint::black_box`. Every image encode/decode
//! pays the same per-module cost the `glyim.module-image/1` contract
//! charges, so its per-call overhead must stay small against a real
//! compile. This bench prints the per-op cost so CI logs can track
//! headroom. Run with:
//!
//! ```text
//! cargo bench -p glyim-bytecode-vm --bench module_image
//! ```

use glyim_bytecode_vm::{Function, Module, Opcode, decode_module, encode_module};
use std::hint::black_box;
use std::time::Instant;

/// The lib-test assembler, mirrored for the measured corpus: records
/// basic-block offsets so jump targets can reference blocks by index.
struct Asm {
    code: Vec<u8>,
    blocks: Vec<usize>,
}

impl Asm {
    fn new() -> Self {
        Asm {
            code: Vec::new(),
            blocks: Vec::new(),
        }
    }
    fn label(&mut self, idx: usize) {
        while self.blocks.len() <= idx {
            self.blocks.push(0);
        }
        self.blocks[idx] = self.code.len();
    }
    fn op(&mut self, o: Opcode) {
        self.code.push(o as u8);
    }
    fn u32(&mut self, v: u32) {
        self.code.extend_from_slice(&v.to_le_bytes());
    }
    fn i64(&mut self, v: i64) {
        self.code.extend_from_slice(&v.to_le_bytes());
    }
    fn load_const(&mut self, v: i64) {
        self.op(Opcode::LoadConst);
        self.i64(v);
    }
    fn load_local(&mut self, idx: u32) {
        self.op(Opcode::LoadLocal);
        self.u32(idx);
    }
    fn store_local(&mut self, idx: u32) {
        self.op(Opcode::StoreLocal);
        self.u32(idx);
    }
    fn finish(self, n_locals: usize, arg_count: usize) -> Function {
        Function::with_blocks(self.code, n_locals, arg_count, self.blocks)
    }
}

/// The measured corpus: `count` leaf functions plus a `main` that calls the
/// first leaf — the multi-function shape (function table, block tables,
/// cross-frame calls) the image format exists to ship.
fn measured_module(count: usize) -> Module {
    let mut functions = Vec::with_capacity(count + 1);
    for i in 0..count {
        let mut leaf = Asm::new();
        leaf.load_const(i as i64);
        leaf.load_local(1); // MIR ABI: the argument lives at local 1
        leaf.op(Opcode::Mul);
        leaf.op(Opcode::Return);
        functions.push(leaf.finish(1, 1));
    }
    let mut main = Asm::new();
    main.label(0);
    main.load_const(0);
    main.load_const(21);
    main.op(Opcode::Call);
    main.u32(1);
    main.u32(0);
    main.u32(1);
    main.label(1);
    main.store_local(0);
    main.load_local(0);
    main.op(Opcode::Return);
    functions.push(main.finish(1, 0));
    Module::new(functions, count)
}

fn main() {
    let module = measured_module(32);
    let image = encode_module(&module);

    const ITERATIONS: u32 = 2_000;
    let runs = 20;
    let mut best_encode = u64::MAX;
    let mut best_decode = u64::MAX;
    for _ in 0..runs {
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            black_box(encode_module(black_box(&module)));
        }
        best_encode = best_encode.min(start.elapsed().as_nanos() as u64 / u64::from(ITERATIONS));

        let start = Instant::now();
        for _ in 0..ITERATIONS {
            black_box(decode_module(black_box(&image)).expect("corpus image must decode"));
        }
        best_decode = best_decode.min(start.elapsed().as_nanos() as u64 / u64::from(ITERATIONS));
    }

    println!(
        "module: {} functions, image {} bytes",
        module.functions.len(),
        image.len()
    );
    println!("best encode: {best_encode} ns/op");
    println!("best decode: {best_decode} ns/op");
}
