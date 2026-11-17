//! Loadable bytecode module images (`glyim.module-image/1`).
//!
//! The VM's [`Module`]/[`Function`] values were, until this seam, reachable
//! only by building them in memory: the emitter writes a flat, per-run
//! instruction stream (`CodegenBackend::generate` concatenates every body's
//! bytes with no function table), and nothing could persist a whole module
//! or load one back. That blocks the backend's own stated purpose — the
//! ledger declares the bytecode backend "for testing and embedded
//! execution", and neither testing (ship the artifact, run it elsewhere)
//! nor embedded execution (ship the bytes, load them on device) can cross a
//! process boundary without a container.
//!
//! This module is that container: a versioned, self-describing, little-endian
//! module image that encodes a [`Module`] to bytes and decodes untrusted
//! bytes back into a validated, executable [`Module`].
//!
//! # Layout (version 1)
//!
//! ```text
//! offset  size  field
//! 0       4     magic: b"GLBC"
//! 4       2     format version, u16 LE (currently 1)
//! 6       4     function count, u32 LE
//! 10      4     entry function index, u32 LE
//! then, per function, in index order:
//!         4     n_locals, u32 LE
//!         4     arg_count, u32 LE
//!         4     block-offset table length, u32 LE
//!         4·len block offsets, u32 LE each
//!         4     code length, u32 LE
//!         len   code bytes
//! ```
//!
//! The wire format mirrors the emitter's operand conventions exactly (the
//! same rule the opcode numbering follows): every payload is little-endian,
//! `i64` for constants, `u32` for counts, indices, and offsets. A function
//! with an empty block-offset table decodes into the VM's raw-offset mode
//! (targets are byte offsets), so hand-assembled single-function images
//! behave identically to [`Function::new`]-built ones.
//!
//! # Errors
//!
//! Decoding validates *untrusted* input and returns an explicit
//! [`ImageError`] for every structural failure (bad magic, unsupported
//! version, truncation, out-of-range entry index, out-of-range block
//! offset, trailing bytes) rather than silently mis-loading. Encoding is
//! the serialization of *trusted* in-memory data and is infallible; a
//! [`Module`] whose `entry` is out of range can never run, so a round trip
//! of one will fail in [`decode_module`], by design.

use crate::{Function, Module};
use tracing::debug;

/// The image magic bytes: every well-formed image starts with them.
pub const IMAGE_MAGIC: [u8; 4] = *b"GLBC";

/// The image format version this crate writes and accepts.
pub const IMAGE_VERSION: u16 = 1;

/// Fixed size of the image header (magic + version + function count +
/// entry index).
const HEADER_SIZE: usize = 10;

/// Width in bytes of every fixed-size integer field in the image (`u32`
/// counts/indices/offsets, `u16` version).
const U32_BYTES: usize = 4;

/// Everything [`decode_module`] can reject.
///
/// The VM's error philosophy applies to the container too: every structural
/// failure is an explicit, typed value with its offending numbers attached,
/// never a silent mis-load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageError {
    /// The bytes do not start with [`IMAGE_MAGIC`].
    BadMagic([u8; 4]),
    /// The format version is one this crate does not understand.
    UnsupportedVersion {
        /// The version the image declares.
        found: u16,
        /// The version this crate supports ([`IMAGE_VERSION`]).
        supported: u16,
    },
    /// The image ends before a declared field does.
    Truncated {
        /// How many bytes a field needs.
        needed: usize,
        /// How many bytes remain.
        available: usize,
    },
    /// The entry index does not name a function in the image.
    EntryOutOfBounds {
        /// The decoded entry index.
        entry: usize,
        /// The decoded function count.
        function_count: usize,
    },
    /// A function's block-offset table points outside its code bytes.
    BlockOffsetOutOfRange {
        /// The function whose table is out of range.
        function: usize,
        /// The offending block offset.
        offset: usize,
        /// The function's code length (the inclusive upper bound).
        code_len: usize,
    },
    /// The image carries bytes beyond the structures its header declares.
    TrailingBytes {
        /// Bytes consumed by the declared structures.
        expected: usize,
        /// Bytes actually present.
        found: usize,
    },
}

impl ImageError {
    /// The human-readable summary of this error (the [`std::fmt::Display`]
    /// payload, factored out so the tracing layer can log it too).
    fn summary(&self) -> String {
        match self {
            ImageError::BadMagic(magic) => {
                format!("bad image magic {magic:02x?} (expected {IMAGE_MAGIC:02x?})")
            }
            ImageError::UnsupportedVersion { found, supported } => {
                format!("unsupported image version {found} (this crate supports {supported})")
            }
            ImageError::Truncated { needed, available } => {
                format!("truncated image: needed {needed} bytes, {available} available")
            }
            ImageError::EntryOutOfBounds {
                entry,
                function_count,
            } => format!("entry index {entry} out of range ({function_count} functions)"),
            ImageError::BlockOffsetOutOfRange {
                function,
                offset,
                code_len,
            } => {
                format!(
                    "function {function} block offset {offset} out of range (code len {code_len})"
                )
            }
            ImageError::TrailingBytes { expected, found } => {
                format!("trailing bytes: declared structures end at {expected}, image is {found}")
            }
        }
    }
}

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.summary())
    }
}

impl std::error::Error for ImageError {}

/// Serialize a [`Module`] into a loadable image.
///
/// Infallible by contract: encoding serializes *trusted* in-memory data and
/// every field is representable, so there is nothing to fail on. The one
/// invariant the caller must uphold — `module.entry < module.functions.len()`
/// — is the same invariant [`Module::run`] already requires; a module that
/// violates it cannot execute, and [`decode_module`] will reject its image.
///
/// The output is deterministic: the same module encodes to byte-identical
/// images (the test suite pins this), so images can be hashed, cached, and
/// diffed like any other build artifact.
#[must_use]
pub fn encode_module(module: &Module) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&IMAGE_MAGIC);
    out.extend_from_slice(&IMAGE_VERSION.to_le_bytes());
    out.extend_from_slice(&(module.functions.len() as u32).to_le_bytes());
    out.extend_from_slice(&(module.entry as u32).to_le_bytes());
    for f in &module.functions {
        out.extend_from_slice(&(f.n_locals as u32).to_le_bytes());
        out.extend_from_slice(&(f.arg_count as u32).to_le_bytes());
        out.extend_from_slice(&(f.block_offsets.len() as u32).to_le_bytes());
        for &off in &f.block_offsets {
            out.extend_from_slice(&(off as u32).to_le_bytes());
        }
        out.extend_from_slice(&(f.code.len() as u32).to_le_bytes());
        out.extend_from_slice(&f.code);
    }
    debug!(
        functions = module.functions.len(),
        bytes = out.len(),
        "module image encoded"
    );
    out
}

/// A cursor over image bytes whose every short read is an explicit
/// [`ImageError::Truncated`] carrying the field's byte need and the
/// remaining availability.
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], ImageError> {
        let available = self.bytes.len() - self.pos;
        let end = self.pos.saturating_add(n);
        if end > self.bytes.len() {
            return Err(ImageError::Truncated {
                needed: n,
                available,
            });
        }
        let s = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn take_array<const N: usize>(&mut self) -> Result<[u8; N], ImageError> {
        let bytes = self.take(N)?;
        let array = bytes.try_into().expect("take(N) yielded exactly N bytes");
        Ok(array)
    }

    fn u16(&mut self) -> Result<u16, ImageError> {
        Ok(u16::from_le_bytes(self.take_array()?))
    }

    fn u32(&mut self) -> Result<u32, ImageError> {
        Ok(u32::from_le_bytes(self.take_array()?))
    }
}

/// Decode and validate an image produced by [`encode_module`] (or any other
/// producer of the same format) into an executable [`Module`].
///
/// Every structural defect is an explicit [`ImageError`]; a successful
/// return is a module whose functions, entry index, and block-offset tables
/// are all in range, ready for [`Module::run`].
///
/// # Errors
///
/// Returns the first validation failure in stream order: bad magic,
/// unsupported version, truncation, out-of-range entry, out-of-range block
/// offsets, or trailing bytes.
pub fn decode_module(bytes: &[u8]) -> Result<Module, ImageError> {
    match decode_validated(bytes) {
        Ok(module) => {
            debug!(
                functions = module.functions.len(),
                entry = module.entry,
                bytes = bytes.len(),
                "module image decoded"
            );
            Ok(module)
        }
        Err(err) => {
            debug!(error = %err, bytes = bytes.len(), "module image rejected");
            Err(err)
        }
    }
}

/// The validating decoder: parse the declared structures, then range-check
/// the derived values, before handing back an executable [`Module`].
fn decode_validated(bytes: &[u8]) -> Result<Module, ImageError> {
    let mut r = Reader { bytes, pos: 0 };

    let magic: [u8; 4] = r.take_array().map_err(|_| ImageError::Truncated {
        needed: HEADER_SIZE,
        available: bytes.len(),
    })?;
    if magic != IMAGE_MAGIC {
        return Err(ImageError::BadMagic(magic));
    }
    let version = r.u16().map_err(|_| ImageError::Truncated {
        needed: HEADER_SIZE,
        available: bytes.len(),
    })?;
    if version != IMAGE_VERSION {
        return Err(ImageError::UnsupportedVersion {
            found: version,
            supported: IMAGE_VERSION,
        });
    }
    // The remaining header fields must also fit inside the fixed header;
    // a shorter image is a truncation of the *header*, not of the payload.
    let function_count = r.u32().map_err(|_| ImageError::Truncated {
        needed: HEADER_SIZE,
        available: bytes.len(),
    })?;
    let entry = r.u32().map_err(|_| ImageError::Truncated {
        needed: HEADER_SIZE,
        available: bytes.len(),
    })?;
    let entry = entry as usize;
    let function_count = function_count as usize;
    if entry >= function_count {
        return Err(ImageError::EntryOutOfBounds {
            entry,
            function_count,
        });
    }

    let mut functions = Vec::with_capacity(function_count);
    for idx in 0..function_count {
        let n_locals = r.u32()? as usize;
        let arg_count = r.u32()? as usize;
        let block_count = r.u32()? as usize;
        let table_bytes = block_count.saturating_mul(U32_BYTES);
        let table = r.take(table_bytes)?;
        let code_len = r.u32()? as usize;
        let code = r.take(code_len)?.to_vec();
        let mut block_offsets = Vec::with_capacity(block_count);
        for chunk in table.as_chunks::<U32_BYTES>().0 {
            let off = u32::from_le_bytes(*chunk) as usize;
            if off > code_len {
                return Err(ImageError::BlockOffsetOutOfRange {
                    function: idx,
                    offset: off,
                    code_len,
                });
            }
            block_offsets.push(off);
        }
        functions.push(Function {
            code,
            n_locals,
            arg_count,
            block_offsets,
        });
    }

    if r.pos != bytes.len() {
        return Err(ImageError::TrailingBytes {
            expected: r.pos,
            found: bytes.len(),
        });
    }
    Ok(Module { functions, entry })
}

#[cfg(test)]
mod image_tests {
    use super::*;
    use crate::{Opcode, Value, Vm};

    fn u32le(v: u32) -> [u8; 4] {
        v.to_le_bytes()
    }
    fn i64le(v: i64) -> [u8; 8] {
        v.to_le_bytes()
    }
    fn op(o: Opcode) -> u8 {
        o as u8
    }

    /// Fixed size of the per-function prologue (`n_locals` + `arg_count` +
    /// block-table length) before the variable-size payload.
    const FUNCTION_PROLOGUE_SIZE: usize = 12;

    /// The lib-test assembler, mirrored: records basic-block offsets so jump
    /// targets can reference blocks by index (the emitter's convention).
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
            self.code.push(op(o));
        }
        fn u32(&mut self, v: u32) {
            self.code.extend_from_slice(&u32le(v));
        }
        fn i64(&mut self, v: i64) {
            self.code.extend_from_slice(&i64le(v));
        }
        fn load_const(&mut self, v: i64) {
            self.op(Opcode::LoadConst);
            self.i64(v);
        }
        fn load_local(&mut self, idx: u32) {
            self.op(Opcode::LoadLocal);
            self.u32(idx);
        }
        fn finish(self, n_locals: usize, arg_count: usize) -> Function {
            Function::with_blocks(self.code, n_locals, arg_count, self.blocks)
        }
    }

    /// fib(n) via recursion + main calling fib(6); the cross-function,
    /// cross-frame shape the image format must preserve exactly.
    fn fib_main_module() -> Module {
        let mut fib = Asm::new();
        fib.label(0);
        fib.load_local(1); // MIR ABI: the argument lives at local 1
        fib.load_const(2);
        fib.op(Opcode::Lt);
        fib.op(Opcode::JumpIf);
        fib.u32(3);
        // dests live at 2/3: local 1 holds the argument under the MIR
        // call-frame ABI
        fib.label(1);
        fib.load_const(0);
        fib.load_local(1);
        fib.load_const(1);
        fib.op(Opcode::Sub);
        fib.op(Opcode::Call);
        fib.u32(1);
        fib.u32(2);
        fib.u32(2);
        fib.label(2);
        fib.load_const(0);
        fib.load_local(1);
        fib.load_const(2);
        fib.op(Opcode::Sub);
        fib.op(Opcode::Call);
        fib.u32(1);
        fib.u32(3);
        fib.u32(4);
        fib.label(4);
        fib.load_local(2);
        fib.load_local(3);
        fib.op(Opcode::Add);
        fib.op(Opcode::Return);
        fib.label(3);
        fib.load_local(1);
        fib.op(Opcode::Return);

        let mut main = Asm::new();
        main.label(0);
        main.load_const(0);
        main.load_const(6);
        main.op(Opcode::Call);
        main.u32(1);
        main.u32(0);
        main.u32(1);
        main.label(1);
        main.load_local(0);
        main.op(Opcode::Return);

        Module::new(vec![fib.finish(8, 1), main.finish(8, 0)], 1)
    }

    /// helper(n) = n * 2; main returns helper(21). Blockless functions:
    /// both use raw byte-offset targets, so the image must preserve the
    /// empty-block-table mode, not just the block-index mode.
    fn blockless_pair_module() -> Module {
        let mut helper = Asm::new();
        helper.load_const(2);
        helper.load_local(1); // MIR ABI: the argument lives at local 1
        helper.op(Opcode::Mul);
        helper.op(Opcode::Return);

        let mut main = Asm::new();
        main.load_const(0); // fn idx 0: helper
        main.load_const(21);
        main.op(Opcode::Call);
        main.u32(1);
        main.u32(0);
        // Raw-offset mode: the resume target is the byte offset of the
        // instruction after the call (9 + 9 + 1 + 4 + 4 + 4 = 31).
        main.u32(31);
        main.load_local(0);
        main.op(Opcode::Return);

        // helper has no jumps, so the block table stays empty (raw-offset
        // mode for its own targets).
        Module::new(
            vec![
                Function::new(helper.code, 1, 1),
                Function::new(main.code, 1, 0),
            ],
            1,
        )
    }

    #[test]
    fn image_round_trips_hand_assembled_module() {
        let module = fib_main_module();
        let image = encode_module(&module);
        let decoded = decode_module(&image).expect("round trip of a valid module must decode");
        assert_eq!(decoded, module, "decode(encode(m)) must equal m");
    }

    #[test]
    fn image_round_trips_blockless_functions() {
        let module = blockless_pair_module();
        let image = encode_module(&module);
        let decoded = decode_module(&image).expect("round trip must decode");
        assert_eq!(decoded, module);
        // The empty block-offset tables must survive: targets stay raw.
        assert!(decoded.functions[0].block_offsets.is_empty());
        assert!(decoded.functions[1].block_offsets.is_empty());
    }

    #[test]
    fn decoded_module_executes_cross_function_calls() {
        let module = fib_main_module();
        let image = encode_module(&module);
        let decoded = decode_module(&image).expect("decode");
        assert_eq!(decoded.run().expect("run"), Value::Int(8), "fib(6) == 8");
        assert_eq!(module.run().expect("run"), Value::Int(8));
    }

    #[test]
    fn decoded_blockless_pair_executes_call_across_functions() {
        let module = blockless_pair_module();
        let image = encode_module(&module);
        let decoded = decode_module(&image).expect("decode");
        assert_eq!(decoded.run().expect("run"), Value::Int(42), "helper(21)");
    }

    #[test]
    fn encode_is_deterministic() {
        let module = fib_main_module();
        assert_eq!(encode_module(&module), encode_module(&module));
    }

    #[test]
    fn image_header_is_self_describing() {
        let module = fib_main_module();
        let image = encode_module(&module);
        assert_eq!(&image[0..4], &IMAGE_MAGIC, "magic first");
        assert_eq!(u16::from_le_bytes([image[4], image[5]]), IMAGE_VERSION);
        assert_eq!(
            u32::from_le_bytes([image[6], image[7], image[8], image[9]]),
            2,
            "function count"
        );
        // The entry index is the 4 bytes after the count; fib+main has
        // entry 1 (main) per the fixture.
        assert_eq!(image[10], 1, "entry index low byte (u32 LE)");
    }

    #[test]
    fn decode_rejects_bad_magic() {
        let mut image = encode_module(&blockless_pair_module());
        image[0] = b'X';
        let err = decode_module(&image).unwrap_err();
        assert_eq!(
            err,
            ImageError::BadMagic([b'X', image[1], image[2], image[3]])
        );
    }

    #[test]
    fn decode_rejects_unsupported_version() {
        let mut image = encode_module(&blockless_pair_module());
        image[4] = 99;
        assert_eq!(
            decode_module(&image).unwrap_err(),
            ImageError::UnsupportedVersion {
                found: 99,
                supported: IMAGE_VERSION
            }
        );
    }

    #[test]
    fn decode_rejects_truncated_header() {
        let image = encode_module(&blockless_pair_module());
        for cut in [0usize, 4, 9] {
            let err = decode_module(&image[..cut]).unwrap_err();
            assert_eq!(
                err,
                ImageError::Truncated {
                    needed: HEADER_SIZE,
                    available: cut
                },
                "cut at {cut}"
            );
        }
    }

    #[test]
    fn decode_rejects_truncated_function_payload() {
        // Cut into the first function's code bytes: the declared code length
        // can no longer be satisfied.
        let module = fib_main_module();
        let image = encode_module(&module);
        let cut = HEADER_SIZE + FUNCTION_PROLOGUE_SIZE + 2;
        let err = decode_module(&image[..cut]).unwrap_err();
        let block_count = u32::from_le_bytes(
            image[HEADER_SIZE + 8..HEADER_SIZE + 12]
                .try_into()
                .expect("block-table length"),
        ) as usize;
        assert_eq!(
            err,
            ImageError::Truncated {
                needed: block_count * 4,
                available: cut - HEADER_SIZE - FUNCTION_PROLOGUE_SIZE
            }
        );
    }

    #[test]
    fn decode_rejects_entry_out_of_range() {
        let mut image = encode_module(&blockless_pair_module());
        // Function count 2; entry 7 does not name a function.
        image[10..14].copy_from_slice(&u32le(7));
        assert_eq!(
            decode_module(&image).unwrap_err(),
            ImageError::EntryOutOfBounds {
                entry: 7,
                function_count: 2
            }
        );
    }

    #[test]
    fn decode_rejects_block_offset_out_of_range() {
        // Hand-write an image whose first function's single block offset
        // points past its code bytes.
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(&IMAGE_MAGIC);
        bytes.extend_from_slice(&IMAGE_VERSION.to_le_bytes());
        bytes.extend_from_slice(&u32le(1)); // one function
        bytes.extend_from_slice(&u32le(0)); // entry 0
        bytes.extend_from_slice(&u32le(0)); // n_locals
        bytes.extend_from_slice(&u32le(0)); // arg_count
        bytes.extend_from_slice(&u32le(1)); // one block offset
        bytes.extend_from_slice(&u32le(9)); // offset 9 > code len 2
        bytes.extend_from_slice(&u32le(2)); // code length 2
        bytes.extend_from_slice(&[op(Opcode::LoadConst), op(Opcode::Return)]);
        assert_eq!(
            decode_module(&bytes).unwrap_err(),
            ImageError::BlockOffsetOutOfRange {
                function: 0,
                offset: 9,
                code_len: 2
            }
        );
    }

    #[test]
    fn decode_rejects_trailing_bytes() {
        let mut image = encode_module(&blockless_pair_module());
        image.push(0xAA);
        let consumed = image.len() - 1;
        assert_eq!(
            decode_module(&image).unwrap_err(),
            ImageError::TrailingBytes {
                expected: consumed,
                found: image.len()
            }
        );
    }

    #[test]
    fn decode_accepts_the_pinned_first_function_bytes() {
        // A golden-style pinned image: helper(n) = n * 2 as a single-function
        // blockless image, entry 0. If the layout ever changes
        // unintentionally, this literal stops decoding.
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(b"GLBC");
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&u32le(1)); // one function
        bytes.extend_from_slice(&u32le(0)); // entry 0
        bytes.extend_from_slice(&u32le(1)); // n_locals
        bytes.extend_from_slice(&u32le(1)); // arg_count
        bytes.extend_from_slice(&u32le(0)); // no block table
        bytes.extend_from_slice(&u32le(9 + 5 + 2)); // code length
        bytes.extend_from_slice(&[op(Opcode::LoadConst)]);
        bytes.extend_from_slice(&i64le(2));
        bytes.extend_from_slice(&[op(Opcode::LoadLocal)]);
        bytes.extend_from_slice(&u32le(0));
        bytes.extend_from_slice(&[op(Opcode::Mul), op(Opcode::Return)]);
        let module = decode_module(&bytes).expect("pinned image must decode");
        let mut vm = Vm::new();
        let result = vm.run_module(&module).expect("pinned image must run");
        // The entry takes its argument from local 0, which zero-initialises
        // to 0 here: helper(0) == 0.
        assert_eq!(result, Value::Int(0));
        assert_eq!(module.functions[0].n_locals, 1);
        assert_eq!(module.functions[0].arg_count, 1);
    }

    #[test]
    fn image_error_displays_context() {
        let err = ImageError::EntryOutOfBounds {
            entry: 7,
            function_count: 2,
        };
        let text = err.to_string();
        assert!(text.contains('7'), "entry in message: {text}");
        assert!(text.contains('2'), "function count in message: {text}");
    }
}

#[cfg(test)]
mod value_asserts {
    use super::*;
    use crate::Value;

    // The round-trip asserts need PartialEq on Function/Module; this keeps
    // the derive honest if Value ever grows a float variant (Eq must not
    // silently hold over NaN-like semantics).
    #[test]
    fn function_equality_is_structural() {
        let a = Function::new(vec![1, 2, 3], 2, 1);
        let b = Function::new(vec![1, 2, 3], 2, 1);
        let c = Function::new(vec![1, 2, 3], 2, 0);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn tuple_value_equality_is_structural() {
        assert_eq!(
            Value::Tuple(vec![Value::Int(1)]),
            Value::Tuple(vec![Value::Int(1)])
        );
        assert_ne!(
            Value::Tuple(vec![Value::Int(1)]),
            Value::Tuple(vec![Value::Int(2)])
        );
    }
}
