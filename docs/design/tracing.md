# Crate Research for Compiler Debugging DX

## Tier 1: No-Brainers (use these)

### `miette` — replaces `ariadne`, unlocks everything else

**What it is:** A full diagnostic toolkit for Rust compilers. Source spans, multi-line annotations, multiple renderers (graphical ANSI, minimal, JSON), `thiserror` integration, and — critically — a `tracing-subscriber` layer.

**Why it's the single most impactful choice:**

- You currently use `ariadne` for rendering and hand-roll `Span`, `Diagnostic`, `Severity`. `miette` subsumes all of these.
- `miette`'s `SourceSpan` is byte-range-based (like yours), but it also supports labeled sub-spans on a single diagnostic — e.g., "expected `Int` here ── and got `Bool` here" on the same error.
- The `MietteHandler` subscriber means: any diagnostic emitted via `tracing::error!` (or returned as `miette::Report`) automatically gets rendered with source context, colors, and file name. **This connects Phase 1 (tracing) and Phase 7 (JSON output) for free.**
- JSON renderer: `miette::JSONReportHandler` — set it as the subscriber and every diagnostic is automatically JSON. No custom serde structs needed for Phase 7.
- Used by: `nu` (Nushell), `biome`/`oxc` (JS tooling), `toml_edit`, `ruff`'s Rust components, `oxide` (oxidecomputer).

**Migration path from `ariadne`:**

```rust
// Your current Diagnostic struct goes away entirely.
// Instead, define error types with thiserror derives:

use miette::{Diagnostic, SourceSpan};
use thiserror::Error;

#[derive(Error, Diagnostic, Debug)]
#[error("type mismatch: expected {expected}, found {found}")]
#[diagnostic(code(glyim::type_mismatch))]
struct MismatchedTypes {
    expected: String,
    found: String,
    #[label("expected this type")]
    expected_span: SourceSpan,
    #[label("but got this type")]
    found_span: SourceSpan,
}

#[derive(Error, Diagnostic, Debug)]
#[error("no 'main' function")]
#[diagnostic(help("every Glyim program must have a `main = () => ...` binding"))]
struct MissingMain {
    #[label("no main function found in this file")]
    file_span: SourceSpan,
}
```

Then rendering is just:
```rust
// Graphical (default, what you see in terminal):
miette::set_hook(Box::new(|_| {
    Box::new(miette::MietteHandler::new())
})).ok();

// JSON (for --json flag):
miette::set_hook(Box::new(|_| {
    Box::new(miette::JSONReportHandler::new())
})).ok();

// Then anywhere:
return Err(MissingMain { file_span: whole_file }.into());
// That's it. No manual render_diagnostics() call needed.
```

**Alternatives considered and rejected:**
- `ariadne` (current): Good renderer, but no typed error integration, no JSON output, no tracing integration, you still need a custom `Diagnostic` struct.
- `codespan-reporting`: Lower-level than miette, no `thiserror` integration, no tracing hook. miette wraps codespan internally.
- `annotate-snippets`: Similar to codespan, no ecosystem integration.

**Crate details:**
```
miette        = "7"       # diagnostic framework
miette-derive = "7"       # #[derive(Diagnostic)]
thiserror     = "2"       # #[derive(Error)]
```

---

### `thiserror` — typed errors everywhere

**What it is:** Derive macros for `std::error::Error`. You already hand-implement `Display` and `Error` on `PipelineError`, `ManifestError`, `ParseError`, `TypeError`.

**Why:** Eliminates ~80 lines of boilerplate across the codebase. More importantly, it composes with `miette`'s `#[derive(Diagnostic)]` — you get `Display`, `Error`, AND source-annotated rendering from a single struct definition.

**What changes:**

```rust
// crates/glyim-cli/src/pipeline.rs — currently 30 lines of Display impl
// Becomes:
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PipelineError {
    #[error("I/O error: {0}")]
    Io(#[source] std::io::Error),
    #[error("{0}")]
    Diag(#[from] miette::Report),  // wraps any miette diagnostic
    #[error("linker error: {0}")]
    Link(String),
    #[error("execution error: {0}")]
    Run(#[source] std::io::Error),
    #[error("manifest error: {0}")]
    Manifest(#[from] crate::manifest::ManifestError),
}
```

```rust
// crates/glyim-parse/src/error.rs — currently 25 lines of Display
#[derive(Error, Diagnostic, Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    #[error("expected {expected} but found {found}")]
    #[diagnostic(code(glyim::parse_expected))]
    Expected {
        expected: SyntaxKind,
        found: SyntaxKind,
        #[label("unexpected {found}")]
        span: SourceSpan,
    },
    #[error("expected {expected} but reached end of input")]
    #[diagnostic(code(glyim::parse_unexpected_eof))]
    UnexpectedEof {
        expected: SyntaxKind,
    },
    // ...
}
```

**Crate details:**
```
thiserror = "2"
```

---

### `tracing` + `tracing-subscriber` — structured observability

**What it is:** The standard Rust instrumentation framework. Spans (timed regions), events (log points), hierarchical, zero-cost when disabled.

**Why for a compiler:** A compiler is a pipeline of discrete phases. Each phase is a span. Each error is an event. This gives you:

- **Timing for free:** `tracing-subscriber`'s `fmt` layer can print span entry/exit with elapsed time. No manual `Instant::now()` needed.
- **Filtering:** `RUST_LOG=glyim_cli::pipeline=debug` to see only pipeline-level events, or `RUST_LOG=glyim_cli::pipeline::phase_parse=trace` to see parse internals.
- **Structured field extraction:** Spans carry `file="test.g"`, errors carry `phase="typeck"`, `code="E0001"` — all queryable.

**The critical integration with miette:**

```rust
// This is the magic line that connects everything:
use miette::MietteHandler;

tracing_subscriber::fmt()
    .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
    .with_writer(std::io::stderr)
    .finish();

// miette::set_hook replaces the default panic/debug handler so that
// any ?-propagated miette::Report gets rendered with source context
// automatically. No need to manually call render_diagnostics().
```

**Crate details:**
```
tracing            = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
```

---

## Tier 2: High Impact (strongly recommended)

### `tracing-chrome` — visual pipeline timeline

**What it is:** A `tracing-subscriber` layer that outputs Chrome Trace Event format. You open the resulting file in `chrome://tracing` (or `ui.perfetto.dev`) and see a Gantt chart of every span.

**Why this is amazing for compiler debugging:**

```
glyim --verbose --dump-ir check test.g 2>trace.json
# Open trace.json in perfetto.dev
```

You see:
```
[compile{file="test.g"}                    ████████████████████████████  340µs
  [phase:lex]                               ███ 12µs
  [phase:parse]                             █████ 45µs
    [parse_item{item="main"}]              ██ 18µs
    [parse_item{item="helper"}]            ███ 22µs
  [phase:lower]                             ████ 67µs
  [phase:typeck]                            ██████████ 189µs
    [check_fn{fn="main"}]                  ███████ 142µs
    [check_fn{fn="helper"}]                ██ 12µs
  [phase:codegen]                           ████ 23µs
```

When a user reports "compilation is slow on this file," you open the trace and immediately see which phase or which specific function is the bottleneck. No profiling build needed.

**Crate details:**
```
tracing-chrome = "0.7"
```

---

### `pretty` — Wadler-style pretty printing for AST/HIR dumps

**What it is:** Philip Wadler's "A Prettier Printer" as a Rust library. Provides `Doc` type with `concat`, `group`, `nest`, `line`, `softline` combinators that automatically wrap at line width.

**Why over hand-rolled tree printing:**

```rust
use pretty::{doc, DocBuilder, Pretty};

impl HirExpr {
    fn pretty<'a, D: Pretty<'a>>(&'a self) -> DocBuilder<'a, D> {
        match self {
            HirExpr::IntLit { value, span, .. } => {
                doc::text(format!("IntLit({value})"))
                    .annotate(doc::Annotation::span(*span))
            }
            HirExpr::Binary { op, lhs, rhs, .. } => {
                doc::text(format!("Bin({op:?})"))
                    .group()
                    .append(doc::line())
                    .append(lhs.pretty().nest(2))
                    .append(doc::line())
                    .append(rhs.pretty().nest(2))
            }
            HirExpr::Block { stmts, .. } => {
                doc::text("Block")
                    .append(doc::space())
                    .append(doc::text("{"))
                    .append(
                        doc::hardline()
                            .append(doc::intersperse(
                                stmts.iter().map(|s| s.pretty()),
                                doc::hardline(),
                            ))
                            .nest(2)
                    )
                    .append(doc::hardline())
                    .append(doc::text("}"))
            }
            // ...
        }
    }
}

// Render to 80-column terminal:
let rendered = expr.pretty(80).to_string();

// Render to 200-column for --dump-hir piped to less:
let rendered = expr.pretty(200).to_string();
```

This gives you properly indented, line-wrapped tree output that adapts to terminal width. Hand-rolled `format!("{:#?}")` doesn't wrap and has no control over layout.

**Alternatives considered:**
- `rowan::Debug` — they already have Rowan, but `Debug` formatting is not pretty-printed.
- `format!("{:#?}")` — no line wrapping, no custom annotation.
- Hand-rolled with `writeln!(out, "{}{}", indent, ...)` — works but doesn't handle wrapping, gets messy fast.

**Crate details:**
```
pretty = "0.12"
```

---

### `serde_json` — for JSON output mode

**What it is:** The standard JSON serializer. Needed for `--json` flag.

**Why:** With `miette`'s `JSONReportHandler`, you get diagnostic JSON for free. But you still need `serde_json` for the compile summary (phases, timing, exit code) that wraps the diagnostics.

**Interaction with miette:**

```rust
if opts.json {
    // Diagnostics come out as JSON via miette's handler
    miette::set_hook(Box::new(|_| {
        Box::new(miette::JSONReportHandler::new())
    })).ok();
    
    // Summary still needs manual serialization:
    let summary = serde_json::json!({
        "success": result.is_ok(),
        "file": ctx.file_path,
        "exit_code": result.unwrap_or(1),
    });
    eprintln!("{}", serde_json::to_string(&summary).unwrap());
}
```

**Crate details:**
```
serde       = { version = "1", features = ["derive"] }
serde_json  = "1"
```

---

## Tier 3: Delight (makes it awesome)

### `tracing-tree` — hierarchical span output as a tree

**What it is:** A `tracing-subscriber` layer that renders spans as an indented tree, similar to `log4rs` tree appenders or Rust's `test` output.

**Why:** The default `tracing-subscriber::fmt` output is flat — one event per line. `tracing-tree` nests spans visually:

```
compile file="test.g"
  phase:lex
    info: 47 tokens
  phase:parse  
    info: 3 items parsed
    error: expected ) but found identifier at bytes 7..8
      at src/test.g:1:8
      ┌─ test.g:1:8
      │  fn f(a b) { a }
      │        ─┬  
      │         ╰── expected ) but found identifier
  phase:lower
    info: 3 items lowered
```

This is *significantly* more readable for compiler debugging than flat log lines. And it works as a drop-in replacement for the fmt layer:

```rust
tracing_tree::HierarchicalLayer::new(2)  // indent width
    .with_targets(true)
    .with_timer(tracing_tree::TimerOption::Off)  // you get timing from chrome traces
    .init();
```

**Trade-off:** `tracing-tree` and `tracing-chrome` can't both be the active layer simultaneously. Use `tracing-tree` for terminal debugging, `tracing-chrome` for performance investigation. Gate with a flag or env var.

**Crate details:**
```
tracing-tree = "0.4"
```

---

### `similar` — diffs for snapshot review and error context

**What it is:** A diff library (Myers, Patience, LCS algorithms) with colored terminal output. Same library that `git` uses under the hood.

**Why for a compiler:**

1. **Error context:** When a type mismatch occurs, show a diff between expected and found types. Useful for complex types like `HashMap<String, Vec<Result<Option<i64>, ParseError>>>`.

2. **Snapshot review integration:** `insta` uses `similar` internally, but you can also use it directly in your `--dump` output to show "what changed" between two HIR nodes.

3. **Future: incremental compilation.** When you have incremental re-compilation, diff the old and new HIR to show exactly what changed.

```rust
use similar::{ChangeTag, TextDiff};

let expected = "Int";
let found = "Bool";
let diff = TextDiff::from_lines(expected, found);

for change in diff.iter_all_changes() {
    let (sign, color) = match change.tag() {
        ChangeTag::Delete => ("-", Style::new().red()),
        ChangeTag::Insert => ("+", Style::new().green()),
        ChangeTag::Equal => (" ", Style::new()),
    };
    print!("{}{color}{}{color:#}", sign, change, color = color);
}
```

**Crate details:**
```
similar = "2"
```

---

### `owo-colors` — conditional terminal colors

**What it is:** Zero-dependency color library with `if cfg!(supports_color)` guards. Colors auto-disable when piped to a file or when `NO_COLOR=1` is set.

**Why:** `ariadne` and `miette` handle their own colors, but for your `dump_tokens`, `dump_ast`, `dump_hir` output, you want consistent colored output that respects `NO_COLOR`. `owo-colors` is the lightest option:

```rust
use owo_colors::OwoColorize;

writeln!(out, "{} {}..{} {:?}", "TOK".cyan(), tok.start, tok.end, tok.kind.display_name().yellow());
```

**Why over `colored`:** `colored` pulls in `lazy_static` and `regex`. `owo-colors` is zero-dep and uses `if cfg!` instead of runtime checks.

**Why over `anstyle`:** `anstyle` is more principled but more verbose API. For a compiler, `owo-colors`'s `.red()` suffix style is more ergonomic.

**Crate details:**
```
owo-colors = "4"
```

---

### `supports-color` — detect terminal color support

**What it is:** Detects whether the terminal supports color, and if so, how many colors (16, 256, 16m/truecolor). Used by `chalk`, `rust-analyzer`, etc.

**Why:** Complements `owo-colors`. Use it to decide between `--dump-ast` using colors vs plain text:

```rust
if supports_color::on(supports_color::Stream::Stdout).is_some() {
    dump_ast_colored(&mut stdout, ast, interner);
} else {
    dump_ast_plain(&mut stdout, ast, interner);
}
```

**Crate details:**
```
supports-color = "3"
```

---

## What You Already Have Right (validation)

| Your choice | Verdict | Why it's correct |
|---|---|---|
| `insta` | ✅ Excellent | Best snapshot testing library in Rust. YAML snapshots for structured data, inline snapshots for strings, `cargo insta review` workflow. No need to change. |
| `rowan` | ✅ Excellent | Correct choice for lossless CST. Used by rust-analyzer, Biome, typst. |
| `clap` | ✅ Excellent | With `derive` feature, the cleanest CLI framework. |
| `inkwell` | ✅ Only option | The only safe LLVM binding worth using. |
| `sha2` | ✅ Fine | For content-addressed macro VFS. Could use `blake3` for speed but sha2 is fine. |
| `nextest` | ✅ Excellent | `cargo nextest run` is strictly better than `cargo test` for compilation speed and output. Your justfile already uses it. |
| `tempfile` | ✅ Fine | Standard choice. |

---

## What to Explicitly Avoid

| Crate | Why not |
|---|---|
| `anyhow` | A compiler needs typed errors at every phase boundary. `anyhow` erases types. Use `miette::Report` instead — it's typed-error-compatible but also carries diagnostic metadata. |
| `log` + `env_logger` | `tracing` is a strict superset. `log` has no spans, no structured fields, no subscriber composability. |
| `codespan-reporting` | `miette` wraps it and adds `thiserror` integration, tracing hooks, and JSON rendering. Using `codespan` directly means reimplementing what miette gives you. |
| `salsa` | Incremental computation framework. Interesting for a future v2.0 with incremental compilation, but massive architecture change for current needs. |
| `logos` | Lexer generator. Your hand-rolled lexer is small, correct, and easy to debug. A generated lexer adds a build dependency and makes debugging harder. |
| `serde` on internal types | Don't derive `Serialize` on `HirExpr`, `HirType`, etc. That couples your IR to a specific serialization format. Serialize at the dump boundary only, in `dump.rs`. |
| `carbon` / `dhat` / ` criterion` | Profiling crates. Useful for performance work, but orthogonal to debugging DX. Add when you need them, not preemptively. |
| `console-subscriber` | Tokio console integration. You're not async, so this is irrelevant. |

---

## Final Dependency Graph

```
thiserror ──────────┐
                    ├──> miette ──> [replaces ariadne + glyim-diag Diagnostic/Severity/Span]
miette-derive ──────┘        │
                             │
tracing ─────────────────────┤
tracing-subscriber ──────────┤
  ├─ fmt layer (flat)        │
  ├─ tracing-tree (hier.) ───┤
  └─ tracing-chrome (.json) ─┘
                             │
pretty ────> dump.rs <───────┘
owo-colors ─> dump.rs
supports-color ─> dump.rs

serde + serde_json ──> json output mode
similar ──> type diff display (future)

[insta, rowan, clap, inkwell, sha2, tempfile] — unchanged
[ariadne] — REMOVED (replaced by miette)
```

---

## Minimal `Cargo.toml` Changes to Start

```toml
# crates/glyim-diag/Cargo.toml — THE KEY CHANGE
[dependencies]
miette        = { version = "7", features = ["fancy-no-backtrace"] }
miette-derive = "7"
thiserror     = "2"
tracing       = "0.1"

# Remove: ariadne = "0.6"
```

```toml
# crates/glyim-cli/Cargo.toml
[dependencies]
# ... existing deps ...
tracing            = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
tracing-chrome     = "0.7"
tracing-tree       = "0.7"
pretty              = "0.12"
owo-colors          = "4"
supports-color      = "3"
serde               = { version = "1", features = ["derive"] }
serde_json          = "1"
similar             = "2"
```

```toml
# crates/glyim-parse/Cargo.toml
[dependencies]
# Remove: ariadne dependency if it's here
# glyim-diag already provides miette re-exports
thiserror = "2"
```

```toml
# crates/glyim-typeck/Cargo.toml
[dependencies]
thiserror = "2"
```

---

## One More Thing: `miette`'s `SourceCode` Integration

The thing that makes `miette` truly superior to hand-rolled rendering is `SourceCode`. You register your source text once:

```rust
// In pipeline context setup:
let source = miette::NamedSource::new(file_path.to_string_lossy(), source_text.as_str());
// Store in CompileContext. Then every error just carries a SourceSpan (byte range).
// miette automatically looks up the source, computes line/column, and renders context.
```

This eliminates your current pattern of passing `&source` and `&file_path` to every `render_diagnostics()` call. The source is registered once at the boundary, and all internal code just works with byte ranges. This is a genuine architectural improvement, not just aesthetics.
