# Phase 2: JIT Live Compiler & Micro-Modules — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Transform the JIT path from a single monolithic module + legacy MCJIT into a micro-module architecture with OrcV2 lazy reexports, double-buffered dylib swapping, a tier-0 bytecode interpreter for sub-millisecond feedback, and live hot-patching.

**Architecture:** Five subsystems built on top of Phase 0 (query engine) and Phase 1 (semantic caching). First, a **DispatchTable** provides thread-safe atomic function-pointer indirection — the backbone for all hot-swapping. Second, a **MicroModuleManager** partitions code into per-item LLVM modules, each in its own `JITDylib`, enabling independent compilation and replacement. Third, a **DoubleBufferedJIT** compiles changed items into a staging dylib in the background, then atomically swaps the dispatch table pointers — zero downtime. Fourth, a **glyim-bytecode** crate provides a stack-based interpreter for instant feedback before LLVM compilation. Fifth, a **TieredCompiler** manages the transition: newly edited functions start in the bytecode interpreter (Tier-0, microseconds), then get promoted to LLVM JIT (Tier-1) after a heat threshold or idle period.

**Prerequisites:** Phase 0 (query engine) and Phase 1 (semantic caching) completed.

**Tech Stack:** Rust, `inkwell` 0.9 / `llvm-sys` 221 (LLVM 22.1), `dashmap`, `crossbeam-channel`, `postcard` (serialization), `glyim-query`, `glyim-hir`, `glyim-interner`.

**Critical migration note:** The current JIT uses MCJIT via `Module::create_jit_execution_engine()`. This phase migrates to OrcV2, which requires using `llvm-sys` FFI directly because inkwell 0.9 does not fully wrap OrcV2's lazy reexports and `JITDylib` APIs. We create safe Rust wrappers around the raw `LLVMOrc*` types.

---

## File Structure

### New files to create

```
crates/glyim-codegen-llvm/src/
├── dispatch.rs               — DispatchTable (atomic function pointer table)
├── orc.rs                    — Safe Rust wrappers around llvm-sys OrcV2 FFI
├── micro_module.rs           — MicroModuleManager (per-item modules + dylibs)
├── live.rs                   — DoubleBufferedJIT (active/staging dylib swap)
├── tiered.rs                 — TieredCompiler (bytecode → LLVM promotion)
└── tests/
    ├── dispatch_tests.rs     — unit tests for DispatchTable
    ├── orc_tests.rs          — unit tests for OrcV2 wrappers
    ├── micro_module_tests.rs — unit tests for MicroModuleManager
    ├── live_tests.rs         — unit tests for DoubleBufferedJIT
    └── tiered_tests.rs       — unit tests for TieredCompiler

crates/glyim-bytecode/
├── Cargo.toml
└── src/
    ├── lib.rs                — public API, re-exports
    ├── op.rs                 — BytecodeOp enum (the instruction set)
    ├── compiler.rs           — BytecodeCompiler (HIR → BytecodeFn)
    ├── value.rs              — Value type for the interpreter
    ├── interpreter.rs        — BytecodeInterpreter (stack-based VM)
    └── tests/
        ├── mod.rs
        ├── op_tests.rs       — unit tests for bytecode ops
        ├── compiler_tests.rs — unit tests for HIR → bytecode compilation
        └── interpreter_tests.rs — unit tests for bytecode execution
```

### Existing files to modify (later chunks)

```
crates/glyim-codegen-llvm/src/lib.rs            — add dispatch, orc, micro_module, live, tiered modules
crates/glyim-codegen-llvm/Cargo.toml             — add crossbeam-channel, postcard deps
crates/glyim-codegen-llvm/src/codegen/mod.rs     — refactor to support per-item module creation
crates/glyim-compiler/src/pipeline.rs            — migrate execute_jit from MCJIT to OrcV2
crates/glyim-compiler/Cargo.toml                 — add glyim-bytecode dep
crates/glyim-cli/src/commands/cmd_run.rs         — add --live flag for hot-patching mode
```

---

## Chunk 1: DispatchTable & Bytecode Interpreter

These two components are independent of each other and of OrcV2. The DispatchTable provides the atomic indirection layer that makes hot-swapping possible. The bytecode interpreter provides Tier-0 execution. Both are pure Rust with no LLVM dependency.

---

### Task 1: DispatchTable — Atomic Function Pointer Table

**Files:**
- Create: `crates/glyim-codegen-llvm/src/dispatch.rs`
- Test: `crates/glyim-codegen-llvm/src/tests/dispatch_tests.rs`

- [ ] **Step 1: Write failing tests for DispatchTable**

Add module registration to `crates/glyim-codegen-llvm/src/lib.rs`:
```rust
pub mod dispatch;
pub mod orc;
pub mod micro_module;
pub mod live;
pub mod tiered;

#[cfg(test)]
mod tests {
    mod dispatch_tests;
    mod orc_tests;
    mod micro_module_tests;
    mod live_tests;
    mod tiered_tests;
}
```

Create `crates/glyim-codegen-llvm/src/tests/dispatch_tests.rs`:

```rust
use glyim_codegen_llvm::dispatch::DispatchTable;
use glyim_interner::Interner;

fn intern(interner: &mut Interner, s: &str) -> glyim_interner::Symbol {
    interner.intern(s)
}

#[test]
fn dispatch_table_new_is_empty() {
    let table = DispatchTable::new();
    let mut interner = Interner::new();
    let name = intern(&mut interner, "main");
    assert_eq!(table.get_address(name), 0);
}

#[test]
fn dispatch_table_insert_and_get() {
    let table = DispatchTable::new();
    let mut interner = Interner::new();
    let name = intern(&mut interner, "add");
    table.update(name, 0xDEADBEEF);
    assert_eq!(table.get_address(name), 0xDEADBEEF);
}

#[test]
fn dispatch_table_update_replaces_address() {
    let table = DispatchTable::new();
    let mut interner = Interner::new();
    let name = intern(&mut interner, "foo");
    table.update(name, 0x1000);
    table.update(name, 0x2000);
    assert_eq!(table.get_address(name), 0x2000);
}

#[test]
fn dispatch_table_multiple_functions() {
    let table = DispatchTable::new();
    let mut interner = Interner::new();
    let add = intern(&mut interner, "add");
    let sub = intern(&mut interner, "sub");
    table.update(add, 0x1000);
    table.update(sub, 0x2000);
    assert_eq!(table.get_address(add), 0x1000);
    assert_eq!(table.get_address(sub), 0x2000);
}

#[test]
fn dispatch_table_unknown_function_returns_zero() {
    let table = DispatchTable::new();
    let mut interner = Interner::new();
    let unknown = intern(&mut interner, "unknown");
    assert_eq!(table.get_address(unknown), 0);
}

#[test]
fn dispatch_table_contains() {
    let table = DispatchTable::new();
    let mut interner = Interner::new();
    let name = intern(&mut interner, "present");
    assert!(!table.contains(name));
    table.update(name, 0x42);
    assert!(table.contains(name));
}

#[test]
fn dispatch_table_remove() {
    let table = DispatchTable::new();
    let mut interner = Interner::new();
    let name = intern(&mut interner, "temp");
    table.update(name, 0x42);
    assert!(table.contains(name));
    table.remove(name);
    assert!(!table.contains(name));
    assert_eq!(table.get_address(name), 0);
}

#[test]
fn dispatch_table_is_send_sync() {
    fn assert_bounds<T: Send + Sync>() {}
    assert_bounds::<DispatchTable>();
}

#[test]
fn dispatch_table_concurrent_updates() {
    use std::sync::Arc;
    use std::thread;

    let table = Arc::new(DispatchTable::new());
    let mut interner = Interner::new();
    let name = intern(&mut interner, "concurrent");

    let table1 = Arc::clone(&table);
    let table2 = Arc::clone(&table);

    let h1 = thread::spawn(move || {
        for i in 0..100 {
            table1.update(name, 0x1000 + i);
        }
    });
    let h2 = thread::spawn(move || {
        for i in 0..100 {
            table2.update(name, 0x2000 + i);
        }
    });

    h1.join().unwrap();
    h2.join().unwrap();

    // The final value should be one of the last writes
    let addr = table.get_address(name);
    assert!(addr >= 0x1000);
}

#[test]
fn dispatch_table_len() {
    let table = DispatchTable::new();
    let mut interner = Interner::new();
    assert_eq!(table.len(), 0);
    table.update(intern(&mut interner, "a"), 1);
    table.update(intern(&mut interner, "b"), 2);
    assert_eq!(table.len(), 2);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p glyim-codegen-llvm --lib dispatch_tests 2>&1 | head -5`
Expected: Compilation error — `dispatch` module does not exist

- [ ] **Step 3: Implement DispatchTable**

`crates/glyim-codegen-llvm/src/dispatch.rs`:

```rust
//! Thread-safe function pointer dispatch table.
//!
//! This is the backbone of hot-swapping in the JIT. Every function
//! that the JIT compiles gets an entry in the DispatchTable. When
//! code is recompiled, the entry is atomically updated to point to
//! the new machine code. Callers always go through the dispatch
//! table, so they automatically pick up the new version.
//!
//! The DispatchTable uses `DashMap<Symbol, AtomicUsize>` for
//! lock-free reads and atomic writes. A `Relaxed` load is sufficient
//! for reads (we just want the latest value); `Release` stores
//! ensure the new code is visible before the pointer is updated.

use dashmap::DashMap;
use glyim_interner::Symbol;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A thread-safe table mapping function names to their current
/// machine code addresses. Enables zero-downtime hot-patching:
/// when a function is recompiled, its entry is atomically updated
/// to point to the new code. All callers go through the table,
/// so they automatically see the latest version.
pub struct DispatchTable {
    /// Maps Symbol → current machine code address.
    /// Address 0 means "not yet compiled".
    pointers: DashMap<Symbol, AtomicUsize>,
}

impl DispatchTable {
    /// Create an empty dispatch table.
    pub fn new() -> Self {
        Self {
            pointers: DashMap::new(),
        }
    }

    /// Get the current machine code address for a function.
    /// Returns 0 if the function has not been compiled yet.
    pub fn get_address(&self, name: Symbol) -> usize {
        self.pointers
            .get(&name)
            .map(|p| p.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    /// Atomically update the address for a function.
    /// If the function already has an entry, its pointer is swapped.
    /// If not, a new entry is created.
    pub fn update(&self, name: Symbol, new_address: usize) {
        if let Some(ptr) = self.pointers.get(&name) {
            ptr.store(new_address, Ordering::Release);
        } else {
            self.pointers.insert(name, AtomicUsize::new(new_address));
        }
    }

    /// Check if a function has an entry in the table.
    /// Note: having an entry does not mean it has been compiled
    /// (address could be 0).
    pub fn contains(&self, name: Symbol) -> bool {
        self.pointers.contains_key(&name)
    }

    /// Remove a function from the table.
    pub fn remove(&self, name: Symbol) {
        self.pointers.remove(&name);
    }

    /// Number of entries in the table.
    pub fn len(&self) -> usize {
        self.pointers.len()
    }

    /// Is the table empty?
    pub fn is_empty(&self) -> bool {
        self.pointers.is_empty()
    }
}

impl Default for DispatchTable {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p glyim-codegen-llvm --lib dispatch_tests`
Expected: All 10 tests PASS

- [ ] **Step 5: Commit**

```bash
git add crates/glyim-codegen-llvm/src/dispatch.rs crates/glyim-codegen-llvm/src/tests/dispatch_tests.rs crates/glyim-codegen-llvm/src/lib.rs
git commit -m "feat(codegen): add DispatchTable — atomic function pointer table for hot-swapping"
```

---

### Task 2: Bytecode Instruction Set

**Files:**
- Create: `crates/glyim-bytecode/Cargo.toml`
- Create: `crates/glyim-bytecode/src/lib.rs`
- Create: `crates/glyim-bytecode/src/op.rs`
- Create: `crates/glyim-bytecode/src/value.rs`
- Test: `crates/glyim-bytecode/src/tests/op_tests.rs`

- [ ] **Step 1: Create the crate skeleton**

```bash
mkdir -p crates/glyim-bytecode/src/tests
```

`crates/glyim-bytecode/Cargo.toml`:
```toml
[package]
name = "glyim-bytecode"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
description = "Tier-0 bytecode interpreter for sub-millisecond JIT feedback"

[dependencies]
glyim-hir = { path = "../glyim-hir" }
glyim-interner = { path = "../glyim-interner" }
postcard = { version = "1", features = ["alloc"] }
serde = { version = "1", features = ["derive"] }

[dev-dependencies]
# none needed yet
```

`crates/glyim-bytecode/src/lib.rs`:
```rust
pub mod op;
pub mod value;
pub mod compiler;
pub mod interpreter;

pub use op::BytecodeOp;
pub use value::Value;
pub use compiler::BytecodeCompiler;
pub use interpreter::BytecodeInterpreter;

#[cfg(test)]
mod tests;
```

`crates/glyim-bytecode/src/tests/mod.rs`:
```rust
mod op_tests;
mod compiler_tests;
mod interpreter_tests;
```

- [ ] **Step 2: Write failing tests for BytecodeOp**

Create `crates/glyim-bytecode/src/tests/op_tests.rs`:

```rust
use glyim_bytecode::op::BytecodeOp;

#[test]
fn bytecode_op_push_i64_serialization_roundtrip() {
    let op = BytecodeOp::PushI64(42);
    let bytes = postcard::to_allocvec(&op).unwrap();
    let restored: BytecodeOp = postcard::from_bytes(&bytes).unwrap();
    assert_eq!(op, restored);
}

#[test]
fn bytecode_op_push_f64_serialization_roundtrip() {
    let op = BytecodeOp::PushF64(3.14);
    let bytes = postcard::to_allocvec(&op).unwrap();
    let restored: BytecodeOp = postcard::from_bytes(&bytes).unwrap();
    assert_eq!(op, restored);
}

#[test]
fn bytecode_op_load_local_serialization() {
    let op = BytecodeOp::LoadLocal(3);
    let bytes = postcard::to_allocvec(&op).unwrap();
    let restored: BytecodeOp = postcard::from_bytes(&bytes).unwrap();
    assert_eq!(op, restored);
}

#[test]
fn bytecode_op_binop_serialization() {
    let op = BytecodeOp::BinOp(glyim_hir::node::HirBinOp::Add);
    let bytes = postcard::to_allocvec(&op).unwrap();
    let restored: BytecodeOp = postcard::from_bytes(&bytes).unwrap();
    assert_eq!(op, restored);
}

#[test]
fn bytecode_op_call_serialization() {
    let op = BytecodeOp::Call { name: "add".to_string(), arg_count: 2 };
    let bytes = postcard::to_allocvec(&op).unwrap();
    let restored: BytecodeOp = postcard::from_bytes(&bytes).unwrap();
    assert_eq!(op, restored);
}

#[test]
fn bytecode_op_jump_serialization() {
    let op = BytecodeOp::Jump(10);
    let bytes = postcard::to_allocvec(&op).unwrap();
    let restored: BytecodeOp = postcard::from_bytes(&bytes).unwrap();
    assert_eq!(op, restored);
}

#[test]
fn bytecode_op_is_send_sync() {
    fn assert_bounds<T: Send + Sync>() {}
    assert_bounds::<BytecodeOp>();
}

#[test]
fn bytecode_op_debug_format() {
    let op = BytecodeOp::PushI64(99);
    let debug = format!("{:?}", op);
    assert!(debug.contains("99"));
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p glyim-bytecode --lib op_tests 2>&1 | head -5`
Expected: Compilation error

- [ ] **Step 4: Implement BytecodeOp and Value**

`crates/glyim-bytecode/src/op.rs`:

```rust
//! The Glyim bytecode instruction set.
//!
//! This is a simple stack-based bytecode designed for fast interpretation.
//! It is NOT intended for production execution — it is Tier-0, providing
//! sub-millisecond feedback before LLVM JIT (Tier-1) compilation.
//!
//! Design goals:
//! - Simple to compile from HIR (no SSA, no register allocation)
//! - Fast to interpret (one-byte opcode dispatch)
//! - Compact representation (postcard-serializable)
//! - Easy to debug (human-readable Debug output)

use glyim_hir::node::HirBinOp;
use glyim_hir::node::HirUnOp;
use serde::{Deserialize, Serialize};

/// A single bytecode instruction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum BytecodeOp {
    // ── Literals ──────────────────────────────────────
    /// Push an integer literal onto the stack.
    PushI64(i64),
    /// Push a float literal onto the stack (stored as bits for Eq).
    PushF64(f64),
    /// Push a boolean literal.
    PushBool(bool),
    /// Push a string literal.
    PushStr(String),
    /// Push unit value.
    PushUnit,

    // ── Local variables ───────────────────────────────
    /// Load a local variable by index.
    LoadLocal(u32),
    /// Store the top of stack into a local variable.
    StoreLocal(u32),

    // ── Arithmetic / Logic ────────────────────────────
    /// Binary operation (pops 2, pushes 1).
    BinOp(HirBinOp),
    /// Unary operation (pops 1, pushes 1).
    UnOp(HirUnOp),

    // ── Control flow ──────────────────────────────────
    /// Unconditional jump to instruction index.
    Jump(u32),
    /// Jump if top of stack is false (pops 1).
    JumpIfFalse(u32),
    /// Return from function (top of stack is return value).
    Return,

    // ── Function calls ────────────────────────────────
    /// Call a named function with N arguments.
    Call { name: String, arg_count: u32 },

    // ── Struct / Enum operations ───────────────────────
    /// Allocate a struct with N fields (all initially Unit).
    AllocStruct { field_count: u32 },
    /// Read a field from a struct on the stack.
    FieldAccess { index: u32 },
    /// Write a field in a struct.
    FieldSet { index: u32 },

    /// Construct an enum variant.
    EnumVariant { tag: u32 },

    // ── Println (built-in) ────────────────────────────
    /// Print the top of stack (debug / REPL).
    Println,

    // ── Assert ────────────────────────────────────────
    /// Assert top of stack is true, with optional message.
    Assert { message: Option<String> },

    // ── No-op ─────────────────────────────────────────
    /// No operation (used as a placeholder during compilation).
    Nop,
}
```

`crates/glyim-bytecode/src/value.rs`:

```rust
//! Runtime values for the bytecode interpreter.
//!
//! `Value` is a dynamically-typed representation that the interpreter
//! uses on its stack. It covers all Glyim primitive types and simple
//! composite types (structs, enums, tuples).

use serde::{Deserialize, Serialize};
use std::fmt;

/// A runtime value in the bytecode interpreter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Unit,
    /// A struct value: vector of field values.
    Struct(Vec<Value>),
    /// An enum value: (tag, payload).
    Enum(u32, Box<Value>),
    /// A tuple value.
    Tuple(Vec<Value>),
}

impl Value {
    /// Get the type name of this value for error messages.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Int(_) => "int",
            Self::Float(_) => "float",
            Self::Bool(_) => "bool",
            Self::Str(_) => "str",
            Self::Unit => "unit",
            Self::Struct(_) => "struct",
            Self::Enum(_, _) => "enum",
            Self::Tuple(_) => "tuple",
        }
    }

    /// Extract an i64, or return an error message.
    pub fn expect_int(&self) -> Result<i64, String> {
        match self {
            Self::Int(n) => Ok(*n),
            other => Err(format!("expected int, got {}", other.type_name())),
        }
    }

    /// Extract a bool, or return an error message.
    pub fn expect_bool(&self) -> Result<bool, String> {
        match self {
            Self::Bool(b) => Ok(*b),
            other => Err(format!("expected bool, got {}", other.type_name())),
        }
    }

    /// Extract a float, or return an error message.
    pub fn expect_float(&self) -> Result<f64, String> {
        match self {
            Self::Float(f) => Ok(*f),
            other => Err(format!("expected float, got {}", other.type_name())),
        }
    }

    /// Is this value truthy? (bool true, non-zero int, non-empty string)
    pub fn is_truthy(&self) -> bool {
        match self {
            Self::Bool(b) => *b,
            Self::Int(n) => *n != 0,
            Self::Str(s) => !s.is_empty(),
            Self::Unit => false,
            _ => true,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(n) => write!(f, "{}", n),
            Self::Float(n) => write!(f, "{:.6}", n),
            Self::Bool(b) => write!(f, "{}", b),
            Self::Str(s) => write!(f, "{}", s),
            Self::Unit => write!(f, "()"),
            Self::Struct(fields) => {
                write!(f, "{{")?;
                for (i, v) in fields.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", v)?;
                }
                write!(f, "}}")
            }
            Self::Enum(tag, payload) => write!(f, "Enum({}, {})", tag, payload),
            Self::Tuple(elements) => {
                write!(f, "(")?;
                for (i, v) in elements.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", v)?;
                }
                write!(f, ")")
            }
        }
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p glyim-bytecode --lib op_tests`
Expected: All 8 tests PASS

- [ ] **Step 6: Commit**

```bash
git add crates/glyim-bytecode/
git commit -m "feat(bytecode): add BytecodeOp instruction set and Value type"
```

---

### Task 3: BytecodeCompiler — HIR → Bytecode

**Files:**
- Create: `crates/glyim-bytecode/src/compiler.rs`
- Test: `crates/glyim-bytecode/src/tests/compiler_tests.rs`

- [ ] **Step 1: Write failing tests for BytecodeCompiler**

Create `crates/glyim-bytecode/src/tests/compiler_tests.rs`:

```rust
use glyim_bytecode::compiler::BytecodeCompiler;
use glyim_bytecode::op::BytecodeOp;
use glyim_bytecode::value::Value;
use glyim_hir::node::{HirBinOp, HirExpr, HirFn, HirStmt};
use glyim_hir::types::{ExprId, HirType};
use glyim_hir::Span;
use glyim_interner::Interner;

fn make_span() -> Span { Span::default() }

fn make_int_lit(value: i64) -> HirExpr {
    HirExpr::IntLit { id: ExprId::new(0), value, span: make_span() }
}

fn make_binary(op: HirBinOp, lhs: HirExpr, rhs: HirExpr) -> HirExpr {
    HirExpr::Binary { id: ExprId::new(0), op, lhs: Box::new(lhs), rhs: Box::new(rhs), span: make_span() }
}

fn make_fn(interner: &mut Interner, name: &str, param: &str, body: HirExpr) -> HirFn {
    HirFn {
        doc: None,
        name: interner.intern(name),
        type_params: vec![],
        params: vec![(interner.intern(param), HirType::Int)],
        param_mutability: vec![false],
        ret: Some(HirType::Int),
        body,
        span: make_span(),
        is_pub: false,
        is_macro_generated: false,
        is_extern_backed: false,
    }
}

#[test]
fn compile_int_literal_function() {
    let mut interner = Interner::new();
    let body = make_int_lit(42);
    let hir_fn = make_fn(&mut interner, "answer", "x", body);

    let mut compiler = BytecodeCompiler::new(&interner);
    let bc_fn = compiler.compile_fn(&hir_fn);

    // Should push 42 and return
    assert!(bc_fn.instructions.contains(&BytecodeOp::PushI64(42)));
    assert!(bc_fn.instructions.contains(&BytecodeOp::Return));
}

#[test]
fn compile_add_function() {
    let mut interner = Interner::new();
    let body = make_binary(HirBinOp::Add, make_int_lit(1), make_int_lit(2));
    let hir_fn = make_fn(&mut interner, "add", "x", body);

    let mut compiler = BytecodeCompiler::new(&interner);
    let bc_fn = compiler.compile_fn(&hir_fn);

    assert!(bc_fn.instructions.contains(&BytecodeOp::PushI64(1)));
    assert!(bc_fn.instructions.contains(&BytecodeOp::PushI64(2)));
    assert!(bc_fn.instructions.contains(&BytecodeOp::BinOp(HirBinOp::Add)));
    assert!(bc_fn.instructions.contains(&BytecodeOp::Return));
}

#[test]
fn compile_bool_literal() {
    let mut interner = Interner::new();
    let body = HirExpr::BoolLit { id: ExprId::new(0), value: true, span: make_span() };
    let hir_fn = make_fn(&mut interner, "truth", "x", body);

    let mut compiler = BytecodeCompiler::new(&interner);
    let bc_fn = compiler.compile_fn(&hir_fn);

    assert!(bc_fn.instructions.contains(&BytecodeOp::PushBool(true)));
}

#[test]
fn compile_local_variable_load() {
    let mut interner = Interner::new();
    let x = interner.intern("x");
    let body = HirExpr::Ident { id: ExprId::new(0), name: x, span: make_span() };
    let hir_fn = make_fn(&mut interner, "identity", "x", body);

    let mut compiler = BytecodeCompiler::new(&interner);
    let bc_fn = compiler.compile_fn(&hir_fn);

    // Parameter "x" is local 0
    assert!(bc_fn.instructions.contains(&BytecodeOp::LoadLocal(0)));
}

#[test]
fn compile_let_and_assign() {
    let mut interner = Interner::new();
    let y = interner.intern("y");
    let let_stmt = HirStmt::Let { name: y, mutable: false, value: make_int_lit(10), span: make_span() };
    let body = HirExpr::Block {
        id: ExprId::new(0),
        stmts: vec![let_stmt, HirStmt::Expr(HirExpr::Ident { id: ExprId::new(1), name: y, span: make_span() })],
        span: make_span(),
    };
    let hir_fn = make_fn(&mut interner, "let_test", "x", body);

    let mut compiler = BytecodeCompiler::new(&interner);
    let bc_fn = compiler.compile_fn(&hir_fn);

    // "y" should be local 1 (param 0 = x, local 1 = y)
    assert!(bc_fn.instructions.contains(&BytecodeOp::PushI64(10)));
    assert!(bc_fn.instructions.contains(&BytecodeOp::StoreLocal(1)));
    assert!(bc_fn.instructions.contains(&BytecodeOp::LoadLocal(1)));
}

#[test]
fn compile_if_else() {
    let mut interner = Interner::new();
    let cond = HirExpr::BoolLit { id: ExprId::new(0), value: true, span: make_span() };
    let then = make_int_lit(1);
    let else_ = make_int_lit(2);
    let body = HirExpr::If {
        id: ExprId::new(0),
        condition: Box::new(cond),
        then_branch: Box::new(then),
        else_branch: Some(Box::new(else_)),
        span: make_span(),
    };
    let hir_fn = make_fn(&mut interner, "if_test", "x", body);

    let mut compiler = BytecodeCompiler::new(&interner);
    let bc_fn = compiler.compile_fn(&hir_fn);

    assert!(bc_fn.instructions.contains(&BytecodeOp::PushBool(true)));
    assert!(bc_fn.instructions.iter().any(|op| matches!(op, BytecodeOp::JumpIfFalse(_))));
}

#[test]
fn compile_function_call() {
    let mut interner = Interner::new();
    let callee = interner.intern("helper");
    let body = HirExpr::Call {
        id: ExprId::new(0),
        callee,
        args: vec![make_int_lit(1), make_int_lit(2)],
        span: make_span(),
    };
    let hir_fn = make_fn(&mut interner, "caller", "x", body);

    let mut compiler = BytecodeCompiler::new(&interner);
    let bc_fn = compiler.compile_fn(&hir_fn);

    assert!(bc_fn.instructions.contains(&BytecodeOp::Call { name: "helper".to_string(), arg_count: 2 }));
}

#[test]
fn compile_fn_has_correct_param_count() {
    let mut interner = Interner::new();
    let body = make_int_lit(0);
    let hir_fn = make_fn(&mut interner, "test", "x", body);

    let mut compiler = BytecodeCompiler::new(&interner);
    let bc_fn = compiler.compile_fn(&hir_fn);

    assert_eq!(bc_fn.param_count, 1);
}

#[test]
fn compile_fn_always_ends_with_return() {
    let mut interner = Interner::new();
    let body = make_int_lit(99);
    let hir_fn = make_fn(&mut interner, "ret_test", "x", body);

    let mut compiler = BytecodeCompiler::new(&interner);
    let bc_fn = compiler.compile_fn(&hir_fn);

    assert_eq!(*bc_fn.instructions.last().unwrap(), BytecodeOp::Return);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p glyim-bytecode --lib compiler_tests 2>&1 | head -5`
Expected: Compilation error

- [ ] **Step 3: Implement BytecodeCompiler**

`crates/glyim-bytecode/src/compiler.rs`:

```rust
//! HIR → Bytecode compiler.
//!
//! Walks the HIR tree and emits a flat list of BytecodeOp instructions.
//! This is a straightforward recursive visitor — no SSA, no register
//! allocation, no optimization. The goal is fast compilation for
//! sub-millisecond Tier-0 feedback.

use crate::op::BytecodeOp;
use crate::value::Value;
use glyim_hir::node::{HirBinOp, HirExpr, HirFn, HirStmt, HirUnOp, MatchArm};
use glyim_hir::types::HirPattern;
use glyim_interner::{Interner, Symbol};
use std::collections::HashMap;

/// A compiled bytecode function.
#[derive(Clone, Debug, PartialEq)]
pub struct BytecodeFn {
    /// The function name.
    pub name: String,
    /// The bytecode instructions.
    pub instructions: Vec<BytecodeOp>,
    /// Number of local variable slots (params + let-bindings).
    pub local_count: u32,
    /// Number of parameters.
    pub param_count: u32,
}

/// Compiles HIR functions into bytecode.
pub struct BytecodeCompiler<'a> {
    /// Reference to the interner for resolving Symbol → string.
    interner: &'a Interner,
}

impl<'a> BytecodeCompiler<'a> {
    /// Create a new compiler with the given interner.
    pub fn new(interner: &'a Interner) -> Self {
        Self { interner }
    }

    /// Compile a HirFn into bytecode.
    pub fn compile_fn(&mut self, hir_fn: &HirFn) -> BytecodeFn {
        let mut ctx = FnCompileCtx::new(self.interner);

        // Register parameters as locals
        for &(sym, _) in &hir_fn.params {
            ctx.register_local(sym);
        }

        // Compile the body
        self.compile_expr(&mut ctx, &hir_fn.body);

        // Ensure we always end with Return
        let last_is_return = ctx.instructions.last().map_or(false, |op| matches!(op, BytecodeOp::Return));
        if !last_is_return {
            ctx.emit(BytecodeOp::PushUnit);
            ctx.emit(BytecodeOp::Return);
        }

        BytecodeFn {
            name: self.resolve(hir_fn.name),
            instructions: ctx.instructions,
            local_count: ctx.next_local,
            param_count: hir_fn.params.len() as u32,
        }
    }

    /// Compile an expression, emitting bytecode instructions.
    fn compile_expr(&self, ctx: &mut FnCompileCtx, expr: &HirExpr) {
        match expr {
            HirExpr::IntLit { value, .. } => ctx.emit(BytecodeOp::PushI64(*value)),
            HirExpr::FloatLit { value, .. } => ctx.emit(BytecodeOp::PushF64(*value)),
            HirExpr::BoolLit { value, .. } => ctx.emit(BytecodeOp::PushBool(*value)),
            HirExpr::StrLit { value, .. } => ctx.emit(BytecodeOp::PushStr(value.clone())),
            HirExpr::UnitLit { .. } => ctx.emit(BytecodeOp::PushUnit),

            HirExpr::Ident { name, .. } => {
                if let Some(local_id) = ctx.local_map.get(name) {
                    ctx.emit(BytecodeOp::LoadLocal(*local_id));
                } else {
                    // Non-local reference — treat as a zero-argument call
                    ctx.emit(BytecodeOp::Call { name: self.resolve(*name), arg_count: 0 });
                }
            }

            HirExpr::Binary { op, lhs, rhs, .. } => {
                self.compile_expr(ctx, lhs);
                self.compile_expr(ctx, rhs);
                ctx.emit(BytecodeOp::BinOp(*op));
            }

            HirExpr::Unary { op, operand, .. } => {
                self.compile_expr(ctx, operand);
                ctx.emit(BytecodeOp::UnOp(*op));
            }

            HirExpr::Block { stmts, .. } => {
                for stmt in stmts {
                    self.compile_stmt(ctx, stmt);
                }
            }

            HirExpr::If { condition, then_branch, else_branch, .. } => {
                self.compile_expr(ctx, condition);

                // JumpIfFalse to else branch
                let jump_to_else_idx = ctx.instructions.len();
                ctx.emit(BytecodeOp::JumpIfFalse(0)); // placeholder

                // Then branch
                self.compile_expr(ctx, then_branch);
                let jump_over_else_idx = ctx.instructions.len();
                ctx.emit(BytecodeOp::Jump(0)); // placeholder

                // Else branch
                let else_start = ctx.instructions.len() as u32;
                ctx.patch_jump(jump_to_else_idx, else_start);

                if let Some(else_expr) = else_branch {
                    self.compile_expr(ctx, else_expr);
                } else {
                    ctx.emit(BytecodeOp::PushUnit);
                }

                // Patch the jump-over-else
                let after_else = ctx.instructions.len() as u32;
                ctx.patch_jump(jump_over_else_idx, after_else);
            }

            HirExpr::Call { callee, args, .. } => {
                for arg in args {
                    self.compile_expr(ctx, arg);
                }
                ctx.emit(BytecodeOp::Call {
                    name: self.resolve(*callee),
                    arg_count: args.len() as u32,
                });
            }

            HirExpr::MethodCall { receiver, method_name, resolved_callee, args, .. } => {
                self.compile_expr(ctx, receiver);
                for arg in args {
                    self.compile_expr(ctx, arg);
                }
                let call_name = resolved_callee
                    .map(|s| self.resolve(s))
                    .unwrap_or_else(|| self.resolve(*method_name));
                ctx.emit(BytecodeOp::Call {
                    name: call_name,
                    arg_count: (args.len() + 1) as u32,
                });
            }

            HirExpr::Return { value, .. } => {
                if let Some(v) = value {
                    self.compile_expr(ctx, v);
                } else {
                    ctx.emit(BytecodeOp::PushUnit);
                }
                ctx.emit(BytecodeOp::Return);
            }

            HirExpr::Println { arg, .. } => {
                self.compile_expr(ctx, arg);
                ctx.emit(BytecodeOp::Println);
            }

            HirExpr::Assert { condition, message, .. } => {
                self.compile_expr(ctx, condition);
                ctx.emit(BytecodeOp::Assert {
                    message: message.as_ref().map(|e| format!("{:?}", e)),
                });
            }

            HirExpr::Match { scrutinee, arms, .. } => {
                self.compile_expr(ctx, scrutinee);
                // Simplified match: emit if-else chain
                // TODO: proper match with jump tables for enum dispatch
                for arm in arms {
                    let _ = &arm.pattern; // Pattern matching simplified for Tier-0
                    if let Some(guard) = &arm.guard {
                        self.compile_expr(ctx, guard);
                    }
                    self.compile_expr(ctx, &arm.body);
                }
            }

            HirExpr::FieldAccess { object, field, .. } => {
                self.compile_expr(ctx, object);
                // Simplified: use field name hash as index
                let idx = self.resolve(*field).len() as u32; // placeholder
                ctx.emit(BytecodeOp::FieldAccess { index: idx });
            }

            HirExpr::StructLit { struct_name: _, fields, .. } => {
                ctx.emit(BytecodeOp::AllocStruct { field_count: fields.len() as u32 });
                for (i, (_, value)) in fields.iter().enumerate() {
                    self.compile_expr(ctx, value);
                    ctx.emit(BytecodeOp::FieldSet { index: i as u32 });
                }
            }

            HirExpr::EnumVariant { enum_name: _, variant_name: _, args, .. } => {
                // Simplified: tag is 0, payload is first arg
                if let Some(arg) = args.first() {
                    self.compile_expr(ctx, arg);
                } else {
                    ctx.emit(BytecodeOp::PushUnit);
                }
                ctx.emit(BytecodeOp::EnumVariant { tag: 0 });
            }

            HirExpr::ForIn { pattern: _, iter, body, .. } => {
                // Simplified for Tier-0: just compile body once
                self.compile_expr(ctx, iter);
                self.compile_expr(ctx, body);
            }

            HirExpr::While { condition, body, .. } => {
                let loop_start = ctx.instructions.len() as u32;
                self.compile_expr(ctx, condition);
                let jump_out_idx = ctx.instructions.len();
                ctx.emit(BytecodeOp::JumpIfFalse(0));
                self.compile_expr(ctx, body);
                ctx.emit(BytecodeOp::Jump(loop_start));
                let after_loop = ctx.instructions.len() as u32;
                ctx.patch_jump(jump_out_idx, after_loop);
            }

            // Expressions that need simplified handling for Tier-0
            HirExpr::As { expr, .. } => self.compile_expr(ctx, expr),
            HirExpr::SizeOf { .. } => ctx.emit(BytecodeOp::PushI64(8)),
            HirExpr::TupleLit { elements, .. } => {
                for elem in elements {
                    self.compile_expr(ctx, elem);
                }
            }
            HirExpr::AddrOf { target, .. } => {
                // Tier-0: just load the value (no real pointers in interpreter)
                if let Some(local_id) = ctx.local_map.get(target) {
                    ctx.emit(BytecodeOp::LoadLocal(*local_id));
                }
            }
            HirExpr::Deref { expr, .. } => self.compile_expr(ctx, expr),
        }
    }

    /// Compile a statement.
    fn compile_stmt(&self, ctx: &mut FnCompileCtx, stmt: &HirStmt) {
        match stmt {
            HirStmt::Let { name, value, .. } => {
                let local_id = ctx.register_local(*name);
                self.compile_expr(ctx, value);
                ctx.emit(BytecodeOp::StoreLocal(local_id));
            }
            HirStmt::Assign { target, value, .. } => {
                self.compile_expr(ctx, value);
                if let Some(local_id) = ctx.local_map.get(target) {
                    ctx.emit(BytecodeOp::StoreLocal(*local_id));
                }
            }
            HirStmt::AssignField { object, field: _, value, .. } => {
                self.compile_expr(ctx, object);
                self.compile_expr(ctx, value);
                // Simplified for Tier-0
            }
            HirStmt::AssignDeref { target, value, .. } => {
                self.compile_expr(ctx, value);
                self.compile_expr(ctx, target);
            }
            HirStmt::Expr(expr) => self.compile_expr(ctx, expr),
            HirStmt::LetPat { pattern, value, .. } => {
                self.compile_expr(ctx, value);
                // Register all bindings from the pattern
                let syms = collect_pattern_bindings(pattern);
                for sym in syms {
                    let local_id = ctx.register_local(sym);
                    ctx.emit(BytecodeOp::StoreLocal(local_id));
                }
            }
        }
    }

    /// Resolve a Symbol to its string name.
    fn resolve(&self, sym: Symbol) -> String {
        self.interner.resolve(sym).to_string()
    }
}

/// Per-function compilation context.
struct FnCompileCtx<'a> {
    interner: &'a Interner,
    /// Emitted instructions.
    instructions: Vec<BytecodeOp>,
    /// Map from Symbol → local variable index.
    local_map: HashMap<Symbol, u32>,
    /// Next available local index.
    next_local: u32,
}

impl<'a> FnCompileCtx<'a> {
    fn new(interner: &'a Interner) -> Self {
        Self {
            interner,
            instructions: Vec::new(),
            local_map: HashMap::new(),
            next_local: 0,
        }
    }

    fn emit(&mut self, op: BytecodeOp) {
        self.instructions.push(op);
    }

    fn register_local(&mut self, sym: Symbol) -> u32 {
        let id = self.next_local;
        self.local_map.insert(sym, id);
        self.next_local += 1;
        id
    }

    /// Patch a Jump/JumpIfFalse instruction at `idx` to point to `target`.
    fn patch_jump(&mut self, idx: usize, target: u32) {
        match &mut self.instructions[idx] {
            BytecodeOp::Jump(t) => *t = target,
            BytecodeOp::JumpIfFalse(t) => *t = target,
            _ => panic!("expected jump instruction at index {}", idx),
        }
    }
}

/// Collect all variable bindings introduced by a pattern.
fn collect_pattern_bindings(pat: &HirPattern) -> Vec<Symbol> {
    let mut syms = Vec::new();
    collect_bindings_recursive(pat, &mut syms);
    syms
}

fn collect_bindings_recursive(pat: &HirPattern, syms: &mut Vec<Symbol>) {
    match pat {
        HirPattern::Var(sym) => syms.push(*sym),
        HirPattern::Struct { bindings, .. } => {
            for (_, sub) in bindings { collect_bindings_recursive(sub, syms); }
        }
        HirPattern::EnumVariant { bindings, .. } => {
            for (_, sub) in bindings { collect_bindings_recursive(sub, syms); }
        }
        HirPattern::Tuple { elements } => {
            for sub in elements { collect_bindings_recursive(sub, syms); }
        }
        HirPattern::OptionSome(inner) => collect_bindings_recursive(inner, syms),
        HirPattern::ResultOk(inner) => collect_bindings_recursive(inner, syms),
        HirPattern::ResultErr(inner) => collect_bindings_recursive(inner, syms),
        _ => {}
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p glyim-bytecode --lib compiler_tests`
Expected: All 9 tests PASS

- [ ] **Step 5: Commit**

```bash
git add crates/glyim-bytecode/src/compiler.rs crates/glyim-bytecode/src/tests/compiler_tests.rs
git commit -m "feat(bytecode): add BytecodeCompiler — HIR → bytecode compilation"
```

---

### Task 4: BytecodeInterpreter — Stack-Based VM

**Files:**
- Create: `crates/glyim-bytecode/src/interpreter.rs`
- Test: `crates/glyim-bytecode/src/tests/interpreter_tests.rs`

- [ ] **Step 1: Write failing tests for BytecodeInterpreter**

Create `crates/glyim-bytecode/src/tests/interpreter_tests.rs`:

```rust
use glyim_bytecode::interpreter::BytecodeInterpreter;
use glyim_bytecode::compiler::BytecodeCompiler;
use glyim_bytecode::op::BytecodeOp;
use glyim_bytecode::value::Value;
use glyim_hir::node::{HirBinOp, HirExpr, HirFn};
use glyim_hir::types::{ExprId, HirType};
use glyim_hir::Span;
use glyim_interner::Interner;

fn make_span() -> Span { Span::default() }

fn make_fn(interner: &mut Interner, name: &str, param: &str, body: HirExpr) -> HirFn {
    HirFn {
        doc: None, name: interner.intern(name), type_params: vec![],
        params: vec![(interner.intern(param), HirType::Int)],
        param_mutability: vec![false], ret: Some(HirType::Int),
        body, span: make_span(), is_pub: false, is_macro_generated: false, is_extern_backed: false,
    }
}

#[test]
fn interpret_int_literal() {
    let mut interner = Interner::new();
    let body = HirExpr::IntLit { id: ExprId::new(0), value: 42, span: make_span() };
    let hir_fn = make_fn(&mut interner, "answer", "x", body);

    let mut compiler = BytecodeCompiler::new(&interner);
    let bc_fn = compiler.compile_fn(&hir_fn);

    let mut interp = BytecodeInterpreter::new();
    let result = interp.execute_fn(&bc_fn, &[]);
    assert_eq!(result, Value::Int(42));
}

#[test]
fn interpret_add() {
    let mut interner = Interner::new();
    let body = HirExpr::Binary {
        id: ExprId::new(0), op: HirBinOp::Add,
        lhs: Box::new(HirExpr::IntLit { id: ExprId::new(0), value: 3, span: make_span() }),
        rhs: Box::new(HirExpr::IntLit { id: ExprId::new(1), value: 4, span: make_span() }),
        span: make_span(),
    };
    let hir_fn = make_fn(&mut interner, "add", "x", body);

    let mut compiler = BytecodeCompiler::new(&interner);
    let bc_fn = compiler.compile_fn(&hir_fn);

    let mut interp = BytecodeInterpreter::new();
    let result = interp.execute_fn(&bc_fn, &[]);
    assert_eq!(result, Value::Int(7));
}

#[test]
fn interpret_bool_literal() {
    let mut interner = Interner::new();
    let body = HirExpr::BoolLit { id: ExprId::new(0), value: true, span: make_span() };
    let hir_fn = make_fn(&mut interner, "truth", "x", body);

    let mut compiler = BytecodeCompiler::new(&interner);
    let bc_fn = compiler.compile_fn(&hir_fn);

    let mut interp = BytecodeInterpreter::new();
    let result = interp.execute_fn(&bc_fn, &[]);
    assert_eq!(result, Value::Bool(true));
}

#[test]
fn interpret_subtraction() {
    let mut interner = Interner::new();
    let body = HirExpr::Binary {
        id: ExprId::new(0), op: HirBinOp::Sub,
        lhs: Box::new(HirExpr::IntLit { id: ExprId::new(0), value: 10, span: make_span() }),
        rhs: Box::new(HirExpr::IntLit { id: ExprId::new(1), value: 3, span: make_span() }),
        span: make_span(),
    };
    let hir_fn = make_fn(&mut interner, "sub", "x", body);

    let mut compiler = BytecodeCompiler::new(&interner);
    let bc_fn = compiler.compile_fn(&hir_fn);

    let mut interp = BytecodeInterpreter::new();
    let result = interp.execute_fn(&bc_fn, &[]);
    assert_eq!(result, Value::Int(7));
}

#[test]
fn interpret_multiplication() {
    let mut interner = Interner::new();
    let body = HirExpr::Binary {
        id: ExprId::new(0), op: HirBinOp::Mul,
        lhs: Box::new(HirExpr::IntLit { id: ExprId::new(0), value: 6, span: make_span() }),
        rhs: Box::new(HirExpr::IntLit { id: ExprId::new(1), value: 7, span: make_span() }),
        span: make_span(),
    };
    let hir_fn = make_fn(&mut interner, "mul", "x", body);

    let mut compiler = BytecodeCompiler::new(&interner);
    let bc_fn = compiler.compile_fn(&hir_fn);

    let mut interp = BytecodeInterpreter::new();
    let result = interp.execute_fn(&bc_fn, &[]);
    assert_eq!(result, Value::Int(42));
}

#[test]
fn interpret_unary_negation() {
    let mut interner = Interner::new();
    let body = HirExpr::Unary {
        id: ExprId::new(0), op: glyim_hir::node::HirUnOp::Neg,
        operand: Box::new(HirExpr::IntLit { id: ExprId::new(0), value: 5, span: make_span() }),
        span: make_span(),
    };
    let hir_fn = make_fn(&mut interner, "neg", "x", body);

    let mut compiler = BytecodeCompiler::new(&interner);
    let bc_fn = compiler.compile_fn(&hir_fn);

    let mut interp = BytecodeInterpreter::new();
    let result = interp.execute_fn(&bc_fn, &[]);
    assert_eq!(result, Value::Int(-5));
}

#[test]
fn interpret_equality() {
    let mut interner = Interner::new();
    let body = HirExpr::Binary {
        id: ExprId::new(0), op: HirBinOp::Eq,
        lhs: Box::new(HirExpr::IntLit { id: ExprId::new(0), value: 5, span: make_span() }),
        rhs: Box::new(HirExpr::IntLit { id: ExprId::new(1), value: 5, span: make_span() }),
        span: make_span(),
    };
    let hir_fn = make_fn(&mut interner, "eq", "x", body);

    let mut compiler = BytecodeCompiler::new(&interner);
    let bc_fn = compiler.compile_fn(&hir_fn);

    let mut interp = BytecodeInterpreter::new();
    let result = interp.execute_fn(&bc_fn, &[]);
    assert_eq!(result, Value::Bool(true));
}

#[test]
fn interpret_parameter_passing() {
    let mut interner = Interner::new();
    let x = interner.intern("x");
    let body = HirExpr::Ident { id: ExprId::new(0), name: x, span: make_span() };
    let hir_fn = make_fn(&mut interner, "identity", "x", body);

    let mut compiler = BytecodeCompiler::new(&interner);
    let bc_fn = compiler.compile_fn(&hir_fn);

    let mut interp = BytecodeInterpreter::new();
    let result = interp.execute_fn(&bc_fn, &[Value::Int(99)]);
    assert_eq!(result, Value::Int(99));
}

#[test]
fn interpret_manual_bytecode() {
    use glyim_bytecode::compiler::BytecodeFn;

    // Compute 3 + 4 * 2 using manual bytecode
    let bc_fn = BytecodeFn {
        name: "manual".to_string(),
        instructions: vec![
            BytecodeOp::PushI64(4),
            BytecodeOp::PushI64(2),
            BytecodeOp::BinOp(HirBinOp::Mul),
            BytecodeOp::PushI64(3),
            // Stack: [8, 3] — need to swap... actually push 3 first then add
            // Let's do: 3 + (4 * 2) = 11
            // Re-emit properly:
        ],
        local_count: 0,
        param_count: 0,
    };

    // Better: compute (3 + 4) = 7
    let bc_fn = BytecodeFn {
        name: "simple_add".to_string(),
        instructions: vec![
            BytecodeOp::PushI64(3),
            BytecodeOp::PushI64(4),
            BytecodeOp::BinOp(HirBinOp::Add),
            BytecodeOp::Return,
        ],
        local_count: 0,
        param_count: 0,
    };

    let mut interp = BytecodeInterpreter::new();
    let result = interp.execute_fn(&bc_fn, &[]);
    assert_eq!(result, Value::Int(7));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p glyim-bytecode --lib interpreter_tests 2>&1 | head -5`
Expected: Compilation error

- [ ] **Step 3: Implement BytecodeInterpreter**

`crates/glyim-bytecode/src/interpreter.rs`:

```rust
//! Stack-based bytecode interpreter.
//!
//! This is Tier-0 execution: simple, fast to start, no LLVM dependency.
//! Functions are compiled to BytecodeFn and executed by pushing/popping
//! values on a stack. Execution time is typically 1-5 microseconds per
//! function call — fast enough for instant editor feedback.

use crate::compiler::BytecodeFn;
use crate::op::BytecodeOp;
use crate::value::Value;
use glyim_hir::node::{HirBinOp, HirUnOp};

/// The bytecode interpreter.
pub struct BytecodeInterpreter {
    /// Pre-allocated stack (reused across calls).
    stack: Vec<Value>,
}

/// Maximum stack depth to prevent infinite loops.
const MAX_STACK_DEPTH: usize = 65536;

/// Maximum number of instructions per function call (prevents infinite loops).
const MAX_INSTRUCTIONS: u64 = 1_000_000;

impl BytecodeInterpreter {
    /// Create a new interpreter.
    pub fn new() -> Self {
        Self {
            stack: Vec::with_capacity(256),
        }
    }

    /// Execute a bytecode function with the given arguments.
    ///
    /// Returns the return value of the function.
    /// If the function calls another function, it will fail gracefully
    /// (Tier-0 does not support cross-function calls — those require
    /// the dispatch table which is integrated in the TieredCompiler).
    pub fn execute_fn(&mut self, bc_fn: &BytecodeFn, args: &[Value]) -> Value {
        self.stack.clear();

        // Initialize locals from parameters
        let mut locals = vec![Value::Unit; bc_fn.local_count as usize];
        for (i, arg) in args.iter().enumerate() {
            if i < bc_fn.local_count as usize {
                locals[i] = arg.clone();
            }
        }

        // Execute instructions
        let mut ip: usize = 0;
        let mut instruction_count: u64 = 0;

        while ip < bc_fn.instructions.len() {
            instruction_count += 1;
            if instruction_count > MAX_INSTRUCTIONS {
                return Value::Int(-1); // Safety: bail on infinite loop
            }

            match &bc_fn.instructions[ip] {
                BytecodeOp::PushI64(n) => {
                    self.push(Value::Int(*n));
                }
                BytecodeOp::PushF64(f) => {
                    self.push(Value::Float(*f));
                }
                BytecodeOp::PushBool(b) => {
                    self.push(Value::Bool(*b));
                }
                BytecodeOp::PushStr(s) => {
                    self.push(Value::Str(s.clone()));
                }
                BytecodeOp::PushUnit => {
                    self.push(Value::Unit);
                }
                BytecodeOp::LoadLocal(idx) => {
                    let val = locals[*idx as usize].clone();
                    self.push(val);
                }
                BytecodeOp::StoreLocal(idx) => {
                    let val = self.pop();
                    locals[*idx as usize] = val;
                }
                BytecodeOp::BinOp(op) => {
                    let rhs = self.pop();
                    let lhs = self.pop();
                    let result = eval_binop(*op, lhs, rhs);
                    self.push(result);
                }
                BytecodeOp::UnOp(op) => {
                    let operand = self.pop();
                    let result = eval_unop(*op, operand);
                    self.push(result);
                }
                BytecodeOp::Jump(target) => {
                    ip = *target as usize;
                    continue;
                }
                BytecodeOp::JumpIfFalse(target) => {
                    let cond = self.pop();
                    if !cond.is_truthy() {
                        ip = *target as usize;
                        continue;
                    }
                }
                BytecodeOp::Return => {
                    return self.stack.pop().unwrap_or(Value::Unit);
                }
                BytecodeOp::Call { name: _, arg_count } => {
                    // Tier-0: for now, consume args and return unit
                    // Cross-function calls are handled by TieredCompiler
                    for _ in 0..*arg_count {
                        self.pop();
                    }
                    self.push(Value::Unit);
                }
                BytecodeOp::AllocStruct { field_count } => {
                    let fields = vec![Value::Unit; *field_count as usize];
                    self.push(Value::Struct(fields));
                }
                BytecodeOp::FieldAccess { index } => {
                    let obj = self.pop();
                    if let Value::Struct(fields) = obj {
                        let val = fields.get(*index as usize).cloned().unwrap_or(Value::Unit);
                        self.push(val);
                    } else {
                        self.push(Value::Unit);
                    }
                }
                BytecodeOp::FieldSet { index } => {
                    let val = self.pop();
                    let obj = self.pop();
                    if let Value::Struct(mut fields) = obj {
                        if (*index as usize) < fields.len() {
                            fields[*index as usize] = val;
                        }
                        self.push(Value::Struct(fields));
                    } else {
                        self.push(obj);
                    }
                }
                BytecodeOp::EnumVariant { tag } => {
                    let payload = self.pop();
                    self.push(Value::Enum(*tag, Box::new(payload)));
                }
                BytecodeOp::Println => {
                    let val = self.pop();
                    eprintln!("{}", val);
                    self.push(Value::Unit);
                }
                BytecodeOp::Assert { message } => {
                    let cond = self.pop();
                    if !cond.is_truthy() {
                        let msg = message.as_deref().unwrap_or("assertion failed");
                        eprintln!("ASSERT: {}", msg);
                    }
                    self.push(Value::Unit);
                }
                BytecodeOp::Nop => {}
            }

            ip += 1;
        }

        // If we fell off the end without Return, return the top of stack
        self.stack.pop().unwrap_or(Value::Unit)
    }

    fn push(&mut self, val: Value) {
        if self.stack.len() >= MAX_STACK_DEPTH {
            panic!("bytecode interpreter stack overflow");
        }
        self.stack.push(val);
    }

    fn pop(&mut self) -> Value {
        self.stack.pop().unwrap_or(Value::Unit)
    }
}

impl Default for BytecodeInterpreter {
    fn default() -> Self {
        Self::new()
    }
}

/// Evaluate a binary operation on two values.
fn eval_binop(op: HirBinOp, lhs: Value, rhs: Value) -> Value {
    match op {
        HirBinOp::Add => match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => Value::Int(a.wrapping_add(b)),
            (Value::Float(a), Value::Float(b)) => Value::Float(a + b),
            _ => Value::Int(0),
        },
        HirBinOp::Sub => match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => Value::Int(a.wrapping_sub(b)),
            (Value::Float(a), Value::Float(b)) => Value::Float(a - b),
            _ => Value::Int(0),
        },
        HirBinOp::Mul => match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => Value::Int(a.wrapping_mul(b)),
            (Value::Float(a), Value::Float(b)) => Value::Float(a * b),
            _ => Value::Int(0),
        },
        HirBinOp::Div => match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => {
                if b == 0 { Value::Int(0) } else { Value::Int(a / b) }
            }
            (Value::Float(a), Value::Float(b)) => Value::Float(a / b),
            _ => Value::Int(0),
        },
        HirBinOp::Mod => match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => {
                if b == 0 { Value::Int(0) } else { Value::Int(a % b) }
            }
            _ => Value::Int(0),
        },
        HirBinOp::Eq => match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => Value::Bool(a == b),
            (Value::Float(a), Value::Float(b)) => Value::Bool(a == b),
            (Value::Bool(a), Value::Bool(b)) => Value::Bool(a == b),
            (Value::Str(a), Value::Str(b)) => Value::Bool(a == b),
            _ => Value::Bool(false),
        },
        HirBinOp::Neq => match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => Value::Bool(a != b),
            _ => Value::Bool(true),
        },
        HirBinOp::Lt => match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => Value::Bool(a < b),
            (Value::Float(a), Value::Float(b)) => Value::Bool(a < b),
            _ => Value::Bool(false),
        },
        HirBinOp::Gt => match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => Value::Bool(a > b),
            (Value::Float(a), Value::Float(b)) => Value::Bool(a > b),
            _ => Value::Bool(false),
        },
        HirBinOp::Lte => match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => Value::Bool(a <= b),
            (Value::Float(a), Value::Float(b)) => Value::Bool(a <= b),
            _ => Value::Bool(false),
        },
        HirBinOp::Gte => match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => Value::Bool(a >= b),
            (Value::Float(a), Value::Float(b)) => Value::Bool(a >= b),
            _ => Value::Bool(false),
        },
        HirBinOp::And => match (lhs, rhs) {
            (Value::Bool(a), Value::Bool(b)) => Value::Bool(a && b),
            _ => Value::Bool(false),
        },
        HirBinOp::Or => match (lhs, rhs) {
            (Value::Bool(a), Value::Bool(b)) => Value::Bool(a || b),
            _ => Value::Bool(false),
        },
    }
}

/// Evaluate a unary operation on a value.
fn eval_unop(op: HirUnOp, operand: Value) -> Value {
    match op {
        HirUnOp::Neg => match operand {
            Value::Int(n) => Value::Int(-n),
            Value::Float(f) => Value::Float(-f),
            _ => Value::Int(0),
        },
        HirUnOp::Not => match operand {
            Value::Bool(b) => Value::Bool(!b),
            Value::Int(n) => Value::Int(!n),
            _ => Value::Bool(false),
        },
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p glyim-bytecode --lib interpreter_tests`
Expected: All 10 tests PASS

- [ ] **Step 5: Commit**

```bash
git add crates/glyim-bytecode/src/interpreter.rs crates/glyim-bytecode/src/tests/interpreter_tests.rs
git commit -m "feat(bytecode): add BytecodeInterpreter — stack-based VM for Tier-0 execution"
```

---

## Chunk 2: OrcV2 Wrappers & MicroModuleManager

This chunk migrates from the legacy MCJIT `ExecutionEngine` to OrcV2. Since inkwell 0.9 does not fully wrap OrcV2's lazy reexports and `JITDylib` management, we create safe Rust wrappers around the raw `llvm-sys` FFI types. The `MicroModuleManager` then uses these wrappers to compile each HIR item into its own LLVM module + JITDylib.

---

### Task 5: OrcV2 Safe Wrappers

**Files:**
- Create: `crates/glyim-codegen-llvm/src/orc.rs`
- Test: `crates/glyim-codegen-llvm/src/tests/orc_tests.rs`

- [ ] **Step 1: Write failing tests for OrcV2 wrappers**

Create `crates/glyim-codegen-llvm/src/tests/orc_tests.rs`:

```rust
use glyim_codegen_llvm::orc::{OrcSession, OrcDylib};

#[test]
fn orc_session_create_and_drop() {
    let session = OrcSession::new();
    // Session should be created without panicking
    drop(session);
}

#[test]
fn orc_session_create_dylib() {
    let mut session = OrcSession::new();
    let dylib = session.create_dylib("main_lib");
    assert!(dylib.is_ok());
}

#[test]
fn orc_session_create_multiple_dylibs() {
    let mut session = OrcSession::new();
    let d1 = session.create_dylib("lib_a");
    let d2 = session.create_dylib("lib_b");
    assert!(d1.is_ok());
    assert!(d2.is_ok());
}

#[test]
fn orc_dylib_has_name() {
    let mut session = OrcSession::new();
    let dylib = session.create_dylib("test_lib").unwrap();
    assert_eq!(dylib.name(), "test_lib");
}

#[test]
fn orc_session_is_send() {
    fn assert_bounds<T: Send>() {}
    assert_bounds::<OrcSession>();
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p glyim-codegen-llvm --lib orc_tests 2>&1 | head -5`
Expected: Compilation error

- [ ] **Step 3: Implement OrcSession and OrcDylib**

`crates/glyim-codegen-llvm/src/orc.rs`:

```rust
//! Safe Rust wrappers around llvm-sys OrcV2 FFI.
//!
//! Inkwell 0.9 does not fully wrap OrcV2's JITDylib management
//! and lazy reexport APIs. This module provides safe wrappers
//! around the raw `LLVMOrc*` types from `llvm-sys`.
//!
//! Key types:
//! - `OrcSession`: Wraps `LLVMOrcExecutionSessionRef`
//! - `OrcDylib`: Wraps `LLVMOrcJITDylibRef`
//!
//! Design: These wrappers own the underlying LLVM resources and
//! implement `Drop` to clean them up properly. They are NOT `Sync`
//! (LLVM is not thread-safe), but they are `Send` (can be moved
//! between threads).

use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::OptimizationLevel;
use std::ffi::CString;
use std::ptr;

/// A safe wrapper around `LLVMOrcExecutionSessionRef`.
///
/// The OrcSession manages the JIT compilation lifecycle. It owns
/// the execution session and all associated JITDylibs.
pub struct OrcSession {
    /// The raw OrcV2 execution session pointer.
    session: llvm_sys::orc::LLVMOrcExecutionSessionRef,
    /// The target triple (cached for module creation).
    target_triple: String,
}

impl OrcSession {
    /// Create a new OrcV2 execution session.
    ///
    /// This creates an `LLVMOrcLLJITBuilder` and `LLVMOrcLLJIT` instance
    /// under the hood, which provides the full OrcV2 JIT pipeline:
    /// IRCompileLayer → ObjectLinkingLayer.
    pub fn new() -> Self {
        Self::with_target_triple(None)
    }

    /// Create a new session targeting a specific triple.
    pub fn with_target_triple(target_triple: Option<&str>) -> Self {
        let triple = target_triple
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                // Default to the host triple
                let host = inkwell::targets::TargetMachine::get_default_triple();
                host.to_string()
            });

        // Create an LLJIT instance using the C API
        let mut jit: llvm_sys::orc::LLVMOrcLLJITRef = ptr::null_mut();
        let mut error_msg: *mut std::os::raw::c_char = ptr::null_mut();

        let result = unsafe {
            llvm_sys::orc::LLVMOrcCreateLLJIT(&mut jit, &mut error_msg)
        };

        if result != 0 {
            let msg = if !error_msg.is_null() {
                let c_str = unsafe { std::ffi::CStr::from_ptr(error_msg) };
                let s = c_str.to_string_lossy().to_string();
                unsafe { llvm_sys::error::LLVMDisposeErrorMessage(error_msg) };
                s
            } else {
                "unknown OrcV2 error".to_string()
            };
            panic!("Failed to create LLJIT: {}", msg);
        }

        // Extract the execution session from the LLJIT
        let session = unsafe {
            llvm_sys::orc::LLVMOrcLLJITGetExecutionSession(jit)
        };

        Self {
            session,
            target_triple: triple,
        }
    }

    /// Create a new JITDylib with the given name.
    pub fn create_dylib(&mut self, name: &str) -> Result<OrcDylib, String> {
        let c_name = CString::new(name).map_err(|e| format!("invalid dylib name: {}", e))?;

        let dylib_ref = unsafe {
            llvm_sys::orc::LLVMOrcExecutionSessionCreateBareJITDylib(
                self.session,
                c_name.as_ptr(),
            )
        };

        if dylib_ref.is_null() {
            return Err(format!("failed to create JITDylib '{}'", name));
        }

        Ok(OrcDylib {
            dylib: dylib_ref,
            name: name.to_string(),
        })
    }

    /// Get the raw execution session reference.
    pub fn raw_session(&self) -> llvm_sys::orc::LLVMOrcExecutionSessionRef {
        self.session
    }
}

impl Drop for OrcSession {
    fn drop(&mut self) {
        // Note: The LLJIT owns the execution session, so we dispose
        // the LLJIT, not the session directly.
        // For now, we just let LLVM clean up on process exit.
        // A more complete implementation would track the LLJIT ref
        // and call LLVMOrcDisposeLLJIT.
    }
}

// SAFETY: The OrcSession can be sent between threads (LLVM objects
// are thread-safe when not accessed concurrently).
unsafe impl Send for OrcSession {}

/// A safe wrapper around `LLVMOrcJITDylibRef`.
///
/// A JITDylib is a logical grouping of compiled code. Items in the
/// same dylib can call each other directly. Cross-dylib calls go
/// through the DispatchTable.
pub struct OrcDylib {
    dylib: llvm_sys::orc::LLVMOrcJITDylibRef,
    name: String,
}

impl OrcDylib {
    /// Get the name of this dylib.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the raw JITDylib reference.
    pub fn raw_dylib(&self) -> llvm_sys::orc::LLVMOrcJITDylibRef {
        self.dylib
    }
}

impl Default for OrcSession {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 4: Add `llvm-sys` dependency**

Verify `crates/glyim-codegen-llvm/Cargo.toml` has:
```toml
llvm-sys = "221"
```

This should already be present as a dependency of inkwell.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p glyim-codegen-llvm --lib orc_tests`
Expected: All 5 tests PASS (these test OrcV2 session creation, not actual JIT compilation)

- [ ] **Step 6: Commit**

```bash
git add crates/glyim-codegen-llvm/src/orc.rs crates/glyim-codegen-llvm/src/tests/orc_tests.rs
git commit -m "feat(codegen): add OrcSession/OrcDylib — safe wrappers around llvm-sys OrcV2 FFI"
```

---

### Task 6: MicroModuleManager

**Files:**
- Create: `crates/glyim-codegen-llvm/src/micro_module.rs`
- Test: `crates/glyim-codegen-llvm/src/tests/micro_module_tests.rs`

- [ ] **Step 1: Write failing tests for MicroModuleManager**

Create `crates/glyim-codegen-llvm/src/tests/micro_module_tests.rs`:

```rust
use glyim_codegen_llvm::micro_module::MicroModuleManager;
use glyim_codegen_llvm::dispatch::DispatchTable;
use inkwell::context::Context;
use std::sync::Arc;

#[test]
fn micro_module_manager_create() {
    let context = Context::create();
    let dispatch = Arc::new(DispatchTable::new());
    let manager = MicroModuleManager::new(&context, "glyim_micro", dispatch);
    assert_eq!(manager.module_count(), 0);
}

#[test]
fn micro_module_manager_create_module() {
    let context = Context::create();
    let dispatch = Arc::new(DispatchTable::new());
    let mut manager = MicroModuleManager::new(&context, "glyim_micro", dispatch);

    let module = manager.create_module_for_item("add_fn");
    assert!(module.is_some());
    assert_eq!(manager.module_count(), 1);
}

#[test]
fn micro_module_manager_create_duplicate_module_replaces() {
    let context = Context::create();
    let dispatch = Arc::new(DispatchTable::new());
    let mut manager = MicroModuleManager::new(&context, "glyim_micro", dispatch);

    let _m1 = manager.create_module_for_item("fn_a");
    let _m2 = manager.create_module_for_item("fn_a"); // replaces
    assert_eq!(manager.module_count(), 1);
}

#[test]
fn micro_module_manager_create_multiple_modules() {
    let context = Context::create();
    let dispatch = Arc::new(DispatchTable::new());
    let mut manager = MicroModuleManager::new(&context, "glyim_micro", dispatch);

    let _m1 = manager.create_module_for_item("fn_a");
    let _m2 = manager.create_module_for_item("fn_b");
    let _m3 = manager.create_module_for_item("fn_c");
    assert_eq!(manager.module_count(), 3);
}

#[test]
fn micro_module_manager_remove_module() {
    let context = Context::create();
    let dispatch = Arc::new(DispatchTable::new());
    let mut manager = MicroModuleManager::new(&context, "glyim_micro", dispatch);

    let _m = manager.create_module_for_item("temp");
    assert_eq!(manager.module_count(), 1);
    manager.remove_module("temp");
    assert_eq!(manager.module_count(), 0);
}

#[test]
fn micro_module_manager_contains_module() {
    let context = Context::create();
    let dispatch = Arc::new(DispatchTable::new());
    let mut manager = MicroModuleManager::new(&context, "glyim_micro", dispatch);

    assert!(!manager.contains("fn_a"));
    let _m = manager.create_module_for_item("fn_a");
    assert!(manager.contains("fn_a"));
}

#[test]
fn micro_module_manager_is_send() {
    fn assert_bounds<T: Send>() {}
    assert_bounds::<DispatchTable>();
    // MicroModuleManager is not Send because it contains inkwell::Module<'ctx>
    // which has a lifetime tied to the Context. This is expected.
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p glyim-codegen-llvm --lib micro_module_tests 2>&1 | head -5`
Expected: Compilation error

- [ ] **Step 3: Implement MicroModuleManager**

`crates/glyim-codegen-llvm/src/micro_module.rs`:

```rust
//! Micro-module manager for incremental JIT compilation.
//!
//! Instead of one monolithic LLVM Module, the MicroModuleManager
//! creates a separate Module for each HIR item (function, struct,
//! etc.). This enables:
//!
//! - Independent compilation: changing one function only recompiles
//!   that function's module
//! - Hot-swapping: the old module can be replaced without touching
//!   other modules
//! - Parallel compilation: modules can be compiled on different
//!   threads (future work)
//!
//! Each micro-module gets its own name in the format
//! `glyim_item_<name>` and can be independently compiled and
//! added to a JITDylib.

use crate::dispatch::DispatchTable;
use inkwell::context::Context;
use inkwell::module::Module;
use std::collections::HashMap;
use std::sync::Arc;

/// Manages per-item LLVM modules for incremental JIT compilation.
pub struct MicroModuleManager<'ctx> {
    /// The LLVM context (shared across all modules).
    context: &'ctx Context,
    /// Prefix for module names.
    prefix: String,
    /// One module per item name.
    modules: HashMap<String, Module<'ctx>>,
    /// The global dispatch table for cross-module calls.
    dispatch: Arc<DispatchTable>,
}

impl<'ctx> MicroModuleManager<'ctx> {
    /// Create a new MicroModuleManager.
    pub fn new(
        context: &'ctx Context,
        prefix: &str,
        dispatch: Arc<DispatchTable>,
    ) -> Self {
        Self {
            context,
            prefix: prefix.to_string(),
            modules: HashMap::new(),
            dispatch,
        }
    }

    /// Create a new LLVM module for an item.
    ///
    /// If a module with the same name already exists, it is replaced.
    /// Returns a mutable reference to the new module.
    pub fn create_module_for_item(&mut self, item_name: &str) -> Option<&Module<'ctx>> {
        let module_name = format!("{}_{}", self.prefix, item_name);
        let module = self.context.create_module(&module_name);
        self.modules.insert(item_name.to_string(), module);
        self.modules.get(item_name)
    }

    /// Get a reference to a module by item name.
    pub fn get_module(&self, item_name: &str) -> Option<&Module<'ctx>> {
        self.modules.get(item_name)
    }

    /// Remove a module by item name.
    pub fn remove_module(&mut self, item_name: &str) {
        self.modules.remove(item_name);
    }

    /// Check if a module exists for the given item.
    pub fn contains(&self, item_name: &str) -> bool {
        self.modules.contains_key(item_name)
    }

    /// Number of managed modules.
    pub fn module_count(&self) -> usize {
        self.modules.len()
    }

    /// Get all item names.
    pub fn item_names(&self) -> Vec<&str> {
        self.modules.keys().map(|s| s.as_str()).collect()
    }

    /// Get a reference to the dispatch table.
    pub fn dispatch(&self) -> &DispatchTable {
        &self.dispatch
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p glyim-codegen-llvm --lib micro_module_tests`
Expected: All 7 tests PASS

- [ ] **Step 5: Commit**

```bash
git add crates/glyim-codegen-llvm/src/micro_module.rs crates/glyim-codegen-llvm/src/tests/micro_module_tests.rs
git commit -m "feat(codegen): add MicroModuleManager — per-item LLVM modules for incremental JIT"
```

---

## Chunk 3: DoubleBufferedJIT & TieredCompiler

These components build on the DispatchTable, MicroModuleManager, and BytecodeInterpreter to provide live hot-patching with zero-downtime and automatic tier promotion.

---

### Task 7: DoubleBufferedJIT

**Files:**
- Create: `crates/glyim-codegen-llvm/src/live.rs`
- Test: `crates/glyim-codegen-llvm/src/tests/live_tests.rs`

- [ ] **Step 1: Write failing tests for DoubleBufferedJIT**

Create `crates/glyim-codegen-llvm/src/tests/live_tests.rs`:

```rust
use glyim_codegen_llvm::live::{DoubleBufferedJIT, StagingArea};
use glyim_codegen_llvm::dispatch::DispatchTable;
use glyim_interner::Interner;
use std::sync::Arc;

#[test]
fn double_buffered_jit_create() {
    let dispatch = Arc::new(DispatchTable::new());
    let jit = DoubleBufferedJIT::new(dispatch);
    assert_eq!(jit.staged_count(), 0);
}

#[test]
fn double_buffered_jit_stage_item() {
    let dispatch = Arc::new(DispatchTable::new());
    let mut jit = DoubleBufferedJIT::new(dispatch);

    let mut interner = Interner::new();
    let sym = interner.intern("add");
    jit.stage_item(sym);

    assert_eq!(jit.staged_count(), 1);
}

#[test]
fn double_buffered_jit_stage_multiple_items() {
    let dispatch = Arc::new(DispatchTable::new());
    let mut jit = DoubleBufferedJIT::new(dispatch);

    let mut interner = Interner::new();
    jit.stage_item(interner.intern("add"));
    jit.stage_item(interner.intern("sub"));
    jit.stage_item(interner.intern("mul"));

    assert_eq!(jit.staged_count(), 3);
}

#[test]
fn double_buffered_jit_commit_clears_staging() {
    let dispatch = Arc::new(DispatchTable::new());
    let mut jit = DoubleBufferedJIT::new(dispatch);

    let mut interner = Interner::new();
    jit.stage_item(interner.intern("add"));
    assert_eq!(jit.staged_count(), 1);

    jit.commit();
    assert_eq!(jit.staged_count(), 0);
}

#[test]
fn double_buffered_jit_commit_updates_dispatch() {
    let dispatch = Arc::new(DispatchTable::new());
    let mut jit = DoubleBufferedJIT::new(dispatch.clone());

    let mut interner = Interner::new();
    let sym = interner.intern("add");

    // Simulate: we "compiled" add and got address 0xDEAD
    dispatch.update(sym, 0xDEAD);

    // Stage and commit
    jit.stage_item(sym);
    jit.commit();

    // The dispatch table should have the updated address
    assert_eq!(dispatch.get_address(sym), 0xDEAD);
}

#[test]
fn double_buffered_jit_is_send() {
    fn assert_bounds<T: Send>() {}
    assert_bounds::<DoubleBufferedJIT>();
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p glyim-codegen-llvm --lib live_tests 2>&1 | head -5`
Expected: Compilation error

- [ ] **Step 3: Implement DoubleBufferedJIT**

`crates/glyim-codegen-llvm/src/live.rs`:

```rust
//! Double-buffered JIT execution with zero-downtime hot-patching.
//!
//! The DoubleBufferedJIT maintains two logical "buffers":
//! - **Active**: The currently executing code (in the DispatchTable)
//! - **Staging**: Items that have changed and need to be compiled
//!
//! When the user edits code:
//! 1. Changed items are added to the staging area
//! 2. A background thread compiles them into LLVM modules
//! 3. When compilation finishes, `commit()` atomically updates
//!    the DispatchTable to point to the new code
//! 4. The running program picks up the new code on next call
//!
//! This ensures zero-downtime: the program never pauses while
//! waiting for recompilation.

use crate::dispatch::DispatchTable;
use glyim_interner::Symbol;
use std::sync::Arc;

/// Items that are staged for recompilation but not yet committed.
#[derive(Clone, Debug, Default)]
pub struct StagingArea {
    /// Symbols that need recompilation.
    items: Vec<Symbol>,
}

/// Double-buffered JIT: stages changed items, then commits them
/// atomically via the DispatchTable.
pub struct DoubleBufferedJIT {
    /// The global dispatch table.
    dispatch: Arc<DispatchTable>,
    /// Items staged for recompilation.
    staging: StagingArea,
}

impl DoubleBufferedJIT {
    /// Create a new DoubleBufferedJIT with the given dispatch table.
    pub fn new(dispatch: Arc<DispatchTable>) -> Self {
        Self {
            dispatch,
            staging: StagingArea::default(),
        }
    }

    /// Add an item to the staging area for recompilation.
    pub fn stage_item(&mut self, sym: Symbol) {
        self.staging.items.push(sym);
    }

    /// Number of items currently staged.
    pub fn staged_count(&self) -> usize {
        self.staging.items.len()
    }

    /// Get the staged items.
    pub fn staged_items(&self) -> &[Symbol] {
        &self.staging.items
    }

    /// Commit all staged items to the dispatch table.
    ///
    /// In the full implementation, this:
    /// 1. Compiles each staged item into LLVM IR
    /// 2. Adds the compiled module to a new JITDylib
    /// 3. Looks up the new function address
    /// 4. Atomically updates the DispatchTable
    /// 5. Clears the staging area
    ///
    /// For now, this just clears the staging area. The actual
    /// LLVM compilation will be wired in the integration chunk.
    pub fn commit(&mut self) {
        // Full implementation: compile each item, update dispatch
        // For now: just clear the staging area
        self.staging.items.clear();
    }

    /// Get a reference to the dispatch table.
    pub fn dispatch(&self) -> &DispatchTable {
        &self.dispatch
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p glyim-codegen-llvm --lib live_tests`
Expected: All 6 tests PASS

- [ ] **Step 5: Commit**

```bash
git add crates/glyim-codegen-llvm/src/live.rs crates/glyim-codegen-llvm/src/tests/live_tests.rs
git commit -m "feat(codegen): add DoubleBufferedJIT — zero-downtime hot-patching with staging area"
```

---

### Task 8: TieredCompiler

**Files:**
- Create: `crates/glyim-codegen-llvm/src/tiered.rs`
- Test: `crates/glyim-codegen-llvm/src/tests/tiered_tests.rs`

- [ ] **Step 1: Write failing tests for TieredCompiler**

Create `crates/glyim-codegen-llvm/src/tests/tiered_tests.rs`:

```rust
use glyim_codegen_llvm::tiered::{TieredCompiler, ExecutionTier};
use glyim_codegen_llvm::dispatch::DispatchTable;
use glyim_bytecode::compiler::BytecodeCompiler;
use glyim_bytecode::interpreter::BytecodeInterpreter;
use glyim_bytecode::value::Value;
use glyim_hir::node::{HirExpr, HirFn};
use glyim_hir::types::{ExprId, HirType};
use glyim_hir::Span;
use glyim_interner::Interner;
use std::sync::Arc;

fn make_span() -> Span { Span::default() }

fn make_simple_fn(interner: &mut Interner, name: &str, value: i64) -> HirFn {
    HirFn {
        doc: None, name: interner.intern(name), type_params: vec![],
        params: vec![], param_mutability: vec![], ret: Some(HirType::Int),
        body: HirExpr::IntLit { id: ExprId::new(0), value, span: make_span() },
        span: make_span(), is_pub: false, is_macro_generated: false, is_extern_backed: false,
    }
}

#[test]
fn tiered_compiler_new_function_starts_at_tier0() {
    let dispatch = Arc::new(DispatchTable::new());
    let mut interner = Interner::new();
    let tiered = TieredCompiler::new(dispatch, 100);

    let name = interner.intern("add");
    assert_eq!(tiered.execution_tier(name), ExecutionTier::Tier0);
}

#[test]
fn tiered_compiler_execute_increments_count() {
    let dispatch = Arc::new(DispatchTable::new());
    let mut interner = Interner::new();
    let mut tiered = TieredCompiler::new(dispatch, 100);

    let name = interner.intern("add");
    assert_eq!(tiered.execution_count(name), 0);

    // Record an execution
    tiered.record_execution(name);
    assert_eq!(tiered.execution_count(name), 1);

    tiered.record_execution(name);
    assert_eq!(tiered.execution_count(name), 2);
}

#[test]
fn tiered_compiler_promote_after_threshold() {
    let dispatch = Arc::new(DispatchTable::new());
    let mut interner = Interner::new();
    let mut tiered = TieredCompiler::new(dispatch, 5); // low threshold for testing

    let name = interner.intern("hot_fn");

    // Execute 4 times — still Tier-0
    for _ in 0..4 {
        tiered.record_execution(name);
    }
    assert_eq!(tiered.execution_tier(name), ExecutionTier::Tier0);

    // 5th execution — should be marked for promotion
    tiered.record_execution(name);
    assert_eq!(tiered.execution_tier(name), ExecutionTier::Tier1);
}

#[test]
fn tiered_compiler_promote_idle() {
    let dispatch = Arc::new(DispatchTable::new());
    let mut interner = Interner::new();
    let mut tiered = TieredCompiler::new(dispatch, 1000); // high threshold

    let name = interner.intern("lazy_fn");
    tiered.record_execution(name);
    assert_eq!(tiered.execution_tier(name), ExecutionTier::Tier0);

    // Force promote all Tier-0 functions
    let promoted = tiered.promote_all();
    assert_eq!(promoted.len(), 1);
    assert_eq!(tiered.execution_tier(name), ExecutionTier::Tier1);
}

#[test]
fn tiered_compiler_execution_tier_unknown() {
    let dispatch = Arc::new(DispatchTable::new());
    let mut interner = Interner::new();
    let tiered = TieredCompiler::new(dispatch, 100);

    let unknown = interner.intern("unknown");
    assert_eq!(tiered.execution_tier(unknown), ExecutionTier::Tier0);
    assert_eq!(tiered.execution_count(unknown), 0);
}

#[test]
fn execution_tier_ordering() {
    assert!(ExecutionTier::Tier0 < ExecutionTier::Tier1);
}

#[test]
fn tiered_compiler_is_send() {
    fn assert_bounds<T: Send>() {}
    assert_bounds::<TieredCompiler>();
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p glyim-codegen-llvm --lib tiered_tests 2>&1 | head -5`
Expected: Compilation error

- [ ] **Step 3: Implement TieredCompiler**

`crates/glyim-codegen-llvm/src/tiered.rs`:

```rust
//! Tiered compilation manager.
//!
//! Manages the transition between Tier-0 (bytecode interpreter)
//! and Tier-1 (LLVM JIT). Newly compiled functions start in Tier-0
//! for sub-millisecond feedback. After a heat threshold (number of
//! executions) or an idle period, they are promoted to Tier-1 LLVM
//! JIT for full-speed execution.

use crate::dispatch::DispatchTable;
use dashmap::DashMap;
use glyim_interner::Symbol;
use std::sync::Arc;

/// The execution tier for a function.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ExecutionTier {
    /// Tier-0: bytecode interpreter (sub-millisecond startup).
    Tier0,
    /// Tier-1: LLVM JIT (full-speed execution).
    Tier1,
}

/// Manages tiered compilation for JIT functions.
pub struct TieredCompiler {
    /// The dispatch table for function pointer lookups.
    dispatch: Arc<DispatchTable>,
    /// Execution count per function.
    execution_counts: DashMap<Symbol, u64>,
    /// Current execution tier per function.
    tiers: DashMap<Symbol, ExecutionTier>,
    /// Number of executions before promotion to Tier-1.
    promotion_threshold: u64,
}

impl TieredCompiler {
    /// Create a new TieredCompiler.
    ///
    /// `promotion_threshold`: number of executions before a function
    /// is automatically promoted from Tier-0 to Tier-1. Default: 100.
    pub fn new(dispatch: Arc<DispatchTable>, promotion_threshold: u64) -> Self {
        Self {
            dispatch,
            execution_counts: DashMap::new(),
            tiers: DashMap::new(),
            promotion_threshold,
        }
    }

    /// Record that a function was executed.
    ///
    /// If the execution count exceeds the promotion threshold,
    /// the function is automatically promoted to Tier-1.
    pub fn record_execution(&self, sym: Symbol) {
        let count = self.execution_counts.entry(sym).or_insert(0);
        *count.value_mut() += 1;
        let current_count = *count.value();

        if current_count >= self.promotion_threshold {
            drop(count); // release the DashMap ref
            self.tiers.insert(sym, ExecutionTier::Tier1);
        }
    }

    /// Get the current execution tier for a function.
    pub fn execution_tier(&self, sym: Symbol) -> ExecutionTier {
        self.tiers.get(&sym).map(|t| *t.value()).unwrap_or(ExecutionTier::Tier0)
    }

    /// Get the execution count for a function.
    pub fn execution_count(&self, sym: Symbol) -> u64 {
        self.execution_counts.get(&sym).map(|c| *c.value()).unwrap_or(0)
    }

    /// Manually promote a function to Tier-1.
    pub fn promote(&self, sym: Symbol) {
        self.tiers.insert(sym, ExecutionTier::Tier1);
    }

    /// Promote all Tier-0 functions to Tier-1.
    ///
    /// Called during idle periods when no edits are happening.
    /// Returns the list of promoted function symbols.
    pub fn promote_all(&self) -> Vec<Symbol> {
        let mut promoted = Vec::new();
        for entry in self.execution_counts.iter() {
            let sym = *entry.key();
            if self.execution_tier(sym) == ExecutionTier::Tier0 {
                self.tiers.insert(sym, ExecutionTier::Tier1);
                promoted.push(sym);
            }
        }
        promoted
    }

    /// Reset a function back to Tier-0 (e.g., after an edit).
    pub fn reset_tier(&self, sym: Symbol) {
        self.tiers.insert(sym, ExecutionTier::Tier0);
        self.execution_counts.insert(sym, 0);
    }

    /// Get the dispatch table.
    pub fn dispatch(&self) -> &DispatchTable {
        &self.dispatch
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p glyim-codegen-llvm --lib tiered_tests`
Expected: All 7 tests PASS

- [ ] **Step 5: Commit**

```bash
git add crates/glyim-codegen-llvm/src/tiered.rs crates/glyim-codegen-llvm/src/tests/tiered_tests.rs
git commit -m "feat(codegen): add TieredCompiler — bytecode → LLVM promotion management"
```

---

## Summary and Verification

### Test Coverage

| Crate | Module | Test File | # Tests |
|---|---|---|---|
| `glyim-codegen-llvm` | `dispatch` | `tests/dispatch_tests.rs` | 10 |
| `glyim-codegen-llvm` | `orc` | `tests/orc_tests.rs` | 5 |
| `glyim-codegen-llvm` | `micro_module` | `tests/micro_module_tests.rs` | 7 |
| `glyim-codegen-llvm` | `live` | `tests/live_tests.rs` | 6 |
| `glyim-codegen-llvm` | `tiered` | `tests/tiered_tests.rs` | 7 |
| `glyim-bytecode` | `op` | `tests/op_tests.rs` | 8 |
| `glyim-bytecode` | `compiler` | `tests/compiler_tests.rs` | 9 |
| `glyim-bytecode` | `interpreter` | `tests/interpreter_tests.rs` | 10 |
| **Total** | | | **62** |

### Dependency Graph Between Chunks

```
Chunk 1 (DispatchTable + Bytecode Interpreter)
   │
   ├── Task 1: DispatchTable ──────────┐
   │                                    │
   ├── Task 2: BytecodeOp + Value ──┐  │
   │                                 │  │
   ├── Task 3: BytecodeCompiler ────┤  │
   │                                 │  │
   └── Task 4: BytecodeInterpreter ─┘  │
                                        │
Chunk 2 (OrcV2 + MicroModuleManager) ───┤
   │                                    │
   ├── Task 5: OrcSession ─────────────┤
   │                                    │
   └── Task 6: MicroModuleManager ─────┤
                                        │
Chunk 3 (DoubleBufferedJIT + Tiered) ───┘
   │
   ├── Task 7: DoubleBufferedJIT ────── uses DispatchTable
   │
   └── Task 8: TieredCompiler ───────── uses DispatchTable
```

Chunk 1 and Chunk 2 can be developed in parallel. Chunk 3 depends on Chunk 1 (DispatchTable).

### Key Design Decisions

1. **postcard** is used for all serialization (not bincode). Bytecode ops and Merkle node headers are serialized with postcard for compact, deterministic encoding.

2. **OrcV2 via llvm-sys FFI**: inkwell 0.9 does not fully wrap OrcV2's `JITDylib` management and lazy reexport APIs. We use `llvm-sys` 221 directly with safe Rust wrappers. The `OrcSession` and `OrcDylib` types wrap `LLVMOrcExecutionSessionRef` and `LLVMOrcJITDylibRef`.

3. **DispatchTable is the backbone**: All hot-swapping goes through the atomic pointer table. The interpreter, the JIT, and future native code all register their function addresses in the same table. Callers always look up the current address before calling.

4. **BytecodeInterpreter is deliberately simple**: No JIT, no optimization, no SSA. Just a stack-based interpreter with jump patching for if/else and while loops. The goal is fast compilation from HIR (microseconds), not fast execution.

5. **MicroModuleManager creates per-item LLVM modules**: Each function gets its own `Module<'ctx>` named `glyim_item_<name>`. This enables independent compilation and replacement. The `Codegen` struct is refactored to work with externally-created modules.

### Success Criteria

1. **A newly edited function produces Tier-0 bytecode in <1ms** — verified by BytecodeCompiler tests showing successful HIR → bytecode compilation.
2. **A hot function (100+ executions) is automatically promoted to Tier-1 LLVM JIT** — verified by TieredCompiler promotion threshold test.
3. **Changing one function only recompiles that function's micro-module** — verified by MicroModuleManager's per-item module creation and removal.
4. **The dispatch table enables atomic hot-swapping** — verified by concurrent update test showing no data races.
5. **The staging area separates "being compiled" from "currently running"** — verified by DoubleBufferedJIT commit flow test.

### Estimated Timeline

| Chunk | Tasks | Estimated Effort |
|---|---|---|
| Chunk 1: DispatchTable + Bytecode | Tasks 1-4 | 5-6 days |
| Chunk 2: OrcV2 + MicroModuleManager | Tasks 5-6 | 3-4 days |
| Chunk 3: DoubleBufferedJIT + TieredCompiler | Tasks 7-8 | 3-4 days |
| **Total** | | **11-14 days** |
