//! Module-image disassembler: render a decoded [`Module`] back to a
//! deterministic, human-readable instruction listing.
//!
//! The image seam made the bytecode backend's output *loadable*
//! (`encode_module`/`decode_module`), but once loaded, the bytes inside a
//! [`Function::code`] were still opaque: nothing could answer "what
//! instructions does this decoded artifact carry?" without re-deriving the
//! wire format by hand. This module is the missing observability half — the
//! debugging counterpart of the assembler, the way `llvm-dis` backs
//! `llvm-as`.
//!
//! [`render_module`] renders a whole module: a header line (format version,
//! function count, entry index, pool size), one listing per function
//! (declared locals/args/block-table/code size, then every instruction with
//! its decoded operands), and the byte-string pool the `OP_LOAD_CONST_STR`
//! indices resolve into. [`render_function`] renders one function's listing
//! alone.
//!
//! # Contract
//!
//! The rendering is **deterministic** (the same module renders to
//! byte-identical text — the test suite pins a golden), **total** (every
//! possible code byte renders: unknown opcode bytes appear as
//! `unknown(0xNN)` rather than skipping or panicking, and a payload that
//! runs past the end of the code renders as `<truncated>`), and **read-only**
//! (rendering never executes anything and never mutates the module).
//!
//! The output is diagnostic text, not a parser-facing format: a listing of a
//! *corrupt* artifact is still renderable, which is exactly the point — the
//! renderer shows the operator what the bytes say, including what they say
//! wrongly.

use std::fmt::Write as _;

use crate::{Function, Module};
use tracing::debug;

/// The cast-kind byte's name, as the VM's `OP_CAST` wire format spells it
/// (the numbering mirrors `cast_value`'s constants). Unknown kind bytes
/// render as `unknown(0xNN)`.
pub(crate) fn cast_kind_name(kind: u8) -> String {
    match kind {
        0 => "IntToInt".to_string(),
        1 => "FloatToInt".to_string(),
        2 => "IntToFloat".to_string(),
        3 => "PtrToPtr".to_string(),
        4 => "FnPtrToPtr".to_string(),
        5 => "PtrToInt".to_string(),
        6 => "IntToPtr".to_string(),
        7 => "FloatToFloat".to_string(),
        other => format!("unknown(0x{other:02x})"),
    }
}

/// The target-type byte's name (the [`crate::CastTarget`] numbering).
/// Unknown target bytes render as `unknown(0xNN)`.
pub(crate) fn cast_target_name(target: u8) -> String {
    match crate::CastTarget::from_u8(target) {
        Some(t) => format!("{t:?}"),
        None => format!("unknown(0x{target:02x})"),
    }
}

/// One decoded operand read: either the payload value or the fact that the
/// code ended mid-payload (`<truncated>`). The renderer must stay total over
/// corrupt code, so every read is fallible at the format level.
enum Read {
    Value(String),
    Truncated,
}

/// Read a little-endian `i64` payload at `pos` (the `OP_LOAD_CONST` /
/// `OP_LOAD_CONST_F64` / `OP_SWITCH_INT` arm value width).
fn read_i64(code: &[u8], pos: usize) -> Read {
    if pos + 8 > code.len() {
        return Read::Truncated;
    }
    let mut b = [0u8; 8];
    b.copy_from_slice(&code[pos..pos + 8]);
    Read::Value(i64::from_le_bytes(b).to_string())
}

/// Read a little-endian `u32` payload at `pos` (index / count / target
/// width).
fn read_u32(code: &[u8], pos: usize) -> Read {
    if pos + 4 > code.len() {
        return Read::Truncated;
    }
    let mut b = [0u8; 4];
    b.copy_from_slice(&code[pos..pos + 4]);
    Read::Value(u32::from_le_bytes(b).to_string())
}

/// Read one byte at `pos` (the `OP_CAST` kind/target and `OP_ASSERT`
/// expected-condition bytes).
fn read_u8(code: &[u8], pos: usize) -> Read {
    code.get(pos)
        .map(|b| Read::Value(b.to_string()))
        .unwrap_or(Read::Truncated)
}

/// Render one function's instruction listing (the body of a [`render_module`]
/// function section, without the module header or pool).
///
/// The listing walks the code bytes linearly in the VM's wire format
/// (mirroring `Vm::drive`'s operand widths exactly): unknown opcode bytes
/// render as `unknown(0xNN)` and consume nothing, a payload that runs past
/// the end of the code renders as `<truncated>` and ends the listing. When
/// the function carries a block-offset table, block labels (`b0@0:`) mark
/// the table's offsets so jump targets read against them; a table-less
/// function (raw byte-offset mode) lists without labels.
pub fn render_function(f: &Function) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "fn: locals={} args={} blocks={} code={}B",
        f.n_locals,
        f.arg_count,
        f.block_offsets.len(),
        f.code.len()
    );
    out.push_str(&render_body(f, None));
    out
}

/// The instruction listing for one function. `pool` is the module's string
/// pool when rendering through [`render_module`] (`OP_LOAD_CONST_STR`
/// indices resolve into it and their content is shown); the standalone
/// [`render_function`] entry has no pool, so pool references render as bare
/// indices.
fn render_body(f: &Function, pool: Option<&[String]>) -> String {
    let mut out = String::new();

    // Block labels: in block-index mode the offsets are jump-target
    // anchors; render each as a label line. Raw-offset mode (empty table)
    // lists without labels.
    let mut block_starts: Vec<(usize, usize)> = f
        .block_offsets
        .iter()
        .enumerate()
        .map(|(i, &off)| (i, off))
        .collect();
    block_starts.sort_by_key(|&(_, off)| off);

    let mut pos = 0usize;
    let mut next_block = 0usize;
    while pos < f.code.len() {
        // Emit the block labels that start at this offset.
        while next_block < block_starts.len() && block_starts[next_block].1 == pos {
            let _ = writeln!(out, "  b{}@{}:", block_starts[next_block].0, pos);
            next_block += 1;
        }
        let start = pos;
        let raw = f.code[pos];
        pos += 1;
        let (text, advance) = decode_instruction(&f.code, pool, raw, pos);
        let _ = writeln!(out, "  {start}: {text}");
        pos += advance;
    }
    // Labels at the very end (a block whose offset is the code length —
    // the `BlockOffsetOutOfRange` bound allows `off == code_len`).
    while next_block < block_starts.len() && block_starts[next_block].1 == pos {
        let _ = writeln!(out, "  b{}@{}:", block_starts[next_block].0, pos);
        next_block += 1;
    }
    out
}

/// Decode one instruction at `raw`/`pos` into its listing text plus the
/// number of payload bytes consumed. Unknown opcodes consume nothing
/// (`(unknown(0xNN), 0)`); a truncated payload renders `<truncated>` and
/// consumes the rest of the code (the listing for that function ends).
fn decode_instruction(
    code: &[u8],
    pool: Option<&[String]>,
    raw: u8,
    pos: usize,
) -> (String, usize) {
    let Some(op) = crate::Opcode::from_u8(raw) else {
        return (format!("unknown(0x{raw:02x})"), 0);
    };
    let trunc = |text: &str| {
        (
            format!("{text} <truncated>"),
            code.len().saturating_sub(pos),
        )
    };
    let take = |r: Read, text: &str, width: usize| -> (String, usize) {
        match r {
            Read::Value(v) => (format!("{text} {v}"), width),
            Read::Truncated => (
                format!("{text} <truncated>"),
                code.len().saturating_sub(pos),
            ),
        }
    };
    match op {
        crate::Opcode::LoadConst => take(read_i64(code, pos), op_name(op), 8),
        crate::Opcode::LoadConstF64 => {
            // The float constant renders as both the IEEE-754 value and the
            // raw bit pattern, so a hand-corrupted bit pattern stays visible.
            match read_i64(code, pos) {
                Read::Value(v) => {
                    let bits = v.parse::<i64>().unwrap_or(0) as u64;
                    let f64v = f64::from_bits(bits);
                    (format!("{} {f64v} (bits 0x{bits:016x})", op_name(op)), 8)
                }
                Read::Truncated => trunc(op_name(op)),
            }
        }
        crate::Opcode::LoadConstStr => match read_u32(code, pos) {
            Read::Value(v) => {
                let idx: usize = v.parse().unwrap_or(0);
                match pool.and_then(|p| p.get(idx)) {
                    Some(s) => (format!("{} pool[{idx}] {s:?}", op_name(op)), 4),
                    None => (format!("{} pool[{idx}] <out of range>", op_name(op)), 4),
                }
            }
            Read::Truncated => trunc(op_name(op)),
        },
        crate::Opcode::LoadLocal
        | crate::Opcode::StoreLocal
        | crate::Opcode::LoadLocalAddr
        | crate::Opcode::Len => take(read_u32(code, pos), op_name(op), 4),
        crate::Opcode::Cast => {
            let kind = match read_u8(code, pos) {
                Read::Value(v) => v.parse::<u8>().unwrap_or(0),
                Read::Truncated => return trunc(op_name(op)),
            };
            let target = match read_u8(code, pos + 1) {
                Read::Value(v) => v.parse::<u8>().unwrap_or(0),
                Read::Truncated => return trunc(op_name(op)),
            };
            (
                format!(
                    "{} {}->{}",
                    op_name(op),
                    cast_kind_name(kind),
                    cast_target_name(target)
                ),
                2,
            )
        }
        crate::Opcode::Jump | crate::Opcode::JumpIf => match read_u32(code, pos) {
            Read::Value(v) => (format!("{} t{v}", op_name(op)), 4),
            Read::Truncated => trunc(op_name(op)),
        },
        crate::Opcode::SwitchInt => {
            let count = match read_u32(code, pos) {
                Read::Value(v) => v.parse::<usize>().unwrap_or(0),
                Read::Truncated => return trunc(op_name(op)),
            };
            let mut arms = Vec::with_capacity(count);
            let mut p = pos + 4;
            for _ in 0..count {
                let value = match read_i64(code, p) {
                    Read::Value(v) => v,
                    Read::Truncated => return trunc(op_name(op)),
                };
                p += 8;
                let target = match read_u32(code, p) {
                    Read::Value(v) => v,
                    Read::Truncated => return trunc(op_name(op)),
                };
                p += 4;
                arms.push(format!("{value}->t{target}"));
            }
            let otherwise = match read_u32(code, p) {
                Read::Value(v) => v,
                Read::Truncated => return trunc(op_name(op)),
            };
            (
                format!(
                    "{} n={count} [{}] otherwise=t{otherwise}",
                    op_name(op),
                    arms.join(", ")
                ),
                4 + count * 12 + 4,
            )
        }
        crate::Opcode::Assert => {
            let expected = match read_u8(code, pos) {
                Read::Value(v) => v,
                Read::Truncated => return trunc(op_name(op)),
            };
            let target = match read_u32(code, pos + 1) {
                Read::Value(v) => v,
                Read::Truncated => return trunc(op_name(op)),
            };
            (format!("{} expected={expected} t{target}", op_name(op)), 5)
        }
        crate::Opcode::Aggregate => match read_u32(code, pos) {
            Read::Value(v) => (format!("{} n={v}", op_name(op)), 4),
            Read::Truncated => trunc(op_name(op)),
        },
        crate::Opcode::Call | crate::Opcode::CallIndirect => {
            let argc = match read_u32(code, pos) {
                Read::Value(v) => v,
                Read::Truncated => return trunc(op_name(op)),
            };
            let dest = match read_u32(code, pos + 4) {
                Read::Value(v) => v,
                Read::Truncated => return trunc(op_name(op)),
            };
            let target = match read_u32(code, pos + 8) {
                Read::Value(v) => v,
                Read::Truncated => return trunc(op_name(op)),
            };
            (
                format!("{} argc={argc} dest={dest} t{target}", op_name(op)),
                12,
            )
        }
        // No-payload opcodes: arithmetic/logic/comparison, unary, control
        // flow, projections, and the terminators.
        crate::Opcode::Add
        | crate::Opcode::Sub
        | crate::Opcode::Mul
        | crate::Opcode::Div
        | crate::Opcode::Rem
        | crate::Opcode::Eq
        | crate::Opcode::Ne
        | crate::Opcode::Lt
        | crate::Opcode::Gt
        | crate::Opcode::Le
        | crate::Opcode::Ge
        | crate::Opcode::And
        | crate::Opcode::Or
        | crate::Opcode::Not
        | crate::Opcode::Neg
        | crate::Opcode::BitAnd
        | crate::Opcode::BitOr
        | crate::Opcode::BitXor
        | crate::Opcode::Shl
        | crate::Opcode::Shr
        | crate::Opcode::StoreField
        | crate::Opcode::Deref
        | crate::Opcode::Discriminant
        | crate::Opcode::Drop
        | crate::Opcode::Repeat
        | crate::Opcode::Return
        | crate::Opcode::Trap => (op_name(op).to_string(), 0),
    }
}

/// The canonical `OP_*` mnemonic for an opcode — the spelling the emitter,
/// the VM docs, and this crate's error messages all use (`LoadConst`'s
/// plain-upper conversion would render `OP_LOADCONST`, not
/// `OP_LOAD_CONST`). A new opcode must be given its mnemonic here, which is
/// the listing decision the exhaustive match forces.
pub(crate) fn op_name(op: crate::Opcode) -> &'static str {
    match op {
        crate::Opcode::LoadConst => "OP_LOAD_CONST",
        crate::Opcode::LoadConstF64 => "OP_LOAD_CONST_F64",
        crate::Opcode::LoadConstStr => "OP_LOAD_CONST_STR",
        crate::Opcode::Add => "OP_ADD",
        crate::Opcode::Sub => "OP_SUB",
        crate::Opcode::Mul => "OP_MUL",
        crate::Opcode::Div => "OP_DIV",
        crate::Opcode::Rem => "OP_REM",
        crate::Opcode::Eq => "OP_EQ",
        crate::Opcode::Ne => "OP_NE",
        crate::Opcode::Lt => "OP_LT",
        crate::Opcode::Gt => "OP_GT",
        crate::Opcode::Le => "OP_LE",
        crate::Opcode::Ge => "OP_GE",
        crate::Opcode::And => "OP_AND",
        crate::Opcode::Or => "OP_OR",
        crate::Opcode::Not => "OP_NOT",
        crate::Opcode::Neg => "OP_NEG",
        crate::Opcode::BitAnd => "OP_BITAND",
        crate::Opcode::BitOr => "OP_BITOR",
        crate::Opcode::BitXor => "OP_BITXOR",
        crate::Opcode::Shl => "OP_SHL",
        crate::Opcode::Shr => "OP_SHR",
        crate::Opcode::LoadLocal => "OP_LOAD_LOCAL",
        crate::Opcode::StoreLocal => "OP_STORE_LOCAL",
        crate::Opcode::LoadLocalAddr => "OP_LOAD_LOCAL_ADDR",
        crate::Opcode::StoreField => "OP_STORE_FIELD",
        crate::Opcode::Deref => "OP_DEREF",
        crate::Opcode::Return => "OP_RETURN",
        crate::Opcode::JumpIf => "OP_JUMP_IF",
        crate::Opcode::Jump => "OP_JUMP",
        crate::Opcode::Call => "OP_CALL",
        crate::Opcode::Cast => "OP_CAST",
        crate::Opcode::Aggregate => "OP_AGGREGATE",
        crate::Opcode::Discriminant => "OP_DISCRIMINANT",
        crate::Opcode::Len => "OP_LEN",
        crate::Opcode::SwitchInt => "OP_SWITCH_INT",
        crate::Opcode::Assert => "OP_ASSERT",
        crate::Opcode::CallIndirect => "OP_CALL_INDIRECT",
        crate::Opcode::Drop => "OP_DROP",
        crate::Opcode::Repeat => "OP_REPEAT",
        crate::Opcode::Trap => "OP_TRAP",
    }
}

/// Render a whole decoded module: the header line (format version, function
/// count, entry index, pool size), one listing per function, and — when the
/// module carries a string pool — the pool section (`OP_LOAD_CONST_STR`
/// indices resolve into it).
///
/// Deterministic (the same module renders byte-identical text), total (any
/// decoded module renders; corrupt code lists as `unknown(0xNN)` /
/// `<truncated>` rather than failing), and read-only.
#[must_use]
pub fn render_module(module: &Module) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "module glyim.module-image/{} functions={} entry={} strings={}",
        crate::IMAGE_VERSION,
        module.functions.len(),
        module.entry,
        module.strings.len()
    );
    for (i, f) in module.functions.iter().enumerate() {
        let _ = writeln!(
            out,
            "fn {i}: locals={} args={} blocks={} code={}B",
            f.n_locals,
            f.arg_count,
            f.block_offsets.len(),
            f.code.len()
        );
        out.push_str(&render_body(f, Some(&module.strings)));
    }
    if !module.strings.is_empty() {
        out.push_str("strings:\n");
        for (i, s) in module.strings.iter().enumerate() {
            let _ = writeln!(out, "  [{i}] {s:?}");
        }
    }
    debug!(
        functions = module.functions.len(),
        strings = module.strings.len(),
        "module image rendered"
    );
    out
}

#[cfg(test)]
mod disasm_tests {
    use super::*;
    use crate::{Function, Module, Opcode};

    fn i64le(v: i64) -> [u8; 8] {
        v.to_le_bytes()
    }
    fn u32le(v: u32) -> [u8; 4] {
        v.to_le_bytes()
    }
    fn op(o: Opcode) -> u8 {
        o as u8
    }

    /// helper(n) = n * 2 (blockless) + main returning helper(21); the same
    /// shape the image walkthrough uses, so the listing pins real emitted
    /// shapes. `strings` carries one entry the code never references (the
    /// pool section must still render).
    fn golden_module() -> Module {
        let mut helper = Vec::new();
        helper.push(op(Opcode::LoadConst));
        helper.extend_from_slice(&i64le(2));
        helper.push(op(Opcode::LoadLocal));
        helper.extend_from_slice(&u32le(1)); // MIR ABI: arg at local 1
        helper.push(op(Opcode::Mul));
        helper.push(op(Opcode::Return));
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(0)); // fn index 0: helper
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(21));
        main.push(op(Opcode::Call));
        main.extend_from_slice(&u32le(1)); // argc
        main.extend_from_slice(&u32le(0)); // dest local
        main.extend_from_slice(&u32le(31)); // resume byte offset
        main.push(op(Opcode::LoadLocal));
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Return));
        Module::with_strings(
            vec![Function::new(helper, 2, 1), Function::new(main, 1, 0)],
            vec!["abc".to_string()],
            1,
        )
    }

    const GOLDEN: &str = "\
module glyim.module-image/2 functions=2 entry=1 strings=1
fn 0: locals=2 args=1 blocks=0 code=16B
  0: OP_LOAD_CONST 2
  9: OP_LOAD_LOCAL 1
  14: OP_MUL
  15: OP_RETURN
fn 1: locals=1 args=0 blocks=0 code=37B
  0: OP_LOAD_CONST 0
  9: OP_LOAD_CONST 21
  18: OP_CALL argc=1 dest=0 t31
  31: OP_LOAD_LOCAL 0
  36: OP_RETURN
strings:
  [0] \"abc\"
";

    #[test]
    fn golden_pinned_listing() {
        assert_eq!(render_module(&golden_module()), GOLDEN);
    }

    #[test]
    fn render_is_deterministic() {
        let module = golden_module();
        assert_eq!(render_module(&module), render_module(&module));
    }

    /// A function with a block-offset table lists `bN@off:` labels, and a
    /// jump target reads against them.
    #[test]
    fn block_mode_lists_block_labels() {
        // b0: load_local 0; load_const 0; lt; jump_if t1; load_const 1;
        //     return
        // b1: load_const 2; return
        let mut code = Vec::new();
        code.push(op(Opcode::LoadLocal));
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(0));
        code.push(op(Opcode::Lt));
        code.push(op(Opcode::JumpIf));
        code.extend_from_slice(&u32le(1)); // block index 1
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(1));
        code.push(op(Opcode::Return));
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(2));
        code.push(op(Opcode::Return));
        let f = Function::with_blocks(code, 1, 1, vec![0, 30]);
        let text = render_function(&f);
        assert!(text.contains("  b0@0:\n"), "b0 label in {text}");
        assert!(text.contains("  b1@30:\n"), "b1 label in {text}");
        assert!(text.contains("OP_JUMP_IF t1"), "target in {text}");
        assert!(text.contains("OP_LT"), "op without payload in {text}");
    }

    /// The float constant lists both the IEEE-754 value and the raw bit
    /// pattern, so a corrupted bit pattern stays visible.
    #[test]
    fn float_constant_lists_value_and_bits() {
        let mut code = Vec::new();
        code.push(op(Opcode::LoadConstF64));
        code.extend_from_slice(&1.5f64.to_bits().to_le_bytes());
        code.push(op(Opcode::Return));
        let text = render_function(&Function::new(code, 1, 0));
        assert!(
            text.contains("OP_LOAD_CONST_F64 1.5 (bits 0x3ff8000000000000)"),
            "float listing in {text}"
        );
    }

    /// `OP_CAST` lists the cast kind and target names; unknown bytes stay
    /// visible as `unknown(0xNN)` instead of a bare number.
    #[test]
    fn cast_lists_kind_and_target_names() {
        let code = vec![
            op(Opcode::Cast),
            1, // FloatToInt
            6, // U8
            op(Opcode::Cast),
            1,    // FloatToInt
            0xEE, // unknown target byte
            op(Opcode::Return),
        ];
        let text = render_function(&Function::new(code, 1, 0));
        assert!(text.contains("OP_CAST FloatToInt->U8"), "cast in {text}");
        assert!(
            text.contains("OP_CAST FloatToInt->unknown(0xee)"),
            "unknown target in {text}"
        );
    }

    /// `OP_LOAD_CONST_STR` lists the pool index and the pool content; an
    /// out-of-range index says so instead of showing nothing.
    #[test]
    fn string_constant_lists_pool_content() {
        let mut code = Vec::new();
        code.push(op(Opcode::LoadConstStr));
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::LoadConstStr));
        code.extend_from_slice(&u32le(9));
        code.push(op(Opcode::Return));
        let module = Module::with_strings(
            vec![Function::new(code, 1, 0)],
            vec!["hello".to_string()],
            0,
        );
        let text = render_module(&module);
        assert!(
            text.contains("OP_LOAD_CONST_STR pool[0] \"hello\""),
            "pool ref in {text}"
        );
        assert!(
            text.contains("OP_LOAD_CONST_STR pool[9] <out of range>"),
            "out-of-range ref in {text}"
        );
    }

    /// Unknown opcode bytes render as `unknown(0xNN)` and consume nothing
    /// (the listing continues), so a corrupt artifact stays inspectable.
    #[test]
    fn unknown_opcode_renders_and_consumes_nothing() {
        let code = vec![0x42, op(Opcode::Return)]; // 0x42 is not an opcode
        let text = render_function(&Function::new(code, 0, 0));
        assert!(text.contains("0: unknown(0x42)"), "unknown in {text}");
        assert!(text.contains("1: OP_RETURN"), "listing continues in {text}");
    }

    /// The renderer is total over every possible byte: all 256 values list
    /// (as a mnemonic or `unknown(0xNN)`) without panicking.
    #[test]
    fn renderer_is_total_over_every_byte() {
        for b in 0u8..=255 {
            let code = vec![b, op(Opcode::Return)];
            let text = render_function(&Function::new(code, 0, 0));
            assert!(text.contains(": "), "byte {b:#04x} must list: {text}");
        }
    }

    /// A payload that runs past the end of the code lists `<truncated>`
    /// instead of panicking or silently wrapping.
    #[test]
    fn truncated_payload_lists_truncated() {
        let code = vec![op(Opcode::LoadConst), 0xAA];
        let text = render_function(&Function::new(code, 0, 0));
        assert!(
            text.contains("OP_LOAD_CONST <truncated>"),
            "truncation in {text}"
        );
    }
}
