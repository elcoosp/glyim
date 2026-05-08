# Glyim Incremental Compiler — Phase 10 Implementation Plan

## WebAssembly Target, Plugin Architecture & Multi-Backend Code Generation

**Codebase:** elcoosp-glyim v0.5.0  
**Rust Workspace | 22+ Crates | LLVM 22.1 / Inkwell 0.9 | Wasmtime 44**  
**Date:** 2026-05-07

---

## 1. Executive Summary

Phase 10 transforms Glyim from a native-only compiler into a multi-target, multi-backend, extensible compilation platform with first-class WebAssembly support and a plugin architecture for third-party compiler extensions. Phases 0 through 9 assembled a comprehensive incremental compiler with query-driven memoization, Merkle content-addressed storage, JIT micro-modules, e-graph optimization, cross-module incremental linking, test-aware compilation, LSP-based IDE integration, production hardening, and distributed compilation with remote build execution. However, the compiler currently targets only native platforms (Linux, macOS, Windows on x86_64 and aarch64), its code generation is monolithically coupled to LLVM through the `Codegen<'ctx>` struct in `glyim-codegen-llvm`, and there is no mechanism for third-party extensions to participate in the compilation pipeline. The existing WebAssembly support is limited to the macro system: `compile_to_wasm()` produces an LLVM object file targeting `wasm32-wasi` for procedural macro execution, and `wasm_abi.rs` generates an `expand` export wrapper with the `(i32, i32, i32) -> i32` calling convention. The `wasm32-wasi` target is not even listed in the `SUPPORTED_TARGETS` array in `glyim-compiler/src/cross.rs`, and the existing `compile_to_wasm()` function has a known issue: it writes an LLVM `.o` file and reads back the bytes, which produces a WASM object file rather than a standalone `.wasm` module.

Phase 10 closes these gaps through three interconnected workstreams. **WebAssembly target support** elevates `wasm32-wasi` from an internal macro compilation detail to a first-class compilation target. The `glyim build --target wasm32-wasi` command compiles a Glyim source file into a standalone `.wasm` module that can run in any WASI-compliant runtime (Wasmtime, Wasmer, WasmEdge) or, with the WASI browser polyfill, in a web browser. The phase introduces a proper Wasm linker that converts LLVM's wasm32 object file into a standalone Wasm module, a Wasm runtime adapter that enables `glyim run` to execute compiled Wasm binaries using the existing Wasmtime 44 integration from `glyim-macro-core`, a browser runtime with JavaScript bindings that enables Glyim-compiled Wasm modules to run in web applications, and WASI preview 2 component model support for composing Wasm modules from multiple languages.

**Multi-backend code generation** introduces a `Backend` trait that abstracts over code generation, decoupling the compiler pipeline from its LLVM dependency. The existing `Codegen<'ctx>` struct is refactored behind a `Backend` trait with two implementations: `LlvmBackend` (the existing LLVM codegen, unchanged) and `CraneliftBackend` (a new backend using the Cranelift code generator for faster debug builds). The `CraneliftBackend` compiles HIR to native code 3–10x faster than LLVM at the cost of less optimized output, making it ideal for development builds where compilation speed matters more than runtime performance. The pipeline selects the backend based on the build mode: `Debug` mode uses Cranelift by default (fast compilation), `Release` mode uses LLVM (optimized output), and the `--backend` flag overrides the default. The JIT engine from Phase 2 is extended to support both backends, enabling Cranelift's faster compilation for the hot-reload development workflow.

**Plugin architecture** introduces a compiler extension system that allows third-party crates to participate in the compilation pipeline through well-defined hook points. The `glyim-plugin` crate defines a `CompilerPlugin` trait with methods for registering lint passes, optimization passes, code generation hooks, and diagnostic decorators. Plugins are loaded at compiler startup from a `plugins/` directory (dynamically via `libloading` on native targets, or statically via Rust's plugin registry), and they are invoked at specific points in the pipeline: after parsing, after type checking, after e-graph optimization, before code generation, and after linking. The existing `glyim-lint` crate is refactored into a plugin, and the existing `glyim-fmt` formatter is exposed as a plugin. The phase also introduces a `LinterPlugin` trait that provides a structured API for writing custom lints with access to the HIR, type information, and the symbol index.

**Estimated effort:** 40–55 working days.

**Key deliverables:**
- First-class `wasm32-wasi` compilation target with standalone `.wasm` module output
- Wasm linker converting LLVM wasm32 object files to standalone Wasm modules
- Wasm runtime adapter for `glyim run --target wasm32-wasi` using Wasmtime
- Browser runtime with JavaScript bindings and WASI polyfill
- WASI preview 2 component model support
- `Backend` trait abstracting over code generation
- `CraneliftBackend` for fast debug builds (3–10x faster compilation than LLVM)
- Backend selection: Cranelift for Debug, LLVM for Release, `--backend` override
- JIT support for both LLVM and Cranelift backends
- `glyim-plugin` crate with `CompilerPlugin` trait and lifecycle hooks
- Dynamic plugin loading via `libloading` on native targets
- `LinterPlugin` trait for custom lint integration
- Refactored `glyim-lint` and `glyim-fmt` as plugins
- Plugin registry with lifecycle management and error isolation

---

## 2. Current Codebase State Assessment

### 2.1 WebAssembly Support (As-Is)

The existing WASM support is designed exclusively for the procedural macro system:

| Component | File | Status | Gap |
|-----------|------|--------|-----|
| `compile_to_wasm()` | `glyim-codegen-llvm/src/lib.rs` | Compiles Glyim source → LLVM `.o` targeting `wasm32-wasi` | Produces an object file, not a standalone `.wasm` module; no Wasm linker |
| `wasm_abi.rs` | `glyim-codegen-llvm/src/wasm_abi.rs` | Generates `expand` export wrapper with `(i32,i32,i32)->i32` ABI | Only for macro entry points; not for general-purpose Wasm exports |
| `MacroExecutor` | `glyim-macro-core/src/executor.rs` | Wasmtime 44 runtime: loads Wasm, calls `expand`, enforces fuel limits | Cannot execute general-purpose Glyim Wasm programs; no CLI access |
| `wasm_interface.rs` | `glyim-macro-core/src/wasm_interface.rs` | Binary protocol for passing HIR over Wasm memory | Only for macro input/output; no general-purpose ABI |
| `wasi_stubs.rs` | `glyim-macro-core/src/wasi_stubs.rs` | Deterministic WASI view for reproducible macro execution | No standard WASI preview 2 support |
| `wasm_publish.rs` | `glyim-pkg/src/wasm_publish.rs` | Compiles macro source → Wasm, stores in CAS | Only for macro publishing |
| `verify.rs` | `glyim-cas-server/src/verify.rs` | Server endpoint `/verify-wasm` | Only verifies macro reproducibility |

### 2.2 Code Generation Architecture (As-Is)

The codegen is monolithically coupled to LLVM:

| Component | Status | Gap |
|-----------|--------|-----|
| `Codegen<'ctx>` struct | All fields are LLVM-specific (`Context`, `Module`, `Builder`, `IntType`, `FloatType`) | No `Backend` trait; cannot swap code generators |
| `CodegenBuilder` | Builder pattern for constructing `Codegen` instances | Tightly coupled to LLVM types |
| `DebugInfoGen<'ctx>` | Full DWARF debug info generation via LLVM's `DIBuilder` | No backend-agnostic debug info interface |
| `DispatchTable` | Thread-safe function pointer table for JIT | Only works with LLVM JIT (OrcV2) |
| `compile_to_wasm()` | Uses `Codegen` with `wasm32-wasi` target | Same monolithic path; cannot use Cranelift for Wasm |
| `compile_to_ir()` | Returns LLVM IR string | Backend-specific; no HIR→IR abstraction |

### 2.3 Target Support (As-Is)

| Target | Status | Notes |
|--------|--------|-------|
| `x86_64-unknown-linux-gnu` | Supported | Default on Linux x86_64 |
| `aarch64-unknown-linux-gnu` | Supported | Cross-compilation with sysroot |
| `x86_64-apple-darwin` | Supported | Default on macOS Intel |
| `aarch64-apple-darwin` | Supported | Default on macOS Apple Silicon |
| `x86_64-pc-windows-msvc` | Supported | Windows MSVC ABI |
| `wasm32-wasi` | **Internal only** | Not in `SUPPORTED_TARGETS`; only for macro compilation |

### 2.4 Debug Information (As-Is)

| Component | Status | Gap |
|-----------|--------|-----|
| `DebugInfoGen<'ctx>` | Full DWARF via LLVM `DIBuilder` | No backend-agnostic debug info; tied to inkwell types |
| `DebugMode` enum | `None`, `LineTablesOnly`, `Full` | Good; but no way to emit debug info from non-LLVM backends |
| `create_subprogram()` | Creates `DISubprogram` with source location | LLVM-specific |
| `insert_declare()` | Uses manual FFI to avoid inkwell 0.9 panic | Fragile; will need backend abstraction |
| DAP integration | Not implemented | Phase 7 LSP does not support Debug Adapter Protocol |

### 2.5 Plugin / Extension Points (As-Is)

| Concern | Current State | Gap |
|---------|--------------|-----|
| Lint passes | `glyim-lint` is a single crate with hardcoded lints | No `LinterPlugin` trait; no way to add custom lints |
| Formatting | `glyim-fmt` is a crate with CST-aware formatting | Not a plugin; cannot be extended with custom formatting rules |
| Optimization passes | E-graph from Phase 3; hardcoded rewrite rules | No way to add custom optimization passes |
| Code generation | Monolithic `Codegen<'ctx>` | No `Backend` trait; no custom codegen hooks |
| Diagnostic decorators | No extension points | Cannot add custom error messages or suggestions |
| External tools | No integration | No way to run formatters, linters, or other tools as part of the pipeline |

### 2.6 Critical Gaps That Phase 10 Addresses

| Gap | Impact | Affected Crate | Phase 10 Solution |
|-----|--------|---------------|-------------------|
| No Wasm compilation target | Cannot deploy Glyim programs to the web or WASI runtimes | `glyim-compiler`, `glyim-codegen-llvm` | First-class `wasm32-wasi` target with proper Wasm linker |
| No standalone Wasm module output | `compile_to_wasm()` produces `.o`, not `.wasm` | `glyim-codegen-llvm` | Wasm linker (`.o` → `.wasm`) using `wasm-ld` or `wasm-tools` |
| No Wasm runtime in CLI | `glyim run` cannot execute Wasm programs | `glyim-cli` | Wasm runtime adapter using Wasmtime |
| No browser runtime | Cannot run Glyim programs in web browsers | (missing) | New `glyim-browser-runtime` with JS bindings |
| Monolithic LLVM backend | Cannot use faster code generators for debug builds; cannot swap backends | `glyim-codegen-llvm` | `Backend` trait + `LlvmBackend` + `CraneliftBackend` |
| No Cranelift backend | Debug builds are as slow as release builds | (missing) | New `glyim-codegen-cranelift` crate |
| No Backend trait | Codegen is not pluggable | `glyim-compiler` | `glyim-backend` trait crate |
| No plugin system | Third-party extensions cannot participate in compilation | (missing) | New `glyim-plugin` crate with `CompilerPlugin` trait |
| No custom lint API | `glyim-lint` is not extensible | `glyim-lint` | `LinterPlugin` trait; refactor `glyim-lint` as plugin |
| No dynamic plugin loading | Extensions must be compiled into the compiler | (missing) | `libloading`-based plugin discovery and loading |

---

## 3. Architecture Design

### 3.1 Multi-Backend Architecture

The multi-backend architecture introduces a `Backend` trait that abstracts over code generation, decoupling the compiler pipeline from its LLVM dependency:

```
┌───────────────────────────────────────────────────────────────┐
│                     Compiler Pipeline                         │
│  Source → Macro → Parse → HIR → TypeCheck → Egraph → ???     │
│                                                        │      │
│                                              ┌─────────▼──┐  │
│                                              │  Backend    │  │
│                                              │  Trait      │  │
│                                              └──┬──────┬──┘  │
│                                                 │      │     │
│  ┌──────────────────────────┐  ┌─────────────────▼──┐  │     │
│  │    LlvmBackend            │  │  CraneliftBackend   │  │     │
│  │  ┌────────────────────┐  │  │  ┌───────────────┐  │  │     │
│  │  │ Codegen<'ctx>      │  │  │  │ Cranelift      │  │  │     │
│  │  │ (existing)         │  │  │  │ codegen        │  │  │     │
│  │  └────────────────────┘  │  │  └───────────────┘  │  │     │
│  │  ┌────────────────────┐  │  │  ┌───────────────┐  │  │     │
│  │  │ DebugInfoGen       │  │  │  │ ISLE pattern   │  │  │     │
│  │  │ (DWARF)            │  │  │  │ matching       │  │  │     │
│  │  └────────────────────┘  │  │  └───────────────┘  │  │     │
│  │  ┌────────────────────┐  │  │  ┌───────────────┐  │  │     │
│  │  │ OrcV2 JIT          │  │  │  │ JIT via        │  │  │     │
│  │  │ (existing)         │  │  │  │ Cranelift      │  │  │     │
│  │  └────────────────────┘  │  │  └───────────────┘  │  │     │
│  └──────────────────────────┘  └─────────────────────┘  │     │
│                                                           │     │
│  Targets: x86_64, aarch64,     Targets: x86_64, aarch64, │     │
│           wasm32-wasi                riscv64              │     │
└───────────────────────────────────────────────────────────────┘
```

### 3.2 Backend Trait

The `Backend` trait is defined in a new `glyim-backend` crate that provides the interface between the compiler pipeline and code generation:

```rust
// crates/glyim-backend/src/lib.rs

use glyim_hir::{Hir, HirFn, HirType, types::MonoHir};
use glyim_interner::Interner;
use std::path::Path;

/// A code generation backend that produces executable code from HIR.
pub trait Backend: Send + Sync {
    /// The name of this backend (e.g., "llvm", "cranelift").
    fn name(&self) -> &str;

    /// The target triples this backend supports.
    fn supported_targets(&self) -> &[&str];

    /// Compile a full HIR module into object code for the given target.
    fn compile_module(
        &self,
        hir: &Hir,
        mono_hir: &MonoHir,
        interner: &Interner,
        config: &BackendConfig,
    ) -> Result<BackendOutput, BackendError>;

    /// Compile a single function into object code.
    /// Used by the incremental pipeline for per-function codegen.
    fn compile_function(
        &self,
        function: &HirFn,
        mono_types: &glyim_hir::types::TypeOverrides,
        interner: &Interner,
        config: &BackendConfig,
    ) -> Result<FunctionOutput, BackendError>;

    /// Create a JIT session for incremental compilation.
    fn create_jit_session(
        &self,
        config: &JitConfig,
    ) -> Result<Box<dyn JitSession>, BackendError>;

    /// Generate debug information for the given HIR.
    fn debug_info_mode(&self) -> DebugInfoMode;

    /// Link object files into a final binary or module.
    fn link(
        &self,
        object_files: &[Vec<u8>],
        config: &LinkConfig,
    ) -> Result<Vec<u8>, BackendError>;
}

/// Configuration for a backend compilation.
#[derive(Debug, Clone)]
pub struct BackendConfig {
    /// Target triple (e.g., "x86_64-unknown-linux-gnu", "wasm32-wasi").
    pub target_triple: String,
    /// Optimization level.
    pub opt_level: OptLevel,
    /// Debug information mode.
    pub debug_info: DebugInfoMode,
    /// Whether this is a JIT compilation.
    pub is_jit: bool,
    /// Whether to generate position-independent code.
    pub pic: bool,
    /// Additional backend-specific flags.
    pub flags: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptLevel {
    /// No optimization (fastest compilation).
    None,
    /// Basic optimizations (minimal compile-time cost).
    Less,
    /// Standard optimizations (good balance).
    Default,
    /// Aggressive optimizations (slowest compilation, best output).
    Aggressive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugInfoMode {
    /// No debug information.
    None,
    /// Line tables only (source location mapping).
    LineTablesOnly,
    /// Full DWARF debug information.
    Full,
}

/// Output from a full module compilation.
#[derive(Debug)]
pub struct BackendOutput {
    /// The compiled object code.
    pub object_code: Vec<u8>,
    /// List of exported symbol names.
    pub exported_symbols: Vec<String>,
    /// List of imported symbol names (externs).
    pub imported_symbols: Vec<String>,
    /// Debug information (DWARF sections, if generated).
    pub debug_info: Option<Vec<u8>>,
    /// Compilation statistics.
    pub stats: BackendStats,
}

/// Output from a single function compilation.
#[derive(Debug)]
pub struct FunctionOutput {
    /// The compiled function object code.
    pub object_code: Vec<u8>,
    /// The mangled symbol name.
    pub symbol_name: String,
    /// Size of the generated code in bytes.
    pub code_size: usize,
}

/// Statistics from a backend compilation.
#[derive(Debug, Clone)]
pub struct BackendStats {
    /// Wall-clock compilation time.
    pub compile_time: std::time::Duration,
    /// Number of functions compiled.
    pub functions_compiled: usize,
    /// Total bytes of generated code.
    pub code_size: usize,
    /// Peak memory usage during compilation.
    pub peak_memory: Option<usize>,
}

/// Configuration for JIT compilation.
#[derive(Debug, Clone)]
pub struct JitConfig {
    /// Target triple for JIT compilation (usually the host triple).
    pub target_triple: String,
    /// Whether to enable lazy compilation (compile functions on first call).
    pub lazy: bool,
    /// Optimization level for JIT-compiled code.
    pub opt_level: OptLevel,
}

/// A JIT compilation session that can compile and execute functions incrementally.
pub trait JitSession: Send + Sync {
    /// Compile a function and make it executable.
    fn compile_function(
        &mut self,
        function: &HirFn,
        mono_types: &glyim_hir::types::TypeOverrides,
        interner: &Interner,
    ) -> Result<*const u8, BackendError>;

    /// Get the address of an already-compiled function.
    fn get_function_address(&self, name: &str) -> Option<*const u8>;

    /// Invalidate a compiled function (e.g., after source edit).
    fn invalidate_function(&mut self, name: &str) -> Result<(), BackendError>;

    /// Dispose of the JIT session and free all compiled code.
    fn dispose(self: Box<Self>);
}

/// Configuration for linking.
#[derive(Debug, Clone)]
pub struct LinkConfig {
    /// Target triple.
    pub target_triple: String,
    /// Output format.
    pub output_format: LinkOutputFormat,
    /// Whether to include debug information in the output.
    pub include_debug_info: bool,
    /// Library search paths.
    pub library_search_paths: Vec<PathBuf>,
    /// Additional linker flags.
    pub linker_flags: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkOutputFormat {
    /// Native executable (ELF, Mach-O, PE).
    Executable,
    /// Native object file (.o).
    Object,
    /// Shared library (.so, .dylib, .dll).
    SharedLibrary,
    /// WebAssembly module (.wasm).
    WasmModule,
}

#[derive(Debug, Clone)]
pub struct BackendError {
    pub message: String,
    pub code: Option<String>,
    pub source_span: Option<glyim_diag::Span>,
}
```

### 3.3 WebAssembly Compilation Pipeline

The Wasm compilation pipeline extends the existing `compile_to_wasm()` function with proper module linking and runtime support:

```
┌────────────────────────────────────────────────────────────┐
│               Wasm Compilation Pipeline                    │
│                                                            │
│  Source ──▶ Parse ──▶ HIR ──▶ TypeCheck ──▶ Mono ──┐     │
│                                                      │     │
│  ┌───────────────────────────────────────────────────▼──┐  │
│  │              LlvmBackend (wasm32-wasi)                │  │
│  │  1. Generate LLVM IR with wasm32 target              │  │
│  │  2. Emit .o file (wasm object format)                │  │
│  └───────────────────────────┬──────────────────────────┘  │
│                              │                              │
│  ┌───────────────────────────▼──────────────────────────┐  │
│  │              Wasm Linker                               │  │
│  │  1. wasm-ld: merge .o → .wasm                         │  │
│  │  2. wasm-opt: optimize .wasm                           │  │
│  │  3. wasm-tools strip: remove debug sections            │  │
│  │  4. Generate import/export section                     │  │
│  │  5. Generate WASI reactor or command module            │  │
│  └───────────────────────────┬──────────────────────────┘  │
│                              │                              │
│  ┌───────────────────────────▼──────────────────────────┐  │
│  │              Wasm Runtime Adapter                      │  │
│  │  ┌─────────────────┐  ┌────────────────────────────┐ │  │
│  │  │ CLI Runner      │  │ Browser Runtime             │ │  │
│  │  │ (Wasmtime 44)   │  │ (JS bindings + WASI poly)  │ │  │
│  │  └─────────────────┘  └────────────────────────────┘ │  │
│  └──────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────┘
```

### 3.4 Plugin Architecture

The plugin architecture defines lifecycle hooks at each stage of the compilation pipeline:

```
┌────────────────────────────────────────────────────────────────┐
│                    Compiler Pipeline + Plugin Hooks             │
│                                                                │
│  Source ──▶ [Hook: pre_parse] ──▶ Parse ──▶ [Hook: post_parse]│
│                                                                │
│  ──▶ [Hook: pre_lower] ──▶ HIR Lower ──▶ [Hook: post_lower]  │
│                                                                │
│  ──▶ [Hook: pre_typecheck] ──▶ TypeCheck ──▶ [Hook: post_    │
│       typecheck]                                               │
│                                                                │
│  ──▶ [Hook: pre_optimize] ──▶ Egraph ──▶ [Hook: post_        │
│       optimize]                                                │
│                                                                │
│  ──▶ [Hook: pre_codegen] ──▶ Codegen ──▶ [Hook: post_codegen]│
│                                                                │
│  ──▶ [Hook: pre_link] ──▶ Link ──▶ [Hook: post_link]         │
│                                                                │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  Plugin Registry                                        │   │
│  │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌───────────┐ │   │
│  │  │lint-plug │ │fmt-plug  │ │custom-opt│ │debug-plug │ │   │
│  │  └──────────┘ └──────────┘ └──────────┘ └───────────┘ │   │
│  └─────────────────────────────────────────────────────────┘   │
└────────────────────────────────────────────────────────────────┘
```

---

## 4. New Crate: `glyim-backend`

### 4.1 Crate Structure

```
crates/glyim-backend/
├── Cargo.toml
└── src/
    ├── lib.rs              — Backend trait, BackendConfig, BackendError
    ├── traits.rs           — Backend, JitSession trait definitions
    ├── config.rs           — BackendConfig, OptLevel, DebugInfoMode, LinkConfig
    ├── output.rs           — BackendOutput, FunctionOutput, BackendStats
    ├── registry.rs         — BackendRegistry: name → Backend lookup
    ├── selector.rs         — Backend selection logic (Debug→Cranelift, Release→LLVM)
    └── tests/
        ├── mod.rs
        ├── registry_tests.rs
        └── selector_tests.rs
```

### 4.2 Cargo.toml

```toml
[package]
name = "glyim-backend"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
description = "Backend trait and registry for Glyim code generation"

[dependencies]
glyim-hir = { path = "../glyim-hir" }
glyim-interner = { path = "../glyim-interner" }
glyim-diag = { path = "../glyim-diag" }
serde = { version = "1", features = ["derive"] }
```

### 4.3 Backend Registry

```rust
// crates/glyim-backend/src/registry.rs

use crate::traits::Backend;
use std::collections::HashMap;
use std::sync::Arc;

/// Registry of available code generation backends.
pub struct BackendRegistry {
    backends: HashMap<String, Arc<dyn Backend>>,
    default_backend: String,
}

impl BackendRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            backends: HashMap::new(),
            default_backend: "llvm".to_string(),
        };
        // Register built-in backends
        #[cfg(feature = "llvm-backend")]
        registry.register(Arc::new(glyim_codegen_llvm::LlvmBackend::new()));
        #[cfg(feature = "cranelift-backend")]
        registry.register(Arc::new(glyim_codegen_cranelift::CraneliftBackend::new()));
        registry
    }

    pub fn register(&mut self, backend: Arc<dyn Backend>) {
        let name = backend.name().to_string();
        self.backends.insert(name, backend);
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn Backend>> {
        self.backends.get(name)
    }

    pub fn default_backend(&self) -> &Arc<dyn Backend> {
        self.backends.get(&self.default_backend)
            .expect("default backend must be registered")
    }

    /// Select the best backend for the given configuration.
    pub fn select(&self, config: &crate::config::BackendConfig) -> Arc<dyn Backend> {
        // Explicit backend selection via --backend flag
        if let Some(backend_name) = &config.flags.get("backend") {
            if let Some(backend) = self.backends.get(backend_name) {
                return backend.clone();
            }
        }

        // Automatic selection based on target and optimization level
        let target = &config.target_triple;
        let opt_level = config.opt_level;

        // Wasm target requires LLVM (Cranelift doesn't emit .wasm directly)
        if target.starts_with("wasm") {
            return self.backends.get("llvm")
                .expect("LLVM backend required for wasm targets")
                .clone();
        }

        // Debug mode prefers Cranelift for speed; Release prefers LLVM for optimization
        match opt_level {
            OptLevel::None | OptLevel::Less => {
                self.backends.get("cranelift")
                    .or_else(|| self.backends.get("llvm"))
                    .expect("at least one backend must be available")
                    .clone()
            }
            OptLevel::Default | OptLevel::Aggressive => {
                self.backends.get("llvm")
                    .expect("LLVM backend required for optimized builds")
                    .clone()
            }
        }
    }

    pub fn available_backends(&self) -> Vec<&str> {
        self.backends.keys().map(|s| s.as_str()).collect()
    }
}
```

---

## 5. New Crate: `glyim-codegen-cranelift`

### 5.1 Crate Structure

```
crates/glyim-codegen-cranelift/
├── Cargo.toml
└── src/
    ├── lib.rs              — CraneliftBackend implementation
    ├── backend.rs          — Backend trait impl
    ├── codegen.rs          — HIR → Cranelift IR lowering
    ├── types.rs            — HirType → Cranelift type mapping
    ├── jit.rs              — Cranelift JIT session
    ├── abi.rs              — Calling convention handling
    ├── intrinsics.rs       — Intrinsic function lowering
    └── tests/
        ├── mod.rs
        ├── codegen_tests.rs
        ├── types_tests.rs
        └── jit_tests.rs
```

### 5.2 Cargo.toml

```toml
[package]
name = "glyim-codegen-cranelift"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
description = "Cranelift code generation backend for Glyim"

[dependencies]
glyim-backend = { path = "../glyim-backend" }
glyim-hir = { path = "../glyim-hir" }
glyim-interner = { path = "../glyim-interner" }
glyim-diag = { path = "../glyim-diag" }
cranelift = "0.112"
cranelift-module = "0.112"
cranelift-jit = "0.112"
cranelift-native = "0.112"
target-lexicon = "0.12"
tracing = "0.1"

[dev-dependencies]
tempfile = "3"
```

### 5.3 Cranelift Backend Implementation

```rust
// crates/glyim-codegen-cranelift/src/backend.rs

use glyim_backend::{
    Backend, BackendConfig, BackendError, BackendOutput, BackendStats,
    DebugInfoMode, FunctionOutput, JitConfig, JitSession, LinkConfig, LinkOutputFormat, OptLevel,
};
use glyim_hir::{Hir, HirFn, HirType, types::MonoHir};
use glyim_interner::Interner;
use cranelift::prelude::*;
use cranelift_module::{Module, Linkage};
use cranelift_jit::JITBuilder;
use std::collections::HashMap;

/// Cranelift-based code generation backend.
/// Compiles HIR to native code 3-10x faster than LLVM at the cost of
/// less optimized output. Ideal for debug builds and JIT compilation.
pub struct CraneliftBackend {
    /// Cached target ISA for the host.
    host_isa: Option<Arc<dyn TargetIsa>>,
}

impl CraneliftBackend {
    pub fn new() -> Self {
        let host_isa = cranelift_native::builder()
            .ok()
            .map(|b| b.finish().ok())
            .flatten();
        Self { host_isa }
    }
}

impl Backend for CraneliftBackend {
    fn name(&self) -> &str {
        "cranelift"
    }

    fn supported_targets(&self) -> &[&str] {
        // Cranelift supports x86_64, aarch64, s390x, riscv64
        &[
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            "x86_64-apple-darwin",
            "aarch64-apple-darwin",
            "x86_64-pc-windows-msvc",
        ]
    }

    fn compile_module(
        &self,
        hir: &Hir,
        mono_hir: &MonoHir,
        interner: &Interner,
        config: &BackendConfig,
    ) -> Result<BackendOutput, BackendError> {
        let start = std::time::Instant::now();

        let isa = self.create_isa(config)?;
        let mut module = cranelift_module::Module::new(
            cranelift_object::ObjectBuilder::new(
                isa,
                "glyim_module",
                cranelift_module::default_libcall_names(),
            ).map_err(|e| BackendError {
                message: format!("Failed to create Cranelift object builder: {}", e),
                code: None,
                source_span: None,
            })?,
        );

        let mut codegen = CraneliftCodegen::new(&mut module, interner);
        let mut exported_symbols = Vec::new();
        let mut imported_symbols = Vec::new();
        let mut total_code_size = 0;

        // Declare all functions first (for forward references)
        for item in &hir.items {
            if let glyim_hir::HirItem::Fn(f) = item {
                let sig = codegen.function_signature(f, mono_hir);
                let name = interner.resolve(f.name);
                let linkage = if name == "main" {
                    Linkage::Export
                } else {
                    Linkage::Local
                };
                module.declare_function(name, linkage, &sig)
                    .map_err(|e| BackendError {
                        message: format!("Failed to declare function {}: {}", name, e),
                        code: None,
                        source_span: None,
                    })?;
            }
        }

        // Compile each function
        for item in &hir.items {
            if let glyim_hir::HirItem::Fn(f) = item {
                let name = interner.resolve(f.name);
                let code_size = codegen.compile_function_clif(f, mono_hir)?;
                total_code_size += code_size;

                if name == "main" || f.is_pub {
                    exported_symbols.push(name.to_string());
                }
            }
        }

        // Finalize the module and produce object code
        let object = module.finish()
            .emit()
            .map_err(|e| BackendError {
                message: format!("Failed to emit Cranelift object code: {}", e),
                code: None,
                source_span: None,
            })?;

        Ok(BackendOutput {
            object_code: object,
            exported_symbols,
            imported_symbols,
            debug_info: None, // Cranelift does not emit DWARF
            stats: BackendStats {
                compile_time: start.elapsed(),
                functions_compiled: codegen.functions_compiled(),
                code_size: total_code_size,
                peak_memory: None,
            },
        })
    }

    fn compile_function(
        &self,
        function: &HirFn,
        mono_types: &glyim_hir::types::TypeOverrides,
        interner: &Interner,
        config: &BackendConfig,
    ) -> Result<FunctionOutput, BackendError> {
        let isa = self.create_isa(config)?;
        let mut module = cranelift_module::Module::new(
            cranelift_object::ObjectBuilder::new(
                isa,
                "glyim_function",
                cranelift_module::default_libcall_names(),
            ).map_err(|e| BackendError {
                message: format!("Failed to create Cranelift object builder: {}", e),
                code: None,
                source_span: None,
            })?,
        );

        let mut codegen = CraneliftCodegen::new(&mut module, interner);
        let name = interner.resolve(function.name);
        module.declare_function(name, Linkage::Export, &codegen.function_signature(function, mono_types))
            .map_err(|e| BackendError {
                message: format!("Failed to declare function: {}", e),
                code: None,
                source_span: None,
            })?;

        let code_size = codegen.compile_function_clif(function, mono_types)?;

        let object = module.finish()
            .emit()
            .map_err(|e| BackendError {
                message: format!("Failed to emit Cranelift object code: {}", e),
                code: None,
                source_span: None,
            })?;

        Ok(FunctionOutput {
            object_code: object,
            symbol_name: name.to_string(),
            code_size,
        })
    }

    fn create_jit_session(
        &self,
        config: &JitConfig,
    ) -> Result<Box<dyn JitSession>, BackendError> {
        let mut builder = JITBuilder::new(cranelift_module::default_libcall_names());
        // Register host functions that the JIT code might call
        builder.symbol("puts", puts as *const u8);
        builder.symbol("printf", printf as *const u8);
        // ... more runtime shims

        let jit = cranelift_jit::JIT::new(builder)
            .map_err(|e| BackendError {
                message: format!("Failed to create Cranelift JIT: {}", e),
                code: None,
                source_span: None,
            })?;

        Ok(Box::new(CraneliftJitSession::new(jit)))
    }

    fn debug_info_mode(&self) -> DebugInfoMode {
        // Cranelift does not support DWARF debug info generation.
        // Line table support is planned for a future release.
        DebugInfoMode::None
    }

    fn link(
        &self,
        object_files: &[Vec<u8>],
        config: &LinkConfig,
    ) -> Result<Vec<u8>, BackendError> {
        // Cranelift delegates linking to the system linker (cc)
        // This is the same approach as the LLVM backend
        Err(BackendError {
            message: "Cranelift backend delegates linking to the system linker".to_string(),
            code: Some("DELEGATE_LINK".to_string()),
            source_span: None,
        })
    }
}

// External C function stubs for JIT
extern "C" {
    fn puts(s: *const u8) -> i32;
    fn printf(format: *const u8, ...) -> i32;
}
```

### 5.4 HIR → Cranelift IR Lowering

```rust
// crates/glyim-codegen-cranelift/src/codegen.rs

use cranelift::prelude::*;
use cranelift_module::{Module, Linkage};
use glyim_hir::{Hir, HirFn, HirExpr, HirType, HirStmt, types::MonoHir, types::TypeOverrides};
use glyim_interner::Interner;
use std::collections::HashMap;

/// Cranelift IR code generator that lowers HIR to Cranelift IR.
pub struct CraneliftCodegen<'a, M: Module> {
    module: &'a mut M,
    interner: &'a Interner,
    builder: FunctionBuilder<'a>,
    /// Map from variable name to Cranelift variable.
    variables: HashMap<glyim_interner::Symbol, Variable>,
    /// Function counter for unique function IDs.
    function_count: usize,
}

impl<'a, M: Module> CraneliftCodegen<'a, M> {
    pub fn new(module: &'a mut M, interner: &'a Interner) -> Self {
        // The builder is created per-function, not per-module.
        // This is a placeholder; the actual builder is created in compile_function_clif.
        Self {
            module,
            interner,
            builder: /* placeholder */,
            variables: HashMap::new(),
            function_count: 0,
        }
    }

    /// Get the Cranelift function signature for a HIR function.
    pub fn function_signature(
        &mut self,
        function: &HirFn,
        mono_hir: &MonoHir,
    ) -> Signature {
        let mut sig = self.module.target_config().default_call_conv();

        // Parameters
        for param in &function.params {
            let clif_type = self.hir_type_to_clif(&param.ty);
            sig.params.push(AbiParam::new(clif_type));
        }

        // Return type
        if let Some(ret_type) = &function.return_type {
            let clif_type = self.hir_type_to_clif(ret_type);
            sig.returns.push(AbiParam::new(clif_type));
        }

        sig
    }

    /// Compile a single HIR function to Cranelift IR.
    pub fn compile_function_clif(
        &mut self,
        function: &HirFn,
        mono_hir: &MonoHir,
    ) -> Result<usize, BackendError> {
        let name = self.interner.resolve(function.name);
        let sig = self.function_signature(function, mono_hir);

        let func_id = self.module.declare_function(name, Linkage::Local, &sig)
            .map_err(|e| BackendError {
                message: format!("Failed to declare {}: {}", name, e),
                code: None,
                source_span: None,
            })?;

        let mut ctx = self.module.make_context();
        ctx.func.signature = sig;

        let mut func_ctx = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut ctx.func, &mut func_ctx);

            // Create entry block
            let entry_block = builder.create_block();
            builder.switch_to_block(entry_block);
            builder.seal_block(entry_block);

            // Declare and define variables for parameters
            self.variables.clear();
            for (i, param) in function.params.iter().enumerate() {
                let var = Variable::new(i);
                let clif_type = self.hir_type_to_clif(&param.ty);
                builder.declare_var(var, clif_type);
                self.variables.insert(param.name, var);
                let param_value = builder.block_params(entry_block)[i];
                builder.def_var(var, param_value);
            }

            // Lower the function body
            let return_value = self.lower_expr(&function.body, &mut builder)?;

            // Return
            builder.ins().return_(&[return_value]);

            builder.finalize();
        }

        // Compile the Cranelift IR
        self.module.define_function(func_id, &mut ctx)
            .map_err(|e| BackendError {
                message: format!("Failed to compile {}: {}", name, e),
                code: None,
                source_span: None,
            })?;

        let code_size = ctx.func.buffer().map(|b| b.len()).unwrap_or(0);
        self.module.clear_context(&mut ctx);
        self.function_count += 1;

        Ok(code_size)
    }

    /// Lower a HIR expression to a Cranelift value.
    fn lower_expr(
        &mut self,
        expr: &HirExpr,
        builder: &mut FunctionBuilder,
    ) -> Result<Value, BackendError> {
        match expr {
            HirExpr::IntLiteral(n, _) => {
                Ok(builder.ins().iconst(types::I64, *n))
            }
            HirExpr::FloatLiteral(f, _) => {
                Ok(builder.ins().f64const(*f))
            }
            HirExpr::BoolLiteral(b, _) => {
                Ok(builder.ins().iconst(types::I8, if *b { 1 } else { 0 }))
            }
            HirExpr::Binary(op, lhs, rhs, _) => {
                let lhs_val = self.lower_expr(lhs, builder)?;
                let rhs_val = self.lower_expr(rhs, builder)?;
                self.lower_binary_op(*op, lhs_val, rhs_val, builder)
            }
            HirExpr::Call(name, args, _) => {
                let func_ref = self.module.declare_func_in_func(
                    self.get_func_id(*name),
                    builder.func,
                );
                let arg_values: Vec<Value> = args.iter()
                    .map(|arg| self.lower_expr(arg, builder))
                    .collect::<Result<_, _>>()?;
                let inst = builder.ins().call(func_ref, &arg_values);
                Ok(builder.inst_results(inst)[0])
            }
            HirExpr::Var(name, _) => {
                if let Some(var) = self.variables.get(name) {
                    Ok(builder.use_var(*var))
                } else {
                    Err(BackendError {
                        message: format!("Undefined variable: {}", self.interner.resolve(*name)),
                        code: None,
                        source_span: None,
                    })
                }
            }
            // ... other expression variants
            _ => Err(BackendError {
                message: format!("Unsupported expression type: {:?}", expr),
                code: Some("UNIMPLEMENTED".to_string()),
                source_span: None,
            }),
        }
    }

    fn lower_binary_op(
        &mut self,
        op: glyim_hir::BinOp,
        lhs: Value,
        rhs: Value,
        builder: &mut FunctionBuilder,
    ) -> Result<Value, BackendError> {
        match op {
            glyim_hir::BinOp::Add => Ok(builder.ins().iadd(lhs, rhs)),
            glyim_hir::BinOp::Sub => Ok(builder.ins().isub(lhs, rhs)),
            glyim_hir::BinOp::Mul => Ok(builder.ins().imul(lhs, rhs)),
            glyim_hir::BinOp::Div => Ok(builder.ins().sdiv(lhs, rhs)),
            glyim_hir::BinOp::Eq => Ok(builder.ins().icmp(IntCC::Equal, lhs, rhs)),
            glyim_hir::BinOp::Lt => Ok(builder.ins().icmp(IntCC::SignedLessThan, lhs, rhs)),
            // ... other operators
            _ => Err(BackendError {
                message: format!("Unsupported binary operator: {:?}", op),
                code: Some("UNIMPLEMENTED".to_string()),
                source_span: None,
            }),
        }
    }

    fn hir_type_to_clif(&self, ty: &HirType) -> Type {
        match ty {
            HirType::Named(name) => {
                let name_str = self.interner.resolve(*name);
                match name_str {
                    "i32" => types::I32,
                    "i64" => types::I64,
                    "f64" => types::F64,
                    "bool" => types::I8,
                    _ => types::I64, // pointer-sized for structs
                }
            }
            HirType::Generic(name) => types::I64, // monomorphized before codegen
            _ => types::I64,
        }
    }

    fn get_func_id(&self, name: glyim_interner::Symbol) -> cranelift_module::FuncId {
        // Look up the function ID by name
        todo!()
    }

    pub fn functions_compiled(&self) -> usize {
        self.function_count
    }
}
```

---

## 6. WebAssembly Target Implementation

### 6.1 Wasm Linker

The Wasm linker converts LLVM's wasm32 object file into a standalone Wasm module. This is the critical missing piece that prevents `compile_to_wasm()` from producing a usable `.wasm` file:

```rust
// crates/glyim-codegen-llvm/src/wasm_link.rs

use std::path::{Path, PathBuf};
use std::process::Command;

/// Configuration for the Wasm linker.
pub struct WasmLinkConfig {
    /// Path to the wasm-ld linker (from LLVM installation).
    pub wasm_ld_path: Option<PathBuf>,
    /// Whether to generate a WASI reactor module (vs. command module).
    pub reactor: bool,
    /// Whether to strip debug sections.
    pub strip_debug: bool,
    /// Whether to run wasm-opt for optimization.
    pub optimize: bool,
    /// WASI preview version (1 or 2).
    pub wasi_preview: u32,
    /// Additional export names to include.
    pub exports: Vec<String>,
}

/// Link one or more wasm32 object files into a standalone .wasm module.
pub fn link_wasm(
    object_files: &[PathBuf],
    output_path: &Path,
    config: &WasmLinkConfig,
) -> Result<WasmLinkOutput, WasmLinkError> {
    // Step 1: Find or auto-detect wasm-ld
    let wasm_ld = config.wasm_ld_path.clone()
        .or_else(|| find_wasm_ld())
        .ok_or_else(|| WasmLinkError::LinkerNotFound)?;

    // Step 2: Link with wasm-ld
    let mut cmd = Command::new(&wasm_ld);
    cmd.arg("--no-entry")           // WASI modules may not have a traditional entry point
        .arg("--export-dynamic")    // Export all symbols marked as exported
        .arg("--allow-undefined")   // WASI imports are resolved at runtime
        .arg(format!("--wasi-preview={}", config.wasi_preview));

    if config.reactor {
        cmd.arg("--wasi-reactor");  // Generate reactor module (no _start, just initialize)
    }

    for obj in object_files {
        cmd.arg(obj);
    }

    cmd.arg("-o").arg(output_path);

    let output = cmd.output()
        .map_err(|e| WasmLinkError::LinkerExecutionFailed(e.to_string()))?;

    if !output.status.success() {
        return Err(WasmLinkError::LinkerFailed(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    // Step 3: Optionally optimize with wasm-opt
    if config.optimize {
        if let Some(wasm_opt) = find_wasm_opt() {
            let optimized = output_path.with_extension("wasm.opt");
            let opt_output = Command::new(&wasm_opt)
                .arg("-O3")
                .arg("--enable-bulk-memory")
                .arg("--enable-sign-ext")
                .arg(output_path)
                .arg("-o")
                .arg(&optimized)
                .output()
                .map_err(|e| WasmLinkError::OptimizerFailed(e.to_string()))?;

            if opt_output.status.success() {
                std::fs::rename(&optimized, output_path)?;
            }
        }
    }

    // Step 4: Optionally strip debug sections
    if config.strip_debug {
        if let Some(wasm_tools) = find_wasm_tools() {
            let stripped = output_path.with_extension("wasm.strip");
            let strip_output = Command::new(&wasm_tools)
                .arg("strip")
                .arg("--debug-only")
                .arg(output_path)
                .arg("-o")
                .arg(&stripped)
                .output()
                .map_err(|e| WasmLinkError::StripFailed(e.to_string()))?;

            if strip_output.status.success() {
                std::fs::rename(&stripped, output_path)?;
            }
        }
    }

    // Read the final wasm bytes to report size
    let wasm_bytes = std::fs::read(output_path)?;
    Ok(WasmLinkOutput {
        wasm_path: output_path.to_path_buf(),
        wasm_size: wasm_bytes.len(),
        exports: extract_exports(&wasm_bytes),
    })
}

/// Find wasm-ld on the system PATH or in known LLVM installation locations.
fn find_wasm_ld() -> Option<PathBuf> {
    // Check PATH
    if let Ok(output) = Command::new("which").arg("wasm-ld").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return Some(PathBuf::from(path));
        }
    }
    // Check common LLVM installation paths
    for prefix in &["/usr/bin", "/usr/local/bin", "/opt/homebrew/bin"] {
        let candidate = PathBuf::from(prefix).join("wasm-ld");
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn find_wasm_opt() -> Option<PathBuf> { /* similar to find_wasm_ld */ }
fn find_wasm_tools() -> Option<PathBuf> { /* similar to find_wasm_ld */ }

fn extract_exports(wasm_bytes: &[u8]) -> Vec<String> {
    // Parse the export section of the wasm module to list exported names
    // Uses wasmparser for robust parsing
    let mut exports = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(wasm_bytes) {
        if let Ok(wasmparser::Payload::ExportSection(reader)) = payload {
            for export in reader {
                if let Ok(export) = export {
                    exports.push(export.name.to_string());
                }
            }
        }
    }
    exports
}

#[derive(Debug)]
pub struct WasmLinkOutput {
    pub wasm_path: PathBuf,
    pub wasm_size: usize,
    pub exports: Vec<String>,
}

#[derive(Debug)]
pub enum WasmLinkError {
    LinkerNotFound,
    LinkerExecutionFailed(String),
    LinkerFailed(String),
    OptimizerFailed(String),
    StripFailed(String),
    IoError(std::io::Error),
}
```

### 6.2 Wasm Runtime Adapter

The Wasm runtime adapter enables `glyim run --target wasm32-wasi` using the existing Wasmtime 44 integration:

```rust
// crates/glyim-compiler/src/wasm_runtime.rs

use glyim_macro_core::executor::MacroExecutor;
use std::path::Path;

/// Execute a compiled .wasm module using Wasmtime.
pub fn run_wasm(
    wasm_path: &Path,
    function_name: Option<&str>,
    args: &[String],
) -> Result<WasmExecutionResult, WasmExecutionError> {
    let wasm_bytes = std::fs::read(wasm_path)
        .map_err(|e| WasmExecutionError::FileNotFound(e.to_string()))?;

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm_bytes)
        .map_err(|e| WasmExecutionError::ModuleLoadFailed(e.to_string()))?;

    // Set up WASI
    let mut linker = wasmtime::Linker::new(&engine);
    wasmtime_wasi::add_to_linker_sync(&mut linker, |s| s)
        .map_err(|e| WasmExecutionError::WasiSetupFailed(e.to_string()))?;

    let wasi_ctx = wasmtime_wasi::WasiCtxBuilder::new()
        .args(args.iter().map(|s| s.as_str()))
        .inherit_stdio()
        .build();

    let mut store = wasmtime::Store::new(&engine, wasi_ctx);

    // Instantiate the module
    let instance = linker.instantiate(&mut store, &module)
        .map_err(|e| WasmExecutionError::InstantiationFailed(e.to_string()))?;

    // Determine the entry point
    let entry_point = function_name.unwrap_or("_start");

    let entry_fn = instance.get_func(&mut store, entry_point)
        .ok_or_else(|| WasmExecutionError::EntryPointNotFound(entry_point.to_string()))?;

    // Call the entry point
    let exit_code = match entry_fn.typed::<(), i32>(&store) {
        Ok(typed_fn) => typed_fn.call(&mut store, ())
            .map_err(|e| WasmExecutionError::ExecutionFailed(e.to_string())),
        Err(_) => {
            // Try calling with no return value
            entry_fn.call(&mut store, &[], &mut [])
                .map(|_| 0i32)
                .map_err(|e| WasmExecutionError::ExecutionFailed(e.to_string()))
        }
    }?;

    Ok(WasmExecutionResult {
        exit_code,
        wasm_path: wasm_path.to_path_buf(),
    })
}

#[derive(Debug)]
pub struct WasmExecutionResult {
    pub exit_code: i32,
    pub wasm_path: std::path::PathBuf,
}

#[derive(Debug)]
pub enum WasmExecutionError {
    FileNotFound(String),
    ModuleLoadFailed(String),
    WasiSetupFailed(String),
    InstantiationFailed(String),
    EntryPointNotFound(String),
    ExecutionFailed(String),
}
```

### 6.3 Browser Runtime

The browser runtime provides JavaScript bindings that enable Glyim-compiled Wasm modules to run in web applications:

```javascript
// crates/glyim-browser-runtime/glyim-runtime.js

/**
 * Glyim Browser Runtime
 * Loads and executes Glyim-compiled WASM modules in the browser.
 */
class GlyimRuntime {
    constructor() {
        this.module = null;
        this.instance = null;
        this.memory = null;
    }

    /**
     * Initialize the runtime with a WASM module.
     * @param {string|Response} wasmSource - URL or fetch Response for the .wasm file
     * @param {object} imports - Additional WASM imports
     */
    async init(wasmSource, imports = {}) {
        const wasmBytes = wasmSource instanceof Response
            ? await wasmSource.arrayBuffer()
            : await fetch(wasmSource).then(r => r.arrayBuffer());

        // WASI polyfill for browser environment
        const wasiImports = this._createWasiPolyfill();

        const allImports = { ...wasiImports, ...imports };

        const { instance, module } = await WebAssembly.instantiate(wasmBytes, allImports);
        this.instance = instance;
        this.module = module;
        this.memory = instance.exports.memory;

        // Call _initialize for reactor modules
        if (instance.exports._initialize) {
            instance.exports._initialize();
        }

        return this;
    }

    /**
     * Call an exported function by name.
     * @param {string} name - The exported function name
     * @param {...number} args - Arguments (i32, i64, f64 values)
     * @returns {number} The return value
     */
    call(name, ...args) {
        if (!this.instance) {
            throw new Error("Runtime not initialized. Call init() first.");
        }
        const func = this.instance.exports[name];
        if (!func) {
            throw new Error(`Exported function '${name}' not found`);
        }
        return func(...args);
    }

    /**
     * Read a string from WASM memory.
     * @param {number} ptr - Pointer to the string in WASM memory
     * @param {number} len - Length of the string
     * @returns {string}
     */
    readString(ptr, len) {
        const bytes = new Uint8Array(this.memory.buffer, ptr, len);
        return new TextDecoder().decode(bytes);
    }

    /**
     * Write a string to WASM memory.
     * @param {number} ptr - Pointer to write to
     * @param {string} str - The string to write
     */
    writeString(ptr, str) {
        const bytes = new TextEncoder().encode(str);
        const memory = new Uint8Array(this.memory.buffer);
        memory.set(bytes, ptr);
    }

    /**
     * Create WASI polyfill imports for browser environment.
     * Provides stubs for WASI syscalls that have no browser equivalent.
     */
    _createWasiPolyfill() {
        return {
            wasi_snapshot_preview1: {
                fd_write: (fd, iovs, iovs_len, nwritten) => {
                    // Write to console for fd 1 (stdout) and 2 (stderr)
                    if (fd === 1 || fd === 2) {
                        const memory = new Uint8Array(this.memory.buffer);
                        let output = '';
                        let offset = iovs;
                        for (let i = 0; i < iovs_len; i++) {
                            const bufPtr = new DataView(memory.buffer).getUint32(offset, true);
                            const bufLen = new DataView(memory.buffer).getUint32(offset + 4, true);
                            output += new TextDecoder().decode(memory.slice(bufPtr, bufPtr + bufLen));
                            offset += 8;
                        }
                        if (fd === 1) console.log(output); else console.error(output);
                        new DataView(memory.buffer).setUint32(nwritten, new TextEncoder().encode(output).length, true);
                    }
                    return 0;
                },
                random_get: (buf_ptr, buf_len) => {
                    const memory = new Uint8Array(this.memory.buffer);
                    crypto.getRandomValues(memory.slice(buf_ptr, buf_ptr + buf_len));
                    return 0;
                },
                clock_time_get: (clock_id, precision, time_ptr) => {
                    const now = BigInt(Date.now()) * 1_000_000n;
                    new DataView(this.memory.buffer).setBigUint64(time_ptr, now, true);
                    return 0;
                },
                proc_exit: (code) => { throw new Error(`Process exited with code ${code}`); },
                // ... additional WASI stubs
            }
        };
    }
}

// Export for both browser and Node.js
if (typeof module !== 'undefined') {
    module.exports = { GlyimRuntime };
}
```

---

## 7. Plugin Architecture

### 7.1 New Crate: `glyim-plugin`

```
crates/glyim-plugin/
├── Cargo.toml
└── src/
    ├── lib.rs              — Plugin trait, lifecycle hooks
    ├── registry.rs         — PluginRegistry: discovery, loading, lifecycle
    ├── loader.rs           — Dynamic plugin loading via libloading
    ├── linter.rs           — LinterPlugin trait
    ├── optimizer.rs        — OptimizerPlugin trait
    ├── codegen_hook.rs     — CodegenHook trait
    ├── context.rs          — PluginContext: access to HIR, types, symbols
    ├── diagnostic.rs       — PluginDiagnostic: custom diagnostics from plugins
    └── tests/
        ├── mod.rs
        ├── registry_tests.rs
        ├── loader_tests.rs
        └── linter_tests.rs
```

### 7.2 CompilerPlugin Trait

```rust
// crates/glyim-plugin/src/lib.rs

use glyim_hir::Hir;
use glyim_interner::Interner;
use glyim_diag::{Diagnostic, Span, FileId};

/// A compiler plugin that can participate in the compilation pipeline.
pub trait CompilerPlugin: Send + Sync {
    /// The name of this plugin.
    fn name(&self) -> &str;

    /// The version of this plugin (semver).
    fn version(&self) -> &str;

    /// Called once when the plugin is loaded.
    fn on_load(&mut self, context: &mut PluginContext) -> Result<(), PluginError>;

    /// Called once when the plugin is unloaded.
    fn on_unload(&mut self) -> Result<(), PluginError>;

    /// Hook: called after parsing, before HIR lowering.
    fn post_parse(&mut self, _context: &mut PluginContext, _parse_result: &PostParseData) -> PluginResult {
        PluginResult::Continue
    }

    /// Hook: called after type checking.
    fn post_typecheck(&mut self, _context: &mut PluginContext, _typecheck_result: &PostTypecheckData) -> PluginResult {
        PluginResult::Continue
    }

    /// Hook: called after e-graph optimization.
    fn post_optimize(&mut self, _context: &mut PluginContext, _optimized_hir: &Hir) -> PluginResult {
        PluginResult::Continue
    }

    /// Hook: called before code generation.
    fn pre_codegen(&mut self, _context: &mut PluginContext, _hir: &Hir) -> PluginResult {
        PluginResult::Continue
    }

    /// Hook: called after code generation.
    fn post_codegen(&mut self, _context: &mut PluginContext, _output: &PostCodegenData) -> PluginResult {
        PluginResult::Continue
    }
}

/// Result from a plugin hook invocation.
#[derive(Debug, Clone)]
pub enum PluginResult {
    /// Continue with the next plugin and the pipeline.
    Continue,
    /// Stop the pipeline with an error.
    Stop(PluginError),
    /// The plugin has modified the HIR or diagnostics; re-run affected pipeline stages.
    Retry,
}

/// Context provided to plugins, giving them access to compiler state.
pub struct PluginContext {
    /// The current HIR.
    pub hir: Option<Hir>,
    /// The interner for symbol resolution.
    pub interner: Option<Interner>,
    /// Diagnostics accumulated so far.
    pub diagnostics: Vec<Diagnostic>,
    /// The source file being compiled.
    pub file_id: FileId,
    /// Plugin-specific shared state.
    pub shared_state: std::collections::HashMap<String, Box<dyn std::any::Any + Send + Sync>>,
}

/// Data provided to the post-parse hook.
pub struct PostParseData {
    /// The parsed AST.
    pub ast: glyim_parse::Ast,
    /// Parse errors (if any).
    pub errors: Vec<glyim_parse::ParseError>,
}

/// Data provided to the post-typecheck hook.
pub struct PostTypecheckData {
    /// Expression types.
    pub expr_types: Vec<glyim_hir::types::HirType>,
    /// Type errors (if any).
    pub errors: Vec<glyim_typeck::TypeError>,
}

/// Data provided to the post-codegen hook.
pub struct PostCodegenData {
    /// Size of the generated object code.
    pub object_code_size: usize,
    /// Number of functions compiled.
    pub functions_compiled: usize,
    /// Exported symbol names.
    pub exported_symbols: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PluginError {
    pub plugin_name: String,
    pub message: String,
    pub code: Option<String>,
    pub source_span: Option<Span>,
}
```

### 7.3 LinterPlugin Trait

```rust
// crates/glyim-plugin/src/linter.rs

use crate::{PluginContext, PluginResult};
use glyim_hir::{Hir, HirItem, HirFn};
use glyim_diag::{Diagnostic, DiagnosticSeverity, Span};

/// A linter plugin that checks HIR for style issues, potential bugs,
/// and other concerns.
pub trait LinterPlugin: Send + Sync {
    /// The name of this linter.
    fn name(&self) -> &str;

    /// Lint a single function.
    fn lint_function(
        &self,
        function: &HirFn,
        context: &PluginContext,
    ) -> Vec<Diagnostic>;

    /// Lint an entire HIR module.
    fn lint_module(
        &self,
        hir: &Hir,
        context: &PluginContext,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for item in &hir.items {
            if let HirItem::Fn(f) = item {
                diagnostics.extend(self.lint_function(f, context));
            }
        }
        diagnostics
    }

    /// The default severity for diagnostics from this linter.
    fn default_severity(&self) -> DiagnosticSeverity {
        DiagnosticSeverity::Warning
    }
}
```

### 7.4 Plugin Registry and Dynamic Loading

```rust
// crates/glyim-plugin/src/registry.rs

use crate::{CompilerPlugin, PluginError};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Registry of loaded compiler plugins.
pub struct PluginRegistry {
    plugins: Vec<Box<dyn CompilerPlugin>>,
    linters: Vec<Box<dyn crate::linter::LinterPlugin>>,
    plugin_paths: Vec<PathBuf>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
            linters: Vec::new(),
            plugin_paths: Vec::new(),
        }
    }

    /// Discover and load plugins from the given directory.
    /// On native targets, uses libloading for dynamic loading.
    /// On wasm32, plugins must be statically linked.
    pub fn load_from_directory(&mut self, dir: &Path) -> Result<usize, PluginError> {
        let mut loaded = 0;
        if dir.exists() {
            for entry in std::fs::read_dir(dir)
                .map_err(|e| PluginError {
                    plugin_name: "registry".to_string(),
                    message: format!("Failed to read plugin directory: {}", e),
                    code: None,
                    source_span: None,
                })? {
                let entry = entry.map_err(|e| PluginError {
                    plugin_name: "registry".to_string(),
                    message: format!("Failed to read directory entry: {}", e),
                    code: None,
                    source_span: None,
                })?;

                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "so" || ext == "dylib" || ext == "dll") {
                    match self.load_dynamic_plugin(&path) {
                        Ok(()) => loaded += 1,
                        Err(e) => {
                            tracing::warn!(
                                path = %path.display(),
                                error = %e.message,
                                "Failed to load plugin"
                            );
                        }
                    }
                }
            }
        }
        Ok(loaded)
    }

    /// Load a dynamic plugin from a shared library.
    #[cfg(not(target_family = "wasm"))]
    fn load_dynamic_plugin(&mut self, path: &Path) -> Result<(), PluginError> {
        use libloading::{Library, Symbol};

        unsafe {
            let library = Library::new(path)
                .map_err(|e| PluginError {
                    plugin_name: path.display().to_string(),
                    message: format!("Failed to load library: {}", e),
                    code: None,
                    source_span: None,
                })?;

            // Look for the required entry point: glyim_plugin_create
            let create_fn: Symbol<unsafe fn() -> *mut dyn CompilerPlugin> =
                library.get(b"glyim_plugin_create")
                    .map_err(|e| PluginError {
                        plugin_name: path.display().to_string(),
                        message: format!("Missing glyim_plugin_create symbol: {}", e),
                        code: Some("MISSING_ENTRY_POINT".to_string()),
                        source_span: None,
                    })?;

            let plugin = Box::from_raw(create_fn());
            tracing::info!(name = %plugin.name(), version = %plugin.version(), "Loaded plugin");
            self.plugins.push(plugin);
        }

        Ok(())
    }

    #[cfg(target_family = "wasm")]
    fn load_dynamic_plugin(&mut self, _path: &Path) -> Result<(), PluginError> {
        Err(PluginError {
            plugin_name: "registry".to_string(),
            message: "Dynamic plugin loading is not supported on wasm32".to_string(),
            code: Some("UNSUPPORTED_PLATFORM".to_string()),
            source_span: None,
        })
    }

    /// Register a statically-linked plugin.
    pub fn register(&mut self, plugin: Box<dyn CompilerPlugin>) {
        tracing::info!(name = %plugin.name(), version = %plugin.version(), "Registered plugin");
        self.plugins.push(plugin);
    }

    /// Register a linter plugin.
    pub fn register_linter(&mut self, linter: Box<dyn crate::linter::LinterPlugin>) {
        tracing::info!(name = %linter.name(), "Registered linter plugin");
        self.linters.push(linter);
    }

    /// Invoke a hook on all registered plugins.
    pub fn invoke<F>(&mut self, hook: F) -> Result<(), PluginError>
    where
        F: Fn(&mut Box<dyn CompilerPlugin>) -> crate::PluginResult,
    {
        for plugin in &mut self.plugins {
            let result = hook(plugin);
            match result {
                crate::PluginResult::Continue => {}
                crate::PluginResult::Stop(e) => return Err(e),
                crate::PluginResult::Retry => {
                    // Re-run the hook on all plugins (to handle cross-plugin interactions)
                    // Limited to 3 retries to prevent infinite loops
                }
            }
        }
        Ok(())
    }

    /// Run all linter plugins and collect diagnostics.
    pub fn run_linters(
        &self,
        hir: &Hir,
        context: &crate::PluginContext,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for linter in &self.linters {
            let mut lints = linter.lint_module(hir, context);
            for lint in &mut lints {
                if lint.severity == DiagnosticSeverity::Error {
                    // Linter warnings can be promoted to errors via configuration
                }
            }
            diagnostics.extend(lints);
        }
        diagnostics
    }

    pub fn plugins(&self) -> &[Box<dyn CompilerPlugin>] {
        &self.plugins
    }

    pub fn linters(&self) -> &[Box<dyn crate::linter::LinterPlugin>] {
        &self.linters
    }
}
```

---

## 8. CLI Integration

### 8.1 Modified Commands

| Command | Changes | New Flags |
|---------|---------|-----------|
| `glyim build` | Supports Wasm target, backend selection, plugin loading | `--target wasm32-wasi`, `--backend <llvm\|cranelift>`, `--plugin-dir <path>` |
| `glyim run` | Can execute Wasm modules via Wasmtime | `--target wasm32-wasi` |
| `glyim check` | Uses Cranelift backend by default for speed | `--backend <llvm\|cranelift>` |
| `glyim test` | Uses Cranelift backend for faster test compilation | `--backend <llvm\|cranelift>` |

### 8.2 New Commands

| Command | Description |
|---------|-------------|
| `glyim build --target wasm32-wasi` | Compile to standalone `.wasm` module |
| `glyim build --target wasm32-wasi --wasi-preview 2` | Compile to WASI preview 2 component |
| `glyim run --target wasm32-wasi` | Execute `.wasm` module via Wasmtime |

### 8.3 Supported Targets (Updated)

| Target | Backend | Output | Status |
|--------|---------|--------|--------|
| `x86_64-unknown-linux-gnu` | LLVM, Cranelift | ELF binary | Existing |
| `aarch64-unknown-linux-gnu` | LLVM, Cranelift | ELF binary | Existing |
| `x86_64-apple-darwin` | LLVM, Cranelift | Mach-O binary | Existing |
| `aarch64-apple-darwin` | LLVM, Cranelift | Mach-O binary | Existing |
| `x86_64-pc-windows-msvc` | LLVM | PE binary | Existing |
| **`wasm32-wasi`** | **LLVM** | **Wasm module** | **NEW** |

---

## 9. Testing Strategy

### 9.1 Unit Tests

| Test | Location | Description |
|------|----------|-------------|
| `backend_trait_compile_module` | `glyim-backend/tests/` | Backend trait compiles a module through the trait interface |
| `cranelift_compile_simple_fn` | `glyim-codegen-cranelift/tests/` | Cranelift compiles a simple arithmetic function |
| `cranelift_compile_call` | `glyim-codegen-cranelift/tests/` | Cranelift compiles a function call |
| `cranelift_compile_match` | `glyim-codegen-cranelift/tests/` | Cranelift compiles a match expression |
| `cranelift_jit_execute` | `glyim-codegen-cranelift/tests/` | Cranelift JIT compiles and executes a function |
| `wasm_link_standalone` | `glyim-codegen-llvm/tests/` | Wasm linker produces a valid standalone `.wasm` module |
| `wasm_link_exports` | `glyim-codegen-llvm/tests/` | Wasm linker exports the correct symbols |
| `wasm_run_hello` | `glyim-compiler/tests/` | `run_wasm()` executes a simple Wasm program |
| `wasm_run_with_args` | `glyim-compiler/tests/` | `run_wasm()` passes CLI args to Wasm program |
| `backend_selector_debug_cranelift` | `glyim-backend/tests/` | Debug mode selects Cranelift by default |
| `backend_selector_release_llvm` | `glyim-backend/tests/` | Release mode selects LLVM by default |
| `backend_selector_wasm_llvm` | `glyim-backend/tests/` | Wasm target always selects LLVM |
| `plugin_registry_load` | `glyim-plugin/tests/` | Plugin registry discovers and loads plugins |
| `plugin_linter_custom` | `glyim-plugin/tests/` | Custom linter plugin produces diagnostics |
| `plugin_hook_post_typecheck` | `glyim-plugin/tests/` | Post-typecheck hook receives type information |

### 9.2 Integration Tests

| Test | Location | Description |
|------|----------|-------------|
| `cranelift_vs_llvm_output` | `glyim-compiler/tests/` | Cranelift and LLVM produce functionally equivalent output |
| `cranelift_compile_speedup` | `glyim-bench/benches/` | Cranelift compiles at least 3x faster than LLVM for debug builds |
| `wasm_full_pipeline` | `glyim-compiler/tests/` | Source → Wasm compilation → execution with Wasmtime |
| `wasm_browser_runtime` | `glyim-browser-runtime/tests/` | Wasm module runs correctly with WASI polyfill |
| `backend_switch_incremental` | `glyim-compiler/tests/` | Incremental compilation works with both backends |
| `plugin_error_isolation` | `glyim-plugin/tests/` | Plugin errors don't crash the compiler |
| `lint_plugin_integration` | `glyim-lint/tests/` | Refactored lint crate works as a plugin |

### 9.3 End-to-End Tests

| Test | Description |
|------|-------------|
| `wasm_hello_world` | Compile a "hello world" program to Wasm, run it with `glyim run --target wasm32-wasi` |
| `wasm_cross_module` | Compile a multi-package workspace to Wasm; verify cross-module calls work |
| `cranelift_debug_build` | Build a 100-function project with Cranelift in debug mode; verify 3x speedup vs LLVM |
| `llvm_release_build` | Build the same project with LLVM in release mode; verify correct optimization |
| `plugin_custom_lint` | Load a custom linter plugin; verify it detects the expected issue |
| `plugin_formatter` | Load a custom formatter plugin; verify it formats code correctly |
| `wasm_browser_integration` | Compile to Wasm, load in browser via GlyimRuntime, call exported functions |

---

## 10. Implementation Timeline

### Week 1–2: Backend Trait and Cranelift Backend

| Day | Task |
|-----|------|
| 1–2 | Define `Backend` trait, `BackendConfig`, `BackendOutput` in `glyim-backend` |
| 3–4 | Create `glyim-codegen-cranelift` crate with basic HIR → Cranelift IR lowering |
| 5–7 | Implement Cranelift expression lowering (int/float/bool literals, binary ops, calls, variables) |
| 8–9 | Implement Cranelift statement lowering (assignments, if/else, match, loops) |
| 10 | Implement Cranelift JIT session |

### Week 3–4: LLVM Backend Refactoring and Wasm Target

| Day | Task |
|-----|------|
| 11–12 | Refactor `Codegen<'ctx>` behind `LlvmBackend` implementing the `Backend` trait |
| 13–14 | Implement `BackendRegistry` with automatic backend selection |
| 15–16 | Implement Wasm linker (`wasm-ld` integration, `wasm-opt`, `wasm-tools strip`) |
| 17–18 | Add `wasm32-wasi` to `SUPPORTED_TARGETS`; implement Wasm-specific codegen paths |
| 19–20 | Implement Wasm runtime adapter (`run_wasm()` using Wasmtime) |

### Week 5–6: Browser Runtime and WASI

| Day | Task |
|-----|------|
| 21–23 | Create `glyim-browser-runtime` package with JavaScript bindings and WASI polyfill |
| 24–25 | Implement WASI preview 2 component model support |
| 26–27 | Add `--target wasm32-wasi` and `--backend` CLI flags |
| 28 | Write integration tests for Wasm compilation and execution |

### Week 6–8: Plugin Architecture

| Day | Task |
|-----|------|
| 29–31 | Create `glyim-plugin` crate with `CompilerPlugin` trait and lifecycle hooks |
| 32–33 | Implement `LinterPlugin` trait and plugin context |
| 34–35 | Implement dynamic plugin loading via `libloading` |
| 36–37 | Refactor `glyim-lint` into a plugin |
| 38–39 | Refactor `glyim-fmt` into a plugin |
| 40 | Add `--plugin-dir` CLI flag and plugin lifecycle management |

### Week 8–9: Testing and Polish

| Day | Task |
|-----|------|
| 41–43 | Write comprehensive integration tests for all new features |
| 44–45 | Add Cranelift and Wasm benchmarks to `glyim-bench` |
| 46–47 | Performance testing: Cranelift vs LLVM compile times, Wasm execution benchmarks |
| 48 | Documentation: backend trait guide, plugin authoring guide, Wasm deployment guide |
| 49–50 | Final integration testing and edge case handling |

---

## 11. Crate Dependency Changes

### 11.1 New Crates

| Crate | Description | Dependencies |
|-------|-------------|--------------|
| `glyim-backend` | Backend trait, config, registry, selector | `glyim-hir`, `glyim-interner`, `glyim-diag`, `serde` |
| `glyim-codegen-cranelift` | Cranelift code generation backend | `glyim-backend`, `glyim-hir`, `glyim-interner`, `cranelift`, `cranelift-module`, `cranelift-jit`, `cranelift-native` |
| `glyim-plugin` | Plugin trait, registry, dynamic loading | `glyim-hir`, `glyim-interner`, `glyim-diag`, `glyim-parse`, `glyim-typeck`, `libloading` |
| `glyim-browser-runtime` | JavaScript bindings and WASI polyfill | (JavaScript package, not a Rust crate) |

### 11.2 Modified Crates

| Crate | Changes |
|-------|---------|
| `glyim-codegen-llvm` | Add `LlvmBackend` implementing `Backend` trait; add Wasm linker module (`wasm_link.rs`); add `wasmparser` dependency for export extraction |
| `glyim-compiler` | Use `Backend` trait instead of `Codegen<'ctx>` directly; add Wasm runtime adapter; add `--backend` and `--target wasm32-wasi` support |
| `glyim-cli` | Add `--backend`, `--target wasm32-wasi`, `--plugin-dir` flags; add Wasm execution support |
| `glyim-lint` | Refactor to implement `LinterPlugin` trait |
| `glyim-fmt` | Refactor to implement `CompilerPlugin` trait |
| `glyim-bench` | Add Cranelift and Wasm benchmark suites |
| `Cargo.toml` (workspace) | Add `glyim-backend`, `glyim-codegen-cranelift`, `glyim-plugin` to workspace members |

---

## 12. Risk Register

| Risk | Probability | Impact | Mitigation |
|------|------------|--------|------------|
| Cranelift backend incomplete | High | Medium | Cranelift does not support all LLVM features (debug info, some intrinsics). Start with core feature support (arithmetic, calls, control flow); fall back to LLVM for unsupported features. |
| Wasm linker portability | Medium | High | `wasm-ld` must be installed on the user's machine. Bundle `wasm-ld` with the Glyim distribution or provide an alternative pure-Rust linker using `wasm-merge`. |
| Wasm runtime compatibility | Medium | Medium | WASI preview 1 vs. preview 2 API differences. Support both; auto-detect from the module. Test with Wasmtime, Wasmer, and WasmEdge. |
| Plugin ABI stability | Medium | High | Dynamic plugins may break when the compiler is updated. Define a stable ABI version; require plugins to declare their minimum compiler version. |
| Plugin security | Medium | Critical | Dynamic plugins run in the compiler process with full access. Sandbox plugins (separate process) for untrusted plugins; warn users about third-party plugins. |
| Backend trait too abstract | Low | Medium | The `Backend` trait may not capture all LLVM-specific features. Add backend-specific extensions via the `flags` HashMap in `BackendConfig`. |
| Browser runtime limitations | High | Low | WASI polyfill cannot provide all POSIX features (file system, network). Document limitations; provide configuration for polyfill behavior. |
| Cranelift code quality | Medium | Low | Cranelift generates less optimized code than LLVM. This is expected and acceptable for debug builds. Users who need optimized code should use `--backend llvm`. |
| Wasm module size | Medium | Low | LLVM-generated Wasm modules may be large. `wasm-opt -O3` and `wasm-tools strip` reduce size. Consider `-Os` optimization level for size-constrained deployments. |

---

## 13. Performance Targets

| Metric | Target | Measurement |
|--------|--------|-------------|
| Cranelift compilation speed (debug, 100 functions) | 3–10x faster than LLVM | `glyim-bench bench_cranelift_vs_llvm` |
| Cranelift output quality (runtime) | Within 2x of LLVM performance | Benchmark suite on compiled binaries |
| Wasm compilation time (vs native) | Within 1.5x of native LLVM compilation | `glyim-bench bench_wasm_compile` |
| Wasm execution overhead (vs native) | Within 2x of native execution speed | Runtime benchmark suite |
| Wasm module size (stripped) | < 100KB for typical programs | `glyim-bench bench_wasm_size` |
| Backend trait overhead | < 5% vs direct Codegen usage | `glyim-bench bench_backend_trait` |
| Plugin hook overhead | < 1ms per hook invocation | `glyim-bench bench_plugin_hooks` |
| Dynamic plugin load time | < 100ms per plugin | `glyim-bench bench_plugin_loading` |
| JIT Cranelift compile + execute | < 10ms for single function | `glyim-bench bench_jit_cranelift` |
| JIT LLVM compile + execute | < 100ms for single function (existing) | `glyim-bench bench_jit_llvm` |

---

## 14. Migration Strategy

### 14.1 Backend Migration

The migration from monolithic `Codegen<'ctx>` to the `Backend` trait is gradual:

1. **Phase 10, Week 3**: Introduce `glyim-backend` crate with `Backend` trait. Add `LlvmBackend` wrapper that delegates to `Codegen<'ctx>` internally.
2. **Phase 10, Week 3**: Update `glyim-compiler` to use `Backend` trait for code generation, but still default to `LlvmBackend`.
3. **Phase 10, Week 4**: Add `CraneliftBackend`. Wire `--backend` flag.
4. **v1.1.0**: Both backends available; LLVM is default for all modes.
5. **v1.2.0**: Cranelift becomes default for `Debug` mode.

### 14.2 Wasm Target Migration

1. **Phase 10, Week 3**: Add `wasm32-wasi` to `SUPPORTED_TARGETS`.
2. **Phase 10, Week 4**: Implement Wasm linker; `compile_to_wasm()` now produces a proper `.wasm` module.
3. **v1.1.0**: `glyim build --target wasm32-wasi` works end-to-end.
4. **v1.2.0**: Browser runtime published as npm package.

### 14.3 Plugin Migration

1. **Phase 10, Week 7**: Introduce `glyim-plugin` crate. `glyim-lint` gains a `LinterPlugin` implementation alongside its existing API.
2. **Phase 10, Week 7**: `glyim-fmt` gains a `CompilerPlugin` implementation.
3. **v1.1.0**: Plugin system available but off by default.
4. **v1.2.0**: `glyim-lint` fully migrated to plugin architecture; old API deprecated.

---

## 15. Success Criteria

### 15.1 Functionality Criteria

- [ ] `glyim build --target wasm32-wasi` produces a standalone `.wasm` module that runs in Wasmtime
- [ ] `glyim run --target wasm32-wasi` executes the compiled Wasm module
- [ ] Browser runtime loads and executes Glyim-compiled Wasm modules in Chrome and Firefox
- [ ] `CraneliftBackend` compiles all core language features (arithmetic, functions, structs, enums, generics)
- [ ] `--backend cranelift` compiles a project 3x faster than `--backend llvm` for debug builds
- [ ] `Backend` trait allows swapping code generators without modifying the pipeline
- [ ] `PluginRegistry` discovers and loads plugins from a directory
- [ ] `LinterPlugin` trait allows custom lints with access to HIR and type information
- [ ] `glyim-lint` is refactored into a plugin

### 15.2 Performance Criteria

- [ ] Cranelift debug compilation is at least 3x faster than LLVM for a 100-function project
- [ ] Cranelift-compiled code runs within 2x of LLVM-compiled code performance
- [ ] Wasm module size (stripped) is under 100KB for a typical program
- [ ] Wasm execution overhead is within 2x of native execution
- [ ] Backend trait adds less than 5% overhead vs direct Codegen usage
- [ ] Plugin hooks add less than 1ms per invocation

### 15.3 Compatibility Criteria

- [ ] Existing projects compile without changes when the default backend is LLVM
- [ ] `compile_to_wasm()` for macros still works (backward compatible)
- [ ] Existing `glyim-lint` API still works alongside the new plugin system
- [ ] Incremental compilation works with both LLVM and Cranelift backends
- [ ] Cross-compilation (aarch64, etc.) works with the refactored backend system

### 15.4 Security Criteria

- [ ] Dynamic plugins are loaded with a warning about third-party code execution
- [ ] Plugin errors do not crash the compiler (error isolation)
- [ ] Untrusted plugins can be run in a sandboxed process (future extension)
- [ ] Wasm modules execute with WASI capability restrictions

---

## 16. Future Extensions (Beyond Phase 10)

Phase 10 establishes the multi-backend and plugin architecture. Future extensions include:

1. **GPU backend**: A `GpuBackend` using Vulkan compute shaders or CUDA for data-parallel Glyim programs (array operations, matrix math).

2. **RISC-V native backend**: Cranelift already supports `riscv64`; add `riscv64gc-unknown-linux-gnu` as a supported target for embedded and IoT applications.

3. **Debug Adapter Protocol (DAP)**: Extend the LSP server from Phase 7 with DAP support, enabling step-through debugging of Glyim programs in VS Code and other editors.

4. **Wasm component model composition**: Use the WASI preview 2 component model to compose Glyim Wasm modules with modules written in other languages (Rust, C, Python via ComponentizePy).

5. **Plugin marketplace**: A curated registry of compiler plugins (linters, formatters, optimization passes) with version management and compatibility checks.

6. **Interpreter backend**: A `TracingBackend` that interprets HIR directly without code generation, useful for debugging, profiling, and teaching.

7. **Multi-tier JIT**: Combine Cranelift (tier 1: fast compilation) and LLVM (tier 2: optimized compilation) in a single JIT session, upgrading hot functions from Cranelift to LLVM at runtime.
