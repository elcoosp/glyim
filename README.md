# Glyim

**A systems programming language where metaprogramming is typed, hygienic, and IDE‑friendly.**

[![Build Status](https://img.shields.io/github/actions/workflow/status/your-org/glyim/ci.yml?branch=main)](https://github.com/your-org/glyim/actions)
[![Version](https://img.shields.io/badge/version-0.5.0--dev-blue)](https://github.com/your-org/glyim/releases)
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

`let` / `let mut`, `if` / `else`, strings, `println`, `assert`, structs, enums, pattern matching, `?` operator, generic types, impl blocks, tuples, and the `__size_of::<T>()` built‑in — Glyim is on a fast track to feeling like a real language.

```glyim
struct Point { x: i64, y: i64 }

enum Shape {
  Circle(f64),
  Rect { a: Point, b: Point },
}

fn area(s: Shape) -> f64 {
  match s {
    Circle(r) => 3.14159 * r * r,
    Rect { a, b } => (b.x - a.x) * (b.y - a.y),
  }
}

let result: Result<f64, Str> = Ok(area(Shape::Circle(5.0)))
let r: f64 = result?
println(r)   // 78.53975 (planned – currently not fully functional)
```

```bash
$ glyim run example.g
78.53975   # (future output; today prints a diagnostic)
```

---

## Current Capabilities (v0.5.0-dev)

**This is a work in progress.** Many features are present in the parser and internal representation but have only partial codegen or type‑checking. The table below shows what’s *fully implemented* vs. what’s *in development*.

| Feature | Status |
|---------|--------|
| `let` / `let mut` stack‑allocated bindings | ✅ |
| Assignment to mutable variables | ✅ |
| Immutability checks (cannot assign to `let` binding) | ❌ (not enforced) |
| `if` / `else` / `else if` as expressions | ✅ |
| String literals (`"…"` – borrowed fat pointers) | ✅ |
| `println(int)` and `println(str)` (via runtime shims) | ✅ |
| `assert(cond[, msg])` with abort | ✅ |
| Error recovery (multiple errors per compile) | ✅ |
| Ariadne‑rendered diagnostics with `^^^` underlines | ✅ |
| `glyim init` scaffolding | ✅ |
| Arithmetic, comparisons, logical operators, lambdas | ✅ |
| **Structs** – parsing, lowering, codegen with `i64` fields | ✅ (typed fields planned) |
| **Enums** – tag‑discriminated variants, `match` codegen (switch) | ✅ (payloads limited to `i64`) |
| **Pattern matching** (wildcards, literals, variable binding) | ✅ |
| **`bool`** type (lexed & parsed, lowered to `i64` for now) | ⚠️ (no distinct type checking) |
| **`Option<T>` and `Result<T, E>`** (built‑in prelude desugared) | ✅ |
| **`?` operator** – currently aborts on `Err`, not early return | ⚠️ (spec requires `return`) |
| **`f64`** float type (lexed & parsed, arithmetic not yet generated) | ❌ (stubbed to `i64 0`) |
| **Raw pointers** (`*const T`, `*mut T`) – parsing only | ⚠️ (no codegen) |
| **`__size_of::<T>()`** intrinsic | ✅ |
| **`@rust("namespace") extern { ... }`** parsing | ✅ |
| **Macro infrastructure** (typed `MacroContext`, `ContentStore`) | ⚠️ (stubs; `@identity` works via hardcoded AST substitution) |
| **Type checker** – basic type propagation, primarily `i64` | ⚠️ (exhaustiveness, generic inference planned) |
| **Monomorphic generics** – parsed, lowered, but **not monomorphized** | ⚠️ (generic functions emit a single LLVM function) |
| **Generic structs & enums** – syntax works; all fields are `i64` | ⚠️ |
| **`impl` blocks** – methods lowered to mangled names; call‑site rewriting missing | ❌ (integration tests disabled) |
| **Tuples** – parsing and lowering; field access unreliable | ⚠️ |
| **Destructuring `let`** | ✅ (simple cases) |
| **Cast expressions** (`expr as Type`) – parsed, validity check stubbed | ⚠️ |
| **Standard prelude** (real generic `Option`, `Result`) | ✅ |
| **DWARF debug info** (DISubprogram, DILocation, DILocalVariable) | ✅ |
| **Heap wrappers** (`glyim_alloc`/`glyim_free`) | ✅ |
| **`no_std` detection** | ✅ |
| **Package manager** (manifest, lockfile, MVS resolver, workspace) | ✅ |
| **CAS server & build cache** | ✅ |
| **Rowan‑first CST** (required by spec) | ❌ (parser builds AST first, CST is secondary) |
| **JIT execution** (remove `cc` dependency) | ❌ (uses system linker) |
| **`glyim run` without C compiler** | ❌ |

> **Legend:** ✅ = fully working · ⚠️ = partially implemented (see details) · ❌ = not yet implemented

---

## Architecture

Glyim enforces a strict 5‑tier dependency graph (ADR‑001) with zero cycles.

```
Tier 5: Ecosystem  glyim-cli · glyim-cas-server
Tier 4: Backend    glyim-codegen-llvm
Tier 3: Analysis   glyim-hir · glyim-typeck · glyim-macro-core · glyim-macro-vfs · glyim-pkg
Tier 2: Frontend   glyim-parse · glyim-lex
Tier 1: Foundation glyim-syntax · glyim-diag · glyim-interner
```

This is enforced by the workspace setup and verified by `just check-dag` and `just check-tiers`.

### Key Design Decisions

| Decision | Choice | Why |
|----------|--------|-----|
| Macro representation | Typed AST (Scala 3 style) | Safety without a JVM |
| Macro caching | Content‑addressable store | Solves multi‑file collision, enables distributed cache |
| Macro purity | Explicit (impure if fs touched) | Deterministic caching by default |
| File output from macros | Sandbox VFS → CAS | No source pollution, reproducible builds |
| Type‑directed expansion | IoC via `MacroContext` trait | Zero cyclic deps |
| Hygiene | Automatic mangling, opt‑out | Eliminates accidental capture |
| Syntax trees | Rowan (Green/Red) (planned) | World‑class IDE support |
| Expression parsing | Pratt parser + recursive descent | Simplicity & performance |
| FFI | `@rust("...")` as opaque types | Safe boundary |
| JIT execution | Inkwell ORC JIT (planned) | Remove C compiler dependency |
| Type checker | Monomorphic with local inference | Fast checks, no HM complexity |
| Package mgmt | MVS resolver | Minimal version selection |
| Build caching | CAS‑backed object cache | Avoid redundant recompilation |

---

## Project Structure

```
glyim/
├── crates/
│   ├── glyim-interner/       # Symbol interning
│   ├── glyim-diag/           # Diagnostics & ariadne rendering
│   ├── glyim-syntax/         # SyntaxKind, Rowan definitions
│   ├── glyim-lex/            # Tokenizer
│   ├── glyim-parse/          # Parser (Pratt + RD), CST builder, recovery
│   ├── glyim-hir/            # HIR: expressions, types, patterns
│   ├── glyim-typeck/         # Type checker (work‑in‑progress)
│   ├── glyim-macro-core/     # Typed macro engine (stub)
│   ├── glyim-macro-vfs/      # ContentStore trait, local CAS
│   ├── glyim-pkg/            # Package manager (resolver, lockfile)
│   ├── glyim-cas-server/     # HTTP CAS server
│   ├── glyim-codegen-llvm/   # LLVM IR codegen (Inkwell)
│   └── glyim-cli/            # CLI (build, run, check, init, test, pkg, cache)
├── stdlib/                   # Standard library specifications (not yet compilable)
├── docs/specs/               # Architecture specifications v0.1.0 – v0.4.0
├── scripts/check_file_sizes.py
└── justfile                  # Build, test, and quality recipes
```

---

## Building from Source

### Prerequisites

- Rust 1.75+ (stable or nightly)
- LLVM 22.1 development libraries  
  - Ubuntu: `sudo apt install llvm-22-dev`  
  - macOS: `brew install llvm@22`  
  - Set `LLVM_SYS_221_PREFIX` if LLVM is in a non‑standard location

### Quick Start

```bash
git clone https://github.com/your-org/glyim.git
cd glyim
cargo build --release

# Run a program (currently requires cc on PATH)
echo 'main = () => 42' > hello.g
cargo run --release -- run hello.g

# Inspect LLVM IR
cargo run --release -- ir hello.g

# Use the package manager
cargo run --release -- init myproject
cd myproject
cargo run --release -- add serde  # adds dependency to glyim.toml
```

We use **[just](https://github.com/casey/just)** for common tasks:
```bash
just test              # run all tests
just ci                # simulate CI pipeline
just check-files       # enforce file size limits
```

---

## Roadmap

| Version | Focus | Status |
|---------|-------|--------|
| **v0.1.0** | Architectural runway — compile `main = () => 42` to native | ✅ |
| **v0.2.0** | “Real language” DX — let/mut, if/else, strings, println, UI tests | ✅ (JIT missing) |
| **v0.3.0** | Types & data — struct, enum, match, float, pointers, working macro | ⚠️ basic types only |
| **v0.4.0** | Generics, impls, tuples, destructuring, standard prelude | ⚠️ parsed, not functional |
| **v0.5.0** | Package manager, debug info, allocators, no_std, build cache | ⚠️ partial |
| **v0.6.0** | LSP, formatter, macro compilation, distributed CAS, stdlib types | 🔜 planned |

### Current Focus Areas (help wanted!)

- **Finishing the type checker** – enforce immutability, proper type propagation, `bool` as distinct type
- **Rowan‑first parser** – rewrite parser to emit CST during parsing (as per spec)
- **Macro engine** – implement `glyim-macro-core` with typed interpretation
- **`?` operator** – desugar to early `return Err(e)` instead of abort
- **Float arithmetic** – generate `fadd`, `fsub`, etc.
- **Monomorphization** – real generic codegen

---

## Known Limitations in the Current Build

- **`glyim run` requires a C compiler** (`cc`/`gcc`) for linking; JIT is not yet enabled.
- **Macros** are limited to a hardcoded `@identity`; the full typed‑macro pipeline is a placeholder.
- **Float literals** do not produce arithmetic; all `f64` operations yield zero.
- **Generic functions** are not monomorphized; they compile to a single LLVM function with `i64` for all type parameters.
- **Impl method calls** (e.g., `p.translate(dx, dy)`) do not resolve; integration tests are disabled.
- **Raw pointer types** cannot be used as variable types or function parameters.
- **The type checker** does not catch most type mismatches; many errors pass silently.
- **The CST** (Rowan) is built *after* parsing, contravening the spec’s requirement for a lossless parse tree.

See the detailed architecture specs in `docs/specs/` for the intended design.

---

## Contributing

Glyim is an ambitious compiler project and we welcome contributors! The best way to start is to look at the issues tagged `good first issue` (once we have them 😅) or pick a task from the **Current Focus Areas** above.

Before contributing, read the [architecture specs](docs/specs/) to understand the design constraints and the strict tier dependency rules.

---

## License

Licensed under [MIT license](LICENSE-MIT).

---

*“The best macro system is the one that makes you forget macros are hard.”*
