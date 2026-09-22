## TIER 6 — LSP polish (`glyim-lsp`)

All of these are quality-of-life gaps, not correctness bugs (a wrong
completion/rename suggestion is recoverable — the user sees it before
accepting). Prioritize accordingly relative to Tiers 0-3.

### 6.1 `reference_graph.rs::build_from_hir` misses `Range`/`Closure` (and likely other) expression forms

Before writing any fix, run:
```
grep -n "fn build_from_hir" -A 200 glyim-lsp/src/reference_graph.rs | grep -n "ExprKind::\|Expr::"
```
and diff the set of `Expr`/`ExprKind` variants it matches against the full
variant list in `glyim-hir/src/lib.rs`'s `pub enum Expr` (confirmed
variants, from this exploration: `Missing, Path, Literal, Block, If, While,
Loop, For, Match, Call, MethodCall, Field, Index, Unary, Binary, Cast, Ref,
Assign, Return, Break, Continue, Closure, Array, Tuple, Struct, Range, ...`
— there may be more after `Struct`, re-check the full enum before starting).
Any variant walked by typeck's own expression traversal (`check_expr.rs`'s
big `match &self.body.exprs[expr_id]` is the canonical "visit every kind"
reference) but *not* matched in `build_from_hir` is a miss. Add the missing
arms, recursing into every child `ExprId`/`PatId` exactly the way
`check_expr.rs` does structurally (walk `Closure { params, body }` into
both `params` patterns and `body`; walk `Range { start, end, .. }` into
both optional sides) — don't invent a different traversal shape than the
one typeck already uses for the same tree, or the two will silently drift
apart again next time a new `Expr` variant is added.

**Verify:** a reference-count/goto-references test on a variable used only
inside `1..x` (a `Range`'s `end`) and only inside a closure body — both
must be found; today (per the report) they're silently skipped, which is a
correctness gap in "find all references" specifically, not a crash.

### 6.2 `find_references` — no read/write distinction

Add a `ReferenceKind { Read, Write }` to whatever struct
`find_references` currently returns (grep its return type), classified the
same way Tier 1.1's capture-mutability tracking does: a reference is
`Write` if it's the direct LHS of `Expr::Assign` or the operand of
`Expr::Ref { mutability: Mutability::Mut, .. }`, `Read` otherwise. This is
the same classification logic as Tier 1.1's `is_mut_use` — if Tier 1.1 is
done first, this can literally reuse `FnCtxt::capture_log`'s classification
approach applied at the LSP layer instead of writing it twice.

### 6.3 `rename.rs` — falls back to text-based single-file search when the reference graph has no entries

Once 6.1 is fixed (reference graph coverage is complete), this fallback
should trigger far less often — but it's still a legitimate fallback for
genuinely out-of-scope references (e.g. names in files the graph hasn't
indexed yet). The real gap worth fixing: the fallback's text search should
at minimum **skip matches inside string/char literals and comments** (a
naive text search renaming `x` will corrupt `"x is a variable"` string
literals otherwise) — check whether it already tokenizes before matching
or does a raw substring search; if raw substring, switch it to lex the
file first (`glyim-frontend`'s lexer is already exposed,
`glyim-frontend/src/lexer.rs`) and only replace `SyntaxKind::Ident` tokens
whose text matches, never inside `StringLit`/`CharLit`/comment tokens.

### 6.4 `completion.rs::provide_completions` — no type-based filtering

**Current:** only uses the symbol index (name-based), not filtered by
receiver type — offers every method in scope regardless of whether it
applies to the expression being completed.

**Fix:** when completion is triggered after `receiver.` (a method-call
completion context, detectable the same way `goto_definition.rs`/`hover.rs`
already detect "cursor is on a method-call receiver" — reuse that same
detection, don't write a third copy), resolve `receiver`'s type via
whatever typeck-result cache `glyim-lsp/src/database.rs` already holds
(it must have one, since `hover.rs` needs types for hover text — reuse
that exact query), then filter the symbol-index candidates to only methods
whose impl's `Self` type unifies with the receiver type, using the same
`resolve_method_call`-style impl search Tier 1.2.a's `ImplDef.items` now
makes efficient (`trait_ctx.impls_of_trait`/inherent-impl equivalent) —
this is a direct beneficiary of the Tier 1.2 schema change, do it after.

### 6.5 `code_action.rs::collect_unused_imports` — text-based, false positives/negatives

**Fix:** switch from text matching to using the already-built reference
graph (6.1) — an import is unused iff the imported name has zero
`Read`/`Write` references (6.2) anywhere in the file's resolved HIR, not
"the identifier doesn't appear as a text substring elsewhere" (which false-
positives on shadowed names and false-negatives on the import name
appearing only inside a string/comment). This is a natural third
beneficiary of 6.1/6.2 — do 6.1 and 6.2 first, this becomes a small
consumer of them rather than its own analysis.

**Verify (6.1-6.5 collectively):** a single LSP integration test file with:
an unused import, a variable read only inside a range/closure, a method
call needing type-filtered completion, and a rename target whose name also
appears inside a string literal — covers all five items in one fixture.

---

## TIER 7 — Test harness realism (`glyim-test`)

### 7.1 `PipelineCompiler::compile` — compiles to `test_output.o`, discards typeck/MIR results

**Current (confirmed, `harness/compiler.rs` line ~76):**
```rust
let output_path = std::path::Path::new("test_output.o");
// ...
match glyim_pipeline::Pipeline::compile_file(&mut db, &path, &*self.backend, output_path) {
    Ok(()) => CompileOutput { diagnostics: Vec::new(), syntax_tree: None, def_map: None,
        typeck_result: None, mir_bodies: Vec::new(), ty_ctx },
```
Two separate bugs here, fix both:

1. **Hardcoded `test_output.o` is a shared-mutable-state hazard** — any
   parallel test run (this harness almost certainly runs tests
   concurrently, check `harness/runner.rs`) will have every `PipelineCompiler`
   instance racing to write/read the same file. Fix:
```rust
let output_path = std::env::temp_dir().join(format!("glyim_test_{}_{}.o", file_id.to_raw(), std::process::id()));
```
   (or use the `tempfile` crate already in `glyim-test`'s... actually check:
   `tempfile` is in `glyip`'s `[dev-dependencies]`, confirm whether
   `glyim-test` has it too; if not add it — it's a natural fit here,
   `tempfile::NamedTempFile` avoids manual cleanup entirely).

2. **Results are discarded even on success.** `Pipeline::compile_file`
   must, internally, produce a `def_map`/typeck result/MIR bodies before it
   can emit an object file — check its signature/return type
   (`glyim-pipeline/src/lib.rs`) for whether it already returns these or
   only `Result<(), Vec<Diagnostic>>`. If it only returns `()`/diagnostics
   today, that's the real gap: **`Pipeline::compile_file` needs a richer
   return type** (or a companion method) that hands back the intermediate
   artifacts, e.g.:
```rust
pub struct CompileArtifacts {
    pub def_map: glyim_def_map::CrateDefMap,
    pub typeck_result: glyim_typeck::TypeckResult,
    pub mir_bodies: Vec<Arc<glyim_mir::Body>>,
}
pub fn compile_file_with_artifacts(db: &mut Database, path: &Path, backend: &dyn CodegenBackend, output: &Path)
    -> Result<CompileArtifacts, Vec<GlyimDiagnostic>>;
```
   implemented by having `compile_file` call through to this new function
   and just drop the artifacts (keeps the existing production entry point's
   behavior identical for `glyip`, which doesn't need the intermediate
   results). Then `PipelineCompiler::compile` calls the new
   `_with_artifacts` variant and actually populates `CompileOutput`'s
   `def_map`/`typeck_result`/`mir_bodies` fields on success, and (for the
   error path) still tries to salvage whatever partial artifacts the
   pipeline produced before failing, if `Pipeline` exposes them alongside
   diagnostics (many real compilers return best-effort partial results
   even on error, specifically so test/IDE tooling like this can inspect
   "how far did it get" — check whether that's already the shape of the
   `Err` variant before assuming you need to add it).

**Verify:** a `glyim-test` snapshot test that asserts on `CompileOutput.
mir_bodies` contents directly (today: always empty, so any such assertion
either can't exist or is vacuously checking `.is_empty()`) — after the fix,
assert the MIR body count/shape for a small fixture matches expectations.

### 7.2 `RunPassStrategy`/`RunFailStrategy` — `executable_path` always `None`

**Already-correct code, confirmed:** both strategies' `evaluate` functions
(`harness/strategy.rs`) fully and correctly implement "no exe → fail with a
clear reason" / "run it, check exit code/stdout/stderr" — this is not a
stub, it's finished code waiting on its one input. The entire fix is
upstream, in `PipelineCompiler` (7.1) and whoever calls `.evaluate(...)`:

1. `glyim-cli/src/lib.rs` currently has `mod linker;` (private) — change to
   `pub mod linker;` so `glyim-test` (and `glyip`, for Tier 3.2) can call
   `glyim_cli::linker::invoke_linker`.
2. Add `glyim-cli = { workspace = true }` to `glyim-test/Cargo.toml`.
3. In `PipelineCompiler::compile` (after 7.1's fix produces a real,
   non-colliding `output_path` object file), on success, link it:
```rust
let exe_path = output_path.with_extension(""); // or a dedicated temp path
let link_result = glyim_cli::linker::invoke_linker(&output_path, &exe_path, None, None);
```
4. Add `executable_path: Option<PathBuf>` to `CompileOutput` (`compiler.rs`,
   the struct at the top of the file), populated from step 3's `exe_path`
   only when linking succeeds.
5. Wherever `harness/runner.rs` currently calls
   `RunPassStrategy::evaluate(&output, source, /* None today */, config,
   timeout)`, change it to pass `output.executable_path.as_deref()`.

**Verify:** re-run any existing `run-pass`/`run-fail` UI-style test fixture
in `glyim-test`'s own test suite — today they must all be failing (or
skipped) with "no executable produced"; after 7.1+7.2 they should actually
execute and check exit codes/output.

### 7.3 `mock/lower_ctx.rs::with_iterator_next` — does nothing

**Current:** intended to store a closure, doesn't.

**Fix:**
```rust
pub struct MockLowerCtx {
    // ...existing fields...
    iterator_next_override: Option<Box<dyn Fn(Ty, Ty) -> Option<SolverIteratorNextInfo>>>,
}
impl MockLowerCtx {
    pub fn with_iterator_next(mut self, f: impl Fn(Ty, Ty) -> Option<SolverIteratorNextInfo> + 'static) -> Self {
        self.iterator_next_override = Some(Box::new(f));
        self
    }
}
impl LowerCtx for MockLowerCtx {
    fn iterator_next_fn(&mut self, iter_ty: Ty, elem_ty: Ty) -> Option<SolverIteratorNextInfo> {
        self.iterator_next_override.as_ref().and_then(|f| f(iter_ty, elem_ty))
    }
}
```
This directly unblocks writing a real unit test for Tier 1.3's fix (you
need a mock that can actually simulate "iterator_next resolved" vs "not
resolved" to test the fallback path in isolation from a full pipeline).

### 7.4 `mock/solver.rs::MockSolver::iterator_next_info` — unconditional `None`

Same fix shape as 7.3, applied to `MockSolver` — add the same
override-closure field/builder so tests can exercise both the "solver found
it" and "solver didn't" branches of Tier 1.3's code.

**Verify (7.3+7.4):** the Tier 1.3 fallback branch (compiler-builtin
`next`) now has an actual unit test path via these mocks, instead of only
being reachable through a full end-to-end pipeline test.
