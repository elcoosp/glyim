# Glyim

*A systems programming language where metaprogramming is typed, hygienic, and IDE-friendly.*

---

## Why Glyim?

Rust's proc-macros operate on untyped `TokenStream`s. Macro Systems 2.0 is stalled. Most languages can't generate associated files without fragile build scripts. And if you've ever fought an unhygienic macro capturing a variable it shouldn't — you know the pain.

Glyim is built from day one to fix this. It's a systems language targeting native compilation via LLVM with a novel combination of:

- **Typed Macros** — Macros declare `Expr<T>` inputs/outputs and query the type checker via `MacroContext`
- **Automatic Hygiene** — Every identifier introduced by a macro is mangled by default; `unhygienic!(var)` is the escape hatch
- **Content-Addressable Store** — Macros that generate files (schemas, configs) write to a CAS-backed VFS — no source pollution, no merge conflicts
- **Lossless Syntax Trees** — Rowan Green/Red tree architecture preserves every whitespace and comment for world-class IDE support

```
"Hello, world!" is boring. Here's what Glyim is actually about:

    @serde
    struct User {
        name: String,
        age: i64,
    }

    // @serde sees: T implements Serialize (via MacroContext)
    // It generates: struct code + file!("schema.json", schema_bytes)
    // schema.json lands in target/glyim-out/ — not in your src/
```

## v0.2.0 — The "Real Language" Release

Glyim now feels like a real language in under 60 seconds:

BTxyz
let name = "Glyim"
let mut count = 0
count = count + 1
if count > 0 {
  println("Hello from Glyim!")
}
assert(count == 1, "count should be 1")
println(count)
```

BTbash
$ glyim run hello.xyz
Hello from Glyim!
1
```

### What's New in v0.2.0

| Feature | Status |
|---|---|
| `let` / `let mut` — stack‑allocated variables | ✅ |
| Assignment (`x = expr`) | ✅ |
| `if`/`else`/`else if` expressions | ✅ |
| String literals (with escape sequences) | ✅ |
| `println(int)` and `println(str)` built‑ins | ✅ |
| `assert(cond[, msg])` with runtime abort | ✅ |
| `use` directive (skeleton) | ✅ |
| Error recovery (multiple errors per compilation) | ✅ |
| Ariadne‑rendered diagnostics (source snippets, ^^^ underlines) | ✅ |
| `glyim init` project scaffolding | ✅ |
| UI diagnostic snapshot tests (12 test cases) | ✅ |
| Extended integration tests (16 cases) | ✅ |
| Core HIR extensions (HirStmt, StrLit, If) | ✅ |

### What Still Works from v0.1.0

| Capability | Status |
|---|---|
| Compile `main = () => 42` to native | ✅ |
| Arithmetic, comparisons, logical operators (12 binary, 2 unary) | ✅ |
| Block expressions (last expression returned) | ✅ |
| Lambda expressions & function definitions | ✅ |
| Full 11‑crate tiered dependency graph (no cycles) | ✅ |
| Macro infrastructure: `MacroContext`, `ContentStore`, hygiene, stub type checker | ✅ |
| CLI: `build`, `run`, `ir`, `check`, `export` (placeholder) | ✅ |
| **~265 tests, all passing** | ✅ |

## The Architecture

Glyim enforces a strict 5‑tier dependency graph. Arrows point from consumer to dependency. Cross‑tier imports are forbidden.

```
Tier 5: Ecosystem          glyim-cli
Tier 4: Backend            glyim-codegen-llvm
Tier 3: Analysis & Macros  glyim-hir · glyim-typeck · glyim-macro-core · glyim-macro-vfs
Tier 2: Frontend           glyim-parse · glyim-lex
Tier 1: Foundation         glyim-syntax · glyim-diag · glyim-interner
```

This isn't aspirational — it's enforced by `Cargo.toml`.

### Key Design Decisions

| Decision | Choice | Why |
|---|---|---|
| Macro representation | Typed AST (Scala 3 style) | Safety without a JVM |
| Macro caching | Content-addressable store | Solves multi‑file collision, enables distributed cache |
| Macro purity | Explicit (impure if fs touched) | Deterministic caching by default |
| File output from macros | Sandbox VFS → CAS | No source pollution, reproducible builds |
| Type‑directed expansion | IoC via `MacroContext` trait | Zero cyclic deps between macro engine and type checker |
| Hygiene | Automatic mangling, opt‑out | Eliminates accidental capture bugs |
| Syntax trees | Rowan (Green/Red) | World‑class IDE support out of the box |
| Expression parsing | Pratt (expressions) + recursive descent (items) | Extreme simplicity in the expression layer |
| FFI | `@rust("...")` as opaque types | Safe boundary without exposing Rust's type system |

## Building from Source

### Prerequisites

- Rust 1.75+
- LLVM 18 development libraries
  - Ubuntu: `sudo apt install llvm-18-dev`
  - macOS: `brew install llvm@18`
  - Set `LLVM_SYS_180_PREFIX` if your LLVM is in a non‑standard location
- A C compiler (`cc` or `gcc`) on PATH for linking

### Build

BTbash
git clone https://github.com/your-org/glyim.git
cd glyim
cargo build --release
```

### Run

BTbash
echo 'main = () => 42' > hello.xyz
cargo run --release -- run hello.xyz
echo $?
# 42
```

### Inspect LLVM IR

BTbash
cargo run --release -- ir hello.xyz
# define i32 @main() {
#   ret i32 42
# }
```

## Project Structure

```
glyim/
├── crates/
│   ├── glyim-interner/       # String → Symbol deduplication
│   ├── glyim-diag/           # Span, Diagnostic, ariadne rendering
│   ├── glyim-syntax/         # SyntaxKind (57 kinds), Rowan GlyimLang
│   ├── glyim-lex/            # Hand‑rolled tokenizer with string support
│   ├── glyim-parse/          # Pratt parser + recursive descent, CST builder, error recovery
│   ├── glyim-hir/            # Span‑free IR (HirExpr, HirFn, HirStmt)
│   ├── glyim-typeck/         # StubTypeChecker (MacroContext impl)
│   ├── glyim-macro-core/     # Typed macro expansion engine
│   ├── glyim-macro-vfs/      # ContentStore trait + LocalContentStore
│   ├── glyim-codegen-llvm/   # LLVM IR via Inkwell (let, if, println, assert)
│   └── glyim-cli/            # CLI (build, run, ir, check, init)
├── docs/
│   └── superpowers/plans/    # Implementation plans (TDD, bite‑sized)
└── tests/
    ├── integration/          # End‑to‑end: compile → link → run → assert exit code
    └── fuzz/                 # Fuzz targets (lexer, parser)
```

## Roadmap

| Version | Focus | Status |
|---|---|---|
| **v0.1.0** | Architectural runway — compile `main = () => 42` to native | ✅ `main` |
| **v0.2.0** | "Real language" DX — let/mut, if/else, strings, println, JIT | ✅ `main` |
| **v0.3.0** | Struct types, generic type inference, `@rust()` FFI, real macro execution | Planned |
| **v0.4.0** | LSP (hover, go‑to‑def, macro expansion preview), formatter | Planned |
| **v0.5.0** | Distributed CAS, package registry, `glyim pkg` | Planned |

## Non‑Goals (Explicit)

These are things Glyim intentionally does **not** try to be:

- **Not a "Macros 2.0" catch‑all** — We build a narrow, highly opinionated typed macro system
- **Not a JVM language** — Native compilation via LLVM, zero runtime GC
- **Not a C/C++ replacement** — We interop with Rust via `@rust()` FFI, not by being C‑compatible
- **Not fast to compile (yet)** — Debug LLVM, no incremental compilation. Performance optimization is deferred.
- **Not a macro VM (yet)** — Macro execution compiles to native Rust/LLVM in v0.1.0; a sandboxed VM is future work

## Contributing

Glyim uses a plan‑driven development workflow:

1. Specs are written as architecture documents (see `docs/`)
2. Plans are broken into bite‑sized TDD steps (see `docs/superpowers/plans/`)
3. Each step: write failing test → implement → verify → commit
4. Every plan chunk is reviewed before execution

See `docs/superpowers/plans/2026-04-28-glyim-v0.1.0.md` for the canonical example of how we work.

## License

MIT OR Apache-2.0

---

*"The best macro system is the one that makes you forget macros are hard."*
