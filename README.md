# Glyim

**A systems programming language where metaprogramming is typed, hygienic, and IDE‑friendly.**

[![Build Status](https://img.shields.io/github/actions/workflow/status/your-org/glyim/ci.yml?branch=main)](https://github.com/your-org/glyim/actions)
[![Version](https://img.shields.io/badge/version-0.2.0-blue)](https://github.com/your-org/glyim/releases)
[![License](https://img.shields.io/badge/license-MIT--2.0-blue)](LICENSE)

---

## Why Glyim?

Rust’s proc‑macros operate on untyped `TokenStream`s. Macro Systems 2.0 is stalled. Most languages can’t generate associated files without fragile build scripts. If you’ve ever fought an unhygienic macro capturing a variable it shouldn’t, you know the pain.

**Glyim** is built from the ground up to fix this. It’s a systems language that compiles to native code via LLVM, with a novel combination of:

- **Typed Macros** — Macros declare `Expr<T>` inputs/outputs and receive a typed `MacroContext` for type queries.
- **Automatic Hygiene** — Every macro‑introduced identifier is mangled by default; `unhygienic!(var)` is the conscious escape hatch.
- **Content‑Addressable Store** — Macros that generate files (schemas, configs) write to a CAS‑backed virtual FS, avoiding source pollution and merge conflicts.
- **Lossless Syntax Trees** — Rowan Green/Red tree architecture preserves every whitespace and comment, enabling world‑class IDE support from day one.

```glyim
@serde
struct User {
    name: String,
    age: i64,
}

// @serde queries the type checker via MacroContext and discovers that User
// implements Serialize. It then emits:
//   - struct code for User
//   - file!("schema.json", schema_bytes) → lands in target/glyim-out/
```

---

## The Language in 60 Seconds

`let` / `let mut`, `if` / `else`, strings, `println`, `assert`, JIT execution — Glyim already feels like a real language.

```glyim
let name = "Glyim"
let mut count = 0
count = count + 1
if count > 0 {
  println("Hello from " + name)   # (concatenation not yet supported)
}
assert(count == 1, "count should be 1")
println(count)
```

```bash
$ glyim init hello && cd hello && glyim run
Hello from Glyim!
1
```

### Current Capabilities (v0.2.0)

| Feature | Status |
|---------|--------|
| `let` / `let mut` stack‑allocated bindings | ✅ |
| Full assignment (`x = expr`) | ✅ |
| `if` / `else` / `else if` as expressions | ✅ |
| String literals (borrowed `&str` fat pointers) | ✅ |
| `println(int)` and `println(str)` built‑ins | ✅ |
| `assert(cond[, msg])` with runtime abort | ✅ |
| `use` directive (skeleton) | ✅ |
| Error recovery (multiple errors per compile) | ✅ |
| Ariadne‑rendered diagnostics with `^^^` underlines | ✅ |
| `glyim init` project scaffolding | ✅ |
| JIT execution (no external C compiler needed for `run`) | ✅ |
| Arithmetic, comparisons, logical operators, lambdas, blocks | ✅ |
| Macro infrastructure stubs (traits, CAS, hygiene) | ✅ |
| **~340 tests, all passing** | ✅ |

---

## Upcoming: v0.3.0 — The Type System

The next major release introduces a monomorphic type checker, real data structures, and the first executing macro. Planned features:

- **Structs** with named fields, struct literals, and dot access
- **Enums** with tag‑discriminated variants and exhaustive `match`
- **Pattern matching** with wildcards, literals, and variable binding
- **`bool`** as a distinct type (comparisons return `bool`, `if` requires `bool`)
- **`Option<T>` and `Result<T, E>`** built‑ins with `Some`/`None`/`Ok`/`Err` and `?` propagation
- **`f64`** float type with all arithmetic operations
- **Raw pointers** `*const T` / `*mut T` for FFI
- **`@rust("namespace") extern { ... }`** FFI blocks
- **`@identity`** macro that successfully expands during compilation
- **Type checker** with annotated function params and local inference for `let`
- **Phase‑0 refactor**: no file >500 LOC, CI with GitHub Actions

See the full [v0.3.0 architecture specification](docs/v0.3.0-architecture.md) for details.

---

## The Architecture

Glyim enforces a strict 5‑tier dependency graph. Arrows point from consumer to dependency; cross‑tier imports are forbidden.

```
Tier 5: Ecosystem          glyim-cli
Tier 4: Backend            glyim-codegen-llvm
Tier 3: Analysis & Macros  glyim-hir · glyim-typeck · glyim-macro-core · glyim-macro-vfs
Tier 2: Frontend           glyim-parse · glyim-lex
Tier 1: Foundation         glyim-syntax · glyim-diag · glyim-interner
```

This is enforced by our Cargo workspace configuration.

### Key Design Decisions

| Decision | Choice | Why |
|----------|--------|-----|
| Macro representation | Typed AST (Scala 3 style) | Safety without a JVM |
| Macro caching | Content‑addressable store | Solves multi‑file collision, enables distributed cache |
| Macro purity | Explicit (impure if fs touched) | Deterministic caching by default |
| File output from macros | Sandbox VFS → CAS | No source pollution, reproducible builds |
| Type‑directed expansion | IoC via `MacroContext` trait | Zero cyclic deps between macro engine and type checker |
| Hygiene | Automatic mangling, opt‑out | Eliminates accidental capture bugs |
| Syntax trees | Rowan (Green/Red) | World‑class IDE support out of the box |
| Expression parsing | Pratt parser (expressions) + recursive descent (items) | Simplicity and performance |
| FFI | `@rust("...")` as opaque types | Safe boundary without exposing Rust’s type system |
| JIT execution | Inkwell ORC JIT for `run` | No external C compiler needed for development |

---

## Project Structure

```
glyim/
├── crates/
│   ├── glyim-interner/       # String → Symbol deduplication
│   ├── glyim-diag/           # Span, Diagnostic, ariadne rendering
│   ├── glyim-syntax/         # SyntaxKind, Rowan definitions
│   ├── glyim-lex/            # Hand‑rolled tokenizer
│   ├── glyim-parse/          # Pratt + recursive descent, CST builder, error recovery
│   ├── glyim-hir/            # Span‑free IR (HirExpr, HirStmt, HirType)
│   ├── glyim-typeck/         # Monomorphic type checker (v0.3.0+)
│   ├── glyim-macro-core/     # Typed macro expansion engine
│   ├── glyim-macro-vfs/      # ContentStore trait + LocalContentStore
│   ├── glyim-codegen-llvm/   # LLVM IR generation (Inkwell)
│   └── glyim-cli/            # CLI (build, run, ir, check, init)
├── docs/
│   ├── v0.1.0-architecture.md
│   ├── v0.2.0-architecture.md
│   └── v0.3.0-architecture.md
└── tests/
    ├── integration/          # End‑to‑end compile → run → assert exit code
    ├── ui/                   # Snapshot tests for error messages
    └── fuzz/                 # Fuzz targets for lexer and parser
```

---

## Building from Source

### Prerequisites

- Rust 1.75+
- LLVM 18 development libraries
  - Ubuntu: `sudo apt install llvm-18-dev`
  - macOS: `brew install llvm@18`
  - Set `LLVM_SYS_180_PREFIX` if LLVM is in a non‑standard location
- A C compiler (`cc` or `gcc`) on PATH for linking (only required for `build`; `run` uses JIT)

### Build

```bash
git clone https://github.com/your-org/glyim.git
cd glyim
cargo build --release
```

### Run a Glyim program

```bash
echo 'main = () => 42' > hello.g
cargo run --release -- run hello.g
echo $?               # prints 42
```

### Inspect LLVM IR

```bash
cargo run --release -- ir hello.g
# define i64 @main() {
#   ret i64 42
# }
```

---

## Roadmap

| Version | Focus | Status |
|---------|-------|--------|
| **v0.1.0** | Architectural runway — compile `main = () => 42` to native | ✅ Completed |
| **v0.2.0** | “Real language” DX — let/mut, if/else, strings, println, JIT | ✅ Completed |
| **v0.3.0** | Types & data — struct/enum/match, type checker, floats, Option/Result, raw pointers, first working macro | 🟡 Planned (see [spec](docs/v0.3.0-architecture.md)) |
| **v0.4.0** | LSP, formatter, macro compilation to native, generic type inference, methods & impls | 🔜 Planned |
| **v0.5.0** | Distributed CAS, package registry, `glyim pkg` | 🔜 Planned |

---

## Non‑Goals

Glyim intentionally does **not** try to be:

- **Not a “Macros 2.0” catch‑all** — We build a narrow, highly opinionated typed macro system.
- **Not a JVM language** — Native compilation via LLVM, zero runtime GC.
- **Not a C/C++ replacement** — We interop with Rust via `@rust()` FFI, not by being C‑compatible.
- **Not fast to compile (yet)** — Debug LLVM, no incremental compilation. Performance optimization is deferred.
- **Not a macro VM (yet)** — Macro execution is interpreted in v0.3.0; compilation to native code is future work.

---

## Contributing

Glyim uses plan‑driven development:

1. Specs are written as architecture documents (see `docs/`)
2. Plans are broken into bite‑sized TDD steps
3. Each step: write failing test → implement → verify → commit
4. Every plan chunk is reviewed before execution

See the `docs/v0.2.0-architecture.md` for the canonical example of how we work.

---

## License

Licensed under [MIT license](LICENSE-MIT).

---

*"The best macro system is the one that makes you forget macros are hard."*
