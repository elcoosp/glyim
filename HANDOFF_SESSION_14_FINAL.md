# HANDOFF — session 14 (final)

## 0. TL;DR

Session 14 closed **two multi-round bugs/features** on top of the multi-round bug hunt from the prior session.

- **Last green commit**: `67a3e73a` — *"feat(statics): real globals + `&STATIC` + `static mut` (LLVM + interp)"*. All **4599/4599** workspace tests pass; corpus (run_pass) passes; 2 skipped.
- **Working tree clean.**
- **The session is 49 commits ahead of `origin/main`** (was 43 at session start).

Closed this session:

1. **Function-pointer coercion** — three commits:
   - `114e50f3` — `infer::unify_tys` gains an `(FnDef, FnPtr)` arm; substitutes the FnDef's own turbofish args into the registered `FnSig` before unifying, so `let g: fn(i32)->i32 = id::<i32>;` works.
   - `cd0c475d` — `MonoCtx::scan_const` enqueues value-position `Fn` constants (previously deferred to `scan_terminator::Call`, which never saw `let f = add;`).
   - `9f337a5f` — LLVM codegen `concretize_fn_sig` on the raw `FnSig` before declaring the LLVM function, so `fn_abi_of` doesn't ICE on `UnknownType(Param(0))`.

2. **Statics feature** — two commits:
   - `2f843733` — **Stage A**: by-value `static` reads folded to the const-evaluated initializer. Adds `TyCtx::static_tys`, `thir::ExprKind::StaticRef`, `HIR::StaticItem::root_expr`, `lower_crate_raw` `StaticDef` arm, `visit_consts` walks statics, and the `check_path` static fallback.
   - `67a3e73a` — **Stage B (B-full)**: real LLVM globals + `&STATIC` + `static mut`. Adds `MirConstKind::StaticRef { def_id, init, is_mut }`, `TyCtx::static_is_mut`, codegen global emission, interpreter `InterpValue::StaticPtr` + `globals` map, and `lower_expr_to_place` / `lower_rvalue` `StaticRef` arms (rvalue + place + `Ref(StaticRef)` special case).

## 1. First five minutes

```bash
cd /Users/adm/Documents/Repos/glyim-v2
pwd && git log -1 --oneline && git status --porcelain
# if .git/index.lock exists and no git proc: rm -f .git/index.lock
cargo build -p glyim-cli
cargo nextest run --workspace > /tmp/suite.log 2>&1; echo $?; tail -n 5 /tmp/suite.log

```

## 2. What still needs doing (orthogonal, pre-existing bugs)

### 2.1 Struct-typed constants / statics fail

```g
struct Pair { a: i32, b: i32 }
const P: Pair = Pair { a: 3, b: 4 };
fn main() -> i32 { P.a + P.b }
// -> [X0000] aggregate constant insert failed: ExtractOutOfRange @0..0
```

**Verified pre-existing** on the pristine tree. Root cause is in
`crates/glyim-pipeline/src/pipeline_context.rs::cv_const` (`ConstValue::Struct`
arm) or the codegen side (`crates/glyim-codegen-llvm/src/lower.rs`
`MirConstKind::Aggregate` arm) — the struct's LLVM type has a different
field count / layout than the emitted aggregate. Investigation path:

- Check what `llvm_type_for_ty(Adt)` produces for `Pair`.
- Verify the `Aggregate` arm receives exactly `llvm_ty.field_count()` elements.
- The `ExtractOutOfRange` error is from `build_insert_value` — likely the
  ADT's `fields` vector is empty or has the wrong arity when registered.

Same failure for `static P: Pair = ...`, so it is orthogonal to statics.

### 2.2 Fn-pointer-valued constants / statics fail

```g
fn add(a: i32, b: i32) -> i32 { a + b }
const F: fn(i32, i32) -> i32 = add;
// -> ConstRef(319) reached LLVM codegen
```

**Verified pre-existing.** Const-eval has **no** `ConstValue::Fn` variant
(`crates/glyim-const-eval/src/value.rs` — enum ends at `Range`). A
path-to-fn in a const initializer hits `Expr::Path` -> "path expressions
not yet supported in const eval for non-local paths".

Fix scope:

- Add `ConstValue::Fn(DefId)` (or `FnDefId`) to `value.rs`.
- In `crates/glyim-const-eval/src/eval.rs::Expr::Path` — if the path
  resolves to an fn def, return `ConstValue::Fn`.
- In `pipeline_context::cv_const` — map `ConstValue::Fn` to
  `MirConstKind::Fn`.
- For the statics side, the `StaticRef` path also needs
  `const_value(const_alias)` to succeed for fn-typed initializers.

### 2.3 Raw-pointer `==` fails

```g
if (p1 as *const i32) == (p2 as *const i32) { 1 } else { 0 }
// -> [X0000] eq: mismatched types @0..0
```

**Orthogonal tyck bug** — `as *const i32` on `&i32` doesn't unify with
raw-pointer equality. Not exercised by any fixture. Fix: either support
`Ptr == Ptr` in binop checking (lower to integer compare), or make
`as *const T` produce `TyKind::RawPtr` as expected.

### 2.4 Pre-existing `static mut` deeper-projection writes

The interpreter's `write_place` explicitly panics on `StaticPtr` with
`proj_count > 1` ("write through StaticPtr with deeper projections is
not yet supported"). LLVM handles them correctly. Future fix: implement
field updates on the global's `Aggregate` in the interpreter.

## 3. Things that were fixed this session

### 3.1 Fn-pointer coercion

- `FnDef -> FnPtr` unify arm with substitution threading (see commit
  message `114e50f3` for detail).
- `mono::scan_const` enqueues value-position `Fn` constants.
- LLVM `concretize_fn_sig` on the raw sig.

### 3.2 Statics — Stage A (by-value reads, foldable scalars)

- `TyCtx::static_tys: HashMap<StaticDefId, Ty>` with `register_static_ty`
  / `static_ty`, mirrored in `from_ty_ctx` / `clone` / `freeze`.
- `thir::ExprKind::StaticRef(StaticDefId)` (zonk skip lists updated at 3
  sites).
- `HIR::StaticItem::root_expr: Option<ExprId>`.
- `lower_static_def` populates `root_expr`.
- `lower_crate_raw` walker gains a `SyntaxKind::StaticDef` arm —
  **critical fix**: statics at the crate root were previously silently
  dropped before ever reaching `lower_mod_def`.
- `visit_consts` walker visits `ItemKind::Static` too.
- `check_fn_items_in_module` gains an `ItemKind::Static` arm that
  registers the static's type (also aliased into `const_tys` under the
  aliased `ConstDefId`) and const-evaluates the initializer into
  `all_const_values` under the aliased id.
- `check_path` tries `static_ty` and emits `StaticRef`.

### 3.3 Statics — Stage B (B-full: real globals + &STATIC + static mut)

- `MirConstKind::StaticRef { def_id, init, is_mut }` (self-contained;
  carries init).
- `TyCtx::static_is_mut` registry.
- **`check_path` reordering**: statics are checked *before* constants.
  Otherwise the `const_tys` alias let `X` resolve to `ConstRef`,
  silently discarding `static mut` writes into a stack temp.
- `mono::scan_const` enqueues `MonoItem::Static` from `StaticRef`.
- `lower_rvalue` `StaticRef` arms (rvalue + place) emit
  `_addr = StaticRef; _val/_place = {Copy(Place{_addr,[Deref]}) | Place{_addr,[Deref]}}`.
- `&STATIC` / `&mut STATIC` special-cases `Ref(StaticRef)` to return the
  `StaticRef` constant directly (it IS the pointer) — otherwise the
  interpreter captures a temp-local ref.
- LLVM `lower_const`'s `StaticRef` arm emits/returns
  `__glyim_static_<id>` global address.
- Interpreter: `InterpValue::StaticPtr { def_id }` +
  `Interpreter::globals: RefCell<HashMap<StaticDefId, InterpValue>>`.
  Lazy init from `init`. `read_place` / `write_place` /
  `eval_unary_op` all handle it. Helper matches extended.
- Bytecode codegen + snapshot formatter gain `StaticRef` arms.

## 4. Verification workflow

```bash
# Build
cargo build -p glyim-cli

# Full suite (slow tests: glyim-test::run_pass_corpus)
cargo nextest run --workspace > /tmp/suite.log 2>&1; echo $?; tail -n 5 /tmp/suite.log

# Corpus only
cargo nextest run -p glyim-test --test run_pass_corpus

# One fixture, bypass CLI cache
GLYIM_CACHE_DIR=$(mktemp -d) ./target/debug/glyim-cli --with-stdlib --emit=exec \
  crates/glyim-test/tests/run-pass/behavior/<name>.g -o /tmp/f.out && /tmp/f.out; echo $?
```

### Interpreter vs LLVM corpus path

The `run_pass_corpus` test defaults to the **MIR interpreter** (LLVM
requires `--features real-llvm` *and* `GLYIM_TEST_REAL_LLVM=1`). This
matters: a feature that works via direct `--emit=exec` (LLVM) may fail
in the corpus (interpreter) — as happened for `&STATIC` in Stage B
until the interpreter was taught `StaticPtr`. **Always run the corpus
before declaring a feature done.**

## 5. Environment gotchas (carried over)

- **macOS `tail`/`head`**: `tail -4` fails; use `tail -n 4`.
- **`RUSTC_WRAPPER=kache`** is set. Backtraces show `/kache/...` paths.
  `touch <file>` or `cargo build -p ...` before trusting a probe's
  absence.
- **CLI compile cache**: always `GLYIM_CACHE_DIR=$(mktemp -d)`.
- **Terminal display corruption** on very long dumps: write to
  `/tmp/x.txt` and grep. This bit the handoff commit itself once
  (§7 note below).
- **Stale `.git/index.lock`**: `rm -f .git/index.lock`, then
  `git read-tree HEAD` if needed.
- **Read-only `.rmeta`** in `target/`: `chmod -R u+w target`.
- **Probe discipline**: never remove probe code with `sed`/python line
  matching. Use `git checkout -- <file>` to revert, or precise
  `str.replace` with `assert old in content` using exact strings from a
  probe step. The statics work used ~10 `GLYIM_DBG_STATIC*` /
  `GLYIM_DBG_SREF` / `GLYIM_DBG_INTP` / `GLYIM_DBG_BODY` probes — all
  removed before commit (verified with `grep`).
- **Pre-existing `GLYIM_DBG_*` probes** in
  `glyim-typeck/src/{env.rs, lib.rs, tyconv.rs, check_expr.rs}` and
  `glyim-type/src/ty_ctx_mut.rs` are from earlier sessions — not
  touched.
- **Heredoc truncation**: writing a large file via a single `cat > file
  << 'EOF'` can silently truncate if the terminal splices output (as
  happened to this very handoff). Append in small chunks with unique
  delimiters and verify `wc -l` after each.

## 6. Architectural notes (new from this session)

- **`StaticRef` is the pointer.** In MIR, `MirConstKind::StaticRef`
  lowers to the **address** of the global (LLVM) / a `StaticPtr` handle
  (interpreter), not the value. By-value reads must load through a
  `Deref` projection. `&STATIC` is the same value — so
  `Rvalue::Ref(StaticRef)` is special-cased to return the `StaticRef`
  constant directly.
- **`static_tys` vs `const_tys` aliasing.** Statics are registered in
  *both* maps under the same raw `LocalDefId` (aliased as
  `ConstDefId`). This lets `PipelineLowerCtx::const_value` recover the
  type for the initializer. But it means `check_path` **must** check
  `static_ty` before `const_ty` — otherwise a `static` resolves to
  `ConstRef` (folded to init), and `static mut` writes land in a stack
  temp.
- **`FnDef -> FnPtr` unify substitutes through the FnDef's own substs.**
  A generic fn item `id::<i32>` has `FnDef(id, [i32])`, but its
  registered `FnSig` carries `Param(0)` in inputs/output. Without
  `ctx.subst_ty(param, &subst_map)` before unifying, you get `T vs i32`.
- **Value-position `Fn` constants must be enqueued in mono by
  `scan_const`.** `scan_terminator::Call` only sees call-position
  constants; `let f = add;` never reaches it. The old
  `if ty_ctx.is_none()` gate in `scan_const` was too narrow — replaced
  with a "substitution is concrete" check.
- **`concretize_fn_sig` is required on any `Fn` constant codegen
  declares.** The raw `FnSig` carries `Param(i)` for generic fns;
  `fn_abi_of` -> `layout_of` ICEs on `UnknownType(Param(0))` without
  substitution.
- **`lower_crate_raw` walker and `lower_mod_def` walker are separate.**
  Adding a new item kind (e.g. `StaticDef`) requires *both*. This was
  the silent bug that ate an hour — statics were dropped at the crate
  root even though `lower_static_def` existed and worked.

## 7. Session stats

- **7 commits**:
  - `114e50f3` fn-pointer typeck
  - `cd0c475d` fn-pointer mono
  - `9f337a5f` fn-pointer codegen
  - `2f843733` statics Stage A
  - `67a3e73a` statics Stage B
  - `c85d7ade` handoff (initial, truncated)
  - `<amended>` handoff (full)
- **10 new execution-asserting fixtures**:
  - `fn_ptr_assignment.g`, `fn_ptr_apply.g`, `fn_ptr_generic.g`
  - `static_read.g`, `static_used_in_expr.g`,
    `static_alongside_const.g`
  - `static_ref_deref.g`, `static_mut_write.g`, `static_mut_read.g`,
    `static_ref_stable_address.g`
- **4599/4599** suite green; **corpus green**.
- **3 pre-existing bugs documented** in §2 for follow-up (all
  orthogonal, none regressed).
