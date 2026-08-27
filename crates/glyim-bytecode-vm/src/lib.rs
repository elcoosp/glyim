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
//! - `OP_AGGREGATE` (tuples), `OP_CAST`, `OP_ASSERT`, `OP_REPEAT`, `OP_DROP`,
//!   `OP_TRAP`.
//!
//! The decoder returns explicit, typed errors (`VmError::UnknownOpcode` /
//! `UnknownOpcode` / `StackUnderflow` / `LocalOutOfBounds` /
//! `CallFrameOverflow` / `AbnormalTermination`) rather than silently
//! mis-executing an unhandled opcode, so adding a new emitter opcode forces a
//! deliberate VM decision.
//!
//! Wire format (operand sizes) mirrors `crates/glyim-codegen/src/lib.rs`
//! exactly: every `OP_*` that carries a payload uses little-endian `i64`
//! (constants) or `u32` (local/block/function indices) as the emitter writes.

/// A runtime value on the VM stack / in a local slot.
///
/// Scalars are `Int(i64)` (also used for bools: 0 = false, 1 = true, and as
/// opaque pointer/address slots in the per-frame memory model). `Tuple` holds
/// aggregate literals produced by `OP_AGGREGATE`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A 64-bit integer scalar (also used for bools/addresses).
    Int(i64),
    /// An aggregate literal (tuple) produced by `OP_AGGREGATE`.
    Tuple(Vec<Value>),
}

impl Value {
    fn as_int(&self) -> i64 {
        match self {
            Value::Int(v) => *v,
            // Address slots are always stored as Int; tuples should never be
            // used arithmetically.
            Value::Tuple(_) => 0,
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
/// Variant.
    LoadConst = 0x01,
/// Variant.
    Add = 0x02,
/// Variant.
    Sub = 0x03,
/// Variant.
    Mul = 0x04,
/// Variant.
    Div = 0x05,
/// Variant.
    Rem = 0x06,
/// Variant.
    Eq = 0x07,
/// Variant.
    Ne = 0x08,
/// Variant.
    Lt = 0x09,
/// Variant.
    Gt = 0x0A,
/// Variant.
    Le = 0x0B,
/// Variant.
    Ge = 0x0C,
/// Variant.
    And = 0x0D,
/// Variant.
    Or = 0x0E,
/// Variant.
    Not = 0x0F,
/// Variant.
    Neg = 0x10,
/// Variant.
    BitAnd = 0x11,
/// Variant.
    BitOr = 0x12,
/// Variant.
    BitXor = 0x13,
/// Variant.
    Shl = 0x14,
/// Variant.
    Shr = 0x15,
/// Variant.
    LoadLocal = 0x16,
/// Variant.
    StoreLocal = 0x17,
/// Variant.
    Return = 0x18,
/// Variant.
    JumpIf = 0x19,
/// Variant.
    Jump = 0x1A,
/// Variant.
    Call = 0x1B,
/// Variant.
    Cast = 0x1C,
/// Variant.
    Aggregate = 0x1D,
/// Variant.
    Discriminant = 0x1E,
/// Variant.
    Len = 0x1F,
/// Variant.
    SwitchInt = 0x20,
/// Variant.
    Assert = 0x21,
/// Variant.
    CallIndirect = 0x22,
/// Variant.
    LoadLocalAddr = 0x29,
/// Variant.
    StoreField = 0x2A,
/// Variant.
    Deref = 0x2B,
/// Variant.
    Drop = 0x2C,
/// Variant.
    Repeat = 0x2D,
/// Variant.
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
            0x29 => Opcode::LoadLocalAddr,
            0x2A => Opcode::StoreField,
            0x2B => Opcode::Deref,
            0x2C => Opcode::Drop,
            0x2D => Opcode::Repeat,
            0xFF => Opcode::Trap,
            _ => return None,
        })
    }
}

/// A single compiled function: its raw instruction bytes, the number of
/// local slots (+ argument count) it requires on a call frame, and a table
/// mapping MIR basic-block indices to byte offsets within `code`. Jump
/// targets in the emitted bytecode are block indices; `block_offsets` lets
/// the VM resolve them. When `block_offsets` is empty, targets are treated as
/// raw byte offsets (used by hand-assembled tests).
#[derive(Debug, Clone)]
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
    pub fn new(code: Vec<u8>, n_locals: usize, arg_count: usize) -> Function {
        Function {
            code,
            n_locals,
            arg_count,
            block_offsets: Vec::new(),
        }
    }

    /// Build a function with an explicit basic-block offset table.
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
        if !self.block_offsets.is_empty() {
            self.block_offsets[target as usize]
        } else {
            target as usize
        }
    }
}

/// A module: a table of functions plus an entry-function index.
///
/// The VM executes functions by index; `OP_CALL`/`OP_CALL_INDIRECT` reference
/// the same index space used by the emitter's `fn_table`.
#[derive(Debug, Clone)]
pub struct Module {
    /// Functions indexed by the emitter's function indices.
    pub functions: Vec<Function>,
    /// Index of the entry function executed by [`Module::run`].
    pub entry: usize,
}

impl Module {
/// new.
    pub fn new(functions: Vec<Function>, entry: usize) -> Module {
        Module { functions, entry }
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
}

impl Vm {
    /// Create an empty VM (no frames).
    pub fn new() -> Vm {
        Vm::default()
    }

    /// Execute the module's entry function.
    pub fn run_module(&mut self, module: &Module) -> ExecResult<Value> {
        if module.functions.is_empty() {
            return Err(VmError::FunctionOutOfBounds(0));
        }
        // Push the entry frame. The entry has no caller, so dest_local is
        // irrelevant and resume_target is ignored.
        self.push_frame(module, module.entry, &[], u32::MAX, usize::MAX)?;
        self.drive(module)
    }

    /// Inspect a local slot of the top call frame (test/debug helper).
    pub fn local(&self, idx: usize) -> Option<Value> {
        self.frames.last().and_then(|f| f.locals.get(idx).cloned())
    }

    /// Execute a single function (no calls in/out). Convenience used by the
    /// original single-function tests and simple cross-backend checks.
    pub fn run(&mut self, chunk: &Chunk) -> ExecResult<Value> {
        let module = Module::new(vec![Function::new(chunk.code.clone(), chunk.n_locals, 0)], 0);
        let _ = chunk.entry; // single-function entry is always 0
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
        let n_locals = f.n_locals.max(args.len());
        let mut locals = vec![Value::Int(0); n_locals];
        for (i, a) in args.iter().enumerate() {
            locals[i] = a.clone();
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
        Ok(())
    }

    /// The single, non-recursive execution driver. Every frame (including the
    /// entry) is pushed onto the heap-allocated `frames` stack; `OP_CALL`
    /// pushes a new frame and continues, `OP_RETURN` pops a frame and writes
    /// its value into the caller's recorded `dest_local`. This keeps host
    /// (Rust) stack depth constant regardless of VM call depth, so deep and
    /// even infinite VM recursion are handled by the `MAX_CALL_DEPTH` guard
    /// instead of overflowing the host stack.
    fn drive(&mut self, module: &Module) -> ExecResult<Value> {
        loop {
            // Borrow split: fetch the pc and current function from the top
            // frame without holding a mutable borrow across the big match.
            if self.frames.is_empty() {
                // The entry frame returned without producing a value on the
                // operand stack (should have pushed its retval before Return).
                return self.pop().map_err(|_| VmError::EmptyReturn);
            }
            let frame_idx = self.frames.len() - 1;
            let (pc, func) = {
                let fr = &self.frames[frame_idx];
                (fr.pc, fr.func)
            };
            let code = &module.functions[func].code;
            if pc >= code.len() {
                return Err(VmError::UnexpectedEndOfCode);
            }
            let raw = code[pc];
            let op = Opcode::from_u8(raw).ok_or(VmError::UnknownOpcode(raw))?;
            self.frames[frame_idx].pc = pc + 1;

            match op {
                Opcode::LoadConst => {
                    let v = Vm::read_i64(code, &mut self.frames[frame_idx].pc)?;
                    self.stack.push(Value::Int(v));
                }
                Opcode::LoadLocal => {
                    let idx =
                        Vm::read_u32(code, &mut self.frames[frame_idx].pc)? as usize;
                    let v = self.local_ref(idx)?.clone();
                    self.stack.push(v);
                }
                Opcode::StoreLocal => {
                    let idx =
                        Vm::read_u32(code, &mut self.frames[frame_idx].pc)? as usize;
                    let v = self.pop()?;
                    self.set_local(idx, v)?;
                }
                Opcode::LoadLocalAddr => {
                    let idx =
                        Vm::read_u32(code, &mut self.frames[frame_idx].pc)? as usize;
                    self.stack.push(Value::Int(idx as i64));
                }
                Opcode::StoreField => {
                    let addr = self.pop()?.as_int() as usize;
                    let v = self.pop()?;
                    self.set_mem(addr, v)?;
                }
                Opcode::Deref => {
                    let addr = self.pop()?.as_int() as usize;
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
                    let b = self.pop()?.as_int();
                    let a = self.pop()?.as_int();
                    let r = self.binop(op, a, b);
                    self.stack.push(Value::Int(r));
                }
                Opcode::Not | Opcode::Neg => {
                    let a = self.pop()?.as_int();
                    let r = self.unop(op, a);
                    self.stack.push(Value::Int(r));
                }
                Opcode::Cast => {
                    let _kind = code[self.frames[frame_idx].pc];
                    self.frames[frame_idx].pc += 1;
                }
                Opcode::Jump => {
                    let target =
                        Vm::read_u32(code, &mut self.frames[frame_idx].pc)? as usize;
                    let off = module.functions[func].resolve_target(target as u32);
                    self.frames[frame_idx].pc = off;
                }
                Opcode::JumpIf => {
                    let target =
                        Vm::read_u32(code, &mut self.frames[frame_idx].pc)? as usize;
                    let off = module.functions[func].resolve_target(target as u32);
                    let cond = self.pop()?.as_int();
                    if cond != 0 {
                        self.frames[frame_idx].pc = off;
                    }
                }
                Opcode::SwitchInt => {
                    let discr = self.pop()?.as_int();
                    let count =
                        Vm::read_u32(code, &mut self.frames[frame_idx].pc)? as usize;
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
                    let cond = self.pop()?.as_int();
                    if (cond != 0) != (expected != 0) {
                        return Err(VmError::AssertionFailed);
                    }
                    let off = module.functions[func].resolve_target(target);
                    self.frames[frame_idx].pc = off;
                }
                Opcode::Len => {
                    let _local = Vm::read_u32(code, &mut self.frames[frame_idx].pc)?;
                    self.stack.push(Value::Int(0));
                }
                Opcode::Discriminant => {
                    let v = self.pop()?;
                    let discr = match &v {
                        Value::Tuple(elems) => elems.len() as i64,
                        Value::Int(_) => 0,
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
                    let target = Vm::read_u32(code, &mut self.frames[frame_idx].pc)?;
                    let off = module.functions[func].resolve_target(target);
                    self.frames[frame_idx].pc = off;
                }
                Opcode::Repeat => {
                    let value = self.pop()?;
                    let count = self.pop()?.as_int();
                    let mut elems = Vec::new();
                    for _ in 0..count.max(0) {
                        elems.push(value.clone());
                    }
                    self.stack.push(Value::Tuple(elems));
                }
                Opcode::Return => {
                    let retval = self.pop().map_err(|_| VmError::EmptyReturn)?;
                    let finished = self.frames.pop().unwrap();
                    if self.frames.is_empty() {
                        // Entry frame returned: the value is the module result.
                        return Ok(retval);
                    }
                    // Write the return value into the caller's dest_local and
                    // resume the caller at its recorded target (a block index,
                    // resolved via the caller's own block-offset table).
                    let caller = self.frames.last_mut().unwrap();
                    let dest = finished.dest_local;
                    let caller_func = caller.func;
                    let resume = module.functions[caller_func].resolve_target(finished.resume_target);
                    if dest < caller.locals.len() {
                        caller.locals[dest] = retval;
                    }
                    caller.pc = resume;
                    // Continue the loop with the caller frame on top.
                }
                Opcode::Call | Opcode::CallIndirect => {
                    // Emitter order: emit_operand(func) [LoadConst idx], then
                    // each arg, then OP_CALL, then argc(u32), then
                    // dest_local(u32), then target(u32). `pc` already points
                    // past the opcode, so argc is read next.
                    let argc = Vm::read_u32(code, &mut self.frames[frame_idx].pc)? as usize;
                    let dest_local =
                        Vm::read_u32(code, &mut self.frames[frame_idx].pc)? as usize;
                    let target = Vm::read_u32(code, &mut self.frames[frame_idx].pc)?;
                    let mut args = Vec::with_capacity(argc);
                    for _ in 0..argc {
                        args.push(self.pop()?);
                    }
                    args.reverse();
                    let fn_idx = self.pop()?.as_int() as usize;
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

    fn binop(&self, op: Opcode, a: i64, b: i64) -> i64 {
        match op {
            Opcode::Add => a.wrapping_add(b),
            Opcode::Sub => a.wrapping_sub(b),
            Opcode::Mul => a.wrapping_mul(b),
            Opcode::Div => a.checked_div(b).unwrap_or(0),
            Opcode::Rem => a.checked_rem(b).unwrap_or(0),
            Opcode::Eq => (a == b) as i64,
            Opcode::Ne => (a != b) as i64,
            Opcode::Lt => (a < b) as i64,
            Opcode::Gt => (a > b) as i64,
            Opcode::Le => (a <= b) as i64,
            Opcode::Ge => (a >= b) as i64,
            Opcode::And => ((a != 0) && (b != 0)) as i64,
            Opcode::Or => ((a != 0) || (b != 0)) as i64,
            Opcode::BitAnd => a & b,
            Opcode::BitOr => a | b,
            Opcode::BitXor => a ^ b,
            Opcode::Shl => a.wrapping_shl(b as u32),
            Opcode::Shr => a.wrapping_shr(b as u32),
            _ => unreachable!("binop called on non-binary op"),
        }
    }

    fn unop(&self, op: Opcode, a: i64) -> i64 {
        match op {
            Opcode::Not => (a == 0) as i64,
            Opcode::Neg => a.wrapping_neg(),
            _ => unreachable!("unop called on non-unary op"),
        }
    }

    fn local_ref(&self, idx: usize) -> ExecResult<&Value> {
        let frame = self.frames.last().ok_or(VmError::StackUnderflow)?;
        frame.locals.get(idx).ok_or(VmError::LocalOutOfBounds(idx))
    }

    fn set_local(&mut self, idx: usize, v: Value) -> ExecResult<()> {
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
        Ok(())
    }

    fn mem(&self, addr: usize) -> ExecResult<&Value> {
        let frame = self.frames.last().ok_or(VmError::StackUnderflow)?;
        frame.mem.get(addr).ok_or(VmError::LocalOutOfBounds(addr))
    }

    fn set_mem(&mut self, addr: usize, v: Value) -> ExecResult<()> {
        let frame = self.frames.last_mut().ok_or(VmError::StackUnderflow)?;
        if addr >= frame.mem.len() {
            frame.mem.resize(addr + 1, Value::Int(0));
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
/// one-function [`Module`] by [`Vm::run`].
#[derive(Debug, Clone)]
pub struct Chunk {
    /// Raw instruction bytes.
    pub code: Vec<u8>,
    /// Number of local slots.
    pub n_locals: usize,
    /// Byte offset of the function entry point (0 for a single function).
    pub entry: usize,
}

impl Chunk {
/// new.
    pub fn new(code: Vec<u8>) -> Chunk {
        Chunk {
            code,
            n_locals: 0,
            entry: 0,
        }
    }

    /// Build a chunk with an explicit local count.
    pub fn with_locals(code: Vec<u8>, n_locals: usize) -> Chunk {
        Chunk {
            code,
            n_locals,
            entry: 0,
        }
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
        assert_eq!(vm.run(&Chunk::new(a.finish(0, 0).code)).unwrap(), Value::Int(-6));
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
        fib.load_local(0);
        fib.load_const(2);
        fib.op(Opcode::Lt); // n < 2
        fib.op(Opcode::JumpIf);
        fib.u32(3); // -> base (block 3)
        // recursive case
        fib.label(1);
        fib.load_const(0); // fn idx 0 (fib) [bottom]
        fib.load_local(0);
        fib.load_const(1);
        fib.op(Opcode::Sub); // n-1 [top = arg]
        fib.op(Opcode::Call);
        fib.u32(1); // argc
        fib.u32(1); // dest local 1
        fib.u32(2); // resume -> block 2 (L_add1)
        fib.label(2); // L_add1
        fib.load_const(0); // fn idx 0 (fib) [bottom]
        fib.load_local(0);
        fib.load_const(2);
        fib.op(Opcode::Sub); // n-2 [top = arg]
        fib.op(Opcode::Call);
        fib.u32(1); // argc
        fib.u32(2); // dest local 2
        fib.u32(4); // resume -> block 4 (L_add2)
        fib.label(4); // L_add2
        fib.load_local(1);
        fib.load_local(2);
        fib.op(Opcode::Add);
        fib.op(Opcode::Return);
        fib.label(3); // L_base
        fib.load_local(0);
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
        fib.load_local(1);
        fib.op(Opcode::Return);

        let module = Module::new(vec![fib.finish(8, 1)], 0);
        let err = module.run().unwrap_err();
        assert_eq!(err, VmError::CallFrameOverflow);
    }
}

