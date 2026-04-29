# Glyim

**A systems programming language where metaprogramming is typed, hygienic, and IDE‑friendly.**

[![Build Status](https://img.shields.io/github/actions/workflow/status/your-org/glyim/ci.yml?branch=main)](https://github.com/your-org/glyim/actions)
[![Version](https://img.shields.io/badge/version-0.3.0-blue)](https://github.com/your-org/glyim/releases)
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

`let` / `let mut`, `if` / `else`, strings, `println`, `assert`, structs, enums, pattern matching, `?` operator — Glyim already feels like a real language.

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
println(r)   // 78.53975
```

```bash
$ glyim run example.g
78.53975
```

### Current Capabilities (v0.3.0)

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
| **Structs** with named fields, struct literals, dot access | ✅ |
| **Enums** with tag‑discriminated variants and exhaustive `match` | ✅ |
| **Pattern matching** with wildcards, literals, variable binding | ✅ |
| **`bool`** as a distinct type (`true`/`false` literals) | ✅ |
| **`Option<T>` and `Result<T, E>`** with `Some`/`None`/`Ok`/`Err` and `?` | ✅ |
| **`f64`** float type with arithmetic | ✅ |
| **Raw pointers** `*const T` / `*mut T` for FFI | ✅ |
| **`@rust("namespace") extern { ... }`** FFI blocks | ✅ |
| **`@identity`** macro that successfully expands during compilation | ✅ |
| **Monomorphic type checker** with local inference and exhaustiveness checking | ✅ |
| **Real tagged‑union codegen** for enums | ✅ |
| Macro infrastructure stubs (traits, CAS, hygiene) | ✅ |
| **26 integration tests, 23 parser tests, 13 UI tests** | ✅ |

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
| Type checker | Monomorphic with local inference | No Hindley‑Milner complexity, fast checking |

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
│   ├── glyim-hir/            # Span‑free IR (HirExpr, HirStmt, HirType, HirPattern)
│   ├── glyim-typeck/         # Monomorphic type checker with exhaustiveness
│   ├── glyim-macro-core/     # Typed macro expansion engine
│   ├── glyim-macro-vfs/      # ContentStore trait + LocalContentStore
│   ├── glyim-codegen-llvm/   # LLVM IR generation (Inkwell) with tagged‑union enums
│   └── glyim-cli/            # CLI (build, run, ir, check, init)
├── docs/
│   ├── specs/
│   │   ├── v0.1.0.md
│   │   ├── v0.2.0.md
│   │   └── v0.3.0.md
│   ├── devlog/
│   └── archive/
├── scripts/
│   └── check_file_sizes.py
└── tests/
    ├── integration/          # 26 end‑to‑end compile → run → assert exit code
    ├── ui/                   # 13 snapshot tests for error messages
    └── parser_v030_tests.rs  # 23 parser unit tests for v0.3.0 features
```

---

## Building from Source

### Prerequisites

- Rust 1.90+ (nightly)
- LLVM 22 development libraries
  - Ubuntu: `sudo apt install llvm-22-dev`
  - macOS: `brew install llvm@22`
  - Set `LLVM_SYS_220_PREFIX` if LLVM is in a non‑standard location

### Development Tools

We use **[just](https://github.com/casey/just)** as a command runner (see `justfile` for available recipes) and **[cargo-insta](https://crates.io/crates/cargo-insta)** for snapshot testing.

```bash
just test       # run all tests
just test-unit  # run only unit tests
just ci         # simulate CI pipeline
```

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
| **v0.3.0** | Types & data — struct/enum/match, type checker, floats, Option/Result, raw pointers, first working macro | ✅ Completed |
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

## License

Licensed under [MIT license](LICENSE-MIT).

---

*"The best macro system is the one that makes you forget macros are hard."*
