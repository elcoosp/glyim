# UCMS Vertical Sliced Implementation Plan – Fully Fledged

**Goal:** Deliver working, user-visible `comptime fn` functionality every 2 days, with maximum parallelism (10 agents).  
**Serialization:** Only `postcard`. No `bincode`.  
**Locked contracts:** All respected – new code is additive (new crates, new modules, no breaking changes).

---

## Sprint 0: Foundation (Days 1–2)

**Vertical Slice:** A `comptime fn` can be parsed, type-checked as a regular function, and the CVM can evaluate a constant expression like `42` in a test.

### Task Decomposition

| ID | Agent | Crate | Task Description | Inputs | Outputs | Dependencies | Est. Hours |
|----|-------|-------|------------------|--------|---------|--------------|-------------|
| S0-T1 | A1 | `glyim-span` | Add stable `ExpnId` based on `hash((crate_id, file_path, line, col, parent))`; add `Span::with_chain(marks: &[Mark]) -> Span`; add `call_stack_formatter` | `HygieneCtx` | `ExpnId::call_stack()`, `Span::with_chain`, formatter | None | 6 |
| S0-T2 | A2 | `glyim-syntax` | Define `TokenStream = Vec<(SyntaxKind, Arc<str>, Mark)>`; implement `concat()`, `from_str()`, `to_string()` | None | `pub mod token_stream` | S0-T1 (Mark) | 6 |
| S0-T3 | A3 | `glyim-cvm` (new) | Create crate, define `CvmValue` enum (Int, Uint, Bool, String, Type, TokenStream, List, Tuple). Implement `List<T>` with `len`, `get`, `push`, `map`, `fold`, `IntoIterator`. | None | `CvmValue`, `List` | S0-T2 (TokenStream) | 8 |
| S0-T4 | A3 | `glyim-cvm` | Implement basic interpreter: evaluate HIR `Expr` for literals, variables (environment), `if`, `match`, `for` over `List`. Step budget (1e6). | `&Expr`, `Env` | `Result<CvmValue, CvmError>` | S0-T3 | 10 |
| S0-T5 | A7 | `glyim-type` | Add `fingerprint: u128` field to `TyCtxMut`. Update on every `alloc_ty`, `new_ty_var`, `new_int_var`, `new_float_var`, `new_region_var` using `xxhash128` of operation + args. Expose `pub fn fingerprint(&self) -> u128`. | `TyCtxMut` | `fingerprint` method | None | 4 |
| S0-T6 | A7 | `glyim-type` | Add `type_vars_created: usize` counter, increment on each `new_ty_var`, `new_int_var`, `new_float_var`. Expose `pub fn total_type_vars_created(&self) -> usize`. | `TyCtxMut` | counter and getter | None | 2 |
| S0-T7 | A8 | `glyim-solve` | Add `pub fn unresolved_type_vars(&self, ctx: &TyCtx) -> usize` to `InferenceTable` (counts `TyVar` with `value == None`). | `InferenceTable` | `unresolved_type_vars` | S0-T6 (counter) | 4 |
| S0-T8 | A8 | `glyim-solve` | Add `pub fn total_created(&self, ctx: &TyCtx) -> usize` (reads `ctx.total_type_vars_created()`). | `InferenceTable` | `total_created` | S0-T6 | 2 |

**Integration (end of day 2):**  
- Merge all branches into `ucms-sprint0`.  
- Test: `comptime fn five() -> i32 { 42 }` can be parsed, type-checked (as a regular function), and the CVM can evaluate `42` in a unit test.

**Parallelism:** All agents work fully in parallel. A3 waits for S0-T2 but can start after day 1.

---

## Sprint 1: First Expansion (Days 3–4)

**Vertical Slice:** A `comptime fn` can be called at compile time, returns a constant, and the driver splices that constant into the AST. No cache yet.

### Task Decomposition

| ID | Agent | Crate | Task Description | Inputs | Outputs | Dependencies | Est. Hours |
|----|-------|-------|------------------|--------|---------|--------------|-------------|
| S1-T1 | A1 | `glyim-pipeline` | Create `expansion` module. Implement `ExpansionDriver` skeleton: worklist `Vec<(SyntaxNode, DefId)>`, cycle detection stack `HashSet<(DefId, u128)>`, measure `(M1, M2)` where `M1 = cst.node_count()`, `M2 = unresolved_type_vars()`. | `CST`, `TyCtx` | Driver struct with `push_work`, `pop_work`, `is_cycle` | S0-T1, S0-T7 | 8 |
| S1-T2 | A6 | `glyim-frontend` | Implement `parse_token_stream_fragment(ts: &TokenStream, file_id: FileId) -> Result<SyntaxNode, (usize, String)>`. Accept `Expr`, `Item`, `Stmt` as root. Use existing `parse_to_syntax` with a modified root kind. | `TokenStream` | `SyntaxNode` or error index+msg | S0-T2 | 6 |
| S1-T3 | A9 | `glyim-pipeline` | Implement `splice(original_cst: &mut SyntaxNode, macro_call_node: SyntaxNode, fragment_ts: TokenStream) -> Result<Range<usize>, SplicingError>`. Parse via S1-T2, replace node, record byte range from `Span`. | CST, call node, token stream | `Range<usize>` | S1-T2 | 6 |
| S1-T4 | A4 | `glyim-cvm` | Implement `type_name(ty: Ty) -> String` intrinsic. Use `PrintTy` from `glyim_type`. | `Ty` | `String` | S0-T3, S0-T5 | 4 |
| S1-T5 | A10 | `glyim-cache` | Create crate. Implement `FreshnessStore`: load/save `freshness.json` (map: u64 → u64) with atomic write (temp + rename). `fn next(&mut self, path_hash: u64) -> u64`. | Path hash | counter | None | 4 |
| S1-T6 | A3 | `glyim-cvm` | Add `__quote` intrinsic (hidden): takes `Vec<(SyntaxKind, String)>`, returns `TokenStream` stamped with `current_mark`. Store `current_mark` in CVM context. | parts | `TokenStream` | S0-T2, S0-T1 | 4 |
| S1-T7 | A2 | `glyim-syntax` | Implement `quote!` macro as a built‑in that expands to `__quote` call. Parse `quote! { ... }` and replace `#var` with token stream of `var`. | source | `TokenStream` | S1-T6 | 6 |
| S1-T8 | A5 | `glyim-cvm` | Implement `emit_diagnostic(span: Span, msg: String, level: u8)` – forwards to `DiagSink`. Level: 0=error,1=warning,2=note,3=help. | args | `()` | S0-T3, S0-T1 | 3 |
| S1-T9 | A1 | `glyim-pipeline` | Integrate driver with CVM and splicer: evaluate a macro call, splice result, update measure, loop until fixed point. | `CST`, `TyCtx` | Expanded `CST` | S1-T1, S1-T3, S0-T4 | 8 |

**Integration (end of day 4):**  
- Merge all.  
- Test:  
  ```glyim
  comptime fn five() -> i32 { 42 }
  fn main() { let x = five!(); assert_eq!(x, 42); }
  ```
  Compiles, expands `five!()` to `42`, type-checks, runs.

**Parallelism:** A1, A6, A9, A4, A10, A3, A2, A5 all work in parallel. A9 depends on S1-T2 (A6). A2 depends on S1-T6 (A3). A1 depends on S1-T3 and S0-T4.

---

## Sprint 2: Code Generation & Hygiene (Days 5–6)

**Vertical Slice:** `quote!` works; macros can generate code with hygiene; spliced code is type-checked and merged.

### Task Decomposition

| ID | Agent | Crate | Task Description | Inputs | Outputs | Dependencies | Est. Hours |
|----|-------|-------|------------------|--------|---------|--------------|-------------|
| S2-T1 | A6 | `glyim-frontend` | Extend fragment parser to accept `Item` and `Stmt` as roots (already in S1-T2, but ensure full grammar). Add error recovery: token window (10 before, 5 after) in error message. | `TokenStream` | `SyntaxNode` or rich error | S1-T2 | 4 |
| S2-T2 | A9 | `glyim-typeck` | Implement `retype_new_nodes(cst: &SyntaxNode, old_ctx: &TyCtxMut, range: Range<usize>, macro_env: &HashMap<Name, Ty>) -> Result<TyCtxMut, Vec<GlyimDiagnostic>>`. Steps: extract fragment, create fresh `TyCtxMut` snapshot, type-check fragment, map variables, emit equality constraints, unify. | new nodes, old ctx, range, env | merged `TyCtxMut` | S0-T5, S0-T6, S0-T7 | 12 |
| S2-T3 | A7 | `glyim-type` | Implement COW snapshot: change `TyCtxMut` internals to use `im::HashMap` for interned types and `im::Vector` for inference vars. `pub fn snapshot(&self) -> TyCtxMut` returns new context sharing data via `Arc`. | `TyCtxMut` | `TyCtxMut` snapshot | S0-T5 | 8 |
| S2-T4 | A8 | `glyim-solve` | Add helper `pub fn unify_fresh_with_original(fresh_table: &InferenceTable, original_table: &mut InferenceTable, ctx: &mut TyCtxMut, mapping: &HashMap<TyVar, TyVar>) -> Result<(), Vec<GlyimDiagnostic>>`. | two tables, mapping | unification result | S0-T7 | 4 |
| S2-T5 | A3 | `glyim-cvm` | Add inspection intrinsics: `token_stream_len`, `token_stream_get`, `token_tree_is_group`, `token_tree_as_group`, `token_tree_is_token`, `token_tree_as_token`, `token_kind_is_punct`, `stringify_token_stream`. | `TokenStream` etc. | various | S0-T2 | 8 |
| S2-T6 | A5 | `glyim-cvm` | Implement `parse_token_stream` intrinsic (calls A6’s parser). If error, returns `Err(msg_with_offset)`. | `String` | `Result<TokenStream, String>` | S2-T1 | 4 |
| S2-T7 | A9 | `glyim-pipeline` | Integrate `retype_new_nodes` into driver after splicing. Add `macro_env` capture: when calling a `comptime fn`, capture generic parameters from call site and pass as environment. | driver output | type‑checked CST | S2-T2 | 4 |

**Integration (end of day 6):**  
- Test:  
  ```glyim
  comptime fn make_add() -> TokenStream {
      quote! { fn add(x: i32, y: i32) -> i32 { x + y } }
  }
  make_add!();
  fn main() { assert_eq!(add(2, 3), 5); }
  ```
  Generates function, type-checks, works.

**Parallelism:** A6 (fragment parser), A9 (retype), A7 (COW), A8 (unify helper), A3 (inspection), A5 (parse intrinsic) all parallel. A9 depends on A7 and A8. A5 depends on A6.

---

## Sprint 3: Persistent Caching & Freshness (Days 7–8)

**Vertical Slice:** Expansions are cached across compilations; `fresh_name` and `fresh_type_var` produce deterministic names; `--clear-cache` flag works.

### Task Decomposition

| ID | Agent | Crate | Task Description | Inputs | Outputs | Dependencies | Est. Hours |
|----|-------|-------|------------------|--------|---------|--------------|-------------|
| S3-T1 | A10 | `glyim-cache` | Implement `ComptimeCache` using `postcard` serialization. Cache key: `(DefId, arg_hash: u128, ty_ctx_fingerprint: u128, capability_mask: u64, mir_hash: u128)`. `arg_hash` = xxhash128 of postcard-serialized `CvmValue` args. On-disk: `target/.comptime_cache/entries/hex(key)`. Store `(output_ast: SyntaxNode, counter_value: u64)`. | macro call info | cache entry | S0-T2, S0-T5, S1-T5 | 10 |
| S3-T2 | A10 | `glyim-cache` | Implement `fn get(&self, key) -> Option<CacheEntry>`, `fn insert(&mut self, key, entry)`, `fn clear()`, `fn dump(path: &Path)` (human-readable table). | cache operations | methods | S3-T1 | 4 |
| S3-T3 | A1 | `glyim-pipeline` | Integrate cache into `ExpansionDriver`: on each macro call, compute key, check cache; on hit, reuse output AST and set counter; on miss, evaluate, splice, type-merge, then cache. | driver | cached expansions | S3-T1 | 6 |
| S3-T4 | A5 | `glyim-cvm` | Implement `fresh_name(prefix: String) -> String` intrinsic: compute current `ExpnId` chain hash, call `FreshnessStore::next()`, return `format!("{}_{}_{}", prefix, path_hash, counter)`. | prefix | unique name | S1-T5, S0-T1 | 4 |
| S3-T5 | A5 | `glyim-cvm` | Implement `fresh_type_var() -> Type` intrinsic: call `TyCtxMut::new_ty_var()` and return `Ty` handle. | none | `Type` | S0-T6 | 2 |
| S3-T6 | A7 | `glyim-type` | Add rolling hash for CST: extend `SyntaxNode` with `hash: u128` field (stored in green node). Provide `fn update_hash(node: &mut SyntaxNode)` that recomputes hash from children and kind. On splicing, update ancestors. | `SyntaxNode` | `hash()` method | S0-T2 | 6 |
| S3-T7 | A8 | `glyim-solve` | Add `pub fn unresolved_region_vars(&self) -> usize` to `InferenceTable`. | none | count | S0-T7 | 2 |
| S3-T8 | A1 | `glyim-cli` | Add `--clear-cache` and `--dump-cache` flags. Implement in `main()` to call cache methods. | CLI args | cache management | S3-T2 | 2 |

**Integration (end of day 8):**  
- Recompile a macro that uses `fresh_name("tmp")` – same names generated. Second compilation hits cache, no re-evaluation.  
- `glyip build --clear-cache` deletes cache directory.  
- Test: `--dump-cache` prints cache entries.

**Parallelism:** A10 (cache), A1 (driver integration), A5 (fresh intrinsics), A7 (CST rolling hash), A8 (region vars), A1 (CLI) all parallel. A1 depends on A10.

---

## Sprint 4: Capabilities & Advanced Queries (Days 9–10)

**Vertical Slice:** Capability system enforces sandboxing; all type query intrinsics available; macros can inspect generic arguments and create new generic parameters.

### Task Decomposition

| ID | Agent | Crate | Task Description | Inputs | Outputs | Dependencies | Est. Hours |
|----|-------|-------|------------------|--------|---------|--------------|-------------|
| S4-T1 | A7 | `glyim-hir` | Extend `FnItem` with `capabilities: CapabilitySet` (bitflags). Parse `#[comptime(capabilities = "fs, env")]` attribute in `glyim_frontend` lowering. | attribute | HIR field | None | 4 |
| S4-T2 | A7 | `glyim-hir` | Implement static capability propagation: compute transitive closure for each `comptime fn` by walking HIR (or MIR). Report error if caller’s caps not superset of callee’s. | HIR | diagnostics | S4-T1 | 6 |
| S4-T3 | A5 | `glyim-cvm` | Add capability mask to CVM context. Before each intrinsic, check if required capability is present; if not, call `compile_error`. | intrinsic call | enforcement | S4-T1 | 2 |
| S4-T4 | A4 | `glyim-cvm` | Implement all remaining type intrinsics: `type_fields`, `type_is_copy`, `type_is_sized`, `type_is_enum`, `type_variants`, `type_generic_args`. Use `TyCtx` methods. | `Ty` | various | S0-T5 | 8 |
| S4-T5 | A4 | `glyim-cvm` | Implement `type_generic_args` returning `List<GenericArg>` where `GenericArg` is a CVM enum with `Ty` and `Lifetime`. Add `is_lifetime`, `as_lifetime`. | `Ty` | `List<GenericArg>` | S0-T5 | 4 |
| S4-T6 | A3 | `glyim-cvm` | Implement `new_generic_param(name: String) -> Type` intrinsic. Creates a new `BoundTy` (generic parameter) in `TyCtxMut`. Requires capability `generics`. | name | `Type` | S0-T5 | 4 |
| S4-T7 | A9 | `glyim-typeck` | Extend `retype_new_nodes` to handle new generic parameters: when encountering a `ParamTy` not in `macro_env`, create a new `BoundTy` via `new_generic_param` and map it. | fragment, env | merged ctx | S4-T6, S2-T2 | 6 |
| S4-T8 | A8 | `glyim-solve` | Add `pub fn instantiate_binder_with_placeholders` for HRTB (needed for some type queries). Reuse existing `glyim_solve::hrtb` module. | binder | placeholder instantiation | S0-T7 | 4 |

**Integration (end of day 10):**  
- Test:  
  ```glyim
  #[comptime(capabilities = "fs")]
  fn read_file(path: String) -> String { ... }
  // calling without fs fails
  ```
- Test:  
  ```glyim
  comptime fn generics_example() {
      let t = new_generic_param("T");
      quote! { struct MyStruct<T> { field: T } }
  }
  ```

**Parallelism:** A7 (capabilities), A5 (enforcement), A4 (type queries), A3 (new_generic_param), A9 (retype extension), A8 (HRTB) all parallel. A9 depends on A4 and A6? No, A9 depends on S4-T6.

---

## Sprint 5: Debugging & Observability (Days 11–12)

**Vertical Slice:** `--step-macros`, `--log-macros-json`, `--macro-stats` work; stall detection prevents infinite loops; error messages show macro call stack.

### Task Decomposition

| ID | Agent | Crate | Task Description | Inputs | Outputs | Dependencies | Est. Hours |
|----|-------|-------|------------------|--------|---------|--------------|-------------|
| S5-T1 | A1 | `glyim-pipeline` | Implement stall detection: after each iteration, compare `(M1, M2)` and CST hash (via S3-T6). If `M1` unchanged and `M2 >= previous M2` for 3 consecutive iterations, emit warning. | driver state | warning diagnostic | S3-T6 | 4 |
| S5-T2 | A1 | `glyim-pipeline` | Add `--step-macros` interactive mode: modify CVM `eval` to yield after each HIR statement, print source line (from `Span`), wait for input (`c`/`s`/`p`/`q`). Implement in driver. | CLI flag | step‑through | S0-T4 | 8 |
| S5-T3 | A1 | `glyim-pipeline` | Add `--log-macros-json`: write events (phase, macro name, def_id, span, duration_ms, cache_hit, generated_nodes) to `macro_log.json` using `serde_json`. | driver events | JSON file | None | 4 |
| S5-T4 | A9 | `glyim-pipeline` | Add `--macro-stats`: collect counts (total expansions, cache hits, cache misses, time in CVM, largest expansion, unused fresh type vars) and print at end of compilation. | driver metrics | terminal output | S5-T3 | 4 |
| S5-T5 | A6 | `glyim-frontend` | Improve error messages in fragment parser: include token window (10 before, 5 after), macro call stack from marks. Use `call_stack_formatter` from S0-T1. | parse error | rich diagnostic | S0-T1 | 4 |
| S5-T6 | A1 | `glyim-pipeline` | Integrate improved errors into driver: when splicing fails, emit diagnostic with token window and call stack. | splicing error | diagnostic | S5-T5 | 2 |
| S5-T7 | A10 | `glyim-cache` | Add `--dump-cache` pretty printer: print table with key (truncated), hit count, last access time. | cache file | human-readable | S3-T2 | 2 |

**Integration (end of day 12):**  
- Run `glyim --step-macros test.g` – can step through macro.  
- Run `glyim --log-macros-json test.g` – produces `macro_log.json`.  
- Run `glyim --macro-stats test.g` – prints summary.  
- Write a macro that stalls – emits warning.

**Parallelism:** A1 (stall, step, JSON log), A9 (stats), A6 (error DX), A10 (dump-cache) all parallel.

---

## Sprint 6: Testing & Stabilisation (Days 13–14)

**Vertical Slice:** All existing `macro_rules!` tests pass with `comptime fn`; cache persistence works across builds; performance benchmarks.

### Task Decomposition

| ID | Agent | Crate | Task Description | Inputs | Outputs | Dependencies | Est. Hours |
|----|-------|-------|------------------|--------|---------|--------------|-------------|
| S6-T1 | A3 | `glyim-cvm` | Write unit tests for each intrinsic: `type_name`, `type_fields`, `fresh_name`, `emit_diagnostic`, `parse_token_stream`, etc. Include edge cases (empty list, error types). | intrinsics | test suite | All intrinsics | 6 |
| S6-T2 | A4 | `glyim-cvm` | Write unit tests for type query intrinsics with various types (structs, enums, generics). | types | test suite | S4-T4 | 4 |
| S6-T3 | A5 | `glyim-cvm` | Write unit tests for I/O and freshness intrinsics (use temp files, mock env). | I/O | test suite | S3-T4, S3-T5 | 4 |
| S6-T4 | A10 | `glyim-cache` | Write integration tests: compile a macro, delete `target/`, recompile, verify cache hit (no re‑evaluation). Test `--clear-cache`. | cache | tests | S3-T1 | 4 |
| S6-T5 | All | `glyim-test` | Convert all existing `macro_rules!` tests to `comptime fn`. Update test harness to use new expander. | old tests | migrated tests | All features | 12 |
| S6-T6 | A1 | `glyim-pipeline` | Run full benchmark suite: measure expansion time for large macro-generated code (e.g., 10k lines). Compare pre/post cache. | macro | performance report | S3-T3 | 4 |
| S6-T7 | A9 | `glyim-typeck` | Validate that unused fresh type vars emit warning. Add test. | `fresh_type_var` unused | warning | S3-T5 | 2 |
| S6-T8 | A1 | `glyim-cli` | Deprecate old `--macro-expand` flag; point to new system. Remove `glyim-meta::Expander` from default pipeline. | CLI | removed code | All | 2 |

**Integration (end of day 14):**  
- All tests pass.  
- `macro_rules!` no longer supported (compiler errors if used).  
- Cache works across incremental rebuilds.  
- Performance report shows 80%+ cache hit rate for typical workloads.

**Parallelism:** A3, A4, A5 (intrinsic unit tests) in parallel; A10 (cache tests); All agents for test migration (split test files by module). A1 and A9 for final integration.

---

## Gantt Chart (10 agents, 14 days)

```
Day:   1 2 3 4 5 6 7 8 9 10 11 12 13 14
Sprint 0 → S1 → S2 → S3 → S4 → S5 → S6

A1: ██[0] ██[1] ░░ ██[3] ░░ ██[5] ░░
A2: ██[0] ██[1] ░░ ░░    ░░    ░░
A3: ██[0] ██[1] ██[2] ░░ ██[4] ░░ ██[6]
A4: ░░    ██[1] ░░    ██[4] ░░ ██[6]
A5: ░░    ██[1] ██[2] ██[3] ██[4] ░░ ██[6]
A6: ░░    ██[1] ██[2] ░░    ██[5] ░░
A7: ██[0] ░░    ██[2] ██[3] ██[4] ░░
A8: ██[0] ░░    ██[2] ░░    ██[4] ░░
A9: ░░    ██[1] ██[2] ░░    ██[4] ██[5] ░░ ██[6]
A10: ░░   ██[1] ░░    ██[3] ░░    ██[5] ██[6]
```

**Legend:** Sprint number in brackets. `░` = idle/integration.

---

## Summary of Deliverables per Sprint

| Sprint | Feature | User‑Visible |
|--------|---------|---------------|
| 0 | Parse + evaluate constants | `comptime fn` compiles as regular function |
| 1 | First expansion | `five!()` expands to `42` |
| 2 | Code generation with `quote!` | `make_add!()` generates new function |
| 3 | Caching + fresh names | Fast recompilation, deterministic names |
| 4 | Capabilities + full type queries | Sandboxed macros, type introspection |
| 5 | Debugging | Step‑through, logs, stats |
| 6 | Production‑ready | All tests pass, old macros removed |

**Total calendar time:** 14 days (3 weeks).  
**Total person‑days:** ~180 (10 agents × 14 days × 0.5 efficiency for coordination).  
**All deliverables use `postcard` serialization, no `bincode`.**  
**Ready to execute sprint by sprint.**
