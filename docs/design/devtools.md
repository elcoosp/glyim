# The Ultimate Glyim Compiler DX Plan

## The Vision: The Browsable Compiler

The fundamental insight driving this entire architecture is that **a compiler pipeline is structurally identical to a nested file system.** 

Historically, compiler debugging tools have tried to map this hierarchical structure into flat text dumps, collapsible tree widgets, or clunky IDE plugins. We are abandoning all of that.

By combining **Miette** (for rich, typed diagnostics), **Msgpack** (for blazing fast state serialization), and **GPUI + Navi Router** (for GPU-accelerated, file-system-based UI routing), we transform the Glyim compiler into a **browsable space**. 

Every phase is a directory. Every AST/HIR node is a file. Navigating the compiler is just clicking links. Debugging a type error is just clicking "Back."

---

## Part 1: The Instrumentation Foundation

Before we can build the GUI, the compiler must emit structured, span-rich, queryable data.

### 1.1 Diagnostics: The Miette Migration
Replace `ariadne` and the custom `glyim-diag` types with `miette` + `thiserror`. This gives us typed errors, labeled spans, `#[related]` causal chains, and automatic JSON/ANSI rendering for free.

**`crates/glyim-diag/Cargo.toml` changes:**
```toml
[dependencies]
miette = { version = "7", features = ["fancy-no-backtrace"] }
thiserror = "2"
# REMOVE: ariadne = "0.6"
```

**Example: `TypeError` with Spans and Causality:**
```rust
use miette::{Diagnostic, SourceSpan};
use thiserror::Error;

#[derive(Error, Diagnostic, Debug)]
#[error("type mismatch: expected {expected}, found {found}")]
#[diagnostic(code(glyim::type_mismatch))]
pub struct MismatchedTypes {
    pub expected: String,
    pub found: String,
    #[label("expected {expected}")]
    pub expected_span: SourceSpan,
    #[label("found {found}")]
    pub found_span: SourceSpan,
    #[related]
    pub related: Vec<miette::Report>, // "x was declared as Bool here..."
}
```

### 1.2 Observability: The Tracing Layer
Wrap every pipeline phase in `tracing` spans with structured fields.

```rust
// In pipeline.rs
let _span = tracing::info_span!(
    "phase:parse",
    items_count = items.len(),
    errors_count = errors.len()
).entered();
```

*   **Terminal debugging:** Uses `tracing-tree` for hierarchical, readable logs.
*   **Performance profiling:** Uses `tracing-chrome` to output a trace file for `perfetto.dev`.
*   **GUI debugging:** The Navi Router's built-in Devtools (`Cmd+Shift+D`) consume GPUI events, making external tracing redundant for GUI users.

### 1.3 The Pipeline Context
Refactor the monolithic `run()` function into a stateful context object that accumulates data for the Devtools.

```rust
pub struct CompileContext<'a> {
    pub file_path: &'a Path,
    pub source: String,
    pub opts: CompileOpts,
}

pub struct CompileState {
    pub raw_tokens: Vec<Token>,
    pub parse_output: Option<ParseOutput>,
    pub hir: Option<Hir>,
    pub typeck_result: Option<TypeckResult>,
    pub ir_string: Option<String>,
}
```

---

## Part 2: The Data Contract (The Bundle)

The compiler and the devtools communicate exclusively through the `CompileBundleV2`. It uses `rmp-serde` (MessagePack) for instant loading, and crucially, includes a **Path Index**.

### The Path Index (The Magic Key)
To make Navi Router work, we must be able to map a byte offset in the source code to a URL path in the compiler tree.

```rust
#[derive(Serialize, Deserialize)]
pub struct CompileBundleV2 {
    pub version: String,
    pub source: String,
    pub file_path: String,
    pub phases: Vec<PhaseOutputV2>,
    
    /// Maps byte_offset -> Navi Path
    /// e.g., 14 -> "/compile/parse/0/body/stmts/1"
    pub path_index: BTreeMap<usize, String>,
}
```

When the compiler builds the bundle, it walks the AST/HIR and records the path to every node. When a user clicks byte 14 in the GUI, the tool looks up `14` in the `path_index`, gets `"/compile/parse/0/body/stmts/1"`, and tells Navi Router to navigate there.

---

## Part 3: The Navi Router Architecture (The GUI)

This is the core of the experience. We use `gpui`, `gpui-component`, and `gpui-navi` to build an interface where the compiler's internal state is navigated exactly like a web app.

### The Route Tree (The File System)
The physical file layout defines the routes.

```text
crates/glyim-devtools/src/routes/
├── __root.rs                 # Layout: SplitPane(Source Editor | <Outlet>)
├── compile/
│   ├── index.rs              # Dashboard
│   ├── lex.rs                # Token List
│   ├── parse/
│   │   ├── index.rs          # AST Root
│   │   └── [id].rs           # AST Node Detail (supports infinite nesting via $id)
│   ├── lower/
│   │   ├── index.rs          # HIR Root
│   │   └── [id].rs           # HIR Node Detail
│   ├── typeck.rs             # Type Map
│   └── codegen.rs            # IR View
└── errors/
    ├── index.rs              # Error List
    └── [id].rs               # Error Detail (Causal Chain)
```

### The `__root.rs` Layout
This never unmounts. The Source Editor lives here so your scroll position is preserved as you navigate phases.

```rust
use navi_macros::define_route;
use navi_router::components::{Link, Outlet};

define_route!(RootRoute, path: "/", is_layout: true, component: RootLayout);

struct RootLayout;
impl RenderOnce for RootLayout {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        h_flex()
            .child(
                // Left: Source Code
                div().size_full().child(SourceEditor::new(cx))
            )
            .child(
                // Right: The Routed Inspector
                v_flex()
                    .child(phase_nav_bar(cx)) // Links to /compile/lex, /parse, etc.
                    .child(div().size_full().child(Outlet::new()))
            )
    }
}
```

### The "Follow" Feature is just `use_navigate!`
When the user clicks byte 24 in the source editor:

```rust
// In SourceEditor click handler
let path = bundle.path_index.get(&24).unwrap(); // e.g., "/compile/lower/0/body/stmts/1"
let navigate = use_navigate!(cx);
navigate.push(path); // The right pane instantly updates to that exact HIR node
```

### Navi Loaders = Lazy Phase Evaluation
If a user deep-links directly to `/compile/lower/0`, Navi Router executes the `loader` defined on that route, ensuring the HIR is actually computed before rendering the component.

```rust
define_route!(
    LowerRoute,
    path: "/compile/lower",
    loader: |params, executor| async move {
        let bundle = executor.global::<Model<CompileBundle>>();
        bundle.update(executor, |b, _| b.ensure_lowered()); // Compute if needed
        true
    },
    before_load: |ctx| async move {
        // Guard: Redirect to parse errors if parsing failed
        if ctx.global::<CompileBundle>().read(ctx).parse_errors.is_empty() {
            BeforeLoadResult::Ok
        } else {
            BeforeLoadResult::Redirect("/errors".into())
        }
    },
    component: HirListView
);
```

### Navi Devtools Replace External Tooling
Press `Cmd+Shift+D` in the GUI:
*   **Routes Tab:** Visualizes the exact tree of compiler phases.
*   **Timeline Tab:** Logs every navigation (replaces `tracing-chrome` for GUI users).
*   **Cache Tab:** Shows which phases are cached/stale.

---

## Part 4: The Supporting CLI/TUI Suite

Not all debugging happens in a GUI. We provide specialized CLI tools for CI, SSH, and scripts.

### 1. `glyim-explain`
Consumes Miette JSON output. Traverses the `#[related]` causal chains to build a human-readable "Why did this fail?" tree. Includes a `--minimize` flag (Delta Debugging algorithm) to shrink a failing file to the smallest reproduction.

### 2. `glyim-diff`
Uses `tree-diff` to semantically compare two `CompileBundleV2` files. Ignores formatting changes; only reports structural changes (e.g., "Op changed from Add to Sub").

### 3. `glyim-inspect` (Rhai REPL)
A terminal REPL for SSH/CI. Uses `rhai` to allow ad-hoc querying of the bundle without modifying the compiler.
```bash
> hir.find("Binary").filter(|x| x.op == "Add")
```

### 4. `glyim-replay`
A `tuirealm`-based terminal TUI for stepping through pipeline phases when a GPU environment isn't available.

### 5. `glyim-bisect`
Uses `git2` to binary search git history and find the exact commit that introduced a test failure.

---

## The Final Crate Matrix

### `crates/glyim-diag/Cargo.toml`
```toml
[dependencies]
miette = { version = "7", features = ["fancy-no-backtrace"] }
thiserror = "2"
tracing = "0.1"
```

### `crates/glyim-cli/Cargo.toml`
```toml
[dependencies]
# ... existing compiler deps ...
miette = "7"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
tracing-tree = "0.7"
tracing-chrome = "0.7"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
rmp-serde = "1.3"
owo-colors = "4"
pretty = "0.12"
```

### `crates/glyim-devtools/Cargo.toml`
```toml
[package]
name = "glyim-devtools"
version.workspace = true
edition.workspace = true

[dependencies]
# GPUI Ecosystem
gpui = { git = "https://github.com/zed-industries/zed", rev = "..." }
gpui-component = { git = "https://github.com/longbridge/gpui-component", rev = "..." }
navi-router = { path = "../navi-router" }
navi-macros = { path = "../navi-macros" }

# Data & Text
serde = { version = "1", features = ["derive"] }
serde_json = "1"
rmp-serde = "1.3"
rope = "0.1"

# Scripting
rhai = { version = "1.19", features = ["serde"] }

# Diffing & Graphing
tree-diff = "0.5"
graphviz-rust = "0.7"
petgraph = "0.6"

# Terminal Tools (Feature gated)
clap = { version = "4", features = ["derive"], optional = true }
owo-colors = { version = "4", optional = true }
ratatui = { version = "0.29", optional = true }
tuirealm = { version = "2.0", optional = true }
crossterm = { version = "0.28", optional = true }
git2 = { version = "0.19", optional = true }

[build-dependencies]
navi-codegen = { path = "../navi-codegen" }

[features]
default = ["gui"]
gui = ["tree-diff", "graphviz-rust", "petgraph", "rhai"]
cli = ["clap", "owo-colors", "tree-diff"]
tui = ["ratatui", "tuirealm", "crossterm"]
bisect = ["git2"]
```

---

## The Final Justfile

```makefile
# ─── Compiler Pipeline ─────────────────────────────────────
# Generate the devtools bundle (binary msgpack)
bundle FILE *FLAGS:
    cargo run -p glyim-cli -- {{ FLAGS }} check {{ FILE }} --bundle /tmp/glyim.bin

# Generate JSON bundle (for explain/diff CLI tools)
bundle-json FILE:
    just bundle {{ FILE }} --format json --output /tmp/glyim.json

# ─── GUI (The Navi Experience) ────────────────────────────
# Open the GUI debugger
debug-gui FILE:
    just bundle {{ FILE }}
    cargo run -p glyim-devtools -- /tmp/glyim.bin {{ FILE }}

# Deep link directly to a compiler node
debug-node FILE PHASE PATH:
    just bundle {{ FILE }}
    cargo run -p glyim-devtools -- /tmp/glyim.bin --url "/compile/{{ PHASE }}/{{ PATH }}"

# ─── CLI Tools ────────────────────────────────────────────
# Explain an error and minimize the reproduction
explain FILE:
    cargo run -p glyim-cli -- check {{ FILE }} --json 2>&1 | \
    cargo run -p glyim-devtools --features cli --bin glyim-explain --minimize

# Semantic diff two compiler states
diff BEFORE AFTER:
    cargo run -p glyim-devtools --features cli --bin glyim-diff -- {{ BEFORE }} {{ AFTER }}

# ─── TUI Fallback ────────────────────────────────────────
# Step-through debugger for SSH
replay FILE:
    just bundle {{ FILE }}
    cargo run -p glyim-devtools --features tui --bin glyim-replay -- /tmp/glyim.bin
```

---

## Implementation Roadmap

### Week 1: The Foundation (Compiler Side)
1. Migrate `glyim-diag` to `miette` + `thiserror`.
2. Add spans to `TypeError`.
3. Centralize error rendering into one `diagnostic_utils.rs` file.
4. Define `CompileBundleV2` schema and implement `rmp-serde` serialization.
5. Add `--bundle` and `--json` flags to `glyim-cli`.

### Week 2: The Path Index
1. Implement the AST/HIR walker that generates the `path_index` (`byte_offset` -> `"/compile/parse/0/body/..."`).
2. Ensure the bundle serializes correctly and instantly.

### Week 3: The Navi Shell
1. Setup `gpui`, `gpui-component`, and `navi-router`.
2. Implement `__root.rs` (SplitPane) and wire up the GPUI Source Editor.
3. Implement `/compile/lex` route to prove the loader/view architecture works.

### Week 4: Deep Linking & "Follow"
1. Implement the `/compile/parse` and `/compile/lower` routes with `$id` dynamic segments.
2. Wire the Source Editor `on_click` handler to lookup the `path_index` and call `use_navigate!().push()`.
3. *Milestone:* Clicking source code updates the inspector pane.

### Week 5: CLI & Polish
1. Implement `glyim-explain` (consuming miette JSON).
2. Implement `glyim-diff` (consuming tree-diff).
3. Add `tracing-tree` and `tracing-chrome` support to the compiler for non-GUI debugging.
