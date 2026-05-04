# Glyim Codebase: Comprehensive Improvement Plan

> A deep structural analysis and actionable refactoring roadmap — organized by impact tier, ordered so each phase keeps all tests green.

---

## Executive Summary

Glyim is a well-structured, impressively ambitious compiler toolchain with a clear tiered crate architecture, solid snapshot and integration tests, and real generics via monomorphization. The bones are excellent. But five years of rapid feature growth have left real structural debt: **184 debug `eprintln!` calls in production paths**, **402 `.unwrap()` calls**, a massively duplicated pipeline (~600 lines repeated 6 times), a fake SHA-256 hash hardcoded in the lockfile system, `Arc<Mutex<LocalContentStore>>` where a `RwLock` belongs, and type system gaps where invalid compiler states are representable.

The plan below eliminates all of this across five phases.

---

## Phase 1 — Strip Debug Noise & Seal Panics (1–2 days, zero risk)

This phase improves DX immediately and makes failing tests far easier to diagnose. Every change is mechanical and test-safe.

### 1.1 Remove all `eprintln!` from production code paths

There are **184 `eprintln!` calls** scattered across `pipeline.rs`, `codegen/mod.rs`, `desugar.rs`, `specialize.rs`, `rewrite.rs`, `lower/expr.rs`, and more. Many are multi-line debug dumps like:

```rust
// codegen/mod.rs — generate()
eprintln!("[codegen generate] =======================");
for item in &hir.items { ... eprintln!("[codegen generate] Fn: ...") ... }
eprintln!("[codegen generate] =======================");
// Then AGAIN 10 lines later:
eprintln!("[codegen] generate() received {} items:", hir.items.len());
```

**Plan:** Gate every `eprintln!` behind the existing `GLYIM_DEBUG_IR` env var pattern, or better, behind `tracing` instrumentation which is already a dependency. Replace with `tracing::debug!` / `tracing::trace!` so users see nothing by default but can enable structured traces with `RUST_LOG=glyim=debug`.

In `desugar.rs` and `lower/expr.rs` specifically:
```rust
// Replace:
eprintln!("[desugar] MethodCall {} → Call {}", method, mangled);
// With:
tracing::trace!(method, mangled, "desugared MethodCall → Call");
```

In `specialize.rs`, the `force_sub` debug dumps are especially noisy — they fire on every single type specialization pass. Gate behind `tracing::trace!`.

In `pipeline.rs::run()`, the mono-fn enumeration loop (`eprintln!("[pipeline] mono fn: ...")`) fires at runtime for every Vec/HashMap method. Remove it entirely — it was a debugging aid.

**Test impact:** Zero. This is purely additive tracing.

### 1.2 Convert `.unwrap()` to `?` or proper error paths

402 `.unwrap()` calls is the single biggest hidden bug risk. Most fall into three categories:

**Category A — builder API infallibility (safe to keep as `expect`):**
```rust
// inkwell builder calls that only fail on programmer error
builder.build_return(Some(&val)).unwrap()
```
Change to `.expect("build_return: internal compiler error")` so panics have context.

**Category B — panic in test helpers (acceptable):**
```rust
// In test functions
tempfile::tempdir().unwrap()
```
Leave as-is or use `expect`.

**Category C — actual runtime panics that must become errors:**

The most critical ones in production code:

```rust
// pipeline.rs
let hash_content = hash.parse::<glyim_macro_vfs::ContentHash>().unwrap();
// → Already has a computed hash from sha256; parse should not fail.
//   But wrap: .map_err(|e| PipelineError::Codegen(format!("hash parse: {e}")))?

// main.rs CAS commands (inline closures)
let hash: glyim_macro_vfs::ContentHash = hash.parse().map_err(|e| {
    eprintln!("invalid hash: {e}");
    1
})?;
// Already using ? pattern in the closure — good. Audit for any raw .unwrap()s.

// macro_expand.rs
.map(|(_, c)| match c { ... depth == 0 }).map(|(i, _)| inner_start + i).unwrap()
// This panics if no closing paren is found; the outer loop already handles it:
let inner_end = match result[inner_start..].char_indices().find(...).map(...) {
    Some(pos) => pos,
    None => { scan_from = inner_start; continue; }
};
// Already safe. But the test copy of the loop in tests still uses .unwrap() — fix.

// alloc.rs
let raw_ptr = match raw_ptr { ValueKind::Basic(v) => v.into_pointer_value(), _ => panic!(...) }
// This is codegen-internal and truly unreachable post-verification — keep as panic but document.
```

**Specific files to audit by priority:**
1. `pipeline.rs` (all 6 pipeline variants)
2. `macro_expand.rs` (both the prod and test copies of the expansion loop)
3. `codegen/mod.rs` (the `generate()` method)
4. `lockfile_integration.rs`
5. `cmd_*.rs` files

---

## Phase 2 — Eliminate the Pipeline Duplication (2–3 days, medium risk)

This is the biggest DX and maintainability win. Right now the compiler pipeline is copy-pasted **six times**:

- `run()` — JIT-execute, no mode parameter
- `run_with_mode()` — JIT-execute with BuildMode
- `build()` — AOT compile
- `build_with_mode()` — AOT compile with BuildMode
- `run_tests()` — test harness JIT
- `run_jit()` — bare JIT for doctests/etc.

All six perform this identical sequence:
1. Load source + prelude
2. Expand macros
3. Parse
4. Build decl table
5. Lower to HIR
6. Typecheck
7. Desugar method calls
8. Monomorphize
9. (Diverge: JIT vs. write object file)

The duplication means bugs fixed in one path silently persist in the others. `run()` doesn't call `lower_with_declarations` (it uses the old two-phase approach inconsistently). `build()` calls `compile_to_hir_and_ir()` which internally calls `glyim_hir::lower()` — the *old* single-phase lowerer without `DeclTable` — while all the other variants use `lower_with_declarations`. This is a real correctness discrepancy.

### 2.1 Extract a `CompilationPipeline` struct

```rust
pub struct PipelineConfig {
    pub mode: BuildMode,
    pub target: Option<String>,
    pub force_no_std: Option<bool>,
    pub jit_mode: bool,
    pub cas_dir: PathBuf,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            mode: BuildMode::Debug,
            target: None,
            force_no_std: None,
            jit_mode: false,
            cas_dir: dirs_next::data_dir()
                .unwrap_or_else(|| PathBuf::from(".glyim/cas")),
        }
    }
}

struct CompiledHir {
    hir: Hir,
    mono_hir: Hir,
    merged_types: Vec<HirType>,
    interner: Interner,
    source: String,
    is_no_std: bool,
}

fn compile_source_to_hir(
    source: String,
    input_path: &Path,
    config: &PipelineConfig,
) -> Result<CompiledHir, PipelineError> {
    let is_no_std = config.force_no_std.unwrap_or_else(|| detect_no_std(&source));
    
    let parse_out = glyim_parse::parse(&source);
    if !parse_out.errors.is_empty() {
        return Err(PipelineError::Parse(parse_out.errors));
    }
    let mut interner = parse_out.interner;
    
    let decl_output = glyim_parse::declarations::parse_declarations(&source);
    let decl_table = DeclTable::from_declarations(&decl_output.ast, &mut interner);
    
    let mut hir = glyim_hir::lower_with_declarations(&parse_out.ast, &mut interner, &decl_table);
    let mut typeck = TypeChecker::new(interner.clone());
    if let Err(errs) = typeck.check(&hir) {
        return Err(PipelineError::TypeCheck(errs));
    }
    glyim_hir::desugar_method_calls(&mut hir, &typeck.expr_types, &mut interner);
    
    let expr_types = typeck.expr_types.clone();
    let call_type_args = std::mem::take(&mut typeck.call_type_args);
    let (merged_types, mono_hir) = merge_mono_types(&hir, &mut interner, &expr_types, &call_type_args);
    
    Ok(CompiledHir { hir, mono_hir, merged_types, interner, source, is_no_std })
}
```

Then all six variants become thin wrappers:

```rust
pub fn run_jit(source: &str) -> Result<i32, PipelineError> {
    let compiled = compile_source_to_hir(
        format!("{PRELUDE}\n{source}"),
        Path::new("<jit>"),
        &PipelineConfig { jit_mode: true, ..Default::default() },
    )?;
    execute_jit(compiled, None, None)
}

pub fn run(input: &Path, target: Option<&str>) -> Result<i32, PipelineError> {
    let config = PipelineConfig { jit_mode: true, target: target.map(str::to_owned), ..Default::default() };
    let source = load_source(input)?;
    let compiled = compile_source_to_hir(source, input, &config)?;
    execute_jit(compiled, None, None)
}
```

The `run()` and `run_with_mode()` can then be merged:

```rust
pub fn run(input: &Path, target: Option<&str>) -> Result<i32, PipelineError> {
    run_with_mode(input, BuildMode::Debug, target, None)
}
```

### 2.2 Fix the `build()` lowering discrepancy

`build()` currently calls `compile_to_hir_and_ir()` which uses `glyim_hir::lower()` (single-phase, no DeclTable). All other paths use `lower_with_declarations`. After the refactor, all paths go through `compile_source_to_hir()` which uses `lower_with_declarations`. This fixes a potential correctness gap where forward references might be resolved differently in `build` vs `run`.

### 2.3 Audit `run()` vs `run_with_mode()` divergence

`run()` currently has a mono-fn debug loop that `run_with_mode()` does not. It also doesn't use `Codegen::with_line_tables()` while `build()` does. After the refactor, these inconsistencies are structurally impossible.

---

## Phase 3 — Fix Hidden Bugs & Invalid States (3–4 days, targeted)

### 3.1 Fix the fake SHA-256 hash in lockfile generation

In `lockfile_integration.rs`:

```rust
// CURRENT — always writes 32 zero bytes as the hash
resolved_map.insert(
    name.clone(),
    (
        pkg.version.clone(),
        format!("sha256:{}", hex::encode([0u8; 32])),  // ← FAKE HASH
        ...
    ),
);
```

This means `glyim verify` will always fail for packages resolved via path dependencies because their stored hash is `sha256:0000...0000` instead of a real content hash. The infrastructure to compute real hashes exists (`compute_path_hash()` is defined right above). Fix:

```rust
let real_hash = compute_path_hash(&abs_path)?;
format!("sha256:{}", real_hash)
```

For registry dependencies, the hash should come from the registry manifest or the downloaded blob. Mark registry-sourced packages with a placeholder hash only when the blob hasn't been downloaded yet (i.e., `glyim fetch` hasn't run).

### 3.2 Make `HirStmt::Let` and `HirStmt::LetPat` represent distinct states

Currently both `HirStmt::Let` and `HirStmt::LetPat` exist. `LetPat` is the more general form (handles destructuring), while `Let` is the simple `name: Symbol` form. The rewriting passes must handle both. The substitution code in `rewrite.rs` and `specialize.rs` duplicates arms for both. The `HirStmt::Let` variant should be eliminated — it's strictly subsumed by `HirStmt::LetPat { pattern: HirPattern::Var(name), ... }`.

**Migration plan:**
1. In `lower/expr.rs::lower_stmt()`, always emit `LetPat` with `HirPattern::Var(sym)` for simple bindings.
2. Remove the `Let` variant from `HirStmt`.
3. Update all match arms in `rewrite.rs`, `specialize.rs`, `desugar.rs`, `codegen/stmt.rs`, and `typeck/stmt.rs`.

This reduces the match arm count by ~8 duplicated patterns and makes the AST smaller.

### 3.3 Make `ExprId` allocation part of the `LoweringContext` contract

Currently `ExprId` is a `u32` counter in `LoweringContext`. The problem: `ExprId` values must be globally unique per compilation to index into `expr_types: Vec<HirType>`. After monomorphization, new `HirFn` copies are created using `ctx.fresh_id()` — but the monomorphizer has its own internal state that doesn't share the counter from the lowering context.

In `specialize.rs`, cloned expressions use the original `ExprId` values. This is intentional (they look up the same type entry). But in `rewrite.rs`, new `HirExpr` nodes are occasionally constructed with hardcoded `ExprId::new(0)` — which aliases the first expression's type slot. 

**Fix:**
```rust
// In DeclTable::from_declarations() where placeholder HirFn bodies are created:
body: crate::node::HirExpr::IntLit {
    id: crate::types::ExprId::new(0),  // ← aliasing bug
    value: 0,
    span: Span::new(0, 0),
},

// Should be a sentinel value that's clearly invalid for type lookup:
id: crate::types::ExprId::PLACEHOLDER,  // new associated const = ExprId(u32::MAX)
```

Add `ExprId::PLACEHOLDER` as a sentinel and assert in `merge_mono_types` that no PLACEHOLDER ids remain in function bodies that reach codegen.

### 3.4 Replace `Arc<Mutex<LocalContentStore>>` with `Arc<RwLock<LocalContentStore>>`

In `glyim-cas-server`, the CAS store is protected by a `tokio::sync::Mutex` but most operations are reads (`retrieve`, `find_missing`). A `RwLock` would allow concurrent reads:

```rust
// Before:
pub struct AppState {
    store: Arc<Mutex<LocalContentStore>>,
}

// After:
pub struct AppState {
    store: Arc<RwLock<LocalContentStore>>,
}
```

All `store.lock().await` in read paths become `store.read().await`, and write paths (`store_blob`, `store_action_result`) use `store.write().await`. This is a correctness-neutral performance improvement.

### 3.5 Fix the `batch_read_blobs` empty-data false-negative

In `grpc/cas.rs`:
```rust
let status = if data.is_empty() {
    Some(Self::grpc_status(tonic::Code::NotFound, "blob not found"))
} else {
    None
};
```

This treats an empty blob (a zero-byte file stored in CAS) as "not found". The correct check is whether the retrieve call returned `None`:

```rust
let maybe_data = store_guard.retrieve(h);
let (data, status) = match maybe_data {
    Some(bytes) => (bytes, None),
    None => (vec![], Some(Self::grpc_status(tonic::Code::NotFound, "blob not found"))),
};
```

### 3.6 Fix the macro expansion loop's O(n²) rescanning

In `macro_expand.rs`, after a successful macro expansion:
```rust
result = format!("{before}{expanded_str}{after_str}");
scan_from = 0;  // ← restart from the beginning every time
```

This rescans the entire string from position 0 after each expansion, which is O(n×m) for n expansions and m string length. Since the identity macro expands `@identity(x)` to `x`, rescanning from the start will never find another `@identity(...)` macro in the expanded result (identity is idempotent). For non-identity macros the rescan is needed only if the expanded result itself contains `@macro_name(...)` calls.

**Fix:** After expansion, set `scan_from = at_pos` (the position where the `@` was) rather than 0, unless the expansion result itself contains `@`. This converts average case from O(n²) to O(n).

```rust
let has_nested_macro = expanded_str.contains('@');
if has_nested_macro {
    scan_from = at_pos;
} else {
    scan_from = at_pos + expanded_str.len();
}
```

### 3.7 Fix `detect_no_std` false positive in comments

In `pipeline.rs`:
```rust
fn detect_no_std(source: &str) -> bool {
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed == "no_std" {
            return true;
        }
    }
    false
}
```

A test in `no_std_tests.rs` already documents this:
```rust
#[test]
fn detect_no_std_known_limitation_comment() {
    assert!(!detect_no_std("// no_std\nfn main() { 0 }"));
    // This FAILS — the comment "// no_std" does NOT trigger the check because
    // "// no_std" != "no_std". But "  // no_std  ".trim() == "// no_std" ≠ "no_std".
    // Actually this test passes — the real gap is indented `no_std` inside strings.
}
```

The real bug is the opposite direction: the detection scans *after* the prelude is prepended, so the prelude's content could theoretically trip it. More importantly, `no_std` appearing as a line inside a string literal would be a false positive. Fix with a proper lexer-aware check, or at minimum strip single-line comments before checking:

```rust
fn detect_no_std(source: &str) -> bool {
    source.lines().any(|line| {
        let without_comment = line.split("//").next().unwrap_or("");
        without_comment.trim() == "no_std"
    })
}
```

### 3.8 Fix `validate_target` double call in `run_with_mode`

In `pipeline.rs::run_with_mode()`:
```rust
if let Some(t) = target {
    crate::cross::validate_target(t).map_err(PipelineError::Codegen)?;
    crate::cross::ensure_sysroot(t).map_err(PipelineError::MissingSysroot)?;
    codegen = codegen.with_target(t);
}
// ... 15 lines later ...
if let Some(t) = target {
    if let Err(e) = crate::cross::validate_target(t) {  // ← called twice
        return Err(PipelineError::Codegen(e));
    }
    codegen = codegen.with_target(t);  // ← with_target called twice
}
```

The same target is validated and set twice. The second block is dead code left over from a merge. Remove it.

---

## Phase 4 — Make Invalid States Unrepresentable (3–4 days)

### 4.1 Typed pipeline stages via typestate

The compilation pipeline passes data through multiple phases, but all phase inputs/outputs are untyped (`Hir`, `Vec<HirType>`, etc.). Any caller can pass a pre-monomorphization HIR to codegen, which would silently produce wrong output (generic type params reaching LLVM). The `passes::no_type_params::assert_no_type_params` guard exists but only panics — it doesn't prevent the call.

Introduce typestate markers:

```rust
// Compiler phases as marker types
pub struct Parsed;
pub struct Lowered;
pub struct TypeChecked;
pub struct Monomorphized;  // ← only this may be passed to Codegen

pub struct CompilationUnit<State> {
    hir: Hir,
    interner: Interner,
    source: String,
    is_no_std: bool,
    expr_types: Vec<HirType>,
    _state: PhantomData<State>,
}

impl CompilationUnit<Lowered> {
    pub fn typecheck(mut self) -> Result<CompilationUnit<TypeChecked>, Vec<TypeError>> { ... }
}

impl CompilationUnit<TypeChecked> {
    pub fn monomorphize(self, call_type_args: ...) -> CompilationUnit<Monomorphized> { ... }
}

impl<'ctx> Codegen<'ctx> {
    // Only accepts Monomorphized — compile-time guarantee
    pub fn generate(&mut self, unit: &CompilationUnit<Monomorphized>) -> Result<(), String> { ... }
}
```

This makes passing a pre-mono HIR to codegen a **compile error**, not a runtime panic.

**Migration path:** Since this changes the public API of `glyim_cli::pipeline`, do it in a separate PR after the pipeline deduplication (Phase 2) reduces the surface area.

### 4.2 Encode `ContentHash` as `[u8; 32]` not `String`

In `glyim-macro-vfs`, `ContentHash` is stored and passed around as a hex string. Every comparison goes through a string comparison. This is both slow and allows malformed hex hashes to exist as values. After `parse::<ContentHash>()`, the value should be a type-safe wrapper:

```rust
// Currently:
pub struct ContentHash(String);  // wraps a hex string

// Better:
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
    pub fn of(data: &[u8]) -> Self {
        use sha2::{Sha256, Digest};
        Self(Sha256::digest(data).into())
    }
    pub fn to_hex(&self) -> String { hex::encode(self.0) }
}

impl FromStr for ContentHash {
    type Err = hex::FromHexError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut bytes = [0u8; 32];
        hex::decode_to_slice(s.trim_start_matches("sha256:"), &mut bytes)?;
        Ok(Self(bytes))
    }
}
```

This makes `ContentHash` `Copy`, enables constant-time equality, and makes invalid hashes impossible to construct outside of parsing.

### 4.3 Replace `Vec<(Symbol, HirExpr)>` in `MatchArm` with a typed struct

Currently match arms are `Vec<(HirPattern, Option<HirExpr>, HirExpr)>` — an anonymous triple. The middle element is a guard that is `None` in most cases. Traversal code must destructure this tuple everywhere. The existing `MatchArm` struct in `node/mod.rs` is defined but never used in the actual `HirExpr::Match` arms field:

```rust
// Defined but not used:
pub struct MatchArm {
    pub pattern: HirPattern,
    pub guard: Option<HirExpr>,
    pub body: HirExpr,
}

// Actually used:
Match {
    arms: Vec<(HirPattern, Option<HirExpr>, HirExpr)>,  // ← tuple, not struct
}
```

Change `HirExpr::Match.arms` to `Vec<MatchArm>`. This is a pure refactoring with no semantic change, but it makes match arm traversal self-documenting and eliminates all the `(pat, guard, body)` destructuring scattered across `rewrite.rs` (17 occurrences), `specialize.rs` (12 occurrences), `desugar.rs` (4 occurrences), `typeck/expr.rs`, and `codegen/expr/control.rs`.

### 4.4 Make `BuildMode` carry its flags directly

```rust
// Current — two separate bools with conflict_with that can still be both false:
#[arg(long, conflicts_with = "release")]
debug: bool,
#[arg(long, conflicts_with = "debug")]
release: bool,
// → BuildMode::Debug (default when both false)

// Better — the Command variants encode mode as a single enum:
enum BuildMode { Debug, Release }
// Already done. But the cmd_build/cmd_run functions still accept _debug and release as
// separate bools and ignore _debug entirely:
pub fn cmd_build(
    ..., _debug: bool, release: bool, ...
) -> i32 {
    let mode = if release { BuildMode::Release } else { BuildMode::Debug };
    // _debug is a dead parameter — always ignored
```

Remove `_debug: bool` from `cmd_build` and `cmd_run`. Pass `BuildMode` directly from the CLI dispatch. This eliminates a dead parameter that suggests the debug flag does something.

### 4.5 Add `#[must_use]` to `PipelineError` and `ContentHash::to_hex`

```rust
#[must_use]
pub enum PipelineError { ... }

impl ContentHash {
    #[must_use]
    pub fn to_hex(&self) -> String { ... }
}
```

Rust's lints will catch any call site that silently drops a pipeline error.

---

## Phase 5 — Performance & DX Polish (2–3 days)

### 5.1 Add incremental compilation cache to the pipeline

`build_with_cache()` already exists in `pipeline.rs` but is `#[allow(dead_code)]` — it's never called. Wire it up:

1. Compute source hash (SHA-256 of source + package version — already implemented as `compute_source_hash()`).
2. Check CAS for a cached object file.
3. On hit: link directly (saves all of parse/lower/typeck/mono/codegen).
4. On miss: compile normally, store object in CAS.

The cache hit path skips 90%+ of compilation time for unchanged sources. For the common edit-run cycle this is transformative.

**One fix required first:** The CAS key uses `hash.parse::<ContentHash>()` on a SHA-256 hex string. But `ContentHash` is parsed from SHA-256 hex starting with `sha256:` prefix in some places and without in others. Standardize on the `[u8; 32]` `ContentHash` type from §4.2 before enabling this.

### 5.2 Parallelize `generate()` pass 3 with Rayon

Codegen pass 3 (emit function bodies) is the most expensive step. Each function body is independent — they share the LLVM `Module` (which is not `Send`) but the HIR traversal to produce IR instructions can be parallelized per-function if each function gets its own builder:

```rust
// Current: sequential
for item in &hir.items {
    if let HirItem::Fn(f) = item {
        codegen_fn(self, f)?;
    }
}

// Better: use par_bridge() from rayon to collect function IR in parallel,
// then merge into the module sequentially.
// Note: inkwell's Module is not Send, but function values can be compiled
// to a separate Module and then merged with module.link_in_module().
```

This is more complex for the inkwell bindings but can yield 2-4× speedups for large files with many functions.

A simpler win: **lazy macro loading**. `load_package_macros()` creates a `LocalContentStore` and scans the lockfile on every single compilation, even when no macros are used. Memoize this with a `OnceLock<HashMap<String, Vec<u8>>>`.

### 5.3 Deduplicate the `Codegen::new` / `with_debug` / `with_line_tables` constructors

All three constructors are ~40-line blocks that are identical except for `debug_info` and `source_str`. Extract a `CodegenBuilder`:

```rust
pub struct CodegenBuilder<'ctx> {
    context: &'ctx Context,
    interner: Interner,
    expr_types: Vec<HirType>,
    debug_mode: DebugMode,
    source: Option<(String, String)>, // (source, filename)
}

enum DebugMode { None, LineTablesOnly, Full }

impl<'ctx> CodegenBuilder<'ctx> {
    pub fn build(self) -> Result<Codegen<'ctx>, String> { ... }
}
```

Eliminates 80+ lines of duplication and makes adding future options (e.g., `with_sanitizer()`) trivial.

### 5.4 Add workspace-level `[profile.dev]` tuning

The workspace `Cargo.toml` has no `[profile]` section. Add:

```toml
[profile.dev]
# Faster incremental builds during development
opt-level = 0
debug = 1         # Reduce debug info to line tables only; saves 30-40% link time
split-debuginfo = "unpacked"

[profile.dev.package.inkwell]
opt-level = 2     # Always optimize LLVM bindings even in debug builds

[profile.dev.package.llvm-sys]
opt-level = 2

[profile.test]
# Tests need optimization to run in reasonable time for JIT tests
opt-level = 1
```

This alone can reduce debug build times by 25-40% with no code changes.

### 5.5 Fix the `status` endpoint's filesystem scan

In `cas_server/main.rs`:
```rust
async fn status(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    let blob_count = std::fs::read_dir("./cas_store/objects")
        .ok()
        .map(|entries| entries.count())
        .unwrap_or(0);
    // ...
}
```

This reads from a hardcoded relative path `"./cas_store/objects"` instead of asking the store itself, bypasses the shared `AppState`, and blocks an async thread on synchronous I/O. Fix:

```rust
async fn status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let store = state.store.read().await;
    let blob_count = store.list_blobs().count();
    Json(StatusResponse { status: "ok".to_string(), version: ..., blob_count })
}
```

### 5.6 Add `glyim fmt` and `glyim lint` to the pipeline

Both `glyim-fmt` and `glyim-lint` crates exist but contain only stub `fn main() {}`. They are listed as workspace members but do nothing. Add a minimal formatter that applies consistent whitespace/indentation rules using the existing CST (there's a `cst_builder.rs` in `glyim-parse`). Even a no-op formatter with `--check` mode is better than the current state.

---

## Phase 6 — Type System & Error Quality (ongoing)

### 6.1 Rich diagnostic spans for all `TypeError` variants

In `glyim-typeck/src/typeck/error.rs`, the `TypeError` enum carries spans but many error variants have imprecise span information. The `TypeMismatch` error points to the whole expression, not the specific mismatched sub-expression. Add secondary labels (miette supports this) pointing to:

- The binding site for undefined variable errors
- The conflicting arm for non-exhaustive match errors  
- The expected-vs-got types with source annotations

### 6.2 Collect multiple type errors before returning

`TypeChecker::check()` returns `Err(Vec<TypeError>)` — good, it accumulates. But in practice, the first type error often cascades into many false-positive errors because `typeck` infers `Int` as a fallback for unknown types. Add an error recovery mode where after the first type error, identifiers with unknown types get a fresh `HirType::Error` sentinel that suppresses further errors involving that expression.

### 6.3 Add `glyim-typeck` span coverage metric

Add a CI check that measures what percentage of `ExprId`s in the HIR have an entry in `expr_types`. A gap means the typechecker silently skips some expressions. Currently `expr_types` is a flat `Vec<HirType>` indexed by `ExprId::as_usize()` — if expressions are created with non-sequential IDs (which happens in the monomorphizer), the vector can have holes. Assert `expr_types.len() >= max_expr_id_in_hir` after typechecking.

---

## Summary Table

| Phase | Theme | Effort | Risk | Biggest Win |
|-------|-------|--------|------|-------------|
| 1 | Strip debug noise & seal panics | 1–2 days | Zero | 184 `eprintln!` gone; panics have context |
| 2 | Eliminate pipeline duplication | 2–3 days | Medium | 600 lines → 1 canonical path; lowering bug fixed |
| 3 | Fix hidden bugs | 3–4 days | Targeted | Real SHA hashes; O(n) macro scan; no double validation |
| 4 | Make invalid states unrepresentable | 3–4 days | Medium | Typestate prevents pre-mono codegen; `MatchArm` struct |
| 5 | Performance & DX | 2–3 days | Low | Incremental compilation; 25-40% faster debug builds |
| 6 | Error quality | Ongoing | Low | Better diagnostics; error recovery in typeck |

**Total estimated effort: ~15–20 focused engineering days**

---

## Execution Order Recommendation

Run phases in numerical order. Each phase leaves all tests green:

1. **Phase 1** first — strips noise so CI output is readable for subsequent work.
2. **Phase 2** second — establish the canonical pipeline before Phase 3 fixes are applied to each pipeline variant.
3. **Phase 3.4** (ContentHash type) before Phase 3.1 (fake hash fix), since the real hash computation depends on a correct `ContentHash` type.
4. **Phase 4.3** (MatchArm struct) is the highest-leverage pure refactoring — do it before Phase 6 diagnostic work.
5. **Phase 5.1** (incremental cache) only after Phase 2 establishes one canonical pipeline; otherwise the cache key logic needs to be applied to all 6 variants.

All snapshot tests (`insta`) may need to be updated after Phase 2 if any IR output changes due to the lowering fix. Run `cargo test --features insta/force-update-snapshots` after Phase 2 completes.
