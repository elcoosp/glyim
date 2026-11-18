//! Switch-dispatch bytecode virtual machine for the Glyim bytecode backend.
//!
//! Production-grade execution engine (Phase 7.1 → 7.x). The VM executes the
//! full opcode set the `glyim-codegen` bytecode backend can emit:
//!
//! - scalar arithmetic / logic / bitwise / shifts (`OP_ADD` … `OP_SHR`),
//! - comparisons (`OP_EQ` … `OP_GE`),
//! - control flow (`OP_JUMP`, `OP_JUMP_IF`, `OP_SWITCH_INT`, `OP_RETURN`),
//! - locals (`OP_LOAD_LOCAL`, `OP_STORE_LOCAL`, `OP_LOAD_LOCAL_ADDR`),
//! - an addressable per-frame memory model so `OP_STORE_FIELD` / `OP_DEREF`
//!   / `OP_LEN` / `OP_DISCRIMINANT` / `OP_INDEX`-style projections resolve,
//! - cross-function calls (`OP_CALL` / `OP_CALL_INDIRECT`) with a real call
//!   stack (recursion supported),
//! - `OP_AGGREGATE` (tuples), `OP_CAST` (the full cast semantics the
//!   interpreter implements: width-aware integer truncation, int/float
//!   conversion, pointer-kind identity), `OP_ASSERT`, `OP_REPEAT`,
//!   `OP_DROP`, `OP_TRAP`,
//! - float arithmetic (`OP_LOAD_CONST_F64` + IEEE-754 `f64` semantics for
//!   `OP_ADD` … `OP_SHR` over `Value::Float` operands, with `1.0 / 0.0` as
//!   `+inf`, matching LLVM's `fdiv` and the MIR interpreter).
//!
//! The decoder returns explicit, typed errors (`VmError::UnexpectedEndOfCode`
//! / `UnknownOpcode` / `StackUnderflow` / `LocalOutOfBounds` / `EmptyReturn`
//! / `AbnormalTermination` / `AssertionFailed` / `CallFrameOverflow` /
//! `FunctionOutOfBounds` / `SwitchNoTarget` / `BinOpTypeMismatch` /
//! `UnOpTypeMismatch` / `CastOperandMismatch` / `UnknownCastKind` /
//! `UnknownCastTarget` / `ExpectedInt` / `StringIndexOutOfBounds` /
//! `FuelExhausted`) rather than silently
//! mis-executing an unhandled opcode or a kind-mismatched operand, so
//! adding a new emitter opcode forces a deliberate VM decision.
//!
//! Wire format (operand sizes) mirrors `crates/glyim-codegen/src/lib.rs`
//! exactly: every `OP_*` that carries a payload uses little-endian `i64`
//! (integer constants, float constants as raw `f64` bits) or `u32`
//! (local/block/function indices) as the emitter writes. `OP_CAST` carries
//! two bytes: the cast kind, then the [`CastTarget`] primitive the cast
//! converts to.

pub mod backtrace;
pub mod callgraph;
pub mod coverage;
pub mod diff;
pub mod disasm;
pub mod image;
pub mod profile;
pub mod rundiff;
pub mod trace;
pub mod watch;

pub use backtrace::{
    BacktraceCollector, FaultFrame, FaultReport, FaultSink, FaultSite, render_backtrace,
};
pub use callgraph::{CallGraphCollector, render_call_graph};
pub use coverage::{CoverageCollector, render_coverage};
pub use diff::render_module_diff;
pub use disasm::{render_function, render_module};
pub use image::{IMAGE_MAGIC, IMAGE_VERSION, ImageError, decode_module, encode_module};
pub use profile::{HotOpcode, ProfileCollector, render_profile};
pub use rundiff::render_run_diff;
pub use trace::{StepDetail, TraceCollector, TraceSink, TracingSink, VmStep, render_trace};
pub use watch::{TracingWatchSink, WatchCollector, WatchEvent, WatchSink, WatchVia, render_watch};

/// A runtime value on the VM stack / in a local slot.
///
/// Scalars are `Int(i64)` (also used for bools: 0 = false, 1 = true, and as
/// opaque pointer/address slots in the per-frame memory model), `Float(f64)`
/// (pushed by `OP_LOAD_CONST_F64` and produced by float arithmetic — the
/// emitter emits float constants as raw `f64` bits, so without a dedicated
/// value kind the machine silently executed float arithmetic as *integer*
/// arithmetic over the bit patterns), and `Tuple` holds aggregate literals
/// produced by `OP_AGGREGATE`.
#[derive(Debug, Clone)]
pub enum Value {
    /// A 64-bit integer scalar (also used for bools/addresses).
    Int(i64),
    /// A 64-bit IEEE-754 float scalar.
    Float(f64),
    /// A byte string resolved from the module's string pool (pushed by
    /// `OP_LOAD_CONST_STR`; the pool travels inside the module image).
    Str(String),
    /// An aggregate literal (tuple) produced by `OP_AGGREGATE`.
    Tuple(Vec<Value>),
}

impl PartialEq for Value {
    /// Structural equality, except floats compare by **bit pattern** (the
    /// `InterpValue::Float` convention in `glyim-mir-interp`): `NaN == NaN`
    /// holds and `0.0 != -0.0`, so test assertions stay deterministic across
    /// platforms and an `Eq` implementor can never silently treat two
    /// different bit patterns as the same value.
    fn eq(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a.to_bits() == b.to_bits(),
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Tuple(a), Value::Tuple(b)) => a == b,
            _ => false,
        }
    }
}

impl Value {
    // No silent scalar coercion: the `as_int()` helper (which mapped every
    // non-Int kind to `0`) is gone — its last caller (the `OP_ASSERT`
    // condition) now goes through the typed [`VmError::ExpectedInt`] path
    // like every other operand role.

    /// The value kind's name, as reported in typed type-mismatch errors.
    fn kind_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "Int",
            Value::Float(_) => "Float",
            Value::Str(_) => "Str",
            Value::Tuple(_) => "Tuple",
        }
    }
}

/// Opcodes emitted by `glyim-codegen`'s bytecode backend.
///
/// The numeric values mirror `crates/glyim-codegen/src/lib.rs` exactly so the
/// VM and emitter agree on the wire format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Opcode {
    /// `Variant`.
    LoadConst = 0x01,
    /// `Variant`.
    Add = 0x02,
    /// `Variant`.
    Sub = 0x03,
    /// `Variant`.
    Mul = 0x04,
    /// `Variant`.
    Div = 0x05,
    /// `Variant`.
    Rem = 0x06,
    /// `Variant`.
    Eq = 0x07,
    /// `Variant`.
    Ne = 0x08,
    /// `Variant`.
    Lt = 0x09,
    /// `Variant`.
    Gt = 0x0A,
    /// `Variant`.
    Le = 0x0B,
    /// `Variant`.
    Ge = 0x0C,
    /// `Variant`.
    And = 0x0D,
    /// `Variant`.
    Or = 0x0E,
    /// `Variant`.
    Not = 0x0F,
    /// `Variant`.
    Neg = 0x10,
    /// `Variant`.
    BitAnd = 0x11,
    /// `Variant`.
    BitOr = 0x12,
    /// `Variant`.
    BitXor = 0x13,
    /// `Variant`.
    Shl = 0x14,
    /// `Variant`.
    Shr = 0x15,
    /// `Variant`.
    LoadLocal = 0x16,
    /// `Variant`.
    StoreLocal = 0x17,
    /// `Variant`.
    Return = 0x18,
    /// `Variant`.
    JumpIf = 0x19,
    /// `Variant`.
    Jump = 0x1A,
    /// `Variant`.
    Call = 0x1B,
    /// `Variant`.
    Cast = 0x1C,
    /// `Variant`.
    Aggregate = 0x1D,
    /// `Variant`.
    Discriminant = 0x1E,
    /// `Variant`.
    Len = 0x1F,
    /// `Variant`.
    SwitchInt = 0x20,
    /// `Variant`.
    Assert = 0x21,
    /// `Variant`.
    CallIndirect = 0x22,
    /// `OP_LOAD_CONST_F64`: push a 64-bit IEEE-754 float constant. The
    /// payload is 8 little-endian bytes of the `f64` bit pattern (the same
    /// width/endianness the emitter's integer constants use).
    LoadConstF64 = 0x23,
    /// `OP_LOAD_CONST_STR`: push a byte string resolved from the module's
    /// string pool. The payload is a `u32` pool index; the pool itself
    /// travels inside the module image (`Module::strings`), so string
    /// constants survive the process boundary as content, not as opaque
    /// interner indices.
    LoadConstStr = 0x24,
    /// `Variant`.
    LoadLocalAddr = 0x29,
    /// `Variant`.
    StoreField = 0x2A,
    /// `Variant`.
    Deref = 0x2B,
    /// `Variant`.
    Drop = 0x2C,
    /// `Variant`.
    Repeat = 0x2D,
    /// `Variant`.
    Trap = 0xFF,
}

impl Opcode {
    fn from_u8(b: u8) -> Option<Opcode> {
        Some(match b {
            0x01 => Opcode::LoadConst,
            0x02 => Opcode::Add,
            0x03 => Opcode::Sub,
            0x04 => Opcode::Mul,
            0x05 => Opcode::Div,
            0x06 => Opcode::Rem,
            0x07 => Opcode::Eq,
            0x08 => Opcode::Ne,
            0x09 => Opcode::Lt,
            0x0A => Opcode::Gt,
            0x0B => Opcode::Le,
            0x0C => Opcode::Ge,
            0x0D => Opcode::And,
            0x0E => Opcode::Or,
            0x0F => Opcode::Not,
            0x10 => Opcode::Neg,
            0x11 => Opcode::BitAnd,
            0x12 => Opcode::BitOr,
            0x13 => Opcode::BitXor,
            0x14 => Opcode::Shl,
            0x15 => Opcode::Shr,
            0x16 => Opcode::LoadLocal,
            0x17 => Opcode::StoreLocal,
            0x18 => Opcode::Return,
            0x19 => Opcode::JumpIf,
            0x1A => Opcode::Jump,
            0x1B => Opcode::Call,
            0x1C => Opcode::Cast,
            0x1D => Opcode::Aggregate,
            0x1E => Opcode::Discriminant,
            0x1F => Opcode::Len,
            0x20 => Opcode::SwitchInt,
            0x21 => Opcode::Assert,
            0x22 => Opcode::CallIndirect,
            0x23 => Opcode::LoadConstF64,
            0x24 => Opcode::LoadConstStr,
            0x29 => Opcode::LoadLocalAddr,
            0x2A => Opcode::StoreField,
            0x2B => Opcode::Deref,
            0x2C => Opcode::Drop,
            0x2D => Opcode::Repeat,
            0xFF => Opcode::Trap,
            _ => return None,
        })
    }

    /// The opcode's mnemonic, as it appears in typed mismatch errors.
    fn name(self) -> &'static str {
        match self {
            Opcode::LoadConst => "LoadConst",
            Opcode::Add => "Add",
            Opcode::Sub => "Sub",
            Opcode::Mul => "Mul",
            Opcode::Div => "Div",
            Opcode::Rem => "Rem",
            Opcode::Eq => "Eq",
            Opcode::Ne => "Ne",
            Opcode::Lt => "Lt",
            Opcode::Gt => "Gt",
            Opcode::Le => "Le",
            Opcode::Ge => "Ge",
            Opcode::And => "And",
            Opcode::Or => "Or",
            Opcode::Not => "Not",
            Opcode::Neg => "Neg",
            Opcode::BitAnd => "BitAnd",
            Opcode::BitOr => "BitOr",
            Opcode::BitXor => "BitXor",
            Opcode::Shl => "Shl",
            Opcode::Shr => "Shr",
            Opcode::LoadLocal => "LoadLocal",
            Opcode::StoreLocal => "StoreLocal",
            Opcode::Return => "Return",
            Opcode::JumpIf => "JumpIf",
            Opcode::Jump => "Jump",
            Opcode::Call => "Call",
            Opcode::Cast => "Cast",
            Opcode::Aggregate => "Aggregate",
            Opcode::Discriminant => "Discriminant",
            Opcode::Len => "Len",
            Opcode::SwitchInt => "SwitchInt",
            Opcode::Assert => "Assert",
            Opcode::CallIndirect => "CallIndirect",
            Opcode::LoadConstF64 => "LoadConstF64",
            Opcode::LoadConstStr => "LoadConstStr",
            Opcode::LoadLocalAddr => "LoadLocalAddr",
            Opcode::StoreField => "StoreField",
            Opcode::Deref => "Deref",
            Opcode::Drop => "Drop",
            Opcode::Repeat => "Repeat",
            Opcode::Trap => "Trap",
        }
    }
}

/// A single compiled function: its raw instruction bytes, the number of
/// local slots (+ argument count) it requires on a call frame, and a table
/// mapping MIR basic-block indices to byte offsets within `code`. Jump
/// targets in the emitted bytecode are block indices; `block_offsets` lets
/// the VM resolve them. When `block_offsets` is empty, targets are treated as
/// raw byte offsets (used by hand-assembled tests).
///
/// Call-frame ABI (the emitter's MIR convention): local 0 is the return
/// place, and the call arguments occupy locals `1..=arg_count` on the
/// callee's frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    /// Raw instruction bytes.
    pub code: Vec<u8>,
    /// Number of local slots to allocate for a call frame.
    pub n_locals: usize,
    /// Number of leading locals that are call arguments.
    pub arg_count: usize,
    /// Basic-block-index → byte-offset resolution table (may be empty).
    pub block_offsets: Vec<usize>,
}

impl Function {
    /// new.
    #[must_use]
    pub fn new(code: Vec<u8>, n_locals: usize, arg_count: usize) -> Function {
        Function {
            code,
            n_locals,
            arg_count,
            block_offsets: Vec::new(),
        }
    }

    /// Build a function with an explicit basic-block offset table.
    #[must_use]
    pub fn with_blocks(
        code: Vec<u8>,
        n_locals: usize,
        arg_count: usize,
        block_offsets: Vec<usize>,
    ) -> Function {
        Function {
            code,
            n_locals,
            arg_count,
            block_offsets,
        }
    }

    /// Resolve a jump target. Block-index mode is used when a non-empty
    /// `block_offsets` table is present; otherwise the target is a raw byte
    /// offset.
    fn resolve_target(&self, target: u32) -> usize {
        // T040-PATCHED [BC-4]: the emitter produces sentinel targets
        // (`u32::MAX`) for diverging calls / bounds traps. Indexing
        // `block_offsets[u32::MAX]` panicked the host process. Return a
        // sentinel and let `drive` translate it to
        // `VmError::AbnormalTermination`.
        if self.block_offsets.is_empty() {
            target as usize
        } else {
            self.block_offsets
                .get(target as usize)
                .copied()
                .unwrap_or(usize::MAX)
        }
    }
}

/// A module: a table of functions plus an entry-function index.
///
/// The VM executes functions by index; `OP_CALL`/`OP_CALL_INDIRECT` reference
/// the same index space used by the emitter's `fn_table`. `strings` is the
/// module's byte-string pool — the table `OP_LOAD_CONST_STR` indices resolve
/// into, carried inside the image so string constants survive a process
/// boundary (version-1 images and `Module::new`-built modules simply carry
/// an empty pool).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    /// Functions indexed by the emitter's function indices.
    pub functions: Vec<Function>,
    /// Index of the entry function executed by [`Module::run`].
    pub entry: usize,
    /// The byte-string pool `OP_LOAD_CONST_STR` indices resolve into.
    pub strings: Vec<String>,
}

impl Module {
    /// new.
    #[must_use]
    pub fn new(functions: Vec<Function>, entry: usize) -> Module {
        Module {
            functions,
            entry,
            strings: Vec::new(),
        }
    }

    /// Build a module with an explicit string pool (the shape the emitter's
    /// `generate_module_image` produces).
    #[must_use]
    pub fn with_strings(functions: Vec<Function>, strings: Vec<String>, entry: usize) -> Module {
        Module {
            functions,
            entry,
            strings,
        }
    }

    /// Execute the module's entry function with no arguments, returning its
    /// result value (or the first error encountered).
    pub fn run(&self) -> ExecResult<Value> {
        let mut vm = Vm::new();
        vm.run_module(self)
    }
}

/// Result of execution.
pub type ExecResult<T> = Result<T, VmError>;

/// Errors the VM can encounter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmError {
    /// Ran past the end of a function's code without a `Return`.
    UnexpectedEndOfCode,
    /// A `u8` did not decode to a known opcode.
    UnknownOpcode(u8),
    /// Stack underflow (popped with too few operands).
    StackUnderflow,
    /// Local index out of bounds.
    LocalOutOfBounds(usize),
    /// A `Return` executed with an empty stack.
    EmptyReturn,
    /// `OP_TRAP` / `OP_UNREACHABLE` was executed.
    AbnormalTermination,
    /// `OP_ASSERT` condition was `expected == false`.
    AssertionFailed,
    /// Call stack exceeded the configured depth limit.
    CallFrameOverflow,
    /// `OP_CALL`/`OP_CALL_INDIRECT` referenced a function index out of range.
    FunctionOutOfBounds(usize),
    /// `OP_SWITCH_INT` discriminant did not match any arm and there is no
    /// otherwise-target (should not happen for well-formed bytecode).
    SwitchNoTarget,
    /// An arithmetic/logic binary op received operand kinds it cannot
    /// operate on: float arithmetic over an `Int` operand, bitwise/shift
    /// ops over `Float` operands, or arithmetic over a `Tuple`. Every one
    /// of these was, before the float seam, silently mis-executed (floats
    /// ran as integer ops over the raw bit patterns, tuples read as 0).
    BinOpTypeMismatch {
        /// The opcode that rejected the operands (e.g. `"Add"`).
        op: &'static str,
        /// The left operand's kind name (`"Int"` / `"Float"` / `"Tuple"`).
        left: &'static str,
        /// The right operand's kind name.
        right: &'static str,
    },
    /// A unary op (`OP_NOT` / `OP_NEG`) received a value kind it cannot
    /// operate on (`OP_NOT` over a `Float`, say).
    UnOpTypeMismatch {
        /// The opcode that rejected the operand (e.g. `"Not"`).
        op: &'static str,
        /// The operand's kind name.
        operand: &'static str,
    },
    /// `OP_CAST` was asked to convert an operand whose kind the cast cannot
    /// convert (e.g. `FloatToInt` over an `Int` operand) — the interpreter
    /// panics on the same shape, so this is its explicit VM counterpart.
    CastOperandMismatch {
        /// The cast kind that rejected the operand (e.g. `"FloatToInt"`).
        kind: &'static str,
        /// The operand's kind name.
        operand: &'static str,
    },
    /// `OP_CAST` carried a cast-kind byte the VM does not know.
    UnknownCastKind(u8),
    /// `OP_CAST` carried a target-type byte the VM does not know.
    UnknownCastTarget(u8),
    /// An operand was expected to be an `Int` (an address, a switch
    /// discriminant, a call target, a branch condition, or a repeat count)
    /// but was another kind. Before the float seam these read as a silent
    /// `0`.
    ExpectedInt {
        /// What the operand was used as (e.g. `"address"`).
        role: &'static str,
        /// The kind name actually found.
        found: &'static str,
    },
    /// `OP_LOAD_CONST_STR` referenced a pool index the module's string pool
    /// does not have (the pool travels inside the image, so this is a
    /// corrupt or hand-mis-assembled artifact, not a runtime condition).
    StringIndexOutOfBounds(usize),
    /// The execution budget set by [`Vm::with_fuel`] was spent: the machine
    /// refused a dispatch after `executed` dispatches had run. The embedded
    /// execution story's runtime complement of [`decode_module`]'s
    /// structural validation — an infinite loop stops with this explicit
    /// error instead of hanging the embedding host.
    FuelExhausted {
        /// How many dispatches ran before the budget was spent.
        executed: u64,
    },
}

impl std::fmt::Display for VmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VmError::UnexpectedEndOfCode => {
                write!(f, "ran past the end of a function's code without a Return")
            }
            VmError::UnknownOpcode(raw) => write!(f, "unknown opcode byte 0x{raw:02x}"),
            VmError::StackUnderflow => {
                write!(f, "stack underflow (popped with too few operands)")
            }
            VmError::LocalOutOfBounds(idx) => write!(f, "local index {idx} out of bounds"),
            VmError::EmptyReturn => write!(f, "a Return executed with an empty stack"),
            VmError::AbnormalTermination => write!(f, "OP_TRAP / OP_UNREACHABLE executed"),
            VmError::AssertionFailed => write!(f, "OP_ASSERT condition did not hold"),
            VmError::CallFrameOverflow => write!(f, "call stack exceeded the depth limit"),
            VmError::FunctionOutOfBounds(idx) => {
                write!(f, "call referenced function index {idx} out of range")
            }
            VmError::SwitchNoTarget => {
                write!(
                    f,
                    "OP_SWITCH_INT discriminant matched no arm and no otherwise"
                )
            }
            VmError::BinOpTypeMismatch { op, left, right } => {
                write!(f, "{op} cannot operate on ({left}, {right}) operands")
            }
            VmError::UnOpTypeMismatch { op, operand } => {
                write!(f, "{op} cannot operate on a {operand} operand")
            }
            VmError::CastOperandMismatch { kind, operand } => {
                write!(f, "cast {kind} cannot convert a {operand} operand")
            }
            VmError::UnknownCastKind(kind) => write!(f, "unknown cast-kind byte 0x{kind:02x}"),
            VmError::UnknownCastTarget(target) => {
                write!(f, "unknown cast-target byte 0x{target:02x}")
            }
            VmError::ExpectedInt { role, found } => {
                write!(f, "expected an Int for the {role}, found {found}")
            }
            VmError::StringIndexOutOfBounds(idx) => {
                write!(f, "string pool index {idx} out of range")
            }
            VmError::FuelExhausted { executed } => {
                write!(f, "execution budget spent after {executed} dispatches")
            }
        }
    }
}

impl std::error::Error for VmError {}

/// The target primitive type an `OP_CAST` converts to, written by the emitter
/// as the cast's second operand byte.
///
/// The numbering is the VM crate's own (the VM has no dependency on the type
/// crates); the emitter maps `TyKind::Int(IntTy::I8)`-style kinds onto these
/// variants. `Unknown` is the emitter's fallback when no type context is
/// available or the target is not a primitive — every cast over it is the
/// identity, which is exactly what the pre-seam VM did for *all* casts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CastTarget {
    /// No target type available: casts are the identity.
    Unknown = 0,
    /// `i8`.
    I8 = 1,
    /// `i16`.
    I16 = 2,
    /// `i32`.
    I32 = 3,
    /// `i64`.
    I64 = 4,
    /// `isize`.
    Isize = 5,
    /// `u8`.
    U8 = 6,
    /// `u16`.
    U16 = 7,
    /// `u32`.
    U32 = 8,
    /// `u64`.
    U64 = 9,
    /// `usize`.
    Usize = 10,
    /// `f32` (a value-level identity in this VM, whose floats are `f64`).
    F32 = 11,
    /// `f64`.
    F64 = 12,
    /// `bool` (identity at the value level).
    Bool = 13,
    /// `char` (identity at the value level).
    Char = 14,
}

impl CastTarget {
    /// Decode a target-type byte, or `None` if the byte is not a
    /// [`CastTarget`] (an explicit [`VmError::UnknownCastTarget`] for the
    /// caller).
    #[must_use]
    pub fn from_u8(b: u8) -> Option<CastTarget> {
        Some(match b {
            0 => CastTarget::Unknown,
            1 => CastTarget::I8,
            2 => CastTarget::I16,
            3 => CastTarget::I32,
            4 => CastTarget::I64,
            5 => CastTarget::Isize,
            6 => CastTarget::U8,
            7 => CastTarget::U16,
            8 => CastTarget::U32,
            9 => CastTarget::U64,
            10 => CastTarget::Usize,
            11 => CastTarget::F32,
            12 => CastTarget::F64,
            13 => CastTarget::Bool,
            14 => CastTarget::Char,
            _ => return None,
        })
    }
}

/// Apply an `OP_CAST`: convert `operand` per the emitter's cast-kind byte and
/// [`CastTarget`], mirroring the MIR interpreter's cast semantics
/// (`glyim-mir-interp`'s `Rvalue::Cast` arm, MIR-14):
///
/// - `IntToInt` and `FloatToInt` are a *real truncation* to the target
///   width/signedness (`(-1i32) as u8 == 255`), computed through `i128` so
///   the narrow-width wrap and the float saturation range match the
///   interpreter exactly;
/// - `IntToFloat` converts, `FloatToFloat` is the `f64`-level identity;
/// - pointer kinds are the identity (the VM's addresses are plain `Int`s —
///   the interpreter documents the same value-level identity for
///   `PtrToPtr`/`FnPtrToPtr`);
/// - an operand whose kind the cast cannot convert (e.g. `FloatToInt` over
///   an `Int`) is an explicit [`VmError::CastOperandMismatch`], the
///   interpreter's panic counterpart.
fn cast_value(kind: u8, target: CastTarget, operand: Value) -> ExecResult<Value> {
    const CAST_INT_TO_INT: u8 = 0;
    const CAST_FLOAT_TO_INT: u8 = 1;
    const CAST_INT_TO_FLOAT: u8 = 2;
    const CAST_PTR_TO_PTR: u8 = 3;
    const CAST_FN_PTR_TO_PTR: u8 = 4;
    const CAST_PTR_TO_INT: u8 = 5;
    const CAST_INT_TO_PTR: u8 = 6;
    const CAST_FLOAT_TO_FLOAT: u8 = 7;
    match kind {
        CAST_INT_TO_INT => Ok(match operand {
            Value::Int(v) => Value::Int(trunc_i128(i128::from(v), target) as i64),
            // The interpreter's IntToInt passes non-int operands through
            // unchanged (its `other => Ok(other)` arm).
            other => other,
        }),
        CAST_FLOAT_TO_INT => match operand {
            Value::Float(f) => Ok(Value::Int(trunc_i128(f as i128, target) as i64)),
            other => Err(VmError::CastOperandMismatch {
                kind: "FloatToInt",
                operand: other.kind_name(),
            }),
        },
        CAST_INT_TO_FLOAT => match operand {
            Value::Int(v) => Ok(Value::Float(v as f64)),
            other => Err(VmError::CastOperandMismatch {
                kind: "IntToFloat",
                operand: other.kind_name(),
            }),
        },
        CAST_FLOAT_TO_FLOAT => match operand {
            Value::Float(f) => Ok(Value::Float(f)),
            other => Err(VmError::CastOperandMismatch {
                kind: "FloatToFloat",
                operand: other.kind_name(),
            }),
        },
        // Pointer casts: identity at this value level (addresses are Ints).
        CAST_PTR_TO_PTR | CAST_FN_PTR_TO_PTR | CAST_PTR_TO_INT | CAST_INT_TO_PTR => Ok(operand),
        _ => Err(VmError::UnknownCastKind(kind)),
    }
}

/// Truncate an `i128` to the target primitive's width/signedness — the VM
/// mirror of the interpreter's `trunc_to_ty` (MIR-14): a wrap through the
/// narrower width, then sign/zero extension back into the wide form. Unknown
/// and non-integer targets leave the value unchanged (the interpreter's
/// fallthrough).
fn trunc_i128(v: i128, target: CastTarget) -> i128 {
    match target {
        CastTarget::I8 => i128::from(v as i8),
        CastTarget::I16 => i128::from(v as i16),
        CastTarget::I32 => i128::from(v as i32),
        CastTarget::I64 => i128::from(v as i64),
        CastTarget::Isize => v as isize as i128,
        CastTarget::U8 => i128::from(v as u8),
        CastTarget::U16 => i128::from(v as u16),
        CastTarget::U32 => i128::from(v as u32),
        CastTarget::U64 => i128::from(v as u64),
        CastTarget::Usize => (v as usize) as i128,
        _ => v,
    }
}

/// Maximum call-stack depth, to guard against infinite recursion.
const MAX_CALL_DEPTH: usize = 1024;

/// A single call frame: locals, an addressable memory region, the current
/// function index, program counter, and the caller's resume target (the
/// block index to jump to when this frame returns).
struct Frame {
    locals: Vec<Value>,
    /// Addressable scratch memory, indexed by `LoadLocalAddr`/`StoreField`.
    mem: Vec<Value>,
    func: usize,
    pc: usize,
    /// Block index to jump to in the *caller* once this frame returns.
    resume_target: u32,
    /// Local slot in the *caller* that receives this frame's return value.
    dest_local: usize,
}

/// The virtual machine: a stack of call frames plus the shared operand stack.
#[derive(Default)]
pub struct Vm {
    /// The operand stack (shared across the whole execution).
    stack: Vec<Value>,
    /// The call stack.
    frames: Vec<Frame>,
    /// The attached execution-trace sink, if any (`None` = the seam costs
    /// one check per dispatch — exactly the pre-seam behavior).
    trace: Option<Box<dyn TraceSink>>,
    /// The attached local-watch sink ([`crate::watch`]), if any: it fires
    /// once per successful local write — the program's `OP_STORE_LOCAL`
    /// stores, the call-argument binds, and the callee-return dest write
    /// (`None` = one check per *write* — the pre-seam behavior).
    watch: Option<Box<dyn crate::watch::WatchSink>>,
    /// The attached fault-backtrace sink ([`crate::backtrace`]), if any: it
    /// fires once when a run ends in a [`VmError`] (`None` = one check per
    /// *failed* run — a clean run pays nothing beyond the site stores).
    fault: Option<Box<dyn crate::backtrace::FaultSink>>,
    /// The machine's current dispatch site — the function index of the
    /// dispatch in flight (the backtrace seam's fault-site tracking). Set
    /// at every dispatch, and to the entry request before the first one, so
    /// a fault report is exact without touching the hot match.
    fault_func: usize,
    /// The loop-top pc paired with [`Vm::fault_func`] (for a pre-execution
    /// fault, `0`).
    fault_pc: usize,
    /// The remaining execution budget ([`Vm::with_fuel`]); `None` = unlimited
    /// (the pre-seam behavior).
    fuel: Option<u64>,
    /// The lifetime dispatch count: every instruction dispatch this VM has
    /// performed (including the faulting one that ends a run with an error).
    executed: u64,
}

impl Vm {
    /// Create an empty VM (no frames).
    #[must_use]
    pub fn new() -> Vm {
        Vm::default()
    }

    /// Attach an execution-trace sink ([`crate::trace`]): every instruction
    /// dispatch is handed to it just before the instruction executes. The
    /// sink observes only — it never changes execution — and `Vm::take_trace`
    /// hands it back after the run.
    #[must_use]
    pub fn with_trace(mut self, sink: Box<dyn TraceSink>) -> Vm {
        self.trace = Some(sink);
        self
    }

    /// Detach and return the trace sink (the typed owner recovers it through
    /// `TraceSink::as_any`). `None` when no sink was attached.
    pub fn take_trace(&mut self) -> Option<Box<dyn TraceSink>> {
        self.trace.take()
    }

    /// Attach a fault-backtrace sink ([`crate::backtrace`]): the sink is
    /// handed one [`crate::backtrace::FaultReport`] when a run ends in a
    /// [`VmError`] — the fault site, the suspended caller chain, the
    /// dispatch count, and the fuel state. The sink observes only — the
    /// caller receives the same error either way — and [`Vm::take_backtrace`]
    /// hands it back after the run.
    #[must_use]
    pub fn with_backtrace(mut self, sink: Box<dyn crate::backtrace::FaultSink>) -> Vm {
        self.fault = Some(sink);
        self
    }

    /// Detach and return the fault-backtrace sink (the typed owner recovers
    /// it through [`crate::backtrace::FaultSink::as_any`]). `None` when no
    /// sink was attached.
    pub fn take_backtrace(&mut self) -> Option<Box<dyn crate::backtrace::FaultSink>> {
        self.fault.take()
    }

    /// Attach a local-watch sink ([`crate::watch`]): every successful local
    /// write — the program's `OP_STORE_LOCAL` stores, the call-argument
    /// binds, and the callee-return dest write — is handed to it just after
    /// the slot is updated. The sink observes only — it never changes
    /// execution — and [`Vm::take_watch`] hands it back after the run.
    #[must_use]
    pub fn with_watch(mut self, sink: Box<dyn crate::watch::WatchSink>) -> Vm {
        self.watch = Some(sink);
        self
    }

    /// Detach and return the local-watch sink (the typed owner recovers it
    /// through [`crate::watch::WatchSink::as_any`]). `None` when no sink
    /// was attached.
    pub fn take_watch(&mut self) -> Option<Box<dyn crate::watch::WatchSink>> {
        self.watch.take()
    }

    /// Set an execution budget: the machine refuses a dispatch once `fuel`
    /// dispatches have run, returning [`VmError::FuelExhausted`] — the
    /// runtime complement of [`decode_module`]'s structural validation, so
    /// an infinite loop in a loaded artifact stops with an explicit error
    /// instead of hanging the embedding host. [`Vm::executed`] reports the
    /// count either way.
    #[must_use]
    pub fn with_fuel(mut self, fuel: u64) -> Vm {
        self.fuel = Some(fuel);
        self
    }

    /// How many instruction dispatches this VM has performed in its lifetime
    /// (including the faulting dispatch that ended a run with an error).
    #[must_use]
    pub fn executed(&self) -> u64 {
        self.executed
    }

    /// Execute the module's entry function.
    pub fn run_module(&mut self, module: &Module) -> ExecResult<Value> {
        // The fault-site tracking starts at the entry request: a pre-
        // execution fault (an empty function table, a bad entry index, a
        // depth overflow on the entry push) reports where the run was
        // asked to begin, with zero dispatches.
        self.fault_func = module.entry;
        self.fault_pc = 0;
        if module.functions.is_empty() {
            return Err(self.report_fault(module, VmError::FunctionOutOfBounds(0)));
        }
        // Push the entry frame. The entry has no caller, so dest_local is
        // irrelevant and resume_target is ignored.
        if let Err(err) = self.push_frame(module, module.entry, &[], u32::MAX, usize::MAX) {
            return Err(self.report_fault(module, err));
        }
        self.drive(module)
    }

    /// Build the fault report for a run that ended in `error` (the
    /// backtrace seam): the tracked dispatch site, the suspended caller
    /// chain (innermost caller first), the dispatch count, and the fuel
    /// state. Called exactly once per failed run, before the error reaches
    /// the caller — the seam observes, it never rewrites the error.
    fn report_fault(&mut self, module: &Module, error: VmError) -> VmError {
        let site = crate::backtrace::FaultSite {
            func: self.fault_func,
            pc: self.fault_pc,
            // The pre-execution faults (an empty function table, a bad
            // entry index, a depth overflow on the entry push) have no
            // dispatched instruction to decode; the site's pc and the
            // zero dispatch count say so.
            opcode: module
                .functions
                .get(self.fault_func)
                .and_then(|f| f.code.get(self.fault_pc))
                .copied()
                .and_then(Opcode::from_u8),
        };
        // Every frame but the one the site belongs to is a suspended
        // caller; innermost caller first (a Rust-style backtrace order).
        let callers: Vec<crate::backtrace::FaultFrame> = self
            .frames
            .iter()
            .rev()
            .skip(1)
            .map(|f| crate::backtrace::FaultFrame {
                func: f.func,
                pc: f.pc,
            })
            .collect();
        let report = crate::backtrace::FaultReport {
            error: error.clone(),
            site,
            callers,
            executed: self.executed,
            fuel_left: self.fuel,
        };
        if let Some(sink) = self.fault.as_mut() {
            sink.fault(&report);
        }
        error
    }

    /// Inspect a local slot of the top call frame (test/debug helper).
    #[must_use]
    pub fn local(&self, idx: usize) -> Option<Value> {
        self.frames.last().and_then(|f| f.locals.get(idx).cloned())
    }

    /// Execute a single function (no calls in/out). Convenience used by the
    /// original single-function tests and simple cross-backend checks.
    pub fn run(&mut self, chunk: &Chunk) -> ExecResult<Value> {
        let module = Module::new(
            vec![Function::new(chunk.code.clone(), chunk.n_locals, 0)],
            0,
        );
        self.run_module(&module)
    }

    /// Push a fresh call frame. Validates depth and function index, allocates
    /// locals (zero-initialised, with the leading `arg_count` slots filled
    /// from `args`), and sets the frame's metadata.
    fn push_frame(
        &mut self,
        module: &Module,
        func: usize,
        args: &[Value],
        resume_target: u32,
        dest_local: usize,
    ) -> ExecResult<()> {
        if self.frames.len() >= MAX_CALL_DEPTH {
            return Err(VmError::CallFrameOverflow);
        }
        let f = module
            .functions
            .get(func)
            .ok_or(VmError::FunctionOutOfBounds(func))?;
        // MIR ABI (the emitter's and the interpreter's convention): local 0
        // is the return place, call arguments occupy locals 1..=arg_count.
        // Binding args at locals[0..] here made every emitted call read its
        // first argument as the zero-initialised return slot instead (the
        // multi-function differential caught it: the single-function path
        // never passes args, so the mismatch hid until cross-function
        // `OP_CALL` ran).
        let n_locals = f.n_locals.max(args.len() + 1);
        let mut locals = vec![Value::Int(0); n_locals];
        for (i, a) in args.iter().enumerate() {
            locals[i + 1] = a.clone();
        }
        let mem_size = (n_locals * 8).max(8);
        self.frames.push(Frame {
            locals,
            mem: vec![Value::Int(0); mem_size],
            func,
            pc: 0,
            resume_target,
            dest_local,
        });
        // The local-watch seam's argument-bind events: the MIR ABI's
        // argument landing is a write into the callee's locals
        // `1..=arg_count`, recorded per slot (only when a sink is attached
        // — the clones below are the sink's copies, not the frame's).
        if self.watch.is_some() {
            for (i, a) in args.iter().enumerate() {
                self.record_watch(
                    func,
                    i + 1,
                    a.clone(),
                    self.frames.len(),
                    crate::watch::WatchVia::ArgBind,
                );
            }
        }
        Ok(())
    }

    /// The single, non-recursive execution driver. Every frame (including the
    /// entry) is pushed onto the heap-allocated `frames` stack; `OP_CALL`
    /// pushes a new frame and continues, `OP_RETURN` pops a frame and writes
    /// its value into the caller's recorded `dest_local`. This keeps host
    /// (Rust) stack depth constant regardless of VM call depth, so deep and
    /// even infinite VM recursion are handled by the `MAX_CALL_DEPTH` guard
    /// instead of overflowing the host stack. A run that ends in an error
    /// reports through the backtrace seam ([`Vm::with_backtrace`]) exactly
    /// once, then returns the error untouched.
    fn drive(&mut self, module: &Module) -> ExecResult<Value> {
        let result = self.drive_inner(module);
        if let Err(err) = &result {
            // The backtrace seam's hook: one call per failed run, before
            // the error reaches the caller (the seam observes — the error
            // the caller receives is the one the machine produced).
            self.report_fault(module, err.clone());
        }
        result
    }

    /// The dispatch loop [`Vm::drive`] wraps. Every error path funnels out
    /// of here with the fault-site tracking ([`Vm::fault_func`] /
    /// [`Vm::fault_pc`]) already pointing at the dispatch in flight — the
    /// loop top refreshes it before the fuel gate and the bounds check, so
    /// a refused dispatch reports the dispatch that was refused.
    fn drive_inner(&mut self, module: &Module) -> ExecResult<Value> {
        loop {
            // Borrow split: fetch the pc and current function from the top
            // frame without holding a mutable borrow across the big match.
            if self.frames.is_empty() {
                // The entry frame returned without producing a value on the
                // operand stack (should have pushed its retval before Return).
                // The fault-site tracking still points at the entry frame's
                // last dispatch — the `OP_RETURN` that ended the run.
                return self
                    .pop()
                    .map_err(|_| self.report_fault(module, VmError::EmptyReturn));
            }
            let frame_idx = self.frames.len() - 1;
            let (pc, func) = {
                let fr = &self.frames[frame_idx];
                (fr.pc, fr.func)
            };
            // Fault-site tracking (the backtrace seam): two integer stores
            // per dispatch keep the machine's current site visible; the
            // hook itself fires only on the fault.
            self.fault_func = func;
            self.fault_pc = pc;
            let code = &module.functions[func].code;
            if pc >= code.len() {
                return Err(VmError::UnexpectedEndOfCode);
            }
            let raw = code[pc];
            let decoded = Opcode::from_u8(raw);
            // Fuel gate (the `with_fuel` seam): a refused dispatch never
            // executes and never reaches the trace hook, so a budget spent
            // mid-run leaves the trace exactly at the last executed step.
            if let Some(fuel) = self.fuel.as_mut() {
                if *fuel == 0 {
                    return Err(VmError::FuelExhausted {
                        executed: self.executed,
                    });
                }
                *fuel -= 1;
            }
            self.executed += 1;
            // Execution-trace hook (the `trace` module's seam): record the
            // dispatch *before* the instruction executes, so a faulting
            // dispatch is the trace's last entry. Unknown opcode bytes are
            // recorded too (`opcode: None`) — the trace is total over
            // faulting runs. With no sink attached this is one `None` check.
            if let Some(sink) = self.trace.as_mut() {
                let detail = decoded
                    .map(|op| crate::trace::step_detail(code, &module.strings, pc + 1, op))
                    .unwrap_or(StepDetail::None);
                sink.step(&VmStep {
                    func,
                    pc,
                    raw,
                    opcode: decoded,
                    frame_depth: self.frames.len(),
                    stack_depth: self.stack.len(),
                    detail,
                });
            }
            let op = decoded.ok_or(VmError::UnknownOpcode(raw))?;
            self.frames[frame_idx].pc = pc + 1;

            match op {
                Opcode::LoadConst => {
                    let v = Vm::read_i64(code, &mut self.frames[frame_idx].pc)?;
                    self.stack.push(Value::Int(v));
                }
                Opcode::LoadConstF64 => {
                    let bits = Vm::read_i64(code, &mut self.frames[frame_idx].pc)?;
                    self.stack.push(Value::Float(f64::from_bits(bits as u64)));
                }
                Opcode::LoadConstStr => {
                    let idx = Vm::read_u32(code, &mut self.frames[frame_idx].pc)? as usize;
                    let s = module
                        .strings
                        .get(idx)
                        .ok_or(VmError::StringIndexOutOfBounds(idx))?
                        .clone();
                    self.stack.push(Value::Str(s));
                }
                Opcode::LoadLocal => {
                    let idx = Vm::read_u32(code, &mut self.frames[frame_idx].pc)? as usize;
                    let v = self.local_ref(idx)?.clone();
                    self.stack.push(v);
                }
                Opcode::StoreLocal => {
                    let idx = Vm::read_u32(code, &mut self.frames[frame_idx].pc)? as usize;
                    let v = self.pop()?;
                    self.set_local(idx, v)?;
                }
                Opcode::LoadLocalAddr => {
                    let idx = Vm::read_u32(code, &mut self.frames[frame_idx].pc)? as usize;
                    self.stack.push(Value::Int(idx as i64));
                }
                Opcode::StoreField => {
                    let addr = self.pop_int("address")? as usize;
                    let v = self.pop()?;
                    self.set_mem(addr, v)?;
                }
                Opcode::Deref => {
                    let addr = self.pop_int("address")? as usize;
                    let v = self.mem(addr)?.clone();
                    self.stack.push(v);
                }
                Opcode::Add
                | Opcode::Sub
                | Opcode::Mul
                | Opcode::Div
                | Opcode::Rem
                | Opcode::Eq
                | Opcode::Ne
                | Opcode::Lt
                | Opcode::Gt
                | Opcode::Le
                | Opcode::Ge
                | Opcode::And
                | Opcode::Or
                | Opcode::BitAnd
                | Opcode::BitOr
                | Opcode::BitXor
                | Opcode::Shl
                | Opcode::Shr => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let r = self.binop(op, a, b)?;
                    self.stack.push(r);
                }
                Opcode::Not | Opcode::Neg => {
                    let a = self.pop()?;
                    let r = self.unop(op, a)?;
                    self.stack.push(r);
                }
                Opcode::Cast => {
                    // Wire format (the emitter's `Rvalue::Cast` arm): the
                    // cast-kind byte, then the target-type byte. The old
                    // single-byte form made `OP_CAST` a no-op — every float
                    // and narrowing cast silently kept its operand's value.
                    let kind = code[self.frames[frame_idx].pc];
                    self.frames[frame_idx].pc += 1;
                    let target = code[self.frames[frame_idx].pc];
                    self.frames[frame_idx].pc += 1;
                    if !matches!(kind, 0..=7) {
                        return Err(VmError::UnknownCastKind(kind));
                    }
                    let target =
                        CastTarget::from_u8(target).ok_or(VmError::UnknownCastTarget(target))?;
                    let operand = self.pop()?;
                    let out = cast_value(kind, target, operand)?;
                    self.stack.push(out);
                }
                Opcode::Jump => {
                    let target = Vm::read_u32(code, &mut self.frames[frame_idx].pc)? as usize;
                    let off = module.functions[func].resolve_target(target as u32);
                    self.frames[frame_idx].pc = off;
                }
                Opcode::JumpIf => {
                    let target = Vm::read_u32(code, &mut self.frames[frame_idx].pc)? as usize;
                    let off = module.functions[func].resolve_target(target as u32);
                    let cond = self.pop_int("branch condition")?;
                    if cond != 0 {
                        self.frames[frame_idx].pc = off;
                    }
                }
                Opcode::SwitchInt => {
                    let discr = self.pop_int("switch discriminant")?;
                    let count = Vm::read_u32(code, &mut self.frames[frame_idx].pc)? as usize;
                    let mut taken: Option<usize> = None;
                    for _ in 0..count {
                        let v = Vm::read_i64(code, &mut self.frames[frame_idx].pc)?;
                        let t = Vm::read_u32(code, &mut self.frames[frame_idx].pc)?;
                        if v == discr {
                            taken = Some(t as usize);
                        }
                    }
                    let otherwise = Vm::read_u32(code, &mut self.frames[frame_idx].pc)?;
                    let off = if let Some(t) = taken {
                        module.functions[func].resolve_target(t as u32)
                    } else {
                        module.functions[func].resolve_target(otherwise)
                    };
                    self.frames[frame_idx].pc = off;
                }
                Opcode::Assert => {
                    let expected = code[self.frames[frame_idx].pc];
                    self.frames[frame_idx].pc += 1;
                    let target = Vm::read_u32(code, &mut self.frames[frame_idx].pc)?;
                    // The assert condition is an Int (the emitter writes the
                    // comparison ops' Int bools) — a Float/Tuple/Str operand
                    // is an explicit [`VmError::ExpectedInt`], not a silent
                    // `as_int()` 0 that flipped the assert's outcome.
                    let cond = self.pop_int("assert condition")?;
                    if (cond != 0) != (expected != 0) {
                        return Err(VmError::AssertionFailed);
                    }
                    let off = module.functions[func].resolve_target(target);
                    self.frames[frame_idx].pc = off;
                }
                Opcode::Len => {
                    let idx = Vm::read_u32(code, &mut self.frames[frame_idx].pc)? as usize;
                    let v = self.local_ref(idx)?.clone();
                    let len = match &v {
                        Value::Tuple(elems) => elems.len() as i64,
                        // Length is only meaningful for aggregates; scalars
                        // read as 0 (the pre-float VM's convention for
                        // non-aggregate locals).
                        Value::Int(_) | Value::Float(_) | Value::Str(_) => 0,
                    };
                    self.stack.push(Value::Int(len));
                }
                Opcode::Discriminant => {
                    let v = self.pop()?;
                    let discr = match &v {
                        Value::Tuple(elems) => elems.len() as i64,
                        Value::Int(_) | Value::Float(_) | Value::Str(_) => 0,
                    };
                    self.stack.push(Value::Int(discr));
                }
                Opcode::Aggregate => {
                    let n = Vm::read_u32(code, &mut self.frames[frame_idx].pc)? as usize;
                    let mut elems = Vec::with_capacity(n);
                    for _ in 0..n {
                        elems.push(self.pop()?);
                    }
                    elems.reverse();
                    self.stack.push(Value::Tuple(elems));
                }
                Opcode::Drop => {
                    let _addr = self.pop()?;
                    // The emitter writes `DROP; JUMP u32`. Do NOT consume the
                    // following opcode here — let `OP_JUMP` execute normally
                    // (RT-3: the old code ate the JUMP opcode + target as one
                    // u32 and jumped to garbage).
                }
                Opcode::Repeat => {
                    // T038-PATCHED [BC-2]: the emitter pushes the value
                    // first, then the count, so the count is on top of
                    // the stack. The previous code popped the count into
                    // `value` and the value into `count`, producing an
                    // empty tuple for the common `[x; 3]` case (3 as
                    // `value`, 0 as `count`).
                    let count = self.pop_int("repeat count")?;
                    let value = self.pop()?;
                    let mut elems = Vec::new();
                    for _ in 0..count.max(0) {
                        elems.push(value.clone());
                    }
                    self.stack.push(Value::Tuple(elems));
                }
                Opcode::Return => {
                    // BC-RETURN-FIX: a unit-returning function pushes nothing,
                    // so an empty stack here means "returns Unit", not an
                    // error. Value-returning functions still push their result.
                    let retval = self.pop().unwrap_or(Value::Int(0));
                    // The dispatch loop only reaches a `Return` dispatch with
                    // a frame on the stack (the empty check above the loop
                    // header returns early), so the pop cannot miss.
                    let finished = self
                        .frames
                        .pop()
                        .expect("the dispatch loop never runs without a frame");
                    if self.frames.is_empty() {
                        // Entry frame returned: the value is the module result.
                        return Ok(retval);
                    }
                    // Write the return value into the caller's dest_local and
                    // resume the caller at its recorded target (a block index,
                    // resolved via the caller's own block-offset table). A
                    // non-entry frame always sits under a caller, so the
                    // borrow cannot miss either.
                    let (caller_func, dest, watch_value) = {
                        let caller = self
                            .frames
                            .last_mut()
                            .expect("a non-entry frame always has a caller");
                        let dest = finished.dest_local;
                        let caller_func = caller.func;
                        let resume =
                            module.functions[caller_func].resolve_target(finished.resume_target);
                        let wrote_dest = dest < caller.locals.len();
                        if wrote_dest {
                            caller.locals[dest] = retval.clone();
                        }
                        // The local-watch seam's callee-return event: the
                        // value clone is paid only when a sink is attached.
                        let watch_value = if wrote_dest && self.watch.is_some() {
                            Some(retval.clone())
                        } else {
                            None
                        };
                        caller.pc = resume;
                        (caller_func, dest, watch_value)
                    };
                    if let Some(value) = watch_value {
                        self.record_watch(
                            caller_func,
                            dest,
                            value,
                            self.frames.len(),
                            crate::watch::WatchVia::CalleeReturn,
                        );
                    }
                    // Continue the loop with the caller frame on top.
                }
                Opcode::Call | Opcode::CallIndirect => {
                    // Emitter order: emit_operand(func) [LoadConst idx], then
                    // each arg, then OP_CALL, then argc(u32), then
                    // dest_local(u32), then target(u32). `pc` already points
                    // past the opcode, so argc is read next.
                    let argc = Vm::read_u32(code, &mut self.frames[frame_idx].pc)? as usize;
                    let dest_local = Vm::read_u32(code, &mut self.frames[frame_idx].pc)? as usize;
                    let target = Vm::read_u32(code, &mut self.frames[frame_idx].pc)?;
                    let mut args = Vec::with_capacity(argc);
                    for _ in 0..argc {
                        args.push(self.pop()?);
                    }
                    args.reverse();
                    let fn_idx = self.pop_int("call target")? as usize;
                    // Push the callee frame and loop with it on top. The
                    // callee's return writes into `dest_local` of *this* frame
                    // via the `Return` arm above.
                    self.push_frame(module, fn_idx, &args, target, dest_local)?;
                }
                Opcode::Trap => {
                    return Err(VmError::AbnormalTermination);
                }
            }
        }
    }

    /// Binary op dispatch. `(Int, Int)` keeps the wrapped-arithmetic and
    /// trap-on-div-by-zero semantics the VM has always had; `(Float, Float)`
    /// runs IEEE-754 `f64` arithmetic where `1.0 / 0.0` is `+inf` (the T106
    /// convention: LLVM's `fdiv` and the MIR interpreter agree, so the third
    /// backend must too) and comparisons push the VM's `Int` bool encoding
    /// (`JumpIf` is jump-if-nonzero). Any other kind pairing — mixed
    /// `Int`/`Float`, `Tuple` operands, bitwise/shift ops over floats — is
    /// an explicit [`VmError::BinOpTypeMismatch`], never a silent
    /// reinterpretation.
    fn binop(&self, op: Opcode, a: Value, b: Value) -> ExecResult<Value> {
        match (&a, &b) {
            (Value::Float(fa), Value::Float(fb)) => {
                let r = match op {
                    Opcode::Add => fa + fb,
                    Opcode::Sub => fa - fb,
                    Opcode::Mul => fa * fb,
                    Opcode::Div => fa / fb,
                    Opcode::Eq => return Ok(Value::Int(i64::from(fa == fb))),
                    Opcode::Ne => return Ok(Value::Int(i64::from(fa != fb))),
                    Opcode::Lt => return Ok(Value::Int(i64::from(fa < fb))),
                    Opcode::Gt => return Ok(Value::Int(i64::from(fa > fb))),
                    Opcode::Le => return Ok(Value::Int(i64::from(fa <= fb))),
                    Opcode::Ge => return Ok(Value::Int(i64::from(fa >= fb))),
                    _ => {
                        return Err(VmError::BinOpTypeMismatch {
                            op: op.name(),
                            left: a.kind_name(),
                            right: b.kind_name(),
                        });
                    }
                };
                Ok(Value::Float(r))
            }
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(self.binop_int(op, *a, *b)?)),
            // String equality is content equality: pool-backed `Value::Str`s
            // compare by bytes, not by the interner index an opaque
            // `OP_LOAD_CONST` used to carry (equality-by-index only worked
            // while the pool lived in the same process).
            (Value::Str(sa), Value::Str(sb)) => match op {
                Opcode::Eq => Ok(Value::Int(i64::from(sa == sb))),
                Opcode::Ne => Ok(Value::Int(i64::from(sa != sb))),
                _ => Err(VmError::BinOpTypeMismatch {
                    op: op.name(),
                    left: a.kind_name(),
                    right: b.kind_name(),
                }),
            },
            _ => Err(VmError::BinOpTypeMismatch {
                op: op.name(),
                left: a.kind_name(),
                right: b.kind_name(),
            }),
        }
    }

    /// The integer binary-op path, unchanged from the pre-float VM: wrapping
    /// arithmetic, div/rem by zero (and `i64::MIN / -1`) trap.
    fn binop_int(&self, op: Opcode, a: i64, b: i64) -> ExecResult<i64> {
        Ok(match op {
            Opcode::Add => a.wrapping_add(b),
            Opcode::Sub => a.wrapping_sub(b),
            Opcode::Mul => a.wrapping_mul(b),
            // RT-7: division/remainder by zero (and `i64::MIN / -1`) must
            // TRAP, not silently yield 0. `checked_*` is None exactly on those
            // cases.
            Opcode::Div => a.checked_div(b).ok_or(VmError::AbnormalTermination)?,
            Opcode::Rem => a.checked_rem(b).ok_or(VmError::AbnormalTermination)?,
            Opcode::Eq => i64::from(a == b),
            Opcode::Ne => i64::from(a != b),
            Opcode::Lt => i64::from(a < b),
            Opcode::Gt => i64::from(a > b),
            Opcode::Le => i64::from(a <= b),
            Opcode::Ge => i64::from(a >= b),
            Opcode::And => i64::from((a != 0) && (b != 0)),
            Opcode::Or => i64::from((a != 0) || (b != 0)),
            Opcode::BitAnd => a & b,
            Opcode::BitOr => a | b,
            Opcode::BitXor => a ^ b,
            Opcode::Shl => a.wrapping_shl(b as u32),
            Opcode::Shr => a.wrapping_shr(b as u32),
            _ => unreachable!("binop called on non-binary op"),
        })
    }

    /// Unary op dispatch: `Neg` is defined for both scalar kinds (IEEE
    /// negation for `Float`), `Not` only for `Int` — over a `Float` it is an
    /// explicit [`VmError::UnOpTypeMismatch`], not a silent bit reinterpretation.
    fn unop(&self, op: Opcode, a: Value) -> ExecResult<Value> {
        match (op, &a) {
            (Opcode::Neg, Value::Int(a)) => Ok(Value::Int(a.wrapping_neg())),
            (Opcode::Neg, Value::Float(a)) => Ok(Value::Float(-*a)),
            (Opcode::Not, Value::Int(a)) => Ok(Value::Int(i64::from(*a == 0))),
            _ => Err(VmError::UnOpTypeMismatch {
                op: op.name(),
                operand: a.kind_name(),
            }),
        }
    }

    /// Pop an operand that must be an `Int` (addresses, discriminants, call
    /// targets, branch conditions, repeat counts). Every other kind is an
    /// explicit [`VmError::ExpectedInt`] — the pre-float VM silently read
    /// these as `0`.
    fn pop_int(&mut self, role: &'static str) -> ExecResult<i64> {
        let v = self.pop()?;
        match v {
            Value::Int(n) => Ok(n),
            other => Err(VmError::ExpectedInt {
                role,
                found: other.kind_name(),
            }),
        }
    }

    fn local_ref(&self, idx: usize) -> ExecResult<&Value> {
        let frame = self.frames.last().ok_or(VmError::StackUnderflow)?;
        frame.locals.get(idx).ok_or(VmError::LocalOutOfBounds(idx))
    }

    fn set_local(&mut self, idx: usize, v: Value) -> ExecResult<()> {
        // The watch seam's context (which frame wrote, at what depth) is
        // captured before the mutable borrow; the event itself is recorded
        // after the write completes (a bounds failure returns below and
        // reports nothing).
        let depth = self.frames.len();
        let writer_func = self.frames.last().map(|f| f.func);
        let frame = self.frames.last_mut().ok_or(VmError::StackUnderflow)?;
        if idx >= frame.locals.len() {
            return Err(VmError::LocalOutOfBounds(idx));
        }
        frame.locals[idx] = v.clone();
        // Mirror into addressable memory so `LoadLocalAddr(idx) + Deref`
        // (offset 0) observes the same value. For aggregates, also unpack the
        // tuple's elements into the following slots so `Field(k)` reads
        // (`base + (k+1)`) reach element `k`.
        if idx < frame.mem.len() {
            frame.mem[idx] = v.clone();
            if let Value::Tuple(elems) = &v {
                for (i, e) in elems.iter().enumerate() {
                    let a = idx + 1 + i;
                    if a < frame.mem.len() {
                        frame.mem[a] = e.clone();
                    }
                }
            }
        }
        // The local-watch seam's `OP_STORE_LOCAL` event. The mirror above
        // is the addressable-memory view of the same write, not a second
        // one; only the semantic slot write is reported.
        if let Some(func) = writer_func {
            self.record_watch(func, idx, v, depth, crate::watch::WatchVia::StoreLocal);
        }
        Ok(())
    }

    /// Hand one local-write event to the attached watch sink
    /// ([`crate::watch`]), if any. The event carries the machine's current
    /// dispatch site ([`Vm::fault_pc`], the producing instruction) and the
    /// lifetime dispatch count — the same site tracking the backtrace seam
    /// uses, so a watch event pairs with the trace step of the same
    /// dispatch number. One `None` check per local write when no sink is
    /// attached.
    fn record_watch(
        &mut self,
        func: usize,
        local: usize,
        value: Value,
        frame_depth: usize,
        via: crate::watch::WatchVia,
    ) {
        if let Some(sink) = self.watch.as_mut() {
            sink.write(&crate::watch::WatchEvent {
                func,
                local,
                value,
                pc: self.fault_pc,
                dispatch: self.executed,
                frame_depth,
                via,
            });
        }
    }

    fn mem(&self, addr: usize) -> ExecResult<&Value> {
        let frame = self.frames.last().ok_or(VmError::StackUnderflow)?;
        frame.mem.get(addr).ok_or(VmError::LocalOutOfBounds(addr))
    }

    fn set_mem(&mut self, addr: usize, v: Value) -> ExecResult<()> {
        let frame = self.frames.last_mut().ok_or(VmError::StackUnderflow)?;
        // T119-PATCHED [VM-1]: reject wild addresses instead of resizing
        // the frame to an attacker/bug-controlled size. The previous
        // `resize(addr + 1, _)` on `a[bad_index]` (or a wrong field
        // offset from T042) allocated gigabytes of host memory — OOM
        // where a clean trap belongs.
        if addr >= frame.mem.len() {
            return Err(VmError::LocalOutOfBounds(addr));
        }
        frame.mem[addr] = v;
        Ok(())
    }

    fn pop(&mut self) -> ExecResult<Value> {
        self.stack.pop().ok_or(VmError::StackUnderflow)
    }

    fn read_u32(code: &[u8], pc: &mut usize) -> ExecResult<u32> {
        if *pc + 4 > code.len() {
            return Err(VmError::UnexpectedEndOfCode);
        }
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&code[*pc..*pc + 4]);
        *pc += 4;
        Ok(u32::from_le_bytes(bytes))
    }

    fn read_i64(code: &[u8], pc: &mut usize) -> ExecResult<i64> {
        if *pc + 8 > code.len() {
            return Err(VmError::UnexpectedEndOfCode);
        }
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&code[*pc..*pc + 8]);
        *pc += 8;
        Ok(i64::from_le_bytes(bytes))
    }
}

/// A chunk of bytecode to execute, plus the number of locals it needs.
///
/// `Chunk` retains the original single-function API; it is reinterpreted as a
/// one-function [`Module`] by [`Vm::run`]. (The former `entry` field is
/// gone: it was written by nothing — both constructors hardcoded 0 and
/// `Vm::run` discarded it — so it was dead API surface.)
#[derive(Debug, Clone)]
pub struct Chunk {
    /// Raw instruction bytes.
    pub code: Vec<u8>,
    /// Number of local slots.
    pub n_locals: usize,
}

impl Chunk {
    /// new.
    #[must_use]
    pub fn new(code: Vec<u8>) -> Chunk {
        Chunk { code, n_locals: 0 }
    }

    /// Build a chunk with an explicit local count.
    #[must_use]
    pub fn with_locals(code: Vec<u8>, n_locals: usize) -> Chunk {
        Chunk { code, n_locals }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u32le(v: u32) -> [u8; 4] {
        v.to_le_bytes()
    }
    fn i64le(v: i64) -> [u8; 8] {
        v.to_le_bytes()
    }
    fn op(o: Opcode) -> u8 {
        o as u8
    }

    /// A tiny assembler that records basic-block offsets so jump targets can be
    /// referenced by block index (matching the emitter's convention).
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
        /// Emit a `LoadConst v` pair.
        fn load_const(&mut self, v: i64) {
            self.op(Opcode::LoadConst);
            self.i64(v);
        }
        /// Emit a `LoadLocal idx` pair.
        fn load_local(&mut self, idx: u32) {
            self.op(Opcode::LoadLocal);
            self.u32(idx);
        }
        /// Emit a `StoreLocal idx` pair.
        fn store_local(&mut self, idx: u32) {
            self.op(Opcode::StoreLocal);
            self.u32(idx);
        }
        /// Build a `Function` with the recorded block table.
        fn finish(self, n_locals: usize, arg_count: usize) -> Function {
            Function::with_blocks(self.code, n_locals, arg_count, self.blocks)
        }
    }

    // --- single-function (MVP regression) ---

    #[test]
    fn run_arithmetic_expression() {
        // (3 + 4) * 2 == 14
        let mut a = Asm::new();
        a.load_const(3);
        a.load_const(4);
        a.op(Opcode::Add);
        a.load_const(2);
        a.op(Opcode::Mul);
        a.op(Opcode::Return);

        let mut vm = Vm::new();
        let result = vm.run(&Chunk::with_locals(a.finish(0, 0).code, 0)).unwrap();
        assert_eq!(result, Value::Int(14));
    }

    #[test]
    fn run_conditional_jump_skips_dead_code() {
        // result = 10; if true { goto end }; result = 99; <end> return
        let mut a = Asm::new();
        a.load_const(10);
        a.store_local(0);
        a.load_const(1); // condition = true
        a.op(Opcode::JumpIf);
        a.u32(1); // block 1 (end)
        a.label(0); // dead block
        a.load_const(99);
        a.store_local(0);
        a.label(1); // end
        a.load_local(0);
        a.op(Opcode::Return);

        let module = Module::new(vec![a.finish(1, 0)], 0);
        let result = module.run().unwrap();
        assert_eq!(result, Value::Int(10));
    }

    #[test]
    fn run_logical_and_short_semantics() {
        // (1 != 0) && (2 < 3) -> 1
        let mut a = Asm::new();
        a.load_const(1);
        a.load_const(0);
        a.op(Opcode::Ne);
        a.load_const(2);
        a.load_const(3);
        a.op(Opcode::Lt);
        a.op(Opcode::And);
        a.op(Opcode::Return);

        let mut vm = Vm::new();
        let result = vm.run(&Chunk::new(a.finish(0, 0).code)).unwrap();
        assert_eq!(result, Value::Int(1));
    }

    #[test]
    fn run_len_reads_tuple_length() {
        // RT-4: `OP_LEN <local>` reads the length of the local's tuple value.
        // (Previously OP_LEN pushed Int(0) and ate a stale encoding.)
        let mut a = Asm::new();
        a.op(Opcode::Aggregate);
        a.u32(3); // build a 3-element tuple from three pushed constants
        a.op(Opcode::Return);

        // Simpler: store a tuple directly via Aggregate of 3 constants.
        let mut b = Asm::new();
        b.load_const(7);
        b.load_const(8);
        b.load_const(9);
        b.op(Opcode::Aggregate);
        b.u32(3);
        b.store_local(0);
        b.op(Opcode::Len);
        b.u32(0);
        b.op(Opcode::Return);
        let module = Module::new(vec![b.finish(1, 0)], 0);
        let result = module.run().unwrap();
        assert_eq!(result, Value::Int(3), "OP_LEN must push the tuple length");
    }

    #[test]
    fn unknown_opcode_reports_error() {
        let bc = vec![0x7F];
        let mut vm = Vm::new();
        let err = vm.run(&Chunk::new(bc)).unwrap_err();
        assert_eq!(err, VmError::UnknownOpcode(0x7F));
    }

    // --- full-opcode coverage ---

    #[test]
    fn run_all_binary_ops() {
        let cases: &[(i64, i64, Opcode, i64)] = &[
            (10, 3, Opcode::Add, 13),
            (10, 3, Opcode::Sub, 7),
            (10, 3, Opcode::Mul, 30),
            (10, 3, Opcode::Div, 3),
            (10, 3, Opcode::Rem, 1),
            (10, 3, Opcode::Eq, 0),
            (3, 3, Opcode::Eq, 1),
            (10, 3, Opcode::Ne, 1),
            (2, 3, Opcode::Lt, 1),
            (3, 2, Opcode::Gt, 1),
            (2, 2, Opcode::Le, 1),
            (3, 2, Opcode::Ge, 1),
            (0b1100, 0b1010, Opcode::BitAnd, 0b1000),
            (0b1100, 0b1010, Opcode::BitOr, 0b1110),
            (0b1100, 0b1010, Opcode::BitXor, 0b0110),
            (1, 4, Opcode::Shl, 16),
            (16, 2, Opcode::Shr, 4),
        ];
        for &(a, b, o, expected) in cases {
            let mut asm = Asm::new();
            asm.load_const(a);
            asm.load_const(b);
            asm.op(o);
            asm.op(Opcode::Return);
            let mut vm = Vm::new();
            assert_eq!(
                vm.run(&Chunk::new(asm.finish(0, 0).code)).unwrap(),
                Value::Int(expected),
                "op {o:?}"
            );
        }
    }

    #[test]
    fn run_not_neg() {
        let mut a = Asm::new();
        a.load_const(0);
        a.op(Opcode::Not); // !0 -> 1
        a.load_const(7);
        a.op(Opcode::Neg); // -7
        a.op(Opcode::Add); // 1 + (-7) = -6
        a.op(Opcode::Return);
        let mut vm = Vm::new();
        assert_eq!(
            vm.run(&Chunk::new(a.finish(0, 0).code)).unwrap(),
            Value::Int(-6)
        );
    }

    #[test]
    fn run_switch_int() {
        // switch (x) { 1 => 100, 2 => 200, _ => 999 }
        for (x, expected) in [(1i64, 100i64), (2, 200), (7, 999)] {
            let mut a = Asm::new();
            a.load_const(x);
            a.op(Opcode::SwitchInt);
            a.u32(2); // 2 arms
            a.i64(1);
            a.u32(0); // -> block 0 (100)
            a.i64(2);
            a.u32(1); // -> block 1 (200)
            a.u32(2); // otherwise -> block 2 (999)
            a.label(0);
            a.load_const(100);
            a.op(Opcode::Return);
            a.label(1);
            a.load_const(200);
            a.op(Opcode::Return);
            a.label(2);
            a.load_const(999);
            a.op(Opcode::Return);
            let module = Module::new(vec![a.finish(1, 0)], 0);
            let mut vm = Vm::new();
            let r = vm.run_module(&module).unwrap();
            assert_eq!(r, Value::Int(expected), "x={x}");
        }
    }

    #[test]
    fn run_assert_passes_and_fails() {
        // pass: assert(1 == 1) then return 42
        let mut ok = Asm::new();
        ok.load_const(1);
        ok.load_const(1);
        ok.op(Opcode::Eq);
        ok.op(Opcode::Assert);
        ok.code.push(1u8); // expected = true
        ok.u32(0); // resume -> block 0
        ok.label(0);
        ok.load_const(42);
        ok.op(Opcode::Return);
        let module_ok = Module::new(vec![ok.finish(1, 0)], 0);
        let mut vm = Vm::new();
        assert_eq!(vm.run_module(&module_ok).unwrap(), Value::Int(42));

        // fail: assert(1 == 0)
        let mut bad = Asm::new();
        bad.load_const(1);
        bad.load_const(0);
        bad.op(Opcode::Eq);
        bad.op(Opcode::Assert);
        bad.code.push(1u8);
        bad.u32(0);
        let module_bad = Module::new(vec![bad.finish(1, 0)], 0);
        let mut vm = Vm::new();
        assert_eq!(
            vm.run_module(&module_bad).unwrap_err(),
            VmError::AssertionFailed
        );
    }

    /// A non-Int assert condition is an explicit `ExpectedInt`, not the old
    /// silent `as_int()` 0: a Float condition read as 0 and *flipped the
    /// assert's outcome* (expected=true failed; worse, expected=false
    /// passed). The typed error names the role, like every other
    /// `pop_int` site.
    #[test]
    fn assert_condition_rejects_non_int_operands() {
        let mut a = Asm::new();
        a.op(Opcode::LoadConstF64);
        a.i64(1.5f64.to_bits() as i64);
        a.op(Opcode::Assert);
        a.code.push(1u8); // expected = true
        a.u32(0); // resume target
        let module = Module::new(vec![a.finish(1, 0)], 0);
        let mut vm = Vm::new();
        assert_eq!(
            vm.run_module(&module).unwrap_err(),
            VmError::ExpectedInt {
                role: "assert condition",
                found: "Float",
            }
        );
    }

    #[test]
    fn run_div_by_zero_traps() {
        // RT-7: `a / 0` and `a % 0` must trap, not silently yield 0.
        let mut a = Asm::new();
        a.load_const(10);
        a.load_const(0);
        a.op(Opcode::Div);
        a.op(Opcode::Return);
        let mut vm = Vm::new();
        assert_eq!(
            vm.run(&Chunk::new(a.finish(2, 0).code)).unwrap_err(),
            VmError::AbnormalTermination,
            "10 / 0 must trap"
        );

        let mut a = Asm::new();
        a.load_const(10);
        a.load_const(0);
        a.op(Opcode::Rem);
        a.op(Opcode::Return);
        let mut vm = Vm::new();
        assert_eq!(
            vm.run(&Chunk::new(a.finish(2, 0).code)).unwrap_err(),
            VmError::AbnormalTermination,
            "10 % 0 must trap"
        );
    }

    #[test]
    fn run_trap_terminates() {
        let bc = vec![op(Opcode::Trap)];
        let mut vm = Vm::new();
        assert_eq!(
            vm.run(&Chunk::new(bc)).unwrap_err(),
            VmError::AbnormalTermination
        );
    }

    #[test]
    fn run_aggregate_and_field_read() {
        // s = (10, 20, 30); return s.1 (offset 1 -> mem[local+2] in our model)
        let mut a = Asm::new();
        a.load_const(10);
        a.load_const(20);
        a.load_const(30);
        a.op(Opcode::Aggregate);
        a.u32(3);
        a.store_local(0);
        a.op(Opcode::LoadLocalAddr);
        a.u32(0);
        a.load_const(2); // Field(1) -> base + (1+1) = +2
        a.op(Opcode::Add);
        a.op(Opcode::Deref);
        a.op(Opcode::Return);

        let mut vm = Vm::new();
        let r = vm.run(&Chunk::with_locals(a.finish(8, 0).code, 8)).unwrap();
        assert_eq!(r, Value::Int(20));
    }

    // --- cross-function calls / recursion ---

    #[test]
    fn run_mutual_call_and_recursion() {
        // fib(n): if n < 2 return n else fib(n-1) + fib(n-2)
        // main: return fib(6) == 8
        let mut fib = Asm::new();
        fib.label(0); // entry
        fib.load_local(1); // MIR ABI: the argument lives at local 1
        fib.load_const(2);
        fib.op(Opcode::Lt); // n < 2
        fib.op(Opcode::JumpIf);
        fib.u32(3); // -> base (block 3)
        // recursive case (dests live at 2/3: local 1 holds the argument
        // under the MIR call-frame ABI)
        fib.label(1);
        fib.load_const(0); // fn idx 0 (fib) [bottom]
        fib.load_local(1);
        fib.load_const(1);
        fib.op(Opcode::Sub); // n-1 [top = arg]
        fib.op(Opcode::Call);
        fib.u32(1); // argc
        fib.u32(2); // dest local 2
        fib.u32(2); // resume -> block 2 (L_add1)
        fib.label(2); // L_add1
        fib.load_const(0); // fn idx 0 (fib) [bottom]
        fib.load_local(1);
        fib.load_const(2);
        fib.op(Opcode::Sub); // n-2 [top = arg]
        fib.op(Opcode::Call);
        fib.u32(1); // argc
        fib.u32(3); // dest local 3
        fib.u32(4); // resume -> block 4 (L_add2)
        fib.label(4); // L_add2
        fib.load_local(2);
        fib.load_local(3);
        fib.op(Opcode::Add);
        fib.op(Opcode::Return);
        fib.label(3); // L_base
        fib.load_local(1);
        fib.op(Opcode::Return);

        let mut main = Asm::new();
        main.label(0);
        main.load_const(0); // fn idx 0 (fib) [bottom]
        main.load_const(6);
        main.op(Opcode::Call);
        main.u32(1); // argc
        main.u32(0); // dest local 0
        main.u32(1); // resume -> block 1
        main.label(1); // L_done
        main.load_local(0);
        main.op(Opcode::Return);

        let module = Module::new(vec![fib.finish(8, 1), main.finish(8, 0)], 1);
        let result = module.run().unwrap();
        assert_eq!(result, Value::Int(8), "fib(6) should be 8");
    }

    #[test]
    fn call_frame_overflow_is_bounded() {
        // always recurse with the same arg -> must terminate with CallFrameOverflow
        let mut fib = Asm::new();
        fib.label(0);
        fib.load_const(0); // fn idx 0 [bottom]
        fib.load_const(1); // arg (unchanged -> always recurse)
        fib.op(Opcode::Call);
        fib.u32(1); // argc
        fib.u32(1); // dest
        fib.u32(0); // resume -> block 0 (loop)
        fib.label(1);
        fib.load_local(2);
        fib.op(Opcode::Return);

        let module = Module::new(vec![fib.finish(8, 1)], 0);
        let err = module.run().unwrap_err();
        assert_eq!(err, VmError::CallFrameOverflow);
    }
}

#[cfg(test)]
mod float_tests {
    use super::*;

    fn u32le(v: u32) -> [u8; 4] {
        v.to_le_bytes()
    }
    fn op(o: Opcode) -> u8 {
        o as u8
    }

    /// The emitter's cast-kind numbering (mirrors `glyim_mir::CastKind`).
    const CAST_INT_TO_INT: u8 = 0;
    const CAST_FLOAT_TO_INT: u8 = 1;
    const CAST_INT_TO_FLOAT: u8 = 2;
    const CAST_PTR_TO_PTR: u8 = 3;
    const CAST_FN_PTR_TO_PTR: u8 = 4;
    const CAST_PTR_TO_INT: u8 = 5;
    const CAST_INT_TO_PTR: u8 = 6;
    const CAST_FLOAT_TO_FLOAT: u8 = 7;

    /// A tiny float assembler: appends a `LoadConstF64 v` pair.
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
        fn load_const(&mut self, v: i64) {
            self.op(Opcode::LoadConst);
            self.code.extend_from_slice(&v.to_le_bytes());
        }
        fn load_const_f64(&mut self, v: f64) {
            self.op(Opcode::LoadConstF64);
            self.code.extend_from_slice(&v.to_le_bytes());
        }
        fn load_local(&mut self, idx: u32) {
            self.op(Opcode::LoadLocal);
            self.u32(idx);
        }
        fn cast(&mut self, kind: u8, target: CastTarget) {
            self.op(Opcode::Cast);
            self.code.push(kind);
            self.code.push(target as u8);
        }
        fn finish(self, n_locals: usize, arg_count: usize) -> Function {
            Function::with_blocks(self.code, n_locals, arg_count, self.blocks)
        }
    }

    fn run_chunk(code: Vec<u8>, n_locals: usize) -> ExecResult<Value> {
        Module::new(vec![Function::new(code, n_locals, 0)], 0).run()
    }

    // --- constants ---

    #[test]
    fn load_const_f64_pushes_a_float_value() {
        let mut a = Asm::new();
        a.load_const_f64(3.75);
        a.op(Opcode::Return);
        assert_eq!(run_chunk(a.code, 0).unwrap(), Value::Float(3.75));
    }

    // --- arithmetic (IEEE semantics; T106: div-by-zero is inf, like LLVM
    // and the MIR interpreter) ---

    #[test]
    fn float_arithmetic_is_ieee() {
        // 1.5 + 2.25 = 3.75
        let mut a = Asm::new();
        a.load_const_f64(1.5);
        a.load_const_f64(2.25);
        a.op(Opcode::Add);
        a.op(Opcode::Return);
        assert_eq!(run_chunk(a.code, 0).unwrap(), Value::Float(3.75));

        // 1.0 / 0.0 = +inf (never traps: the interpreter and LLVM agree)
        let mut d = Asm::new();
        d.load_const_f64(1.0);
        d.load_const_f64(0.0);
        d.op(Opcode::Div);
        d.op(Opcode::Return);
        assert_eq!(run_chunk(d.code, 0).unwrap(), Value::Float(f64::INFINITY));
    }

    #[test]
    fn float_comparisons_produce_bool_ints() {
        let mut a = Asm::new();
        a.load_const_f64(2.5);
        a.load_const_f64(3.5);
        a.op(Opcode::Lt);
        a.op(Opcode::Return);
        assert_eq!(run_chunk(a.code, 0).unwrap(), Value::Int(1));

        let mut b = Asm::new();
        b.load_const_f64(3.5);
        b.load_const_f64(2.5);
        b.op(Opcode::Ge);
        b.op(Opcode::Return);
        assert_eq!(run_chunk(b.code, 0).unwrap(), Value::Int(1));
    }

    #[test]
    fn float_div_zero_comparisons_are_ordered() {
        // NaN comparisons are false; inf comparisons are ordered.
        let mut a = Asm::new();
        a.load_const_f64(f64::NAN);
        a.load_const_f64(1.0);
        a.op(Opcode::Lt);
        a.op(Opcode::Return);
        assert_eq!(run_chunk(a.code, 0).unwrap(), Value::Int(0));
    }

    // --- type discipline: mixed-kind operands are explicit errors, never a
    // silent reinterpretation (the pre-float VM ran float binops as *integer*
    // ops over the raw bit patterns) ---

    #[test]
    fn mixed_int_float_arithmetic_is_a_typed_error() {
        let mut a = Asm::new();
        a.load_const(1);
        a.load_const_f64(2.0);
        a.op(Opcode::Add);
        a.op(Opcode::Return);
        assert_eq!(
            run_chunk(a.code, 0).unwrap_err(),
            VmError::BinOpTypeMismatch {
                op: "Add",
                left: "Int",
                right: "Float"
            }
        );
    }

    #[test]
    fn tuple_arithmetic_is_a_typed_error() {
        let mut a = Asm::new();
        a.load_const(7);
        a.load_const(8);
        a.op(Opcode::Aggregate);
        a.u32(2);
        a.load_const(1);
        a.op(Opcode::Add);
        a.op(Opcode::Return);
        assert_eq!(
            run_chunk(a.code, 2).unwrap_err(),
            VmError::BinOpTypeMismatch {
                op: "Add",
                left: "Tuple",
                right: "Int"
            }
        );
    }

    #[test]
    fn bitwise_and_shift_ops_reject_float_operands() {
        for o in [Opcode::BitAnd, Opcode::Shl] {
            let mut a = Asm::new();
            a.load_const_f64(1.0);
            a.load_const_f64(2.0);
            a.op(o);
            a.op(Opcode::Return);
            assert_eq!(
                run_chunk(a.code, 0).unwrap_err(),
                VmError::BinOpTypeMismatch {
                    op: match o {
                        Opcode::BitAnd => "BitAnd",
                        _ => "Shl",
                    },
                    left: "Float",
                    right: "Float"
                },
                "op {o:?}"
            );
        }
    }

    // --- unary ---

    #[test]
    fn float_negation_is_ieee() {
        let mut a = Asm::new();
        a.load_const_f64(3.5);
        a.op(Opcode::Neg);
        a.op(Opcode::Return);
        assert_eq!(run_chunk(a.code, 0).unwrap(), Value::Float(-3.5));
    }

    #[test]
    fn logical_not_rejects_float_operands() {
        let mut a = Asm::new();
        a.load_const_f64(1.0);
        a.op(Opcode::Not);
        a.op(Opcode::Return);
        assert_eq!(
            run_chunk(a.code, 0).unwrap_err(),
            VmError::UnOpTypeMismatch {
                op: "Not",
                operand: "Float"
            }
        );
    }

    // --- casts (the full interpreter MIR-14 semantics: a real truncation to
    // the target width/signedness, not an identity) ---

    #[test]
    fn cast_int_to_int_truncates_to_target_width() {
        // (-1i32) as u8 == 255 (MIR-14, mirrors the interpreter)
        let mut a = Asm::new();
        a.load_const(-1);
        a.cast(CAST_INT_TO_INT, CastTarget::U8);
        a.op(Opcode::Return);
        assert_eq!(run_chunk(a.code, 0).unwrap(), Value::Int(255));

        // 300 as i8 == 44 (modular wrap through the narrower width)
        let mut b = Asm::new();
        b.load_const(300);
        b.cast(CAST_INT_TO_INT, CastTarget::I8);
        b.op(Opcode::Return);
        assert_eq!(run_chunk(b.code, 0).unwrap(), Value::Int(44));
    }

    #[test]
    fn cast_int_to_float_converts() {
        let mut a = Asm::new();
        a.load_const(3);
        a.cast(CAST_INT_TO_FLOAT, CastTarget::F64);
        a.op(Opcode::Return);
        assert_eq!(run_chunk(a.code, 0).unwrap(), Value::Float(3.0));
    }

    #[test]
    fn cast_float_to_int_truncates_to_target_width() {
        // 3.99 as i32 == 3
        let mut a = Asm::new();
        a.load_const_f64(3.99);
        a.cast(CAST_FLOAT_TO_INT, CastTarget::I32);
        a.op(Opcode::Return);
        assert_eq!(run_chunk(a.code, 0).unwrap(), Value::Int(3));

        // 300.0 as u8 == 44 (the interpreter's trunc_to_ty: f -> i128, then
        // a modular wrap through the narrower width)
        let mut b = Asm::new();
        b.load_const_f64(300.0);
        b.cast(CAST_FLOAT_TO_INT, CastTarget::U8);
        b.op(Opcode::Return);
        assert_eq!(run_chunk(b.code, 0).unwrap(), Value::Int(44));
    }

    #[test]
    fn cast_float_to_float_preserves_value() {
        let mut a = Asm::new();
        a.load_const_f64(1.5);
        a.cast(CAST_FLOAT_TO_FLOAT, CastTarget::F64);
        a.op(Opcode::Return);
        assert_eq!(run_chunk(a.code, 0).unwrap(), Value::Float(1.5));
    }

    #[test]
    fn pointer_cast_kinds_are_identity_in_the_vm() {
        // The VM's address model is Int-only (the interpreter documents the
        // same identity for PtrToPtr/FnPtrToPtr at its value level).
        for kind in [
            CAST_PTR_TO_PTR,
            CAST_FN_PTR_TO_PTR,
            CAST_PTR_TO_INT,
            CAST_INT_TO_PTR,
        ] {
            let mut a = Asm::new();
            a.load_const(0x1234);
            a.cast(kind, CastTarget::Unknown);
            a.op(Opcode::Return);
            assert_eq!(
                run_chunk(a.code, 0).unwrap(),
                Value::Int(0x1234),
                "kind {kind}"
            );
        }
    }

    #[test]
    fn unknown_cast_bytes_are_typed_errors() {
        let mut a = Asm::new();
        a.load_const(1);
        a.code.push(op(Opcode::Cast));
        a.code.push(9); // no cast kind 9 exists
        a.code.push(CastTarget::F64 as u8);
        a.op(Opcode::Return);
        assert_eq!(
            run_chunk(a.code, 0).unwrap_err(),
            VmError::UnknownCastKind(9)
        );

        let mut b = Asm::new();
        b.load_const(1);
        b.code.push(op(Opcode::Cast));
        b.code.push(CAST_INT_TO_INT);
        b.code.push(200); // no CastTarget 200 exists
        b.op(Opcode::Return);
        assert_eq!(
            run_chunk(b.code, 0).unwrap_err(),
            VmError::UnknownCastTarget(200)
        );
    }

    #[test]
    fn cast_with_empty_stack_underflows() {
        let mut a = Asm::new();
        a.code.push(op(Opcode::Cast));
        a.code.push(CAST_INT_TO_FLOAT);
        a.code.push(CastTarget::F64 as u8);
        a.op(Opcode::Return);
        assert_eq!(run_chunk(a.code, 0).unwrap_err(), VmError::StackUnderflow);
    }

    // --- value semantics ---

    #[test]
    fn float_equality_is_bit_pattern_based() {
        // The image crate's value-assert note anticipated this: Eq must not
        // silently hold over NaN-like semantics, so Float equality compares
        // bit patterns (the InterpValue::Float convention).
        assert_eq!(Value::Float(f64::NAN), Value::Float(f64::NAN));
        assert_ne!(Value::Float(0.0), Value::Float(-0.0));
        assert_eq!(Value::Float(1.5), Value::Float(1.5));
        assert_ne!(Value::Float(1.0), Value::Int(1));
    }

    #[test]
    fn float_module_executes_through_the_image_round_trip() {
        // s = a + b * 2.0 with a/b as float arguments on a callee frame, then
        // a float-to-int cast in main: the cross-function float shape. The
        // MIR call-frame ABI puts the arguments at locals 1..=arg_count.
        let mut helper = Asm::new();
        helper.load_local(1);
        helper.load_local(2);
        helper.op(Opcode::Add);
        helper.load_const_f64(2.0);
        helper.op(Opcode::Mul);
        helper.op(Opcode::Return);

        let mut main = Asm::new();
        main.load_const(0); // fn idx 0: helper
        main.load_const_f64(1.25);
        main.load_const_f64(2.0); // (1.25 + 2.0) * 2.0 = 6.5
        main.op(Opcode::Call);
        main.u32(2);
        main.u32(0);
        main.u32(1);
        main.label(1);
        main.load_local(0); // the returned Float(6.5)
        main.cast(CAST_FLOAT_TO_INT, CastTarget::I64);
        main.op(Opcode::Return);

        let module = Module::new(vec![helper.finish(4, 2), main.finish(1, 0)], 1);
        let image = crate::image::encode_module(&module);
        let decoded = crate::image::decode_module(&image).expect("float module image decodes");
        assert_eq!(decoded.run().unwrap(), Value::Int(6));
    }
}

#[cfg(test)]
mod string_tests {
    use super::*;
    use crate::image::{decode_module, encode_module};

    fn u32le(v: u32) -> [u8; 4] {
        v.to_le_bytes()
    }
    fn op(o: Opcode) -> u8 {
        o as u8
    }

    /// A module carrying a string pool: `main` loads two pool strings and
    /// compares them by content (`OP_EQ` over `(Str, Str)`).
    fn pool_module() -> Module {
        let mut code = Vec::new();
        code.push(op(Opcode::LoadConstStr));
        code.extend_from_slice(&u32le(0)); // "abc"
        code.push(op(Opcode::LoadConstStr));
        code.extend_from_slice(&u32le(1)); // "abd"
        code.push(op(Opcode::Eq));
        code.push(op(Opcode::Return));
        Module::with_strings(
            vec![Function::new(code, 1, 0)],
            vec!["abc".to_string(), "abd".to_string()],
            0,
        )
    }

    #[test]
    fn load_const_str_resolves_from_the_module_pool() {
        let mut code = Vec::new();
        code.push(op(Opcode::LoadConstStr));
        code.extend_from_slice(&u32le(1));
        code.push(op(Opcode::Return));
        let module = Module::with_strings(
            vec![Function::new(code, 1, 0)],
            vec!["ignored".to_string(), "payload".to_string()],
            0,
        );
        assert_eq!(module.run().unwrap(), Value::Str("payload".to_string()));
    }

    #[test]
    fn string_constants_compare_by_content() {
        // "abc" vs "abd": content comparison must say 0. (Pre-seam, a string
        // constant was an opaque interner index under OP_LOAD_CONST —
        // equality-by-index only worked by interning accident, and the pool
        // was lost across the process boundary.)
        assert_eq!(pool_module().run().unwrap(), Value::Int(0));

        // Equal contents (different pool slots) compare equal.
        let mut code = Vec::new();
        code.push(op(Opcode::LoadConstStr));
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::LoadConstStr));
        code.extend_from_slice(&u32le(1));
        code.push(op(Opcode::Eq));
        code.push(op(Opcode::Return));
        let module = Module::with_strings(
            vec![Function::new(code, 1, 0)],
            vec!["same".to_string(), "same".to_string()],
            0,
        );
        assert_eq!(module.run().unwrap(), Value::Int(1));
    }

    #[test]
    fn string_index_out_of_range_is_a_typed_error() {
        let mut code = Vec::new();
        code.push(op(Opcode::LoadConstStr));
        code.extend_from_slice(&u32le(7));
        code.push(op(Opcode::Return));
        let module =
            Module::with_strings(vec![Function::new(code, 1, 0)], vec!["x".to_string()], 0);
        assert_eq!(
            module.run().unwrap_err(),
            VmError::StringIndexOutOfBounds(7)
        );
    }

    #[test]
    fn strings_survive_the_image_round_trip() {
        let module = pool_module();
        let image = encode_module(&module);
        let decoded = decode_module(&image).expect("string pool image decodes");
        assert_eq!(decoded.strings, module.strings, "pool round trips exactly");
        assert_eq!(
            decoded.run().unwrap(),
            Value::Int(0),
            "still compares by content"
        );
    }

    #[test]
    fn legacy_v1_images_still_decode_without_a_pool() {
        // Hand-write a version-1 image (no strings section): the decoder
        // accepts it as a pool-less module, so images shipped before the
        // string pool keep loading.
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(&crate::image::IMAGE_MAGIC);
        bytes.extend_from_slice(&1u16.to_le_bytes()); // version 1
        bytes.extend_from_slice(&u32le(1)); // one function
        bytes.extend_from_slice(&u32le(0)); // entry 0
        bytes.extend_from_slice(&u32le(1)); // n_locals
        bytes.extend_from_slice(&u32le(0)); // arg_count
        bytes.extend_from_slice(&u32le(0)); // no block table
        bytes.extend_from_slice(&u32le(1)); // code length 1
        bytes.push(op(Opcode::Return));
        let module = decode_module(&bytes).expect("v1 image decodes");
        assert!(module.strings.is_empty(), "v1 carries no pool");
    }

    #[test]
    fn string_equality_never_crosses_kinds() {
        let mut code = Vec::new();
        code.push(op(Opcode::LoadConstStr));
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&1i64.to_le_bytes());
        code.push(op(Opcode::Eq));
        code.push(op(Opcode::Return));
        let module =
            Module::with_strings(vec![Function::new(code, 1, 0)], vec!["x".to_string()], 0);
        assert_eq!(
            module.run().unwrap_err(),
            VmError::BinOpTypeMismatch {
                op: "Eq",
                left: "Str",
                right: "Int"
            }
        );
    }
}

#[cfg(test)]
mod fuel_tests {
    use crate::{Function, Module, Opcode, StepDetail, TraceCollector, Value, Vm, VmError};

    fn i64le(v: i64) -> [u8; 8] {
        v.to_le_bytes()
    }
    fn u32le(v: u32) -> [u8; 4] {
        v.to_le_bytes()
    }
    fn op(o: Opcode) -> u8 {
        o as u8
    }

    /// helper(n) = n * 2 + main returning helper(21): 9 dispatches total.
    fn golden_module() -> Module {
        let mut helper = Vec::new();
        helper.push(op(Opcode::LoadConst));
        helper.extend_from_slice(&i64le(2));
        helper.push(op(Opcode::LoadLocal));
        helper.extend_from_slice(&u32le(1));
        helper.push(op(Opcode::Mul));
        helper.push(op(Opcode::Return));
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(0));
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(21));
        main.push(op(Opcode::Call));
        main.extend_from_slice(&u32le(1));
        main.extend_from_slice(&u32le(0));
        main.extend_from_slice(&u32le(31));
        main.push(op(Opcode::LoadLocal));
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Return));
        Module::with_strings(
            vec![Function::new(helper, 2, 1), Function::new(main, 1, 0)],
            vec!["abc".to_string()],
            1,
        )
    }

    /// Exactly-enough fuel runs to completion; one dispatch short is an
    /// explicit `FuelExhausted` carrying the executed count.
    #[test]
    fn fuel_counts_dispatches_exactly() {
        let module = golden_module();
        let mut vm = Vm::new().with_fuel(9);
        let value = vm.run_module(&module).expect("9 dispatches fit in 9 fuel");
        assert_eq!(value, Value::Int(42));
        assert_eq!(vm.executed(), 9);

        let mut vm = Vm::new().with_fuel(8);
        let err = vm.run_module(&module).expect_err("8 fuel is one short");
        assert_eq!(err, VmError::FuelExhausted { executed: 8 });
        assert_eq!(vm.executed(), 8);
    }

    /// Zero fuel rejects the first dispatch — the program never runs.
    #[test]
    fn zero_fuel_rejects_the_first_dispatch() {
        let module = golden_module();
        let mut vm = Vm::new().with_fuel(0);
        let err = vm.run_module(&module).expect_err("no fuel, no run");
        assert_eq!(err, VmError::FuelExhausted { executed: 0 });
        assert_eq!(vm.executed(), 0);
    }

    /// No fuel attached: unlimited execution (the pre-seam behavior), with
    /// the dispatch count still tracked for post-run introspection.
    #[test]
    fn no_fuel_is_unlimited() {
        let module = golden_module();
        let mut vm = Vm::new();
        let value = vm.run_module(&module).expect("unlimited run");
        assert_eq!(value, Value::Int(42));
        assert_eq!(vm.executed(), 9);
    }

    /// The DoS-close proof: a self-jumping `loop` — unbounded execution
    /// before this seam — stops at exactly the fuel budget with the explicit
    /// error, not a hang.
    #[test]
    fn infinite_loop_stops_at_the_budget() {
        // b0: jump b0
        let mut code = Vec::new();
        code.push(op(Opcode::Jump));
        code.extend_from_slice(&u32le(0));
        let f = Function::with_blocks(code, 0, 0, vec![0]);
        let module = Module::new(vec![f], 0);
        let mut vm = Vm::new().with_fuel(500);
        let err = vm.run_module(&module).expect_err("the loop never halts");
        assert_eq!(err, VmError::FuelExhausted { executed: 500 });
    }

    /// Fuel composes with the trace seam: both hooks observe the same run,
    /// and an exhausted run still surfaces the faulting step's capture.
    #[test]
    fn fuel_composes_with_trace() {
        let module = golden_module();
        let collector = TraceCollector::bounded(3);
        let mut vm = Vm::new().with_fuel(4).with_trace(Box::new(collector));
        let err = vm.run_module(&module).expect_err("4 fuel is 5 short");
        assert_eq!(err, VmError::FuelExhausted { executed: 4 });
        let mut boxed = vm.take_trace().expect("attached");
        let c = boxed
            .as_any()
            .downcast_ref::<TraceCollector>()
            .expect("collector");
        assert_eq!(c.len(), 3, "the bounded capture stopped at its cap");
        assert!(c.overflowed());
        assert_eq!(c.steps()[0].detail, StepDetail::ConstInt(0));
    }

    /// Fuel composes with the image round trip: decode the shipped bytes,
    /// then run the loaded artifact under a budget.
    #[test]
    fn fuel_composes_with_module_image() {
        let module = golden_module();
        let bytes = crate::encode_module(&module);
        let loaded = crate::decode_module(&bytes).expect("decode");
        let mut vm = Vm::new().with_fuel(9);
        let value = vm.run_module(&loaded).expect("runs under budget");
        assert_eq!(value, Value::Int(42));
    }
}
