# Glyim — Complete Bug, Stub, Miscompilation & Performance Fix Plan

> **Repo:** https://github.com/elcoosp/glyim @ commit `6bab26edb480751331d149589f8983cd49eacc03` ("fix(frontend): FE-7/9/10/11/15 -- five parser findings (incl. an ICE)")
> **Scope:** ~207,000 lines of Rust across 30+ workspace crates (full compiler pipeline: lexer → parser → def-map → macros → HIR → typeck/solve → THIR→MIR → borrowck → MIR opts → interpreter + LLVM & bytecode backends), the `.g` standard library (lang-core / lang-alloc / lang-std), `glyip` (build tool), `glyim-lsp`, and `tools/glyim-pilot` (Rust orchestrator + TypeScript browser extension).
> **Method:** every production file read line-by-line by 8 independent parallel audit passes, each finding verified against exact source (file + line at `6bab26ed`), cross-checked with callers and tests, and de-duplicated against the repo's own prior audit (`docs/roadmaps/glyim-bug-and-performance-audit.md` @ `eff1f1fa`) and its status tracker (`docs/roadmaps/audit-status.md`). The 8 highest-severity claims were re-verified line-by-line before publication.
> **Totals: 199 findings — 42 Critical, 53 High, 72 Medium, 32 Low (incl. 17 performance issues).**

---

## 0. How an AI agent should use this plan (READ THIS FIRST)

Work strictly top-to-bottom through the numbered tasks. The order is deliberate: Wave 1 fixes the bugs that corrupt every later verification (broken snapshots, miscompiling optimizer, crashing codegen, segfaulting stdlib FFI), Wave 2 fixes silent wrong-code, Wave 3 edge-cases, Wave 4 cleanups. Fixing in any other order wastes your time re-running failing suites.

For each task:

1. `cd` to the repo root. All paths are relative to it.
2. **Locate** the code with the exact `File:` path and line numbers given. If lines drifted (because earlier tasks in this same plan edited the file), search for the quoted snippet instead — the quoted code is authoritative, not the line number.
3. **Apply** the fix shown in the diff/code block (`-` lines are what you delete, `+` lines are what you write). The diffs are minimal on purpose.
4. **Verify** with the command under **Verify**. Do not move on until it passes.
5. After every 10 tasks, run the batch checkpoint (below) and `git commit` with message `fix(wave-N): <task IDs>`.
6. If a task cannot be completed (fix doesn't compile, test missing), do NOT delete error paths or "simplify" the code to make it compile. Skip the task, note its ID in a `SKIPPED.md` file at the repo root, and continue.

### 0.1 Build & batch checkpoint

```bash
# If LLVM 22 is unavailable, build with: cargo build --workspace --no-default-features
cargo build --workspace                    # baseline build (needs LLVM 22: set LLVM_SYS_220_PREFIX)
cargo test --workspace                     # full suite — must be green (or green + previously-skipped) before continuing
```

Baseline expectation: `docs/roadmaps/audit-status.md` records 4530/4530 tests passing at a nearby commit. After Wave 1 several previously-".pending"/failing fixtures may turn green; that is expected and good.

### 0.2 Rules of engagement

- Do NOT refactor, rename, reformat, or "improve" anything not shown in a task's diff.
- Every fix preserves the existing public API unless the task explicitly says otherwise and lists the call sites to update.
- Severity meanings used throughout: **Critical** = silent miscompile, crash/ICE, segfault, hang, security hole, or data loss. **High** = valid programs rejected, wrong results visible to users, dead advertised features. **Medium** = edge-case wrongness, latent bugs that fire when adjacent code changes. **Low** = minor. **P** = performance.
- Categories: **BUG** (logic error), **STUB** (placeholder/unimplemented/wired-to-nothing), **MISCOMPILE** (emits wrong IR/bytecode/MIR/tree silently), **PERF** (performance defect).

### 0.3 ID conventions (IMPORTANT — avoid confusion with the old audit)

The repo's old audit (`docs/roadmaps/glyim-bug-and-performance-audit.md`) already used IDs like `FE-1`, `HIR-1`, `LL-1`, `RT-1`, `SOLVE-1`. This plan uses **new, non-colliding namespaces**:

| Namespace in this plan | Area |
|---|---|
| `FE-101..FE-116` | frontend: lexer/parser/CST (`glyim-frontend`, `glyim-syntax`, `glyim-vfs`) |
| `DM-1..5` | name resolution (`glyim-def-map`) |
| `MAC-1..8` | macros (`glyim-meta`) |
| `PM-1..2` | proc-macro bridge (`glyim-proc-macro`) |
| `HIRX-1..9` | HIR lowering (`glyim-hir`) — X = "new HIR", distinct from old-audit HIR-* |
| `TY-24..28`, `SOLVE-24..30`, `TCK-24..31`, `CE-24..27` | type system, solver, typeck, const-eval |
| `OPT-1..4`, `LOW-1..15`, `BCK-1..2`, `INT-1..6` | MIR opts, lowering, borrowck, interpreter |
| `LL-15..23`, `LAY-1..3`, `BC-1..7`, `VM-1`, `RT-34..37` | LLVM backend, layout, bytecode backend/VM, runtime (BC here means *bytecode emitter*, distinct from borrowck `BCK-*`) |
| `PIPE-1..5`, `CLI-1..4`, `LSP-1..11`, `GLYIP-1..7` | pipeline, CLI, LSP, build tool |
| `STD-1..19` | stdlib `.g` sources |
| `HARNESS-1..9`, `SPAN-1..3`, `DIAG-1..2` | test harness, span, diagnostics |
| `PILOT-1..18`, `EXT-1..8` | glyim-pilot Rust server, TS extension |

If you meet an old-audit ID (e.g. `SOLVE-7`) in repo docs, that item is tracked in `docs/roadmaps/audit-status.md` — do not re-fix it here.

---

## 1. Execution roadmap

### Wave 1 — Criticals (12 tasks, do first, in this order)
`OPT-1`, `FE-102`, `TCK-24`, `HIRX-1`, `LL-15`, `PIPE-1`, `PIPE-2`, `BC-1`, `INT-2`, `STD-4`→`STD-5`→`STD-6`, `STD-1`→`STD-2`→`STD-3`, `PILOT-1`.

### Wave 2 — Highs (silent wrong code & dead features; grouped by crate)
- frontend: `FE-101`, `FE-103`, `FE-104`, `FE-105`, `FE-106`, `FE-107`
- lower/mir/interp: `LOW-1`, `LOW-2`, `LOW-3`, `LOW-4`, `LOW-5`, `LOW-6`, `LOW-7`, `LOW-15`, `INT-1`
- types: `SOLVE-24`, `SOLVE-25`, `TCK-25`, `CE-24`, `TY-24`, `TY-25`
- codegen/VM/runtime: `BC-2`, `BC-3`, `BC-4`, `BC-5`, `BC-6`, `RT-34`
- macros/resolution: `MAC-1`, `MAC-2`, `DM-1`, `DM-2`
- HIR: `HIRX-2`, `HIRX-3`
- LSP: `LSP-1`, `LSP-2`, `LSP-3`, `LSP-4`, `LSP-9`
- pipeline/CLI/glyip: `PIPE-3`, `CLI-1`, `GLYIP-1`, `GLYIP-2`, `GLYIP-3`
- stdlib: `STD-7`, `STD-8`, `STD-9`, `STD-10`
- harness: `HARNESS-1`, `HARNESS-2`
- pilot/extension: `PILOT-2`, `PILOT-3`, `PILOT-4`, `PILOT-5`, `PILOT-6`, `PILOT-7`, `PILOT-8`, `EXT-1`, `EXT-2`, `EXT-3`

### Wave 3 — Mediums (everything marked Medium)
### Wave 4 — Lows & performance (everything marked Low / PERF)

After each wave: `cargo test --workspace`, `git commit`.
---

# WAVE 1 — Critical fixes

> **T-000 (procedural, do before everything):** `cargo build --workspace` and `cargo test --workspace` to establish the baseline. Record the pass count. Delete the six stale `.snap.new` files ONLY after completing task T002 (they are the evidence for it): `crates/glyim-test/src/snapshot/snapshots/glyim_test__snapshot__snapshot_cst@{full_program,match_expr,index_expr,complex_match,error_handling,complex_program}.snap.new`.

---

### T001 [OPT-1] [Critical] [MISCOMPILE] Const-prop ignores side effects of `Call` terminators and writes through references

- File: `crates/glyim-opt/src/constant_prop.rs:420-431` (transfer), `453-486` (rewrite)
- Code:
```rust
for stmt in &block.statements {
    if let StatementKind::Assign(place, rvalue) = &stmt.kind {
        out.remove(&place.local);
        defined.insert(place.local);
        if place.projection.is_empty()
            && let Some(c) = evaluate_rvalue_to_const(rvalue, &out, ctx, body.locals[place.local].ty)
        { out.insert(place.local, Some(c)); }
    }
}
```
- Problem: The fixpoint transfer never inspects the block **terminator**; the exit map (still containing e.g. `a → Int(5)`) flows into the successor of every `Call`. Also, a projected assign (`Assign(Place{local: r, projection: [Deref]}, v)` — produced by closure by-ref capture writes) only pins the **base** local (`map.insert(local, None)` pins `r`, not the pointee `a`). Trigger: `let mut a = 5; let r = &mut a; inc(r); a + 1` — after the call, the successor's entry map still says `a = 5`, so `a + 1` folds to `6` (runtime: `7`). `optimize()` runs unconditionally on every body in the codegen path (`crates/glyim-pipeline/src/lib.rs:519,544`), so this silently changes program output of every optimized build.
- Fix (2 steps):
  1. In the per-block transfer, after simulating statements, add:
```rust
// A Call terminator may write through references and mutate any
// global/indirect state. Conservatively kill all constants.
if matches!(block.terminator.kind, glyim_mir::TerminatorKind::Call { .. }) {
    out.clear();
}
```
  2. In the rewrite loop's projected-assign branch, when `place.projection` contains `ProjectionElem::Deref`, additionally pin **all** locals (cheap conservative option: `map.clear()`), not just the base local.
- Verify: `cargo build -p glyim-cli && printf 'fn inc(x: &mut i32) { *x = *x + 1; }\nfn main() -> i32 { let mut a = 5; let r = &mut a; inc(r); a + 1 }\n' > /tmp/t.g && target/debug/glyim-cli /tmp/t.g --emit=llvm-ir -o /tmp/t.ll` — the emitted IR must return 7, not 6. Then `cargo test -p glyim-opt constant_prop`.

---

### T002 [FE-102] [Critical] [BUG] PatIdent absorbs trailing whitespace — 6 blessed snapshot tests are red at HEAD

- File: `crates/glyim-frontend/src/parser/pat.rs:176-184` (regression introduced by the FE-9 fix in commit `6bab26ed`)
- Code:
```rust
} else {
    self.start_node(SyntaxKind::PatIdent);
    self.bump();
    // FE-9: `x @ subpat` — binding with a sub-pattern.
    if self.current_kind() == SyntaxKind::At {
        self.bump(); // @
        self.parse_pat_inner();
    }
    self.finish_node();
}
```
- Problem: `current_kind()` → `current()` → `flush_trivia()` emits pending whitespace **into the open `PatIdent` node**. Since the FE-9 fix placed `finish_node()` after the `@` check, `let x = arr[0];` now yields `PatIdent@16..18` containing `Whitespace@17..18 " "` instead of `PatIdent@16..17`. This is exactly the divergence in **all six** `.snap.new` files (`index_expr`, `match_expr`, `error_handling`, `full_program`, `complex_match`, `complex_program`) — the CST contract is broken and 6 insta tests fail at HEAD. HIR name extraction is token-based so the compiler still works, but any `node.text()`/`text_range()` consumer (LSP rename/hover on patterns, tooling) sees polluted ranges.
- Fix: Decide whether `@` follows **without flushing trivia**, and only then keep the node open:
```rust
self.start_node(SyntaxKind::PatIdent);
self.bump();
let at_follows = {
    let mut p = self.pos;
    while let Some(t) = self.tokens.get(p) {
        if !t.kind.is_trivia() { break; }
        p += 1;
    }
    self.tokens.get(p).map(|t| t.kind) == Some(SyntaxKind::At)
};
if at_follows {
    self.flush_trivia();   // whitespace legitimately precedes `@`
    self.bump();           // @
    self.parse_pat_inner();
}
self.finish_node();
```
- Verify: `cargo test -p glyim-frontend --lib` — the 6 snapshot failures disappear. Then delete the stale `.snap.new` files listed in T-000.

---

### T003 [TCK-24] [Critical] [MISCOMPILE] Call arguments are never type-checked against formal parameter types (and no arity check for FnDef callees)

- File: `crates/glyim-typeck/src/check_expr.rs:840-843` (types discarded), `1005-1077` (only Param/Infer/literal cases handled), `1181-1197` (method args only checked for `Builtin` dispatch)
- Code:
```rust
let mut arg_exprs = Vec::with_capacity(args.len());
for &arg_id in args {
    arg_exprs.push(self.check_expr(arg_id).0);   // arg TYPE thrown away
}
...
for (i, arg_expr) in arg_exprs.iter().enumerate() {
    if let Some(GenericArg::Ty(param_ty)) = inputs.get(i) {
        if let TyKind::Param(pt) = self.ctx.ty_kind(*param_ty) {
            subst.insert(pt.index, GenericArg::Ty(arg_expr.ty));      // generics: subst only
        } else if matches!(self.ctx.ty_kind(*param_ty), TyKind::Infer(_)) ... {
            self.unify(*param_ty, arg_expr.ty, span);                 // ctor case only
        } else {
            // ... literal anchoring + align_impl_param_subst ONLY — no check
        }
    }
}
```
and for method calls (`MethodCall` arm):
```rust
let expected_args: Vec<Ty> = match &dispatch {
    Some(MethodDispatch::Builtin(fn_id)) => ...,
    _ => Vec::new(),        // Static/Virtual: expected arg types = none
};
```
- Problem: A plain call `f(arg)` never unifies `arg`'s type against the formal's type, and there is no arity check for `FnDef` callees (arity is checked only on the `FnPtr` (:888) and `Closure` (:961) paths; nothing in `glyim-lower` either). `fn f(x: &i32){...} f(5);`, `fn g(x: i32){...} g("s", 1, 2);` are silently accepted; MIR then passes the wrong value/count — stack/layout mismatch at codegen. Same for user-impl method calls (`Static` dispatch): `c.push("hello")` on `Vec<i32>` type-checks. The comment at :1044-1050 shows this was a deliberate de-cascading decision, but nothing replaced the missing check.
- Fix: In the `is_fn_def` branch, after building `inputs`:
  1. For every `(i, arg_expr)` with `i >= inputs.len()` push an `"expected N arguments, got M"` diagnostic.
  2. Otherwise, when neither the formal nor the arg is a bare `Param`, call `self.unify(arg_expr.ty, *param_ty, span)` guarded by a snapshot/rollback (already exists on `InferenceTable`) so coercion-shaped mismatches (`&mut→&`, array→slice, `str`/`&str`, `Adt1050`/`String`) still pass — mirroring what the `MethodCall` Builtin path already does at :1208-1212.
  3. For `Static` method dispatch, build `expected_args` from `self.ctx.fn_sig(fn_def_id)` inputs skipping the receiver, and unify as above.
- Verify: `cargo build -p glyim-cli`; `/tmp/t.g` = `fn f(x: &i32) {} fn main() { f(5); }` → must error; `fn h() {} h(1,2);` → must error; `cargo test -p glyim-typeck` stays green.

---

### T004 [HIRX-1] [Critical] [MISCOMPILE] Async `rewrite_for_poll` wraps the **first**-allocated Block — any nested block in an async body collapses the poll body

- File: `crates/glyim-hir/src/lower/lower_async.rs:694-742` (search at :694-696)
- Code:
```rust
let root_block = (0..body.exprs.len())
    .map(|i| ExprId::from_raw(i as u32))
    .find(|&rid| matches!(body.exprs[rid], Expr::Block { .. }));
```
- Problem: `lower_block_to_expr` allocates children **before** the block (`lower_expr.rs:198`), so the outermost fn-body Block is the **last** Block in the arena — exactly what `root_expr_id` (:158-165) documents and implements with `.rev().find(...)`. `rewrite_for_poll` uses a **forward** `.find`, which picks the first-allocated Block = any nested block (an `if` branch, `match` arm, loop body, or `{}` expression). It then wraps *that* block's tail in `Poll::Ready(..)` and synthesizes the new root from *that* block's `stmts` only (:726-742) — every other statement of the async body, including the `.await` polling, becomes unreachable. Trigger (no await even needed):
```g
async fn f(c: bool) -> i32 { let x = if c { 1 } else { 2 }; x }
// poll body collapses to `Poll::Ready(1)`; block_on(f) returns 1 for any c
```
This is the same forward-search old-audit HIR-10 flagged; the tracker's "HIR-10 verified fixed" verdict was verified on a body whose only Block is the outer one (first == outer coincidentally). The "verified fixed" entry for HIR-10 in `docs/roadmaps/audit-status.md` is therefore wrong.
- Fix: Replace the forward search with the same rule used by `root_expr_id`:
```rust
let root_block = (0..body.exprs.len()).rev()
    .map(|i| ExprId::from_raw(i as u32))
    .find(|&rid| matches!(body.exprs[rid], Expr::Block { .. }));
```
(or better, reuse `root_expr_id(body)` directly).
- Verify: build per docs (`cargo build -p glyim-runtime -p glyim-cli`), compile a fixture with `async fn add_one(x: i32) -> i32 { let b = if x > 0 { 1 } else { 2 }; x + b }` — must produce the correct sum, not 1/2. Add it as `crates/glyim-test/tests/runtime/m5/one_step_nested_block.g` (`// check-stdout: 42`). Update `docs/roadmaps/audit-status.md` to mark HIR-10 as re-opened→fixed.

---

### T005 [LL-15] [Critical] [BUG] Every `Drop` terminator calls `glyim_drop_in_place` with the wrong arity — garbage fn-pointer deref or silent no-drop

- File: `crates/glyim-codegen-llvm/src/lower.rs:2800-2807` (decl), `2838-2843`/`2920-2923` (call); `crates/glyim-runtime/src/lib.rs:145`
- Code:
```rust
// lower.rs — codegen declares and calls it with ONE parameter:
let fn_type = self.context.void_type().fn_type(
    &[self.context.ptr_type(AddressSpace::default()).into()],
    false,
);
self.module.add_function("glyim_drop_in_place", fn_type, None)
...
let args_vals: Vec<BasicValueEnum<'ctx>> = vec![drop_arg];
```
```rust
// runtime — the actual symbol takes TWO parameters:
pub unsafe extern "C" fn glyim_drop_in_place(ptr: *mut u8, drop_fn: Option<DropFn>) {
    if ptr.is_null() { return; }
    if let Some(drop) = drop_fn { unsafe { drop(ptr) } }   // reads 2nd register (rsi)
}
```
- Problem: Any program whose MIR has a `Drop` terminator for a needs-drop local (`String`, `Vec`, ADT with `Drop` impl) calls `glyim_drop_in_place(ptr)` while the callee reads a second pointer argument from `rsi`. `Option<DropFn>` is niche-optimized to the raw pointer, so `if let Some(drop)` is true whenever the garbage register is non-zero → **call through a garbage pointer (SIGSEGV/UB)**; when it happens to be 0, drop is a silent no-op → heap buffers leak and no destructor ever runs. There is no other caller (repo-wide grep: only runtime tests pass 2 args).
- Fix (2 steps):
  1. Short-term, make codegen emit the 2-arg form: declare `fn_type(&[ptr_ty.into(), ptr_ty.into()], false)` and push a second `ptr_ty.const_null()` arg.
  2. Proper fix: in `lower_terminator`'s `Drop` arm, resolve/generate a real per-type drop-glue function (field-wise drops + `glyim_dealloc(ptr, size, align)` for the owning type) and pass its address as `drop_fn`. (Alternatively, change the runtime symbol to take 1 arg and never call user destructors — but then also remove the `DropFn` plumbing.)
- Verify: `cargo build -p glyim-runtime -p glyim-cli`; `target/debug/glyim-cli prog.g --emit=llvm-ir -o p.ll` with `fn main() { let s = "x".to_string(); }`; grep `p.ll` for `call void @glyim_drop_in_place(ptr` — it must show 2 args matching the runtime's definition. Then `--emit=exec` runs without segfault.

---

### T006 [PIPE-1] [Critical] [MISCOMPILE] Every drop-glue body is lowered into the single shared function `__glyim_fn_0`, corrupting the entry-main calling convention

- File: `crates/glyim-pipeline/src/mono_cache.rs:414` (root), `crates/glyim-codegen-llvm/src/lower.rs:4040,4113-4128`
- Code:
```rust
// mono_cache.rs, generate_drop_glue:
let def_id = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));
let mut body = Body::dummy(def_id);
// lower.rs:
let fn_name = format!("__glyim_fn_{}", body.owner.local_id.to_raw());
...
let cc = match fn_sig.abi { Abi::Glyim => 8u32, ... };
function.set_call_conventions(cc);
```
- Problem: `MonoItem::DropGlue` bodies (enqueued by `mono.rs:192` for every local whose type needs drop) are part of `mono_items` → `all_bodies` → `backend.generate()` lowers each of them. All glue bodies carry `owner = DefId(0,0)` and `arg_count = 0`, so they all target `__glyim_fn_0`, and since there is no `fn_sig` for DefId(0,0) they take `Abi::Glyim` → `set_call_conventions(8)` (fastcc). In the typical program `fn main(){}` is item 0, so `__glyim_fn_0` **is main's function**: each glue lowering appends dead blocks into main and flips main back to fastcc after `lower.rs:4126-4128` deliberately forced it to C-convention for the C-ABI wrapper (their own comment says on AArch64 "calling a fastcc fn from a ccc site silently drops the return value"). When main is not item 0, all N glue bodies still pile into one shared `__glyim_fn_0` (multiple "entry" blocks). Also `scan_body_for_refs` (mono.rs:187-193) enqueues glue for `place.local`'s type, so inside an array-glue body (local 0 = UNIT) the element glue is never registered — the comment's claim at mono_cache.rs:448-451 is false.
- Fix (2 steps):
  1. Give each glue body a unique synthetic owner, e.g. hash the type:
```rust
let hash = sha2::Sha256::digest(ty_debug_string.as_bytes());
let synthetic = LocalDefId::from_raw(0x8000_0000 | (u32::from_be_bytes([hash[0],hash[1],hash[2],hash[3]]) & 0x7FFF_FFFF));
let def_id = DefId::new(CrateId::from_raw(0), synthetic);
```
  2. Or (simpler): skip lowering `MonoItem::DropGlue` bodies in `generate()` entirely (codegen's `Drop` terminator calls the runtime `glyim_drop_in_place`, lower.rs:2796-2838 — the glue bodies are never called) and keep them only for mono-graph scanning.
- Verify: `cargo build -p glyim-cli -p glyim-runtime && printf 'fn main() { let s = String::new(); }\n' > /tmp/t.g && target/debug/glyim-cli /tmp/t.g --emit=llvm-ir -o /tmp/t.ll && grep -c 'define' /tmp/t.ll` — main must not be emitted with `fastcc` and `__glyim_fn_0` must not contain stray unreachable blocks.

---

### T007 [PIPE-2] [Critical] [MISCOMPILE] Array drop-glue uses the *element index* as a *local index* → OOB panic (ICE) at codegen for `[T; N]`, N≥2, T: !Copy

- File: `crates/glyim-pipeline/src/mono_cache.rs:767-775` (with lower.rs:830-847, local_ty at lower.rs:66)
- Code:
```rust
fn element_place_at(base: &Place, index: u32) -> Place {
    let idx_local = LocalIdx::from_raw(index);   // element index used as LOCAL index!
    let mut proj = base.projection.to_vec();
    proj.push(ProjectionElem::Index(idx_local));
```
- Problem: the array-glue body is `Body::dummy` + pushes no per-element locals (it has exactly 1 local, the UNIT-typed ptr). Codegen's `ProjectionElem::Index` arm loads *the value of that local* (`get_local_ptr` → `self.locals[local]`, and `local_ty(self.body, local)` → `body.locals[local]`). For element `i ≥ 1`, `body.locals[i]` is out of bounds → `IndexVec` panic; for `[T; 1]` it silently GEPs by local 0's garbage. The array-glue body is guaranteed to reach codegen (see T006 chain), so any program that lets a `[String; 3]`-like local be dropped panics the compiler (caught by `run()`'s ICE handler → exit 1). Slice glue (generate_slice_drop_glue) does it correctly with real locals — array glue is the broken twin. MIR has `ProjectionElem::ConstantIndex` for exactly this.
- Fix: use `ProjectionElem::ConstantIndex { offset: i, min_length: n, from_end: false }` in `element_place_at`, and in codegen-llvm's `place_ptr` handle `ConstantIndex` with a constant GEP:
```rust
fn element_place_at(base: &Place, index: u32, len: u32) -> Place {
    let mut proj = base.projection.to_vec();
    proj.push(ProjectionElem::ConstantIndex { offset: index, min_length: len, from_end: false });
    Place { local: base.local, projection: proj.into_boxed_slice() }
}
```
- Verify: `printf 'fn main() { let a = [make(), make()]; }\nfn make() -> String { ... }\n' > /tmp/arr.g && target/debug/glyim-cli /tmp/arr.g --emit=exec -o /tmp/arr` — currently ICEs ("index out of bounds"); after fix compiles.

---

### T008 [BC-1] [Critical] [MISCOMPILE] `OP_SWITCH_INT` wire mismatch: emitter writes 16-byte `u128` arm values, VM and peephole read 8 — every enum match desyncs

- File: `crates/glyim-codegen/src/lib.rs:1081-1088` (emitter), `760-777` (peephole decoder); `crates/glyim-bytecode-vm/src/lib.rs:497-515` (VM)
- Code:
```rust
// emitter: v is u128 (SwitchTargets::iter() -> (u128, BasicBlockIdx), glyim-mir lib.rs:628)
for (v, t) in targets.iter() {
    bc.extend_from_slice(&v.to_le_bytes());          // 16 bytes
    bc.extend_from_slice(&t.to_raw().to_le_bytes()); // 4 bytes
}
// VM: 8 + 4 per arm
let v = Vm::read_i64(code, &mut pc)?;
let t = Vm::read_u32(code, &mut pc)?;
```
- Problem: All MIR enum dispatch is a non-bool `SwitchInt`. After the first arm the VM's PC lands 8 bytes ahead: it reinterprets the high half of the value + the target bytes as the next value, and eventually jumps to garbage block indices (`resolve_target` on arbitrary u32s → panic or mid-instruction PC). The peephole decoder (`take(12)` per arm) is also wrong vs the emitter's 20 bytes per arm.
- Fix: In the emitter, truncate consistently (matches LLVM's `val as u64` and the VM's `read_i64`):
```rust
bc.extend_from_slice(&(*v as u64).to_le_bytes());   // 8 bytes, was v.to_le_bytes() (u128)
```
Update the peephole decoder to `take(12)` per arm (8-byte value + 4-byte target). Optionally harden the VM: after decoding, `debug_assert!` that the remaining bytes are consumed exactly.
- Verify: `cargo test -p glyim-bytecode-vm` after adding a round-trip test: hand-encode the emitter's exact byte pattern for a 2-arm switch and assert the VM dispatches to the correct blocks (fails before, passes after).

---

### T009 [INT-2] [Critical] [BUG] Interpreter `Drop` arm: `debug_assert!(false, …)` — any droppable local (e.g. `String`) reaching the interpreter panics in debug builds

- File: `crates/glyim-mir-interp/src/lib.rs:515-547`
- Code:
```rust
TerminatorKind::Drop { place, target, cleanup: _ } => {
    // By the time MIR reaches the interpreter, `glyim-opt`'s
    // drop-elaboration pass must have rewritten `Drop` terminators ... into
    // `Call`s to the generated drop-glue functions. ...
    debug_assert!(
        false,
        "Drop terminator reached the interpreter for place {place:?}; ...");
```
- Problem: The premise is false — `elaborate_drops` **keeps** `TerminatorKind::Drop` for every `needs_drop` type (drop_elaboration.rs:437-456); it never rewrites to drop-glue calls. `TyCtx::needs_drop(String) == true` and `elaborate_scope_drops` emits `Drop{String local}` at every scope exit, so `fn main() { let s = String::from("x"); ... }` run through the interpreter (the harness's portable run path, `crates/glyim-test/src/harness/interpreter_runner.rs`, and `glyip`) panics the interp thread in any debug build — the harness then reports a timeout. No run-pass fixture has a `String` local (grep: zero hits), which is why 4530 tests stay green.
- Fix: Replace the unconditional `debug_assert!` with a type check:
```rust
let ty = self.current_local_ty(place.local).unwrap_or(glyim_type::Ty::ERROR);
let needs_drop = self.tcx.map(|c| c.needs_drop(ty)).unwrap_or(false);
if !needs_drop {
    bb_idx = target; // non-droppable: nothing to do
} else {
    // droppable: interpret as a no-op with a warning until real
    // drop-glue calls are wired (see T005), so execution can proceed.
    tracing::warn!("Drop of needs_drop place {place:?} treated as no-op by interpreter");
    bb_idx = target;
}
```
- Verify: run-pass fixture with a `String` local under the interp path (`cargo test -p glyim-test -- run-pass string_local`); today: timeout/panic in debug; after: clean exit.

---

### T010 [STD-4] [Critical] [BUG] env module ↔ runtime FFI ABI mismatch — every string-returning env call segfaults

- File: `crates/glyim-lang-std/lib/env.g:7-19, 35-47, 110-122, 133-148, 151-163`; `crates/glyim-runtime/src/lib.rs:212-217, 335-352, 374, 410, 446, 481`
- Code:
```glyim
// env.g declares:
fn glyim_env_var(key: *const u8, key_len: usize, buf: *mut u8, cap: usize) -> isize;
// runtime implements:
pub unsafe extern "C" fn glyim_env_var(name: *const u8, name_len: usize,
    out_ptr: *mut *mut u8, out_len: *mut usize) -> i32
```
- Problem: `glyim_env_var`, `glyim_env_current_dir`, `glyim_env_current_exe`, `glyim_env_home_dir`, `glyim_env_temp_dir`, `glyim_env_args_get` all take `(out_ptr: *mut *mut u8, out_len: *mut usize)` in the runtime and *allocate* the string. The .g side passes `(raw buffer ptr, capacity)`: the runtime writes an 8-byte heap pointer into `buf[0..8]`, then writes the length to address `cap` = 4096 → write to near-NULL → SIGSEGV. `env::var/current_dir/args/…` cannot work at all.
- Fix: change env.g to declare the real ABI:
```glyim
let mut p: *mut u8 = null_mut();
let mut n: usize = 0;
let rc = unsafe { glyim_env_var(key.as_ptr(), key.len(), &mut p, &mut n) };
if rc == 0 {
    let bytes = slice::from_raw_parts(p, n);
    let s = String::from_utf8_lossy(bytes).to_string();
    unsafe { glyim_free_cstr(p, n) };   // runtime exports it, lib.rs:23
    ...
}
```
Apply the same pattern to every mismatched declaration in env.g (compare each `extern` block against the runtime's actual signatures).
- Verify: run-pass fixture `println(env::var("GLYIM_TEST_VAR").unwrap_or("missing"))` with env set (currently segfaults; after fix prints the value).

---

### T011 [STD-5] [Critical] [BUG] process module ↔ runtime FFI mismatch + `env()`/`current_dir()`/piped stdio silently ignored

- File: `crates/glyim-lang-std/lib/process.g:76-113, 142-152, 155-195, 203-213`; `crates/glyim-runtime/src/lib.rs:635-641, 709, 754-761, 835`
- Code:
```glyim
// process.g: 7 args, returns pid as i32
fn glyim_process_spawn(program, program_len, args, args_len, stdin_cfg, stdout_cfg, stderr_cfg) -> i32;
// runtime: 5 args, handle via out-param
pub unsafe extern "C" fn glyim_process_spawn(cmd, cmd_len, args, args_len, out_handle: *mut usize) -> i32
```
- Problem: (a) `spawn` passes stdio configs as args 5-7 (garbage to the callee) and supplies no `out_handle`, so the runtime writes the handle to whatever register value lands there (stdin_cfg) → arbitrary write. (b) `glyim_process_wait` runtime is `(handle, *mut i32)`; .g declares `(pid: u32) -> i32` → exit code written through garbage pointer. (c) `wait_output`: 7 args vs runtime's 6, different semantics. (d) `kill`: runtime takes `(handle, signal)`, .g passes one arg. (e) Even with the ABI fixed, `self.env`/`self.current_dir` (process.g:10-11) are never sent to the runtime — `Command::env()/current_dir()` are no-ops, and `Command::output()` (:122-129) rebuilds the command dropping both. `Child.stdin/stdout/stderr` are always `Option::None`; `ChildStdin/Stdout/Stderr` are dead types — `Stdio::Piped` for stdin does nothing.
- Fix: Align all four declarations with the runtime signatures (handle-based). Add runtime support for env/cwd/stdio config (extend `glyim_process_spawn` with env array + cwd + three stdio configs), and make `output()` use `self` instead of rebuilding the command.
- Verify: fixture running `sh -c 'echo $FOO'` with `.env("FOO","bar").output()` prints `bar`.

---

### T012 [STD-6] [Critical] [BUG] `fs::metadata` ABI mismatch — fd value used as a path pointer; only size is retrievable

- File: `crates/glyim-lang-std/lib/fs.g:138-149, 433-436`; `crates/glyim-runtime/src/fs.rs:480-484`
- Code:
```glyim
// fs.g:  fn glyim_fs_metadata(fd: i32, out: *mut MetadataRaw) -> i32;
// runtime: fn glyim_fs_metadata(path: *const u8, path_len: usize, out_size: *mut u64) -> i32
```
- Problem: `File::metadata()` passes `self.fd` (e.g. 3) where the runtime expects a path pointer → `path_from_raw(0x3, …)` reads address 3 → SIGSEGV. Even ignoring that, the runtime can only return size — perm/type/times of `MetadataRaw` (fs.g:206-216) are never filled. `fs::metadata`, `exists`, `is_dir`, `is_file`, `symlink_metadata`, `DirEntry::metadata` all inherit this. Also `symlink_metadata` (fs.g:433-436) is byte-identical to `metadata` — it follows symlinks despite its doc.
- Fix: add a runtime `glyim_fs_metadata_fd(fd: i32, out: *mut MetadataRaw) -> i32` (fstat-based) and a no-follow variant (`glyim_fs_symlink_metadata` via lstat); point fs.g at them; fill the whole `MetadataRaw`.
- Verify: `fs::exists("Cargo.toml")` fixture (currently segfaults).

---

### T013 [STD-1] [Critical] [BUG] `Mutex<T>`/`RwLock<T>` never store `T` — guards reinterpret lock state as the data

- File: `crates/glyim-lang-std/lib/sync.g:7-28` (and :98-118, :84-94)
- Code:
```glyim
struct Mutex<T> {
    inner: UnsafeCell<MutexInner>,
    _marker: PhantomData<T>,
}
fn new(t: T) -> Mutex<T> {
    Mutex { inner: UnsafeCell::new(MutexInner { locked: AtomicBool::new(false), _padding: [0u8; 63] }), _marker: PhantomData }
}
```
- Problem: `t` is dropped on the floor; there is no `T` field. `MutexGuard::deref` does `&*(self.mutex.inner.get() as *const T)` (sync.g:86), i.e. it reinterprets `MutexInner` (AtomicBool + 63 padding bytes) as `T` — every read through a guard returns the lock's own bits. Same defect for `RwLock<T>` (`RwLockInner` reinterpreted at :185, :204) and `Mutex::get_mut`/`into_inner` (`as T` cast of `MutexInner` at :55, :61). `OnceLock` (struct with only `state`) has the same shape (no `get()` at all).
- Fix: add a `value: UnsafeCell<T>` field; `new` stores `t`; guards deref through `&*(*self.mutex.inner.get()).value.get()`-style access; `into_inner` returns `self.value.into_inner()`. Mirror for `RwLock` (read guards → `&T`, write guards → `&mut T`).
- Verify: fixture `let m = Mutex::new(42); assert_eq!(*m.lock(), 42);` through run-pass, or a unit test in `crates/glyim-lang-std/src/tests`.

---

### T014 [STD-2] [Critical] [BUG] "Atomic" types are non-atomic; `Mutex::lock` is a racy spin; `Ordering` ignored

- File: `crates/glyim-lang-std/lib/sync.g:347-355, 380-392, 416-424, 31-40`
- Code:
```glyim
fn compare_exchange(&self, current: bool, new: bool, order: Ordering) -> Result<bool, bool> {
    let cur = self.load(order);
    if cur == current { self.store(new, order); Result::Ok(new) } else { Result::Err(cur) }
}
```
- Problem: load-then-store is not atomic: two threads can both win `compare_exchange(false→true)` and both enter the critical section (`Mutex::lock` spins on this, sync.g:35). `fetch_add/fetch_sub` lose updates (`Barrier`, `Condvar.waiters`). `order` parameters are discarded (loads are plain reads). Also `Mutex::lock` declares `extern "C" fn glyim_mutex_lock` (sync.g:32-34) but never calls it — and the runtime exports no such symbol.
- Fix: register real `glyim_atomic_*` FFI (runtime `std::sync::atomic` ops) or lower these types to dedicated MIR intrinsics; make `lock()` call `glyim_mutex_lock(self.inner.get() as *const u8)`; delete the dead extern decl otherwise.
- Verify: fixture spawning 2 threads doing `n.fetch_add(1, SeqCst)` 1000× each, assert final count 2000 (currently intermittent fails).

---

### T015 [STD-3] [Critical] [STUB] `Condvar::notify_one/notify_all` never wake anyone

- File: `crates/glyim-lang-std/lib/sync.g:264-273`
- Code:
```glyim
fn notify_one(&self) {
    if self.waiters.load(Ordering::Relaxed) > 0 {
        self.waiters.fetch_sub(1, Ordering::Relaxed);
    }
}
```
- Problem: `wait` parks the thread (`thread::park()`, :260), but notify only decrements a counter — no `glyim_thread_unpark` is ever called, so every waiter sleeps forever. Also `waiters` is incremented before `drop(mutex_guard)`+park, so a notify between those steps is lost.
- Fix: track waiter thread-ids and call `glyim_thread_unpark(id)` in `notify_one` (one) / `notify_all` (loop), or implement via a runtime condvar FFI (`glyim_condvar_wait/notify`).
- Verify: fixture `m.lock(); condvar.notify_one()` after a parked waiter must return (currently hangs → timeout).

---

### T016 [PILOT-1] [Critical] [BUG] glyim-pilot: Unauthenticated WS + unvalidated `session_id` → worktree path/branch injection

- File: `tools/glyim-pilot/src/server/ws.rs:74-80`, `src/main.rs:251-296`, `src/git_ops/worktree.rs:14-15`
- Code:
```rust
// ws.rs — only loopback check, no token, no origin check
if !addr.ip().is_loopback() { ...continue; }
// main.rs OpsReady: stream_id comes straight from the WS message
let stream_id = session_id.clone();                       // attacker-controlled
let worktree_dir = worktree_base.join(format!("stream-{stream_id}")); // worktree.rs:14
let branch_name = format!("stream-{stream_id}/{branch_version}");     // worktree.rs:15
```
- Problem: Any local process (or any page/content-script chain that reaches the WS client) can connect to `ws://127.0.0.1:8420` and send `ops.ready` with an arbitrary `session_id`. That string is joined into the worktree path and git branch name with **no charset validation anywhere** (config, persistence, git_ops). `session_id = "../../tmp/evil"` → `git worktree add --detach ../glyim-worktrees/stream-../../tmp/evil main` creates a directory/worktree outside `worktree_base` (branch step then fails on `..` in refname, but the escaped dir + git metadata are already created). Spec NFR-SEC-002 only covers `::WRITE` paths (which *are* validated) — the worktree root itself is the unvalidated input. This tool drives file writes + `git`/`gh` on the developer machine, so this is Critical.
- Fix (3 steps):
  1. Add a startup-generated token: server writes `.glyim-pilot.token` (0600), extension reads it via a tiny local HTTP endpoint or file, sends `?token=` on WS connect; reject in `run()` if mismatch.
  2. Validate IDs once, centrally:
```rust
fn validate_id(s: &str) -> Result<(), PilotError> {
    let ok = !s.is_empty() && s.len() <= 64
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    ok.then_some(()).ok_or_else(|| PilotError::Session(format!("invalid session id {s:?}")))
}
// call in ws.rs after deserialize (both session_id and trace_id) before emitting ServerEvent::Message
```
  3. Add a unit test feeding `../../tmp/evil` as session id and asserting rejection.
- Verify: `cargo test -p glyim-pilot` + manual: `websocat ws://127.0.0.1:8420` send `{"type":"ops.ready","sessionId":"../../tmp/evil","content":"::DONE","turn":0,"v":1}` → expect reject log, no directory `../glyim-worktrees/stream-../../tmp/evil`.

---

**Wave 1 checkpoint:** `cargo build --workspace && cargo test --workspace && cargo test -p glyim-pilot 2>/dev/null; git add -A && git commit -m "fix(wave-1): T001-T016 criticals (const-prop, snapshots, arg typeck, async, drop FFI, drop-glue, bytecode wire, interp Drop, stdlib FFI/sync, pilot auth)"`
---

# WAVE 2 — High-severity fixes

## 2.1 Frontend (`glyim-frontend`)

### T017 [FE-101] [High] [BUG] `&&pat` double-`bump()` swallows the inner pattern's first token

- File: `crates/glyim-frontend/src/parser/pat.rs:35`
- Code:
```rust
SyntaxKind::AndAnd => {
    self.start_node(SyntaxKind::PatRef);
    self.bump(); // &
    self.start_node(SyntaxKind::PatRef);
    self.bump(); // &
    self.parse_pat_inner();
    self.finish_node();
    self.finish_node();
}
```
- Problem: `bump()` consumes whatever token is current; the lexer fuses `&&` into one `AndAnd` token. The first `bump()` eats the whole `"&&"`, the second `bump()` eats the **first token of the inner pattern** (e.g. the `x` of `&&x`), and `parse_pat_inner()` then errors on what follows (`=`, `=>`, …), producing cascading errors and a corrupted tree. `let &&y = &r;` → `PatRef{"&&", PatRef{"y"(bare)}}` + "expected pattern, found Eq" + "expected ';' after let statement". Compounding it: the single-`&` arm (line 44) never creates `PatRef` at all, and `lower_pat` (`crates/glyim-hir/src/lower/lower_pat.rs`) has **no** `PatRef` arm, so ref-pattern nodes lower to `None`.
- Fix (3 steps):
  1. Replace the arm with:
```rust
SyntaxKind::AndAnd => {
    self.start_node(SyntaxKind::PatRef);
    self.builder.token(GlyimLang::kind_to_raw(SyntaxKind::And), "&");
    self.skip_token(); // consume the fused `&&` without emitting it
    self.parse_pat_single(); // handles `&`/`&&`/`mut`/`ref` recursively
    self.finish_node();
}
```
  2. Wrap the single `And` arm (pat.rs:44-50) in `start_node(PatRef)` / `finish_node()` for consistency.
  3. In `lower_pat`, add `SyntaxKind::PatRef =>` lowering the inner pattern (binding-mode refinement can follow later).
- Verify: `cargo test -p glyim-frontend --lib pattern_parsing` plus a new test parsing `fn main() { let &&y = &r; }` and asserting `diagnostics.is_empty()`.

---

### T018 [FE-103] [High] [BUG] `&&expr` is unparseable in expressions

- File: `crates/glyim-frontend/src/parser/expr.rs:248`
- Code:
```rust
if matches!(
    self.current_kind(),
    SyntaxKind::Bang | SyntaxKind::Minus | SyntaxKind::Star | SyntaxKind::And
) {
    self.recursion_depth += 1;
    self.start_node(SyntaxKind::UnaryExpr);
    self.bump();
```
- Problem: The lexer fuses `&&` into `AndAnd`, so a double borrow `let r = &&x;` never matches the unary arm. It falls to `parse_primary_expr`'s `_ =>` arm → "expected expression, found AndAnd", the token is bumped, and `x` then triggers "expected ';' after let statement" — two errors and no unary nodes. Only `&(&x)` parses.
- Fix: add an `AndAnd` arm to `parse_unary_expr` that mirrors ty.rs: emit two nested `UnaryExpr` nodes with synthetic `&` tokens and skip the fused token (this shape already lowers correctly — `lower_unary_expr` reads the first `And` token and recurses into inner expr nodes):
```rust
SyntaxKind::AndAnd => {
    self.recursion_depth += 1;
    self.start_node(SyntaxKind::UnaryExpr);
    self.builder.token(GlyimLang::kind_to_raw(SyntaxKind::And), "&");
    self.start_node(SyntaxKind::UnaryExpr);
    self.builder.token(GlyimLang::kind_to_raw(SyntaxKind::And), "&");
    self.skip_token(); // consume the fused `&&`
    if self.current_kind() == SyntaxKind::KwMut { self.bump(); }
    self.parse_unary_expr();
    self.finish_node();
    self.finish_node();
    self.recursion_depth -= 1;
    return;
}
```
- Verify: add a frontend unit test parsing `fn main() { let r = &&x; }` asserting zero diagnostics; `cargo test -p glyim-frontend --lib`.

---

### T019 [FE-104] [High] [BUG] `&&mut T` / `&&'a T` types error and orphan the pointee

- File: `crates/glyim-frontend/src/parser/ty.rs:8`
- Code:
```rust
SyntaxKind::AndAnd => {
    if let Some(_tok) = self.current() {
        self.start_node(SyntaxKind::RefType);
        self.builder.token(GlyimLang::kind_to_raw(SyntaxKind::And), "&");
        self.start_node(SyntaxKind::RefType);
        self.builder.token(GlyimLang::kind_to_raw(SyntaxKind::And), "&");
        self.skip_token();
        self.parse_type();
        self.finish_node(); // inner
        self.finish_node(); // outer
    }
}
```
- Problem: After the inner `&`, `parse_type()` is called with `mut`/`'a` pending — neither is handled, so `KwMut` hits the `_ =>` arm ("expected type, found KwMut", bumps `mut`), the pointee `T` is left unconsumed, and the enclosing construct (param, field, cast…) error-cascades. The single-`&` arm (line 22) handles `mut`/lifetime in a loop; the `AndAnd` arm bypasses that logic entirely.
- Fix: run the same mut/lifetime loop after the inner synthetic `&`:
```rust
self.start_node(SyntaxKind::RefType);
self.builder.token(GlyimLang::kind_to_raw(SyntaxKind::And), "&");
self.skip_token();
loop {
    if self.current_kind() == SyntaxKind::KwMut { self.bump(); continue; }
    if self.current_kind() == SyntaxKind::Lifetime { self.bump(); continue; }
    break;
}
self.parse_type();
```
- Verify: unit test parsing `fn f(x: &&'a mut T) {}` (and `&&mut u8`) asserting zero diagnostics; `cargo test -p glyim-frontend --lib`.

---

### T020 [FE-105] [High] [BUG] Generic args on the first path segment don't parse in expression position — `Vec<u8>::new()`

- File: `crates/glyim-frontend/src/parser/expr.rs:825` (with caller parse_path_expr at :461)
- Code:
```rust
while self.current_kind() == SyntaxKind::ColonColon {
    self.bump(); // ::
    // After :: we can have an identifier, generic args, or (for use statements) * or {
    match self.current_kind() {
        ...
        SyntaxKind::Lt => {
            self.parse_type_arg_list();
        }
```
- Problem: The loop only continues on `::`, and nothing handles `Lt` after the **first** segment. `Vec<u8>::new()` parses the path as just `Vec`, then `<` is not a postfix operator, so the expression ends and `<u8>::new()` error-cascades ("expected ';' after expression", "unexpected token in statement", …). Only the turbofish form `Vec::<u8>::new()` works. (Type position is fine because ty.rs re-checks `Lt` after `parse_path`.)
- Fix: in `parse_path_expr` (expr.rs:461-466), accept generic args and continuation after `parse_path`:
```rust
pub(crate) fn parse_path_expr(&mut self) {
    self.start_node(SyntaxKind::PathExpr);
    self.parse_path();
    // value paths may carry generic args on any segment
    // (`Vec<u8>::new()`); `parse_path` stops at the first `<`.
    loop {
        match self.current_kind() {
            SyntaxKind::Lt => self.parse_type_arg_list(),
            SyntaxKind::ColonColon => {
                self.bump();
                if matches!(self.current_kind(),
                    SyntaxKind::Ident | SyntaxKind::KwSelf
                    | SyntaxKind::KwSuper | SyntaxKind::KwCrate)
                { self.bump(); } else { break; }
            }
            _ => break,
        }
    }
    self.finish_node();
    self.last_was_path = true;
}
```
- Verify: `cargo test -p glyim-typeck --lib multi_seg_path value_path` plus a new frontend test on `fn main() { let v = Vec::<u8>::new(); let w = Vec::new(); }` — and a case with `Vec<u8>::new()` asserting zero diagnostics once fixed.

---

### T021 [FE-106] [High] [MISCOMPILE] `&`, `^`, `|`, `<<`, `>>` share one precedence level — silently wrong arithmetic

- File: `crates/glyim-frontend/src/parser/expr.rs:171`
- Code:
```rust
pub(crate) fn parse_bitwise_expr(&mut self) {
    // See `parse_or_expr` — checkpoint taken once, never reset.
    let cp = self.checkpoint();
    self.parse_additive_expr();
    while matches!(
        self.current_kind(),
        SyntaxKind::And
            | SyntaxKind::Or
            | SyntaxKind::Caret
            | SyntaxKind::Shl
            | SyntaxKind::Shr
    ) {
```
- Problem: One left-associative loop over all five operators flattens Rust's three distinct levels (`<<`/`>>` > `&` > `^` > `|`). `4 | 1 & 3` parses as `(4 | 1) & 3` = **1** where Rust (and any Rust-like semantics) computes `4 | (1 & 3)` = **5** — no diagnostic, wrong runtime result. Same for `a ^ b & c` and `a & 1 << 2` (here `(a & 1) << 2`, Rust `a & (1 << 2)`). The run-pass corpus (`chained_bitand.g`, `chained_shift.g`, `mixed_precedence.g`) only exercises single-operator chains, so nothing catches it.
- Fix: split into three levels between comparison and additive, e.g. insert `parse_bit_or_expr` → `parse_bit_xor_expr` → `parse_bit_and_expr` → `parse_shift_expr` → `parse_additive_expr`, each with the same checkpoint/while pattern (`parse_bit_or_expr` loops on `Or`, `parse_bit_xor_expr` on `Caret`, `parse_bit_and_expr` on `And`, `parse_shift_expr` on `Shl|Shr`), and have `parse_comparison_expr` call `parse_bit_or_expr`.
- Verify: add run-pass tests: `fn main() -> i32 { 4 | 1 & 3 }` (exit 5) and `fn main() -> i32 { 2 & 1 << 1 }` (exit 2); `cargo test -p glyim-test run_pass_corpus`.

---

### T022 [FE-107] [High] [BUG] Negative literals rejected in patterns, incl. negative ranges

- File: `crates/glyim-frontend/src/parser/pat.rs:187`
- Code:
```rust
SyntaxKind::IntLit
| SyntaxKind::FloatLit
| SyntaxKind::StringLit
| SyntaxKind::CharLit
| SyntaxKind::KwTrue
| SyntaxKind::KwFalse => {
    let start_cp = self.checkpoint();
    self.bump(); // consume start literal
```
- Problem: a `-1` pattern starts with `Minus`, which falls to the `_ =>` arm ("expected pattern, found Minus") and is bumped; the `1` then breaks the enclosing construct (`=>` check fails, etc.). `match x { -1 => … }` and range patterns `-10..=-1` are unparseable, while the macro layer accepts them (`is_fragment_literal` in parser/mod.rs:416 explicitly allows `Minus + literal` for `:literal`) — inconsistent. `ByteLit` is also missing from this list (`b'a'..b'z'` patterns fail).
- Fix: in `parse_pat_inner_impl`, accept an optional leading `Minus` before the literal group: on `SyntaxKind::Minus` with a literal at `peek_kind()`, `bump()` the minus and proceed with the same literal/range logic (keeping the `Minus` token inside `PatLit`/`PatRange`), and add `SyntaxKind::ByteLit` to both literal matches.
- Verify: `cargo test -p glyim-frontend --lib pattern_parsing` plus a new test `fn main() { match x { -1 => 1, _ => 0 } }` asserting zero diagnostics; a typeck run-pass with `match -2 { -2 => 1, _ => 0 }` returning 1.

---

## 2.2 THIR→MIR lowering, MIR passes, interpreter

### T023 [LOW-1] [High] [BUG] Move semantics unmodeled: non-Copy values lower as `Copy`, drops run only at function fall-through (double-drop / leak / early-return leak)

- File: `crates/glyim-lower/src/lower_rvalue.rs:166` (`VarRef` → `Operand::Copy`), :61-68 (`Stmt::Return`); `crates/glyim-lower/src/builder.rs:222-300` (`elaborate_scope_drops`)
- Code:
```rust
// lower_rvalue.rs:166 — every variable read is a Copy, even non-Copy:
glyim_mir::Rvalue::Use(glyim_mir::Operand::Copy(place))
// builder.rs:295 — drops chained only on the single fall-through return:
self.basic_blocks[fall_through].terminator = glyim_mir::Terminator {
    kind: glyim_mir::TerminatorKind::Goto { target }, ...
```
- Problem: (a) `let a = s;` (s owning, e.g. `String`, which `TyCtx::needs_drop` reports true) lowers to `Copy(s)` — both `a` and `s` own the value and `elaborate_scope_drops` drops **both** at scope exit → double drop; (b) `return e;` / `return;` mid-body terminates straight to `Return` with **zero** drops (all in-scope owners leak); (c) `let s = make();` inside a loop body assigns into the same MIR local each iteration without dropping the previous value → per-iteration leak; (d) block/loop-scoped locals are dropped at function exit, not scope exit. Impact today is muted because `generate_drop_glue` for `TyKind::String` falls into the no-op `_ => Return` arm (`crates/glyim-pipeline/src/mono_cache.rs`), but the MIR is semantically wrong and becomes a double-free the moment real destructors land.
- Fix (3 steps):
  1. Make `Stmt::Return`/`ExprKind::Return` jump to a per-body epilogue block that runs the same drop chain as `elaborate_scope_drops` before `Return`.
  2. In `lower_stmt`'s `Let`, emit a `Drop` of the old value of a re-assigned local before the `Assign` (or track may-init and route through drop-elab flags).
  3. Longer term, thread a THIR move/copy distinction so `VarRef` of a non-`Copy` emits `Operand::Move` and DCE/borrowck see real moves.
- Verify: `target/debug/glyim-cli t.g --emit=mir` with `fn f() -> i32 { let s = "x"; if c { return 1; } 0 }` — today the `return 1` block has no `Drop` chain; after the fix all exits route through the epilogue. `cargo test -p glyim-lower drop_elaboration`.

---

### T024 [LOW-2] [High] [MISCOMPILE] Out-of-order struct-literal fields are kept in source order → field values swapped

- File: `crates/glyim-typeck/src/check_expr.rs:1917-1925` (THIR builds `thir_fields` in source order); `crates/glyim-lower/src/lower_rvalue.rs:892-895`
- Code:
```rust
// lower_rvalue.rs:892
let mut mir_operands = Vec::new();
for (_name, field_expr) in fields {
    mir_operands.push(self.lower_expr_to_operand(field_expr));
}
```
- Problem: `struct P { x: i32, y: i32 }` and `P { y: 2, x: 1 }` — the aggregate operands are `[2, 1]` (source order) while ADT layout/`Field(0)`/`Field(1)` follow declaration order, so `p.x == 2`, `p.y == 1`. Nothing in typeck reorders (`for &(field_name, field_expr_id) in fields` pushes in source order) and no test covers out-of-order literals (all run-pass fixtures are in-order), which is why audit-status's LL-6 "verified fixed" passed — it never exercised the swap.
- Fix: in the `ExprKind::Struct` lowering arm, resolve each field's index via `ctx.field_index_by_name(*adt_id, *variant_idx, name)` and place each operand at `mir_operands[idx]` of a `vec.resize(field_count, Unit)` vector (mirroring `bind_pattern`'s `Struct` arm which already resolves by name).
- Verify: add `tests/run-pass/struct_field_order.g` with `let p = P { y: 2, x: 1 }; assert(p.x == 1)`; `cargo test -p glyim-test -- run-pass struct_field`.

---

### T025 [LOW-3] [High] [STUB] `?` operator is lowered identically to its operand — no discriminant check, no early return

- File: `crates/glyim-lower/src/lower_rvalue.rs:1086-1097`
- Code:
```rust
thir::ExprKind::Try { expr: inner } => {
    // ... MIR lowering evaluates the operand and (as a first-cut) yields its
    // success value directly. A full lowering of `?` into a
    // discriminant-check + early-return terminator pair is a codegen follow-up
    self.lower_expr_to_rvalue(inner)
}
```
- Problem: typeck produces `ExprKind::Try` for real user code (`crates/glyim-typeck/src/check_expr.rs:2432`) and types the expression as the *success* type `T`, but lowering emits the whole `Result<T,E>`/`Option<T>` value. `let x = foo()?;` binds the entire enum to a `T`-typed local — a silent miscompile (tag read as payload), not just a missing feature.
- Fix: lower `Try` as: materialize operand into `_t`; `_d = Discriminant(_t)`; `SwitchInt(_d)` → Err/None variant ⇒ `Assign(_0, <error value>)` + `Return`, Ok variant ⇒ `Assign(dest, Copy(Downcast(0).Field(0)))`. Keep `result_ty` from `expr.ty`.
- Verify: `cargo test -p glyim-lower` with a THIR-builder test asserting the lowered body contains a `Discriminant` + `Return` arm for `Try`; `target/debug/glyim-cli t.g --emit=mir` on a program using `?`.

---

### T026 [LOW-4] [High] [BUG] Slice-pattern matches never bind pattern variables

- File: `crates/glyim-lower/src/lower_rvalue.rs:1700-1719`
- Code:
```rust
if !slice_dispatch {
    let scrut_local = self.alloc_local(scrutinee.ty, ...);
    ...
    self.bind_pattern(&arm.pat, Some(scrut_local), arm.pat.span);
}
```
- Problem: `slice_dispatch` is true whenever the scrutinee is a slice/array and any arm is a `Slice` pattern — exactly the matches whose `Slice`/`Binding` patterns need binding. The binding step is skipped, so `match arr { [a, b] => a + b, _ => 0 }` resolves `a`/`b` via the `local_for_var` fallback `LocalIdx::from_raw(var_id.to_raw())` (lower_rvalue.rs:1166-1174) — an unrelated MIR local (wrong value) or an index ≥ `locals.len()` (interpreter/`Place::ty` OOB panic). The existing test only asserts `Len`+`SwitchInt` with a `Unit` arm body (tests/slice_patterns.rs:124-138), so this is uncovered.
- Fix: delete the `!slice_dispatch` guard around the `scrut_local` materialization + `bind_pattern` call (bind always; the `Slice` arm of `bind_pattern` uses `ConstantIndex`/`Subslice` projections that slice_desugar already normalizes). Keep the switch on `Len` as-is.
- Verify: `cargo test -p glyim-lower slice_patterns` extended with a `Binding` arm body reading `a`; end-to-end `cargo test -p glyim-test -- slice` after adding a run-pass fixture `match s { [a, b] => a + b, ... }`.

---

### T027 [LOW-5] [High] [PERF] Range patterns expand every covered value into the `SwitchInt` — user-triggerable compiler hang/OOM

- File: `crates/glyim-lower/src/lower_rvalue.rs:1781-1783`
- Code:
```rust
for v in s_val..=end_val {
    targets.push((v, arm_bb));
}
```
- Problem: `match x { 0..=18446744073709551615 => .., _ => .. }` (or `i64::MIN..i64::MAX`) pushes ~1.8e19 `(u128, bb)` pairs — the compiler effectively hangs/OOMs. Even modest ranges (`0..=1_000_000`) allocate a million switch branches. The unit test's own doc locks in per-value expansion ("Range patterns (`0..=9`) lower to a `SwitchInt` over the covered values", tests/range_patterns.rs:3-5) with no cap.
- Fix: cap expansion (e.g. >256 values ⇒ emit a chain of range comparisons): if `end_val - s_val > 256`, emit `cond = (discr >= s) & (discr <= e)` as a boolean `SwitchInt` to `arm_bb` instead of per-value branches (fall through to the next arm otherwise).
- Verify: `printf 'fn main() -> i32 { match 5 { 0..=18446744073709551615 => 1, _ => 2 } }\n' > /tmp/big.g && timeout 10 target/debug/glyim-cli /tmp/big.g --emit=mir` — today it never finishes; after the fix it completes instantly.

---

### T028 [LOW-6] [High] [BUG] Assignment through a bare deref (`*r = v`) writes to a fresh temp — the write is silently lost

- File: `crates/glyim-lower/src/lower_rvalue.rs:1213-1307` (`lower_expr_to_place` has no `Unary`/`Deref` arm; fallback at :1295-1305 returns a temp); THIR shape: `crates/glyim-typeck/src/check_stmt.rs:379-401` + `check_expr.rs:261-276` (`*r` is `ExprKind::Unary { op: UnOp::Deref }`)
- Code:
```rust
// lower_stmt::Assign (lower_rvalue.rs:57)
let place = self.lower_expr_to_place(lhs);   // lhs = Unary{Deref} → fallback temp!
let rvalue = self.lower_expr_to_rvalue(rhs);
self.push_stmt(glyim_mir::StatementKind::Assign(place, rvalue), *span);
```
- Problem: `*r = 5` first evaluates `UnaryOp(Deref, r)` into a **new temp local** (the fallback), then assigns `5` to that temp. The pointee is never written. Reads of `*r` work (`eval_unary_op` handles `UnOp::Deref`), writes don't; no fixture covers `*p = v` (grep over all `.g` fixtures finds none), so it is latent but any user program using it silently misbehaves.
- Fix: add a `Unary { op: UnOp::Deref, operand }` arm to `lower_expr_to_place`:
```rust
thir::ExprKind::Unary { op: thir::UnOp::Deref, operand } => {
    let base = self.lower_expr_to_place(*operand);
    return self.place_with_projection(base, ProjectionElem::Deref);
}
```
— that makes the assign destination `Deref(r)`, which `write_place`/codegen already implement (`crates/glyim-mir-interp/src/lib.rs:1315-1389`).
- Verify: run-pass fixture `let mut x = 0; let r = &mut x; *r = 5; exit x` → exit code 5; `cargo test -p glyim-lower` with a THIR test whose `Stmt::Assign` lhs is `Unary{Deref}` asserting the destination place's projection is `[Deref]`.

---

### T029 [LOW-7] [Medium→raised: High] [MISCOMPILE] `&arr[a..b]` on a fixed-size array uses the first *element value* as the data pointer

- File: `crates/glyim-lower/src/lower_rvalue.rs:2143-2158`
- Code:
```rust
let first_elem_place = if matches!(self.ctx.ty_ctx().ty_kind(base_ty), TyKind::Array(_, _)) {
    let mut proj = base_place.projection.to_vec();
    proj.push(ProjectionElem::ConstantIndex { offset: 0, min_length: 0, from_end: false });
    ...
let data_ptr_ptr = ...; // assigned Operand::Copy(first_elem_place) — the element VALUE
```
- Problem: For a slice base, `Field(0)` reads the fat pointer's data pointer (the documented slice representation), but for an **array** base `ConstantIndex(0)` reads `arr[0]`'s value; the lowered fat pointer becomes `{ data: arr[0] + start*elem_size, len: end-start }` — subsequent indexing dereferences an attacker-uncontrolled "address" (segfault/garbage) in both interp and codegen. Only the slice path is exercised by tests (tests/dynamic_range_slice.rs).
- Fix: for the array base, take the array's address instead of element 0: emit the place `base_place` and have the backend/interp support an `AddrOf` rvalue, or (mechanically, no new rvalue) lower `&arr[a..b]` on arrays by first binding `&arr[..]` through the existing slice path: `let tmp = Ref(base_place)` then reuse the slice-arm `Field(0)` on `Deref(tmp)` so the "data pointer" read is the fat-ptr field of a slice, not `arr[0]`.
- Verify: run-pass fixture `let arr = [1,2,3,4]; let s = &arr[1..3]; exit(s[0])` → 2; today it crashes/garbage; `cargo test -p glyim-lower dynamic_range_slice` extended with an array base.

---

### T030 [LOW-15] [High] [BUG] Closure parameters are allocated twice; the body reads the never-initialized second set

- File: `crates/glyim-lower/src/builder.rs:414-425` (`lower_closure` pre-allocates params) vs :148-169 (`lower_body` re-allocates them and overwrites `local_var_map`)
- Code:
```rust
// lower_closure: locals C+1..=C+P hold the incoming args, mapped into local_var_map
builder.param_map.insert(param.local, local);
builder.local_var_map.insert(param.local, local);
// then builder.lower_body() runs and RE-allocates:
let local = self.alloc_local(param.ty, Mutability::Not, param.span);  // C+P+1..
self.local_var_map.insert(*var_id, local);   // overwrites the mapping
```
- Problem: call args (captures then params) land in locals `1..=C+P` (the first set — `callee_locals[i + 1] = Some(val)` in the interpreter, and the same convention in codegen's `arg_count = captures + params`), but `lower_body`'s re-allocation plus `local_var_map.insert` retargets every `VarRef` of a closure parameter to the second set (indices `> arg_count`), which `StorageLive` explicitly wipes (interp `execute_statement`: locals `> arg_count` reset to `None`). Any closure **with parameters** reads uninitialized locals: `let f = |x: i32| x + 1;` → interp error "read from uninitialized local"; codegen reads a never-stored alloca. No fixture exercises closure params (all closure tests are hand-built THIR with `params: vec![]`), which is why it is latent.
- Fix: in `lower_body`, skip re-allocating params when `param_map` already contains `param.local` (reuse the pre-allocated local and emit only `StorageLive`-guard + pattern binding against it):
```rust
let local = if let Some(&existing) = self.param_map.get(&param.local) {
    existing
} else {
    let l = self.alloc_local(param.ty, Mutability::Not, param.span);
    self.param_map.insert(param.local, l);
    l
};
```
or move the param allocation out of `lower_closure` and let `lower_body` own it (allocating captures first).
- Verify: extend `crates/glyim-pipeline/tests/closure_byref_runtime.rs` with a closure that takes an `i32` param and returns it; today it errors with "read from uninitialized local", after the fix it returns the passed value. `cargo test -p glyim-lower closure`.

---

### T031 [INT-1] [High] [BUG] Interpreter `Rvalue::Ref` discards projections — `&x.field` / `&arr[i]` alias the whole local

- File: `crates/glyim-mir-interp/src/lib.rs:660-666` (creation), `1062-1091` (deref read), `1315+` (deref write)
- Code:
```rust
Rvalue::Ref(place, _borrow_kind) => {
    let local_idx = place.local.index();
    Ok(InterpValue::Ref { frame: self.frame_depth, local: local_idx })
    // place.projection is dropped on the floor
}
```
- Problem: `let r = &mut p.x; *r = 5;` creates `Ref{frame, local: p}`; the subsequent `write_place([Deref])` overwrites the **whole struct** `p` with `Int(5)` (`target_frame_locals[target.1] = Some(val)`), and `*r` reads back the whole struct where `i32` is expected (later `Field` projection on a scalar → `InterpError::Panic("field projection on non-aggregate")`). Same for `&arr[i]` (whole array aliased). Codegen is unaffected, so interp and native builds disagree.
- Fix: extend `InterpValue::Ref` with `proj: Vec<ProjectionElem>` (clone from the `Rvalue::Ref` place), and in `read_place`/`write_place` `Deref` handling rebuild the target place as `Place{local: target, projection: ref.proj.clone()}` before recursing into `read_place`/`write_place_frame` (the projection-aware recursion already exists — `write_place_frame` `proj_count > 1` branch).
- Verify: `cargo test -p glyim-mir-interp ref_test` extended with `&p.x` write/read; run-pass fixture `let mut p = Point{x:1,y:2}; let r = &mut p.x; *r = 5; exit(p.y)` → 2 (today errors/corrupts).

---

## 2.3 Type system, solver, typeck, const-eval

### T032 [SOLVE-24] [High] [BUG] Unification ignores reference mutability — `&T` unifies with `&mut T` in both directions

- File: `crates/glyim-solve/src/infer.rs:447-451`
- Code:
```rust
(TyKind::Ref(r_a, ty_a, _mut_a), TyKind::Ref(r_b, ty_b, _mut_b)) => {
    // Reference mutability is not a hard unification constraint
    // in this compiler (`&Vec<u8>` vs `&[u8]`, `&mut T` vs `&T`
    // at FFI boundaries, …). Unify the pointees with a
    // deref-coercion attempt first.
```
- Problem: `let r: &mut i32 = &x;` is accepted by `check_stmt.rs:255/280/396` (which call this unify) — a shared borrow flows into a `&mut` slot; combined with T003 (unchecked call args) `fn f(m: &mut i32)` called as `f(&x)` puts a `&i32` into a `&mut`-typed parameter → MIR `Ref(Shared)` where `Ref(Mut)` is expected → mutation through an immutable borrow, invisible to borrowck. No downstream pass re-checks mutability (the only mutability-sensitive unify left is `solver.rs:295-299` `tys_match`, used by nothing on this path).
- Fix: in this arm require `mut_a == mut_b || (mut_a == Mut && mut_b == Not)`; when `mut_a == Mut && mut_b == Not` (upgrade coercion being attempted by a coercion *caller*), keep succeeding but carry the information: emit `Constraint::RefMutUpgrade { a, b }` or return a marker so `FnCtxt::unify` can split "coercion OK" from "unify OK"; FFI cases that intentionally flip mutability must go through an explicit cast.
- Verify: `/tmp/t.g` = `fn main() { let x = 5; let r: &mut i32 = &x; }` → must error E0388-style; `cargo test -p glyim-solve -p glyim-typeck` green (the stdlib's `&mut`-reborrow-as-`&` cases go through the allowed direction).

---

### T033 [SOLVE-25] [High] [MISCOMPILE] Two distinct unit-like ADTs unify — `let x: A = B;` accepted across different types

- File: `crates/glyim-solve/src/infer.rs:712-722`
- Code:
```rust
let unit_a = ctx.adt_def(id_a).map(|d| {
    d.fields.is_empty()
        && d.variants.iter().all(|v| v.fields.is_empty())
}).unwrap_or(false);
...
if unit_a && unit_b {
    return Ok(Vec::new());
}
```
- Problem: any two fieldless structs (and any *fieldless enums* — for `enum Color { R, G }` the ADT-level `fields` is empty and both variants are fieldless) unify. `struct A; struct B; fn main() { let x: A = B; }` type-checks, and `let c: Color = SomeUnitStruct;` too. The value is then read/written under the wrong layout/discriminant semantics (enum discriminant read from a ZST slot = garbage). This "Script 422" workaround exists for macro-expanded empty literals in io.g — it should not be a unification rule.
- Fix: delete the `unit_a && unit_b` equivalence arm (keep `id_a != id_b → Err`); for the io.g macro case, type the expanded empty literal as `Ty::UNIT` at the macro/lowering site instead.
- Verify: `cargo build -p glyim-cli`; `/tmp/t.g` above must produce a mismatched-types error; confirm io.g/`print!` still compiles (`cargo test --workspace`).

---

### T034 [TCK-25] [High] [MISCOMPILE] `usize::MAX` / `u32::MAX` / `i32::MIN` … evaluate to literal **0**

- File: `crates/glyim-typeck/src/unify.rs:459-477`
- Code:
```rust
// The literal's actual value is not materialized here
// (const-eval produces the real value at MIR). Emit a
// literal placeholder with the right *type* — that's all
// `check_expr` callers consume.
let lit = if is_uint {
    thir::Literal::Uint(0, None)
} else {
    thir::Literal::Int(0, None)
};
```
- Problem: the THIR node is a `Literal` — the path name is gone, and `glyim-lower/src/lower_rvalue.rs:1521` lowers `thir::Literal::Uint(val)` to `MirConst::Uint(val)`, so the value really is 0 at runtime. `usize::MAX` (used in `crates/glyim-lang-alloc/lib/alloc.g:21` as `if size > usize::MAX - (align - 1)`) becomes `size > 0 - 7` → wraps → overflow check never fires. `i32::MIN` is also 0. The comment's claim that const-eval re-materializes the value is false for expression paths (only `ConstRef` nodes are re-evaluated, and this node is not one).
- Fix: emit the real value here:
```rust
let lit = match (path_name.as_str(), is_uint) {
    ("MAX", true)   => thir::Literal::Uint(uint_max_for(ty), None),
    ("MIN", false)  => thir::Literal::Int(int_min_for(ty) as i128, None),
    ("MAX", false)  => thir::Literal::Int(int_max_for(ty) as i128, None),
    _ => return None, // fall through to normal path checking
};
```
or, better, add `thir::ExprKind::BuiltinConst { prim, min_max }` and lower it in `lower_rvalue.rs` so the value is materialized once.
- Verify: `fn main() { let x = usize::MAX; if x == 0 { print("BUG"); } }` — after fix `x != 0`; add unit test asserting `usize::MAX - 1`-style folding in `lower_rvalue` tests.

---

### T035 [CE-24] [High] [MISCOMPILE] Two conflicting bit conventions for `FloatBits` — f32 const casts are read back as garbage f64

- File: `crates/glyim-const-eval/src/eval.rs:1404-1409` (producer writes **f32 bits**) vs `crates/glyim-const-eval/src/value.rs:156-161, 172-217` (all consumers read **f64 bits**); literals store f64 bits (`crates/glyim-hir/src/lower/lower_expr.rs:1104-1108`)
- Code:
```rust
"f32" => val
    .as_f64()
    .map(|v| {
        ConstValue::FloatBits((v as f32).to_bits() as u64, FloatTy::F32)
    })
```
```rust
pub fn as_f64(&self) -> Option<f64> {
    match self {
        ConstValue::FloatBits(bits, _) => Some(f64::from_bits(*bits)),
```
- Problem: `FloatBits(_, F32)` produced by a cast stores *f32* bit patterns, but every consumer (`as_f64`, `checked_add/sub/mul/div`, comparisons) reinterprets `bits` with `f64::from_bits`. In a const: `(1u8 as f32) as f64` → `f64::from_bits(0x3F800000)` ≈ 2.0e-314, and `const A: f32 = ...; const B: f32 = A + A;` computes garbage in f64 space then stores f64 bits under an F32 tag. (Literals never hit this because the HIR always stores f64 bits with tag F64.)
- Fix: pick one convention — the smaller diff is a tag-aware accessor:
```rust
pub fn as_f64(&self) -> Option<f64> {
    match self {
        ConstValue::FloatBits(bits, FloatTy::F32) =>
            Some(f32::from_bits(*bits as u32) as f64),
        ConstValue::FloatBits(bits, FloatTy::F64) => Some(f64::from_bits(*bits)),
        ConstValue::Float(v) => Some(*v),
        _ => None,
    }
}
```
and make all writers store f64 bits (change the f32 cast arm to `ConstValue::FloatBits(v.to_bits(), FloatTy::F32)` where `v: f64`), rounding only at materialization.
- Verify: `const B: f64 = (1u8 as f32) as f64;` with `B == 1.0` must hold; add const-eval unit test for f32 add/sub after cast.

---

### T036 [TY-24] [High] [MISCOMPILE] `TyCtx::needs_drop` ignores the ADT's substitution — generic structs holding droppable types lose their drop glue

- File: `crates/glyim-type/src/ty_ctx.rs:631-648`
- Code:
```rust
TyKind::Adt(adt_id, _substs) => {
    if self.has_drop_impl(*adt_id) { return true; }
    match self.adt_def(*adt_id) {
        Some(adt) => {
            ...
            adt.variants.iter().any(|v| v.fields.iter().any(|f| self.needs_drop_rec(f.ty, visited)))
```
- Problem: for `struct Holder<T> { v: T }` the declared field type is `Param(T)`, so `needs_drop(Holder<String>)` walks the *formal* `Param` (→ `false`) and ignores `_substs = [String]`. `glyim-lower/builder.rs:233+308` (`elaborate_scope_drops` → `TyCtx::needs_drop`) then emits no `Drop` terminator, `glyim-opt/drop_elaboration.rs:127,372` allocates no drop flag, and `glyim-pipeline/mono_cache.rs:487-506` (`generate_struct_drop_glue` filters fields by the same predicate) generates empty glue → the `String`/destructor inside is never dropped (leak / skipped `Drop` bodies). Note `glyim-codegen-llvm/lower.rs:3948-3957` has a *third* copy of this logic that also ignores substs and `drop_impls` — the three implementations disagree.
- Fix: in the `Adt` arm, build `map: param_index -> GenericArg` from `_substs` (positions align with `adt_def.generic_params`), then test `self.needs_drop_rec(self.field_ty_substituted(f.ty, &map), visited)` — e.g. resolve `Param(p)` to `substitution_args(_substs)[p.index]` before recursing; keep `has_drop_impl` check first. Delete the codegen-llvm twin or route it through `TyCtx::needs_drop`.
- Verify: `struct Holder<T>{v:T}` + `struct Loud; impl Drop for Loud { ... }` (after T037) — drop glue must appear; add unit test in `crates/glyim-type/src/tests/needs_drop.rs`: `needs_drop(Holder<String>) == true`.

---

### T037 [TY-25] [High] [MISCOMPILE] User `impl Drop for X` is never registered — user destructors never run

- File: `crates/glyim-typeck/src/lib.rs` (impl-registration loop — zero occurrences of `"Drop"`/`mark_has_drop` in the whole crate); `crates/glyim-type/src/ty_ctx_mut.rs:1346`
- Code: `mark_has_drop` is called only for builtins:
```rust
self.mark_has_drop(AdtId::from_raw(1020)); // Vec
self.mark_has_drop(AdtId::from_raw(1040)); // Box
self.mark_has_drop(AdtId::from_raw(1050)); // String
```
- Problem: `needs_drop(X)` for a user type with `impl Drop for X` and no droppable fields returns `false` (no `drop_impls` entry, fields trivial) → `glyim-lower/builder.rs:233` emits no `Drop` terminator and `glyim-opt/drop_elaboration.rs:372` turns any existing one into `Goto` — the destructor body is dead. Even when glue *is* generated (droppable fields), `mono_cache.rs::generate_drop_glue` only recurses into fields; nothing in the pipeline ever emits a call to the impl's `drop` method (the `impl Drop for MutexGuard` in `sync.g:75` never unlocks its mutex). The `Drop` trait id (2002) is registered (ty_ctx_mut.rs:1975) but never consulted.
- Fix (2 steps):
  1. In typeck's impl scan, when the impl's trait resolves to the `Drop` TraitDefId, call `ctx.mark_has_drop(self_adt_id)`.
  2. In `generate_drop_glue`/drop terminator lowering, look up `TyCtx::resolve_trait_method(Drop, ty, "drop")` and emit a call to it (passing `&mut place`) before/instead of field-wise glue.
- Verify: `struct Loud; impl Drop for Loud { fn drop(&mut self) { print("dropped"); } } fn main() { let l = Loud; }` with `--emit=exec` must print `dropped`; same for a `MutexGuard` unlock test.

---

## 2.4 Bytecode backend, VM, runtime

### T038 [BC-2] [High] [MISCOMPILE] `OP_REPEAT` pops value and count in the wrong order — `[x; N]` builds an N-element array of garbage / empty array

- File: `crates/glyim-codegen/src/lib.rs:938-942` (emitter); `crates/glyim-bytecode-vm/src/lib.rs:560-568` (VM)
- Code:
```rust
// emitter: value FIRST, count SECOND (top of stack)
bc.push(OP_REPEAT); self.emit_operand(bc, operand, ...)?;   // value
self.emit_operand(bc, &Operand::Constant(mir_const), ...)?; // count
// VM:
let value = self.pop()?;              // ← actually the count
let count = self.pop()?.as_int();     // ← actually the value
```
- Problem: `[0u8; 3]` pops 3 as `value` and 0 as `count` → produces an empty tuple instead of a 3-element array; lengths and element values are swapped for every `Repeat` (array-repeat literals, `vec![x; n]`-style MIR).
- Fix: Swap the two pops in the VM (`let count = self.pop()?.as_int(); let value = self.pop()?;`) or reverse the emit order in the emitter; add a wire-format comment. Prefer fixing the VM to keep golden-opcode tests stable.
- Verify: `cargo test -p glyim-bytecode-vm run_repeat` (add): encode `LoadConst 7; LoadConst 3; Repeat; Len` and assert `3` (today yields 0).

---

### T039 [BC-3] [High] [MISCOMPILE] Enum discriminants: emitter never writes a tag; VM returns tuple arity

- File: `crates/glyim-codegen/src/lib.rs:902-909` (Aggregate ignores `AggregateKind::Adt(.., variant, ..)`); `crates/glyim-bytecode-vm/src/lib.rs:536-543`
- Code:
```rust
// emitter: variant is discarded — no tag is ever pushed/stored
Rvalue::Aggregate(_, operands) => {
    bc.push(OP_AGGREGATE); bc.extend_from_slice(&(operands.len() as u32).to_le_bytes());
    for o in operands { self.emit_operand(bc, o, local_tys)?; }
}
// VM:
Opcode::Discriminant => {
    let discr = match &v { Value::Tuple(elems) => elems.len() as i64, Value::Int(_) => 0 };
```
- Problem: `enum E { A(i32), B, C }`: constructing `B` (0 fields) and `C` (0 fields) both produce discriminant 0; `A` (1 field) → 1. Match dispatch is by **field count**, so any enum where variant index ≠ field count mis-dispatches (two arms take the same block). Old-audit RT-6 flagged the VM side; the emitter side (tag never emitted) is equally missing.
- Fix: In `emit_rvalue`'s Aggregate arm, match on the kind: for `AggregateKind::Adt(_, variant, _)`, first emit `OP_LOAD_CONST (variant.to_raw() as i64)` and include it as element 0 (or add a dedicated `OP_SET_TAG`); in the VM's `Discriminant`, return the stored tag element for aggregates (and 0 for scalars) instead of `elems.len()`.
- Verify: hand-assemble/emit `Some(1)` vs `Some(2)` vs `None` for a 2-variant Option-like and a 3-variant enum; assert discriminants 0/1/2 (today: 1/1/0 by arity).

---

### T040 [BC-4] [High] [BUG] `resolve_target` indexes `block_offsets` unchecked — `u32::MAX` trap sentinels panic the host

- File: `crates/glyim-bytecode-vm/src/lib.rs:235-241`
- Code:
```rust
fn resolve_target(&self, target: u32) -> usize {
    if !self.block_offsets.is_empty() {
        self.block_offsets[target as usize]     // ← no bounds check
    } else { target as usize }
}
```
- Problem: The emitter deliberately produces `u32::MAX` targets: `Call { target: None }` (codegen lib.rs:1122) and the ZST-index bounds trap (`OP_ASSERT … u32::MAX`, lib.rs:347-351). With a `block_offsets` table present (RT-12 says the pipeline emits one), any such target → `self.block_offsets[4294967295]` → **Rust index panic in the host process**, i.e. a program-reachable VM crash; without a table, PC jumps to 4 GB → `UnexpectedEndOfCode`.
- Fix: Return a sentinel:
```rust
self.block_offsets.get(target as usize).copied().unwrap_or(usize::MAX)
```
and treat `usize::MAX` in `drive` as `VmError::AbnormalTermination` (matching the emitter's trap semantics).
- Verify: `cargo test -p glyim-bytecode-vm` — add a module with `block_offsets = vec![0]` and bytecode `Call` with target `0xFFFFFFFF`; expect `VmError::AbnormalTermination`, not a panic.

---

### T041 [BC-5] [High] [MISCOMPILE] Peephole decoder consumes 4 operand bytes for `OP_DISCRIMINANT` (emitter/VM use none) — O1+ deletes the following `STORE_LOCAL`

- File: `crates/glyim-codegen/src/lib.rs:703` (decoder) vs `910-914` (emitter); `crates/glyim-bytecode-vm/src/lib.rs:536` (VM)
- Code:
```rust
OP_LOAD_LOCAL | OP_STORE_LOCAL | OP_JUMP | OP_JUMP_IF | OP_LEN | OP_DISCRIMINANT => take(4),
```
- Problem: The stream for `discr_local = Discriminant(e)` is `[operand][OP_DISCRIMINANT][OP_STORE_LOCAL][u32 local]`. At O1+ the decoder eats `OP_STORE_LOCAL` + the u32 as DISCRIMINANT's "operand", re-encoding a stream where the discriminant is never stored — every enum match reads an uninitialized local. Exactly old RT-5; the RT-4-style fix covered `OP_LEN` but not `OP_DISCRIMINANT`.
- Fix: Remove `OP_DISCRIMINANT` from the `take(4)` list (it takes no operand), or give it a real 4-byte local operand on both emitter and VM. Prefer the former (zero wire change).
- Verify: `cargo test -p glyim-codegen` — emit `Assign(local, Discriminant(place))` and run `peephole` at O1; assert the output still contains `OP_STORE_LOCAL` after `OP_DISCRIMINANT` (byte-compare against O0 minus folded parts).

---

### T042 [BC-6] [High] [MISCOMPILE] Field/Downcast addressing: emitter adds *byte* offsets to the VM's *slot*-based memory model

- File: `crates/glyim-codegen/src/lib.rs:300-305` (Field), `374-388` (Downcast, unaligned `tag_size`); `crates/glyim-bytecode-vm/src/lib.rs:656-678` (`set_local` unpacks tuple to slots), `1061-1080` (VM test documents `Field(k) = base + (k+1)`)
- Code:
```rust
// emitter: layout BYTE offset
ProjectionElem::Field(idx) => {
    let offset = self.layout_provider.field_offset(current_ty, *idx);  // e.g. 8 for field 1 of {i64,i64}
    bc.push(OP_LOAD_CONST); bc.extend_from_slice(&(offset as i64).to_le_bytes()); bc.push(OP_ADD);
}
// VM model (its own test): "s.1 (offset 1 -> mem[local+2] in our model)"
```
- Problem: The VM stores one value per `mem` slot and unpacks a `Tuple` into slots `idx+1+i` **only on `set_local`** (not `set_mem`/`StoreField`), expecting `Field(k)` ≡ `base+k+1`. The emitter emits layout byte offsets (0, 4, 8, …) and, for enums, an unaligned `tag_size` added via `Downcast`, while `Aggregate` never writes a tag (T039). Net effect: for `struct {i64, i64}`, `s.1` reads `mem[base+8]` instead of `mem[base+2]`; `s.0` reads the whole tuple; struct/enum field access is wrong for anything beyond trivial layouts (currently unobserved because golden-opcode tests never execute and `--backend=bytecode --emit=exec` is unwired per old RT-19).
- Fix: Pick one model and align both sides. Simplest: keep the VM's slot model — change the emitter's `Field(idx)` to emit `idx+1` (+ existing tag offset for enums), change `set_mem` to perform the same tuple unpacking as `set_local`, and have `Aggregate(Adt)` write the tag into slot `base` with fields at `base+1+i` (also fixes T039's storage half).
- Verify: after the change, e2e: `--backend=bytecode` a program with a 3-field struct and an enum match; compare exit code/output with the interpreter run.

---

### T043 [RT-34] [High] [BUG] Async reactor registers the synthetic **SocketId** as a raw fd — mio wraps fd 1/2/3… (stdout/stderr), real socket never watched, double-ownership on close

- File: `crates/glyim-runtime/src/reactor.rs:302` (`mio::net::TcpStream::from_raw_fd(fd)`), `195-198` (`deregister` drops the source ⇒ closes the fd); `crates/glyim-runtime/src/lib.rs:1045-1057` (`glyim_net_tcp_connect` returns `SocketId`, a counter starting at 1); `crates/glyim-lang-std/lib/net.g:270,309`
- Code:
```rust
// net.g — stream.fd is the value returned by glyim_net_tcp_connect == SocketId, not an OS fd:
self.token = unsafe { glyim_reactor_register(self.stream.fd, 1, tid) };
// reactor.rs — reinterprets it as a raw fd AND takes ownership:
let src = unsafe { mio::net::TcpStream::from_raw_fd(fd) };
sources.lock().unwrap().insert(token, Box::new(src));
```
- Problem: `ReadFuture`/`WriteFuture` (net.g:199-309) register `stream.fd` = 1, 2, 3… (the store's synthetic id). `from_raw_fd(1)` makes mio **own stdout's fd**: readiness of unrelated std fds wakes the executor spuriously, the actual socket is never watched (async reads/writes hang until the 100 ms poll fallback / forever), and on `deregister`/reactor shutdown the owned source is dropped → **closes fd 1/2/3**, corrupting the process's std streams. Even when the id collides with a real socket fd, the fd is now doubly owned (socket store + reactor) → double-close/fd-reuse.
- Fix (pick one):
  - (a) expose real fds: add `glyim_net_tcp_as_raw_fd(fd: i32) -> i32` (socket2 `SockRef::from(&stream).as_raw_fd()`) and have net.g pass that to `glyim_reactor_register`; or
  - (b) make `register_fd` take the SocketId, look the stream up in the store, register via `mio::net::TcpStream::from_std(stream.try_clone()?)`, and *not* own the original. Also drop the fd-ownership in `deregister` accordingly.
- Verify: e2e: `glyim-cli async_tcp.g --emit=exec` doing `block_on(stream.read(...))` against a local listener — today readiness never fires and strace shows registration of fd 1; after fix, `strace -f` shows epoll watching the socket's real fd and read completing.

---

## 2.5 Macros & name resolution

### T044 [MAC-1] [High] [BUG] `macro_rules!` scoping is absent: flat whole-tree registry, last definition wins, no use-before-def/module-scope rules

- File: `crates/glyim-meta/src/expander/mod.rs:183-193` (collection), :187 (last-wins), :352-354 (defs stripped from output)
- Code:
```rust
pub(crate) fn collect_macros(&mut self, node: &SyntaxNode, _interner: &mut Interner) {
    for child in node.children() {
        if child.kind() == SyntaxKind::MacroDef {
            if let Some(def) = self.parse_macro_def(&child) {
                self.macros.insert(def.name, def);
            }
        } else {
            self.collect_macros(&child, _interner);
        }
    }
}
```
- Problem: Before expanding anything, every `MacroDef` **anywhere** in the tree (any module, any nesting) is flattened into one `HashMap<Name, MacroDef>`; a later definition silently replaces an earlier one (`HashMap::insert`), and a macro is visible **before** its definition and across module boundaries. Trigger: `mod a { macro_rules! m { () => { 1 } } } mod b { macro_rules! m { () => { 2 } } } fn main() -> i32 { m!() }` expands with `b`'s body regardless of placement; two same-name macros produce no diagnostic. Additionally `Resolver::resolve_path` never fills `PerNs::macros` (def-map lib.rs:245-263), so `pub use`-ing a macro is impossible — macros are only reachable through this global scan.
- Fix (3 steps):
  1. Make `collect_macros` scope-aware — carry a module path stack, record `(module_path, name) -> def` in insertion order, and at each `MacroCall` site resolve the name by walking the **current node's ancestor modules** and requiring the definition to appear textually before the call (store each def's `text_range().start()` and compare with the call's).
  2. Emit `duplicate definition of macro 'm'` when two defs share (module, name).
  3. Populate `PerNs::macros` so `use` can re-export macros.
- Verify: `cargo test -p glyim-meta` + new cases: same-name macros in two modules expand per-module; use-before-def errors; duplicate in one module errors.

---

### T045 [MAC-2] [High] [MISCOMPILE] Nested repetition bindings are flattened — wrong repetition count and lost inner separator

- File: `crates/glyim-meta/src/expander/matcher.rs:495-551` (flatten at :524-528), `crates/glyim-meta/src/expander/substitution.rs:84-102` (max-count) and :180-195 (`extract_repetition_bindings`)
- Code (matcher — each outer iteration's bindings are `extend`ed flat):
```rust
RepetitionKind::ZeroOrMore => {
    for rep in &repetitions {
        for (k, v) in rep {
            bindings.entry(k.clone()).or_default().extend(v.clone());
        }
    }
}
```
Code (substitution — outer count = flat length; inner rep always sees exactly 1 binding):
```rust
let repetitions: usize = var_names
    .iter()
    .filter_map(|name| bindings.get(name).map(|v| v.len()))
    .max()
    .unwrap_or(0);
```
- Problem: Matching `$( $( $x:expr ),* );*` against `1, 2 ; 3, 4, 5` yields `x = [[1],[2],[3],[4],[5]]` (grouping lost), so the template `$( let _row: i32 = 0 $( + $x )*; )*` expands to **five** `let _row: i32 = 0 + x_k;` statements (outer count 5) instead of **two** (`0+1+2` and `0+3+4+5`); the inner separator `,` is never emitted. HIR-3's fix only made `find_all_metavars` find deeply nested metavars ("expands to nothing" → "expands"), it did not restore depth.
- Fix: Make bindings depth-tagged, e.g. `HashMap<SmolStr, Vec<RepNode>>` where `RepNode { tokens: Vec<TokenTree>, children: Vec<RepNode> }`; have the matcher's Repetition arm nest the per-iteration map under the outer metavar instead of `extend`, and in `substitute` recurse per outer index so the inner repetition consumes that index's `children` (steps: 1. change `MatchResult` value type; 2. `bindings.entry(k).or_default().push(RepNode::from(rep_bindings))`; 3. `extract_repetition_bindings` returns the *child slice* for index i; 4. adaptation in `mod.rs:492`).
- Verify: `cargo test -p glyim-meta hir3` + new test: macro `rows!{ ($($($x:expr),*);*) => { $( let r: i32 = 0 $( + $x )*; )* } }` invoked with `1, 2 ; 3` must produce exactly two `let` (count `KwLet` tokens = 2, first contains both `+ 1 + 2`).

---

### T046 [DM-1] [Medium→High] [BUG] Glob `use` forces `Visibility::Public` — plain globs become public re-exports

- File: `crates/glyim-def-map/src/lib.rs:369-390` (glob branch at :621-631)
- Code:
```rust
for (name, (id, vis, span)) in source_scope.types {
    if vis == Visibility::Public {
        modules[target]
            .scope
            .declare(name, id, vis, span, Namespace::Types);
    }
}
```
and the glob branch that never consults `use_vis`:
```rust
if has_glob {
    if let Some(path_node) = use_path_node {
        ...
        import_all_public_for_modules(mod_id, parent_module, modules);
    }
    return;
}
```
- Problem: For simple `use` the binding stores `use_vis` (`Inherited` unless `pub use`, per test `u08_t01`), but the glob branch **discards** `use_vis` and declares every globbed name with the source's `Public`. So `mod b { use crate::a::*; }` acts as `pub use crate::a::*;`: a third module can `use crate::b::X;` and `validate_import_visibility` passes (target `X` is `pub` in `a`). In Rust a private glob binding is not re-exportable. Trigger: `mod a { pub struct S; } mod b { use crate::a::*; } use crate::b::S;` compiles although `b` re-exports `S` privately — a privacy escape.
- Fix: Thread `use_vis` into the glob path and declare with the minimum of the binding visibility and the source visibility: change `import_all_public_for_modules(source, target, modules)` to take `use_vis: &Visibility` and use
```rust
let eff = if *use_vis == Visibility::Public { vis } else { use_vis.clone() };
modules[target].scope.declare(name, id, eff, span, ns);
```
at all three declare sites (types/values/macros if globs ever cover them).
- Verify: `cargo test -p glyim-def-map` plus a new case: `mod a { pub struct S; } mod b { use crate::a::*; }` then assert `b`'s binding for `S` is `Visibility::Inherited` and a root `use crate::b::S;` yields the private diagnostic.

---

### T047 [DM-2] [Medium→High] [BUG] Intermediate path segments never consult `use`-imported names — import silently fails, fallback can pick the wrong item

- File: `crates/glyim-def-map/src/lib.rs:264-291` (`Resolver::resolve_path`), :338-364 (`resolve_module_path_for_modules`)
- Code:
```rust
} else if let Some((_, child_id)) = module_data
    .children
    .iter()
    .find(|(n, _)| *n == segment.name)
{
    current_module = *child_id;
} else if path.kind == PathKind::Plain && i == start_idx {
    ... // only searches ancestors' `children`
} else {
    return PerNs::default();
}
```
- Problem: Intermediate segments are looked up **only** in `ModuleData::children` (real child modules + enum synthetic modules). A module name brought in by a previous `use` lives in `scope.types`, never in `children`, so the fixed-point loop can never resolve it: `use a::b; use b::f;` → the second import resolves to `PerNs::default()` on every pass, **with no diagnostic** (process_use_tree silently drops failures). The typeck mirror (`tyconv.rs:924`) then falls back to "walk all modules, first scope defining the bare name wins" — so when the same name exists in two modules, the module **creation order**, not the import, decides what `f()` calls: `mod c { pub fn f() -> i32 { 2 } }` declared before `a::b::f` makes `f()` return 2 silently.
- Fix: In both functions, when the `children` lookup (and the ancestor search for `i == start_idx`) fails, also consult the current module's `scope.types` entry and, if its `def_id` maps to a module in `def_to_module`, descend there:
```rust
if let Some((id, _, _)) = self.modules[current_module].scope.types.get(&segment.name) {
    if let Some(m) = def_to_module_of(id) { current_module = m; continue; }
}
```
(the def-map builder must pass/derive the def-id→module map; it already exists as `def_to_module` in `build_def_map`).
- Verify: `cargo test -p glyim-def-map use_declarations` + new test with source `mod a { pub mod b { pub fn f() -> i32 { 1 } } } use a::b; use b::f;` asserting root scope contains `f` and a diagnostic-free def-map.

---

## 2.6 HIR lowering

### T048 [HIRX-2] [High] [MISCOMPILE] Loop labels and `break 'a`/`continue 'a` are parsed and then silently dropped by HIR — break binds to the innermost loop

- File: `crates/glyim-hir/src/lower/lower_expr.rs:1959-1973` (break), :1818-1830 (loop), :1866- (for/while); Expr has no label field (`crates/glyim-hir/src/lib.rs:494-628`); parser consumes labels at `crates/glyim-frontend/src/parser/expr.rs:499-510, 736-766`
- Code:
```rust
fn lower_break_expr(...) -> Option<ExprId> {
    let value = node
        .children()
        .find(|c| is_expr_node(c) || c.kind() == SyntaxKind::Block)
        .and_then(|n| lower_expr(&n, interner, body, diags, struct_field_map));
    let expr = Expr::Break { value };
```
- Problem: FE-10 taught the parser to consume `break 'label value;` / `continue 'label;` and `parse_label()` consumes `'a:` on loop/while/for — but `Expr::{Loop,While,For,Break,Continue}` carry no label and lowering ignores the `Lifetime` token entirely (grep for "label" in glyim-hir: zero hits). Trigger:
```g
'a: loop { loop { break 'a; } ; unreachable-after-fix }
```
`break 'a` lowers as a plain `Expr::Break`, binding to the **inner** loop → the outer loop never exits (silently wrong control flow / hang). Same for labeled `continue` and `break 'a value`.
- Fix (4 steps):
  1. Add `label: Option<Name>` to `Expr::{Loop, While, For, Break, Continue}` in `crates/glyim-hir/src/lib.rs`.
  2. In the four lowering sites read the leading `Lifetime`/`Ident`+`Colon` tokens (`node.children_with_tokens()` before the keyword, and after `break`/`continue`).
  3. Thread a label stack through `lower_block_to_expr`/loop lowering so an unlabeled break binds the innermost and a labeled one matches by name.
  4. Have glyim-lower's builder resolve the label to the target loop's exit block — or, as an interim, emit a diagnostic: `GlyimDiagnostic::type_error(span, "labeled break/continue is not yet supported")` instead of silently mis-binding.
- Verify: fixture `crates/glyim-test/tests/runtime/label_break.g` with `check-stdout` proving exit of the labeled loop; compile-fail test asserting a diagnostic until full support lands.

---

### T049 [HIRX-3] [Medium→High] [BUG] `move` closures: the `KwMove` arm can never fire — `is_move` is always `false`

- File: `crates/glyim-hir/src/lower/lower_expr.rs:421-425` (vs parser `crates/glyim-frontend/src/parser/expr.rs:543-546`)
- Code:
```rust
for child in node.children() {          // ← nodes only
    match child.kind() {
        SyntaxKind::KwMove => { is_move = true; }   // KwMove is a *token* kind
```
- Problem: `parse_closure_expr` does `self.bump()` for `move`, so `KwMove` is a direct **token** child of `ClosureExpr`; `node.children()` yields nodes only, so the arm is dead and `is_move` is permanently `false`. Typeck keys capture kind off it (check_expr.rs:2146 `let kind = if *is_move { ByValue } else { ByRef }`), so every `move |..| ..` closure captures **by reference** — a closure that must own its captures (returned closures, spawned tasks) reads dead/upheld-by-borrowck-only stack slots, or borrowck rejects programs that should compile. HIR-31 fixed `ByRef` aliasing but left this dead arm.
- Fix: detect the token:
```rust
let is_move = node.children_with_tokens().any(|el| matches!(&el,
    syntax::SyntaxElement::Token(t) if t.kind() == SyntaxKind::KwMove));
```
and delete the `SyntaxKind::KwMove` node arm.
- Verify: `cargo test -p glyim-hir` + `cargo test -p glyim-test` run-pass fixture where a `move` closure is returned from a function and mutates/drops its captured local.

---

## 2.7 LSP

### T050 [LSP-1] [High] [STUB] The server never sends `textDocument/publishDiagnostics` — the diagnostics feature is invisible to editors

- File: `crates/glyim-lsp/src/handler.rs:26-212` (no publish handler; `_client: async_lsp::ClientSocket` unused), `driver.rs:123-139`
- Code:
```rust
pub fn build_router(
    db: Arc<AnalysisDatabase>,
    _analysis_tx: mpsc::Sender<AnalysisMessage>,
    _client: async_lsp::ClientSocket,   // never used
) -> Router<()> {
```
- Problem: `AnalysisDriver::analyze_file` computes diagnostics into `db.diagnostics`, but no code path ever emits a `PublishDiagnostics` notification to the client (grep for `publish_diagnostics`/`PublishDiagnostics` over the crate returns nothing). Editors show zero squiggles; additionally the driver only stores lex/parse/def-map diagnostics — `typeck_result.diagnostics` are never added (driver.rs:123-126), so even internally no type errors exist to publish. The unused-`use`, "add missing match arm", and "generate impl" quick-fixes key off `db.diagnostics` and can therefore never trigger in production (only in tests that hand-populate it).
- Fix: In `analyze_file`, after updating `db.diagnostics`, send `ServerNotification::<PublishDiagnostics>(PublishDiagnosticsParams { uri, diagnostics, version })` through a stored `ClientSocket`; also extend `all_diagnostics` with `typeck_result.diagnostics` and register the builtin traits (mirroring `register_builtin_traits`) so typeck matches the compiler.
- Verify: run the server against a client (e.g. the glyim-pilot extension or `nvim-lspconfig`), open a file with a syntax error → today no publish arrives; after fix diagnostics update per keystroke.

---

### T051 [LSP-2] [High] [BUG] Rebuilding one file's reference graph deletes every other file's references that share a name

- File: `crates/glyim-lsp/src/reference_graph.rs:76-78`
- Code:
```rust
self.references
    .retain(|_, refs| refs.iter().all(|r| r.file_id != file_id));
```
- Problem: `references` maps name → refs from *all* files. The retain predicate drops the **whole entry** if it contains *any* ref from the rebuilt file — including refs belonging to other files. Concretely: `main.g` and `util.g` both reference `count`; one keystroke in `main.g` wipes `util.g`'s `count` entries. `find_references` then returns a partial set, and graph-based rename (rename.rs:88-127) edits only the surviving file's occurrences — cross-file rename silently degrades depending on edit order.
- Fix:
```rust
for refs in self.references.values_mut() {
    refs.retain(|r| r.file_id != file_id);
}
self.references.retain(|_, v| !v.is_empty());
```
- Verify: unit test: `build_from_hir(fileA, "fn count(){}", …)` then `build_from_hir(fileB, "fn main(){ count(); }", …)`, rebuild A, assert `find_references("count")` still contains B's use — currently empty.

---

### T052 [LSP-3] [High] [BUG] Renaming an assignment target replaces the whole `x = 5` / `i += 1` expression, destroying the statement

- File: `crates/glyim-lsp/src/reference_graph.rs:513-527` (span source: `crates/glyim-hir/src/lower/lower_expr.rs`, `body.alloc_expr(expr, node_span(node))` for `AssignExpr`)
- Code:
```rust
Expr::Assign { lhs, rhs } => {
    if let Expr::Path(path) = &body.exprs[*lhs] ... {
        add_ref(
            &name_str,
            span,      // <-- the WHOLE Assign expression's span (`x = 5`)
            true, ReferenceKind::Variable, AccessKind::Write,
        );
    }
```
- Problem: INF-13 fixed the `let` case (uses `pat_span`), but the `Assign` arm still records the enclosing expression's span (`node_span` of the full `AssignExpr`, including the initializer — verified in `lower_assign_expr`). `rename_symbol` turns each ref into a `TextEdit` over that range, so renaming `x` rewrites `x = 5;` into `z;` and `i += 1;` into `j;` (compound assigns lower to `Assign` with the full span too).
- Fix: record the LHS identifier's own span: take the lhs expr's span (`body.expr_spans.get(*lhs)`) and, better, the path segment span; i.e. `add_ref(&name_str, lhs_span, true, Variable, AccessKind::Write)`.
- Verify: extend rename_tests with `let mut x = 0; x = 5;` rename `x`→`z`; assert the edit on line 2 is 1 column wide and `x = 5` is not clobbered — currently the edit covers `x = 5`.

---

### T053 [LSP-4] [High] [BUG] `for`/closure/match pattern bindings are recorded with `Span::DUMMY` → rename inserts the new name at file position 0:0

- File: `crates/glyim-lsp/src/reference_graph.rs:172-181` (also symbol_index.rs:299-301)
- Code:
```rust
Pat::Binding { name, .. } => {
    let name_str = interner.resolve(*name).to_string();
    add_ref(
        &name_str,
        Span::DUMMY,   // lo=hi=0 (glyim-span/src/lib.rs:67-72)
        true, ReferenceKind::Definition, access,
    );
}
```
- Problem: `walk_pattern` (used for `for`-loop vars, closure params, match arms) stores the binding with `Span::DUMMY` (0..0). `rename_symbol` maps every ref to an edit: `span_to_position(0,0)` → `Range (0,0)-(0,0)` → a `TextEdit` that **inserts the new name at the very top of the file**, while the real binding site is never renamed. Renaming any `for i in …` variable corrupts the file and renames only the uses.
- Fix: thread real pattern spans: HIR `Pat` needs a span (like `Expr::Let.pat_span`); until then, skip DUMMY-span refs in `rename_symbol` and `find_references` (`if r.span.is_dummy() { continue; }`) so at least no corruption occurs, and add span storage to `Pat` as the real fix.
- Verify: rename test on `fn main() { for i in 0..10 { use(i); } }` → today edits include one at 0:0; after fix none, binding renamed.

---

### T054 [LSP-9] [High] [BUG] The formatter has zero string/comment awareness and corrupts documents containing format strings

- File: `crates/glyim-lsp/src/formatting.rs:5-61`
- Code:
```rust
'{' => {
    result.push('{');
    result.push('\n');
    indent_level += 1;
    result.push_str(&"    ".repeat(indent_level));
}
```
- Problem: `format_code` reformats every character with no lexical context. `println!("{} items", n)` becomes a multi-line mangled string; commas inside strings gain spaces (`"a,b"` → `"a, b"`); whitespace inside string literals is collapsed (the `' '` arm suppresses doubles but newlines/braces are injected); `%`/`;` inside strings insert line breaks. The capability is advertised (`document_formatting_provider`, handler.rs:56) so any user invoking "format document" on realistic code silently rewrites string contents — a data-corrupting "formatter".
- Fix: lex first (`glyim_frontend::lex`) and only re-indent at token granularity, or minimally: track in_string/in_char/in_line_comment/in_block_comment state in the char loop and pass literals/comments through verbatim (only count braces outside them).
- Verify: `format_document` on `fn main() { println!("{} {}"); }` — today returns an edit whose new_text contains `"{\n    } ..."` inside the literal; after fix the literal is byte-identical.

---

## 2.8 Pipeline, CLI, glyip

### T055 [PIPE-3] [Medium→High] [BUG] `--emit=mir` runs a stale standalone chain: no monomorphization, no opt, and closure bodies silently omitted

- File: `crates/glyim-pipeline/src/lib.rs:895-1021` (loop at 1003-1010)
- Code:
```rust
    for (_owner_def_id, thir_body) in &typeck_result.thir_bodies {
        let lower_result = glyim_lower::lower_body(&lower_ctx, thir_body);
        ...
        mir_bodies.push(lower_result.body);
    }
```
- Problem: the module doc (lib.rs:112-118) states "Every emit mode (`--emit=obj|exec|mir|llvm-ir|asm`) … must derive from a `PreparedCompilation`". `emit_llvm_ir`/`emit_asm` were fixed to route through `prepare_compilation`, but `emit_mir` was not: it prints pre-mono, pre-opt, non-drop-elaborated MIR and — unlike `compile_file_to_mir` (lib.rs:856-862, which registers `closure_bodies`) — drops every closure body entirely, so `--emit=mir` output is missing all closures and disagrees with what codegen consumes.
- Fix: implement `emit_mir` as:
```rust
let prepared = Pipeline::prepare_compilation(db, input, None, None)?;
// then format_body over prepared.all_bodies (mirroring emit_llvm_ir)
```
- Verify: `target/debug/glyim-cli prog.g --emit=mir -o p.mir` on a program using a closure — closure body absent before fix, present after; also generic stdlib bodies no longer print raw `Param` locals.

---

### T056 [CLI-1] [Medium→High] [BUG] Linker arguments are joined with spaces and re-split on whitespace — any path containing a space breaks `--emit=exec`

- File: `crates/glyim-cli/src/linker.rs:117-134` (join) and `44-53` (split); user flags split again at `crates/glyim-cli/src/lib.rs:597-601`
- Code:
```rust
Some(parts.join(" "))                      // build_link_flags
...
for flag in flags.split_whitespace() {     // UnixLinker::link
    cmd.arg(flag);
}
```
- Problem: `link_with_args` flattens `LinkArgs` (including the runtime staticlib path and every object) into one string that `UnixLinker::link` re-splits on whitespace. A project/repo under e.g. `/home/u/My Projects/glyim` makes `find_runtime_staticlib()` return a path with a space; it is split into two argv entries and the link fails with "no such file or directory: '/home/u/My'". Same for `--link-flags` containing quoted paths.
- Fix: change `LinkerInvoker::link` to take `&[String]` (or `Vec<OsString>`) instead of `Option<&str>`; have `build_link_flags` return `Vec<String>` and `cmd.arg(flag)` each element directly; keep a shell-style splitter only for `--link-flags` (respect quotes) or document that it must be pre-split.
- Verify: `mkdir "/tmp/sp ace" && GLYIM_RUNTIME_LIB="/tmp/sp ace/libglyim_runtime.a" glyim-cli x.g --emit=exec -o x` — currently fails at link; passes after fix.

---

### T057 [GLYIP-1] [High] [STUB] Glyip.lock is never written, read, or validated after `glyip new` — reproducible builds don't exist

- File: `crates/glyip/src/commands.rs:148-150` (also 221-222, 544-545); dead API at `lockfile.rs:121,136,176`
- Code:
```rust
let resolver = DependencyResolver::new_no_index();
let _lockfile = resolver.resolve(&config, project_dir)?;
info!("Dependencies resolved");
```
- Problem: Every command resolves fresh and **discards** the lockfile (`_lockfile`). `Lockfile::write_to_dir` is called exactly once — by `cmd_new` to create an empty file — and `read_from_dir`/`validate_against_manifest` have zero production callers. So: `Glyip.lock` stays `{version = 1}` forever (never updated when the manifest changes), version resolution re-picks the newest matching release each build, and `validate_against_manifest` is additionally broken by design: it compares the manifest's *requirement string* against the locked version (`get_crate(name, "^1.2")` can never equal locked `1.2.3`), so even if wired it would report `VersionMismatch` for every caret/tilde dep.
- Fix (3 steps): In `cmd_build`/`cmd_test`/`cmd_run`:
  1. `let existing = Lockfile::read_from_dir(project_dir)?;`
  2. If `existing.validate_against_manifest(&config)` is empty, resolve **from** the lockfile (reuse locked versions/sources); otherwise re-resolve and `resolved.write_to_dir(project_dir)?`.
  3. Fix validation to use `VersionReq::parse(manifest_version).matches(&locked_version)` instead of string equality.
- Verify: project with `foo = "1.0"` dep; `glyip build` twice; before fix `Glyip.lock` is empty after both; after fix it contains `foo-1.0.0` and editing the manifest version updates it.

---

### T058 [GLYIP-2] [High] [STUB] Dependencies are never compiled or linked; `glyip run` executes a non-executable file and can never succeed

- File: `crates/glyip/src/commands.rs:621-695` (no link step), `705-720` (`run_binary`), `tests/run_cmd.rs:26-31` (acknowledges the breakage)
- Code:
```rust
match glyim_pipeline::Pipeline::compile_file(&mut db, entry, backend.as_ref(), &output_path, None, None) { ... }
// cmd_run:
let exit_code = run_binary(&build_result.output, &opts.args, &run_env)?;
```
- Problem: `compile_source` compiles only the entry file to a *relocatable object* (LLVM) or raw bytecode (BytecodeBackend — the default: bin/glyip.rs:134 hardcodes `backend: "bytecode"`) and stops. There is no linker invocation anywhere in glyip, no `-L`/`-l` for resolved deps, and dep crates are never built at all (`_lockfile` sources unused). `cmd_run` then `Command::new()`s an ET_REL object / bytecode blob → `ENOEXEC` every time. The project's own tests accept this: `Err(_) => { // Expected if the compiler pipeline isn't fully wired up. }` (run_cmd.rs:28-30), and `run_with_args` ignores the result entirely.
- Fix (3 steps):
  1. In `compile_source`, after `compile_file`, when the package has a `bin` target, link like the CLI: `link_args.objects.push(runtime_lib)` + `glyim_cli::linker::link_with_args(&output_path, &exe_path, …)` (mirror glyim-cli/src/lib.rs:573-610), writing the exe next to the object.
  2. Compile each path-dep's lib target first and pass its objects/`-L`+`-l` via `LinkArgs`.
  3. `cmd_run` executes the exe path, not the object.
- Verify: `glyip new hello && cd hello && glyip build --backend llvm && file target/debug/hello` — today "current ar archive"/ELF relocatable; after fix ELF executable and `glyip run` exits 0.

---

### T059 [GLYIP-3] [High] [BUG] `glyip build --target` and `--opt-level`/`--release` are silently ignored by the codegen backend

- File: `crates/glyip/src/commands.rs:649-663`
- Code:
```rust
let backend: Box<dyn glyim_codegen::CodegenBackend> = if opts.backend == "llvm" {
    Box::new(
        glyim_codegen_llvm::LlvmBackend::new().with_lto(
            opts.lto.unwrap_or(glyim_codegen_llvm::passes::LtoKind::None),
        ),
    )
```
- Problem: `LlvmBackend::new()` defaults to `target_triple: "x86_64-unknown-linux-gnu"` and `opt_level: 0` (codegen-llvm/src/lib.rs:77-90) and, unlike the CLI (lib.rs:401-405), glyip never calls `.with_target(...)`, `.with_opt_level(...)`, or `.with_db(&db)`. Consequences: `--target aarch64-unknown-linux-gnu` emits an x86_64 object while the build *layout/paths* claim the requested triple (`output_dir_for_target`), i.e. wrong-arch artifacts under the wrong directory; `glyip build --release` (opt_level 2) still compiles at O0 — the flag is recorded in the fingerprint config hash (so a flag change forces a pointless full rebuild) yet has no effect on output. The bytecode branch similarly passes `TargetInfo::default()`.
- Fix: construct like the CLI:
```rust
let triple = opts.target.clone().unwrap_or_else(host_target_triple);
LlvmBackend::new().with_target(&triple).with_opt_level(opts.opt_level)
// and TargetInfo::from_triple(&triple) for the bytecode backend
```
also propagate the triple into `CrateConfig::target_triple` (currently hardcoded `"x86_64-unknown-linux-gnu"` fallback at lines 633-636 instead of the host).
- Verify: `glyip build --release --backend llvm -O 2 && llvm-objdump -d target/release/<name> | head` — before fix identical to a debug build; after fix O2 code and, with `--target`, the object triple matches (`llvm-readobj --file-headers`).

---

## 2.9 Stdlib `.g` sources

### T060 [STD-7] [High] [BUG] `Box<T>::drop` deallocates with `Box`'s layout, not `T`'s

- File: `crates/glyim-lang-alloc/lib/boxed.g:40-51`; `crates/glyim-lang-core/lib/mem.g:32-34`
- Code:
```glyim
impl<T> Drop for Box<T> {
    fn drop(&mut self) {
        let layout = Layout::from_size_align(
            mem::size_of_val(self),        // T inferred as Box<T> => pointer size (8)
            mem::align_of_val(self),
        ).expect("Box layout invalid at drop");
```
- Problem: `self: &mut Box<T>`, so `size_of_val::<Box<T>>(self)` = size of the pointer, not of `T`. The allocation was made with `(size_of::<T>(), align_of::<T>())` (boxed.g:13-16). `glyim_dealloc` forwards size/align to Rust's `alloc::dealloc` (runtime lib.rs:130-139), which requires the *same* layout — deallocating a 200-byte struct with an 8-byte layout is UB (alignment mismatch corrupts size-classed allocators).
- Fix: use `mem::size_of::<T>()` / `mem::align_of::<T>()` directly in `Box::drop` (as rc.g already does for `RcInner<T>`).
- Verify: run-pass fixture leaking a `Box<[u8; 128]>`-like large struct under a size-checking allocator / `valgrind` on the LLVM path.

---

### T061 [STD-8] [High] [BUG] `RawVec::reserve` — unchecked `new_cap * size_of::<T>()` and `next_power_of_two` overflow → undersized alloc + copy overflow

- File: `crates/glyim-lang-alloc/lib/raw_vec.g:22-47`
- Code:
```glyim
let new_cap = required_cap.next_power_of_two().max(8);
let layout = Layout::from_size_align(
    new_cap * mem::size_of::<T>(),      // unchecked multiply, wraps
    mem::align_of::<T>(),
).expect("RawVec layout invalid");
...
ptr::copy_nonoverlapping(self.ptr, new_ptr, self.cap);   // copies old cap into smaller buffer
```
- Problem: For `required_cap` where `new_cap * size_of::<T>() > usize::MAX` (e.g. `Vec::<u64>` growth reaching `cap = usize::MAX/2`, or a huge `reserve`), the product wraps to a tiny/zero size → `from_size_align` passes → tiny allocation → then `copy_nonoverlapping` copies `self.cap` (huge) elements into it → heap overflow. `next_power_of_two(>2^63)` itself overflows. `Vec::push` (vec.g:25-34) reaches `reserve` on every growth.
- Fix: compute `new_cap` with checked math: reject when `required_cap > usize::MAX / size_of::<T>()` (`handle_alloc_error`), and add `debug_assert!(required_cap <= usize::MAX/2 + 1)` before `next_power_of_two`.
- Verify: add a checked-mul test in `crates/glyim-lang-alloc/src/tests` calling `reserve(usize::MAX)` on a `RawVec<u8>` and asserting abort/error, not silent 8-byte alloc.

---

### T062 [STD-9] [High] [BUG] `Vec::extend_from_slice` has no `Copy`/`Clone` bound — bit-duplicates non-Copy `T` (double drop)

- File: `crates/glyim-lang-alloc/lib/vec.g:66-75`
- Code:
```glyim
fn extend_from_slice(&mut self, other: &[T]) {
    self.buf.reserve(self.len + other.len());
    for item in other {
        unsafe { let end = self.buf.as_mut_ptr().add(self.len);
                 ptr::write(end, ptr::read(item)); }     // bitwise copy
        self.len += 1;
    }
}
```
- Problem: `ptr::read(item)` duplicates the value; the source slice still owns it. For `Vec<String>::extend_from_slice(&other)` both vectors later drop the same heap buffer → double free. `String::push_str` is safe only because `u8: Copy`.
- Fix: `fn extend_from_slice(&mut self, other: &[T]) where T: Copy` (or `where T: Clone` + `ptr::write(end, item.clone())`).
- Verify: compile-fail fixture extending a `Vec<String>` without Copy should now be rejected; run-pass with `Vec<i32>` unchanged.

---

### T063 [STD-10] [High] [BUG] `BufReader::fill_buf` reads into a cleared (empty) `Vec` — reader is permanently at EOF

- File: `crates/glyim-lang-std/lib/io.g:417-431`
- Code:
```glyim
fn fill_buf(&mut self) -> Result<&[u8], Error> {
    if self.pos == self.cap {
        self.buf.clear();
        let n = self.inner.read(&mut self.buf)?;   // &mut Vec<u8> coerces to EMPTY slice (len 0)
        self.cap = n;
        self.pos = 0;
    }
    Result::Ok(&self.buf[self.pos..self.cap])
}
```
- Problem: `Read::read` takes `&mut [u8]`; `&mut self.buf` derefs to a slice of the Vec's *length*, which is 0 after `clear()` → `read` returns `Ok(0)` → `cap = 0` forever → `BufReader::read` returns 0 (EOF) and `read_until`/`read_line` return empty. `BufReader` delivers no bytes, ever.
- Fix: keep a fixed scratch array or:
```glyim
self.buf.resize(8192, 0);
let n = self.inner.read(&mut self.buf[..])?;
self.cap = n;
self.pos = 0;
```
(don't `clear`), tracking logical length separately.
- Verify: run-pass fixture wrapping `stdin()`/`empty_reader` in `BufReader` and reading a line (currently returns empty).

---

## 2.10 Test harness

### T064 [HARNESS-1] [High] [BUG] MIR interpreter path never captures stdout — `check-stdout` is untestable (and failing) on the default path

- File: `crates/glyim-test/src/harness/interpreter_runner.rs:102-120, 143-149`
- Code:
```rust
fn interpret_bodies(...) -> InterpOutput {
    let stdout = String::new();     // never written
    ...
    InterpOutput { exit_code, stdout, stderr }
```
- Problem: `InterpOutput.stdout` is always `""`. The default run path (no LLVM → `MockCodegen` → `InterpRunner`, per run_pass_corpus.rs "falls back to the MIR interpreter ... runs everywhere") therefore cannot satisfy any `// check-stdout:` directive (`OutputCheck::check` does `result.stdout.contains(expected)`), and fixtures like `tests/codegen/llvm_rvalue_stubs.g` and `tests/runtime/drop_order.g` can only pass on the native-LLVM path.
- Fix: give the interpreter an output sink (route `println!`/`glyim_stdout_write` through a captured buffer in `Interpreter`) and return it from `interpret_bodies`.
- Verify: `cargo test -p glyim-test --test run_pass_corpus` on a host without LLVM — check-stdout fixtures must pass instead of `StdoutMismatch`.

---

### T065 [HARNESS-2] [High] [BUG] `check-stdout:` values are not unescaped — `drop_order.g` can never pass

- File: `crates/glyim-test/src/harness/config.rs:173-176`; `tests/runtime/drop_order.g:3`
- Code:
```rust
} else if let Some(value) = content.strip_prefix("check-stdout:") {
    config.check_stdout = Some(value.trim().to_string());   // literal backslash-n kept
// fixture: // check-stdout: drop 2\ndrop 1
```
- Problem: The directive stores the two characters `\` + `n`; the program prints a real newline, so `result.stdout.contains("drop 2\\ndrop 1")` is false forever — the fixture is permanently red on every path that evaluates it.
- Fix: unescape `\n`/`\t`/`\\` in `parse_test_config` (e.g. a small state machine over `value`), or compare line-by-line.
- Verify: run `drop_order.g` through the runner after T064/T065 fixes — must pass.

---

## 2.11 glyim-pilot & extension

### T066 [PILOT-2] [High] [BUG] Timed-out child processes are never killed (kill_on_drop missing)

- File: `tools/glyim-pilot/src/process.rs:60-63`
- Code:
```rust
let output_fut = tokio::process::Command::new(program)
    .args(args)
    .current_dir(cwd)
    .output();          // <-- no .kill_on_drop(true)
match tokio::time::timeout(timeout, output_fut).await { ... }
```
- Problem: On timeout the future is dropped but the child keeps running (tokio only reaps orphans, doesn't kill). A hung `cargo test`/`cargo llvm-cov` continues holding the `target/` flock and consuming CPU indefinitely; every subsequent gate run queues behind it. (Old-audit INF-27, still unfixed.)
- Fix: add `.kill_on_drop(true)` to the builder:
```rust
tokio::process::Command::new(program)
    .args(args).current_dir(cwd)
    .kill_on_drop(true)
    .output();
```
- Verify: `cargo test -p glyim-pilot run_timed_command_timeout` then `pgrep -f "sleep 10"` → empty after the 1s timeout.

---

### T067 [PILOT-3] [High] [BUG] Coverage/mutation gates compare percent (0–100) against fraction defaults (0.80/0.75) — gates always pass

- File: `tools/glyim-pilot/src/config/types.rs:423-428, 499-501` → `src/gates/done_pipeline.rs:19-28` → `src/gates/coverage.rs:41` / `src/gates/mutation.rs:41`
- Code:
```rust
fn default_coverage_min() -> f64 { 0.80 }          // config: fraction
fn default_mutation_kill_rate() -> f64 { 0.75 }
// done_pipeline.rs
CoverageGate { min_coverage: config.coverage_min }
// coverage.rs
if pct >= self.min_coverage { /* pass */ }          // pct parsed from "85.50% coverage" → 85.5
```
- Problem: The regexes parse percentages on a 0–100 scale (`85.5`), but the config/default threshold is a 0–1 fraction. `coverage_min=0.80` means the gate passes at **0.8 %** real coverage; `mutation_kill_rate=0.75` passes at **0.75 %** kill rate. Spec REQ-FUNC-047 requires ≥80 % / ≥75 %. Classic "gate that always passes". (Note: `mutation.rs`'s own unit test correctly uses `75.0` — the config path is the bug.)
- Fix: standardize on percent at the config boundary:
```rust
fn default_coverage_min() -> f64 { 80.0 }
fn default_mutation_kill_rate() -> f64 { 75.0 }
```
update `done_defaults()` literals to `80.0/75.0`, and document `coverage_min` as percent in the `.glyim-pilot.toml` template. Reject `x < 1.0` values with a config error to catch legacy configs.
- Verify: `cargo test -p glyim-pilot` + unit test: feed `CoverageGate { min_coverage: 80.0 }` output `1.5% coverage` → expect `fail`.

---

### T068 [PILOT-4] [High] [STUB] Fix-round counter never persisted; max_fix_rounds escalation + emergency WIP commit unreachable

- File: `tools/glyim-pilot/src/commit/engine.rs:174,178` (produces `new_fix_round`), `src/session/persistence.rs` (no `set_fix_round`), `src/orchestrator/turn.rs:251` (only reads), `src/commit/engine.rs:202-204` (`emergency_commit` never called)
- Code:
```rust
let current_fix_round = ctx.persistence.get_fix_round(&ctx.stream_id).await; // always 0
...
CommitDecision::GateFailed { new_fix_round, .. } => ... // value dropped, never stored
```
- Problem: `new_fix_round` is computed (`current + 1`) but never written back — `grep set_fix_round` finds nothing. Therefore `new_fix_round > max_fix_rounds` is never true, so REQ-FUNC-023 (increment fix-round counter; after max, emergency WIP commit + escalate to developer) is dead: a stream stuck in a gate-failure loop feeds the AI "❌ Commit gate failed" **forever**, burning provider turns. `CommitEngine::emergency_commit` has zero callers.
- Fix (3 steps):
  1. Add to `StatePersistence`:
```rust
pub async fn set_fix_round(&self, stream_id: &str, v: u32) -> Result<(), PilotError> {
    self.try_update_session(stream_id, |s| { s.fix_round = v; Ok(()) }).await
}
```
  2. In `turn.rs` after `engine.evaluate_commit(...)`: `ctx.persistence.set_fix_round(&ctx.stream_id, decision.new_fix_round()).await?;`
  3. In the `CommitDecision::Escalated` arm call `engine.emergency_commit(&commit_ctx).await?` before mapping to `OrchestratorAction::Escalate`.
- Verify: `cargo test -p glyim-pilot` + integration: 4 consecutive `::COMMIT` with failing gate and `max_fix_rounds=3` → 4th yields `Escalated`, state file shows `fix_round: 4`, worktree has WIP commit.

---

### T069 [PILOT-5] [High] [BUG] Processing-set entry leaked on unlock timeout → session permanently stalled

- File: `tools/glyim-pilot/src/orchestrator/turn.rs:133-147`
- Code:
```rust
let guard = match tokio::time::timeout(Duration::from_secs(5), lock_future).await {
    Ok(g) => g,
    Err(_) => {
        tracing::warn!("processing lock acquisition timed out, skipping turn");
        return result;              // <-- stream_id stays in `processing` forever
    }
};
let mut guard = guard;
guard.remove(&stream_id);
```
- Problem: If the final lock acquisition times out, the `stream_id` inserted at entry is never removed. Every subsequent `ops.ready` for that stream hits `!guard.insert(...)` → `WaitForResponse` → the stream is bricked until server restart. The comment "this is a race, but fine" is wrong.
- Fix: use a guard struct so removal is unconditional:
```rust
struct ProcessingGuard { set: Arc<Mutex<HashSet<String>>>, id: String }
impl Drop for ProcessingGuard {
    fn drop(&mut self) {
        let set = self.set.clone(); let id = self.id.clone();
        tokio::spawn(async move { set.lock().await.remove(&id); });
    }
}
```
Insert, wrap in the guard, drop the timeout/unlock dance entirely.
- Verify: test that spawns two concurrent `process_turn_dispatch` with the same stream id while `persistence` lock is held 6s → after both complete, `processing` set is empty (assert via exposed snapshot).

---

### T070 [PILOT-6] [High] [BUG] `agent wave --next` aborts its own spawned work; `--wait` waits on statuses that are never produced

- File: `tools/glyim-pilot/src/cli/agent.rs:307-328`, `:122-131, 154-159`
- Code:
```rust
tokio::spawn(async move { let _ = run_stream(..., true).await; });   // spawned, never awaited
println!("Wave {} started. ...");
// handle_agent_command returns → main() returns → #[tokio::main] drops runtime → tasks aborted
...
if status != "committed" && status != "escalated" { bail!(...) }     // statuses never exist
```
- Problem: (a) `run_stream` tasks are spawned and immediately killed when the CLI process exits — even the initial HTTP POST may not complete; the wave silently never dispatches. (b) `wait_for_session` polls `/session/{id}/status`, but session status is never updated anywhere (see T072/T084: `TransitionValidator` has zero callers, status stays `INIT` — confirmed in the repo's own `.glyim-pilot-state.json` where a session with `turn:7` still has `status:"INIT"`). `"committed"`/`"escalated"` are never emitted, so `agent run --wait` always ends in `Timeout waiting for session to complete` after 1h.
- Fix: (a) drop the `tokio::spawn` and `futures_util::future::join_all(futs).await` the `run_stream` calls (or `tokio::spawn` + collect `JoinHandle`s and await all before returning). (b) Have the server update status via `TransitionValidator::transition` at the obvious points in `turn.rs` (Executing on ops applied, Committing before pipeline, Committed on success, Reviewing on self-review, Complete on PR) so the HTTP status endpoint reflects reality.
- Verify: `cargo run -p glyim-pilot -- agent wave --next` with one fake stream → CLI stays alive until streams finish; `curl 127.0.0.1:8421/session/<id>/status` returns `executing`/`committed`, not `init`.

---

### T071 [PILOT-7] [High] [BUG] `::APPROVED` applies fresh ops un-gated and pushes without committing

- File: `tools/glyim-pilot/src/orchestrator/turn.rs:156-196`
- Code:
```rust
if !ops.ops.is_empty() {
    let results = apply_ops_async(...).await?;      // ops applied in THIS turn
}
...
if ops.approved {
    push_branch(...).await?;                        // never calls commit_all, never runs gates
    let pr_url = create_pr(...).await?;
```
- Problem: Spec REQ-FUNC-016 says `::APPROVED` is valid *after the self-review gate*; here it (a) applies any file ops contained in the approved block with **zero gates** (no fmt/check/clippy/test, no banned-patterns — the one turn where edits go straight to a PR), and (b) pushes the branch without `commit_all`, so any files applied in this final turn are left uncommitted in the worktree and are **missing from the PR**.
- Fix: in the `ops.approved` arm: (1) reject with feedback if `persistence.get_status != Reviewing` (require self-review happened; store status per T070); (2) run the commit pipeline first — reuse `CommitEngine` with message `"stream-<id>: final review fixes"` and only push if `CommitDecision::Committed`; (3) if `status_porcelain` is non-empty after that, escalate instead of pushing.
- Verify: integration test — apply `::WRITE bad.rs` + `::APPROVED` with `fmt=false, check=false` config and a deliberate compile error file → expect Feedback (gate failed), no PR created.

---

### T072 [PILOT-8] [High] [MISCOMPILE] `SessionState::new` fabricates a random `session_id`; server↔provider association never matches

- File: `tools/glyim-pilot/src/session/state.rs:76-80`, `src/main.rs:243-249`
- Code:
```rust
pub fn new(stream_id: String, provider_id: String, worktree_path: String) -> Self {
    let now = Utc::now();
    Self { session_id: uuid::Uuid::new_v4().to_string(), ... }   // ignores real id
// main.rs
.find(|s| s.session_id == session_id)   // extension's session_id ≠ stored uuid → never matches
.map(|s| s.provider_id).unwrap_or_else(|| config.defaults.provider.clone());
```
- Problem: The server looks up the provider by `session_id`, but every stored session has a fresh UUID instead of the session id the extension actually sends (`W1-C01` etc.). Result: `provider_id` resolution always falls back to `config.defaults.provider` — wrong-data whenever the CLI assigned a non-default provider (e.g. wave dispatch failover), and the repo's own state file shows the mismatch (`session_id: "d5d2ffbd…"` vs `stream_id: "W1-C01"`). Also breaks `get_session_status` for callers that pass the extension-side id.
- Fix: change signature to `new(session_id: String, stream_id: String, provider_id: String, worktree_path: String)` and pass the real id from `main.rs:287` (`SessionState::new(session_id.clone(), stream_id.clone(), provider_id.clone(), …)`).
- Verify: unit test — `SessionState::new("S01".into(), ...).session_id == "S01"`; end-to-end: start session with provider `zai`, send `ops.ready`, check state file `provider_id == "zai"`.

---

### T073 [EXT-1] [High] [STUB] `code_extractor` doesn't extract code blocks — returns the entire raw response

- File: `tools/glyim-pilot/extension/src/code_extractor.ts:3-11` (vs spec REQ-FUNC-057; server counterpart `src/protocol/parser.rs:5-41` dead)
- Code:
```ts
// Always treat the whole response as a single block
// because the assistant outputs raw directives without backticks.
const blocks = [normalized];
```
- Problem: Spec REQ-FUNC-057 requires extracting `glyim-ops` fenced blocks (with nested-fence handling) from `<pre><code>`. The implementation returns the **entire assistant text**, so: (1) any example/diagram the model writes (e.g. "here's the format: `::WRITE /etc/x`") is parsed as an op by the Rust parser, which silently ignores unknown lines (PILOT-16) and only rejects `::`-prefixed mistakes — prose directives execute; (2) the careful nested-fence parser on the Rust side (`extract_ops_blocks`) is dead code, so the two sides implement *different* protocols.
- Fix: port the Rust logic verbatim:
```ts
export function extractGlyimOpsBlocks(response: string): string[] {
  const lines = normalizeLineEndings(response).split('\n');
  const blocks: string[] = []; let inside = false;
  for (const line of lines) {
    const t = line.trim();
    if (!inside && t === '```glyim-ops') { inside = true; continue; }
    if (inside && t === '```') { inside = false; continue; }
    if (inside) blocks[blocks.length - 1] = (blocks[blocks.length - 1] ?? '') + line + '\n';
  }
  return blocks.length ? blocks : [normalizeLineEndings(response)]; // fallback keeps legacy behavior
}
```
and send each block separately (server-side `parse_ops_block` is already per-block). Cross-verify parity with a shared fixture file so both parsers agree.
- Verify: `npx vitest` (add test): response with prose + one fenced block → one block returned, prose excluded; also `cargo test -p glyim-pilot test_extract_nested_fences` fixtures mirrored in TS.

---

### T074 [EXT-2] [High] [BUG] `content.ts` accepts `stream_complete` from any frame/origin — spoofed ops execution

- File: `tools/glyim-pilot/extension/src/content.ts:3-18`, `background.ts:362-382`
- Code:
```ts
window.addEventListener('message', (event) => {
  if (event.data && event.data.type === 'stream_complete') { chrome.runtime.sendMessage({ type: 'stream.complete', sessionId: event.data.sessionId, ... }); }
```
- Problem: The content script runs on the provider pages, but `window` message events are received from **every** frame (ads, iframes, injected scripts) with no `event.origin`/`event.source` check. The injected page helper (`background.ts:184`) also `postMessage`s `sessionId` into the page world, so a malicious script on the provider page (or an iframe ad) can learn the sessionId and post a fake `stream_complete` whose `fullResponse` is attacker-chosen glyim-ops → background extracts it and sends `ops.ready` → the server writes attacker files into the session worktree (within path containment, but arbitrary code content, and the only remaining defense is the currently-disabled banned-pattern gate). Spec security arch: extension interacts only with registered sessions — violated.
- Fix (3 steps):
  1. In `content.ts`: `if (event.source !== window) return;` + validate `typeof event.data.sessionId === 'string' && event.data.turn === thisTurn` (turn injected into page).
  2. Better: skip the page round-trip — have the injected script use `chrome.runtime.sendMessage` directly (it already runs in the extension's isolated world) and delete `content.ts` forwarding entirely.
  3. Defense-in-depth server-side: turn the extension message schema strict — reject `ops.ready` whose `turn` is older than the last processed turn for that stream (replay guard).
- Verify: on a provider page console: `window.postMessage({type:'stream_complete', sessionId:'X', turn:0, fullResponse:'::WRITE evil.txt\npwned\n::END'}, '*')` → with fix, no message forwarded; server-side turn guard rejects stale turns.

---

### T075 [EXT-3] [High] [BUG] zai/qwen `assistantSelector` targets code-editor line containers — textContent loses newlines, directives glue into garbage paths

- File: `tools/glyim-pilot/extension/src/providers/index.ts:20,31` (`'.language-glyim-ops .cm-content'`, `'.view-lines'`), `adapter.ts:160-163`
- Code:
```ts
getAssistantText(): string { const lastEl = document.querySelector(`${this.assistantSelector}:last-of-type`); return lastEl?.textContent ?? ''; }
```
- Problem: CodeMirror (`.cm-content`) and Monaco (`.view-lines`) render each line as a separate `<div>`; `textContent` concatenates children **without separators**, so the extracted response is one giant line: `::WRITE src/lib.rsfn main() {}::END::COMMIT "x"`. The Rust parser then treats everything after `::WRITE ` as a **path** → applies a write to a file literally named `src/lib.rsfn main() {}::END::COMMIT "x"` (created on Linux) and loses the entire payload. Also `:last-of-type` on deepseek's `pre` selector picks only the last code block, dropping sibling directives.
- Fix: extract per-line text with newlines restored:
```ts
getAssistantText(): string {
  const lastEl = document.querySelector(this.assistantSelector); // drop :last-of-type
  if (!lastEl) return '';
  const lineEls = lastEl.querySelectorAll('.cm-line, .view-line');
  return lineEls.length ? Array.from(lineEls).map(l => l.textContent ?? '').join('\n') : (lastEl.textContent ?? '');
}
```
plus a parser-level tripwire: in `parse_ops_block`, reject a `::WRITE` path containing whitespace or `::` (path must match `^[\w./\-]+$`) → returns E0100 instead of creating garbage files.
- Verify: `cargo test -p glyim-pilot` new test `write_path_with_space_is_error`; manual: run session on z.ai with the fixed adapter and confirm `git -C worktree status` shows real files, not one glued filename.

---

**Wave 2 checkpoint:** `cargo build --workspace && cargo test --workspace && cargo test -p glyim-pilot; git add -A && git commit -m "fix(wave-2): T017-T075 highs (parser, lowering, types, bytecode, macros, res, HIR, LSP, pipeline, glyip, stdlib, harness, pilot)"`
---

# WAVE 3 — Medium-severity fixes

## 3.1 Frontend

### T076 [FE-108] [Medium] [BUG] Rest pattern `..` unsupported in tuple / tuple-struct patterns

- File: `crates/glyim-frontend/src/parser/pat.rs:51` (and the PatStruct tuple form at :125)
- Code:
```rust
SyntaxKind::LParen => {
    self.start_node(SyntaxKind::PatTuple);
    self.bump(); // (
    while self.current_kind() != SyntaxKind::RParen && self.current().is_some() {
        self.parse_pat();
        if self.current_kind() == SyntaxKind::Comma {
            self.bump();
        }
    }
```
- Problem: `..` inside `(...)` is delegated to `parse_pat`, which has no `DotDot` arm → "expected pattern, found DotDot" + cascade. `let (a, ..) = t;`, `let (a, .., c) = t;` and tuple-struct `S(..)` / `S(a, ..)` all fail, though `PatSlice` handles `..` correctly (pat.rs:89) — the support exists but only for slices. This is a very common idiom (`Point(..)` to ignore fields).
- Fix: in both tuple-pattern loops (parse_pat_single's LParen arm and parse_pat_inner_impl's PatStruct `LParen` arm), special-case `DotDot` like the slice arm does:
```rust
if self.current_kind() == SyntaxKind::DotDot {
    self.bump(); // ..
    if self.current_kind() == SyntaxKind::Comma { self.bump(); }
    continue;
}
```
and (optionally) wrap it in a `PatRest`-style marker or record it for `lower_pat` (which currently reads tuple children positionally — confirm `lower_pat`'s `PatTuple` arm tolerates the stray `..` token; it scans for pattern children so the bare `..` token is inert).
- Verify: frontend tests on `let (a, ..) = (1,2,3);` and `match p { Point(..) => 1 }` asserting zero diagnostics.

---

### T077 [FE-110] [Medium] [MISCOMPILE] Parenthesized type `(T)` silently becomes a 1-tuple

- File: `crates/glyim-frontend/src/parser/ty.rs:75` (consumer: `crates/glyim-hir/src/lower/lower_type.rs:143`)
- Code (ty.rs):
```rust
SyntaxKind::LParen => {
    self.start_node(SyntaxKind::TupleType);
    self.bump(); // (
    while self.current_kind() != SyntaxKind::RParen && self.current().is_some() {
        self.parse_type();
        if self.current_kind() == SyntaxKind::Comma {
            self.bump();
        }
    }
    self.expect(SyntaxKind::RParen);
    self.finish_node();
}
```
- Problem: `(i32)` always produces `TupleType`, and `lower_type_ref` maps `TupleType` to `TypeRef::Tuple(vec![i32])`. In Rust `(T)` is exactly `T`, so `fn f(x: (i32)) -> (i32)` silently has a 1-tuple parameter/return (ABI, layout, and generics all wrong), and `let y: (u8) = 5;` produces a confusing mismatch error. No diagnostic is emitted at the parse/lower site.
- Fix: only build `TupleType` when a comma (or ≥2 elements) is present:
```rust
SyntaxKind::LParen => {
    let cp = self.checkpoint();
    self.bump(); // (
    self.parse_type();
    if self.current_kind() == SyntaxKind::Comma {
        self.start_node_at(cp, SyntaxKind::TupleType);
        while self.current_kind() == SyntaxKind::Comma {
            self.bump();
            if self.current_kind() == SyntaxKind::RParen { break; } // trailing comma
            self.parse_type();
        }
        self.expect(SyntaxKind::RParen);
        self.finish_node();
    } else {
        // `(T)` — parenthesized type: the inner type node stays the type;
        // `(` / `)` remain as loose tokens inside the parent.
        self.expect(SyntaxKind::RParen);
    }
}
```
- Verify: `cargo test -p glyim-frontend --lib` (no snapshot uses `(T)` types — confirm) and a typeck test asserting `let x: (i32) = 5;` compiles and layout matches `i32`.

---

### T078 [FE-111] [Medium] [BUG] Depth-guard gaps: `parse_type` and `else if` chains recurse unbounded; deep-nesting corpus fixtures were emptied

- File: `crates/glyim-frontend/src/parser/ty.rs:6` (whole file has zero `recursion_depth` references) and `crates/glyim-frontend/src/parser/expr.rs:488`
- Code (expr.rs):
```rust
if self.current_kind() == SyntaxKind::KwElse {
    self.bump();
    if self.current_kind() == SyntaxKind::KwIf {
        self.parse_if_expr();
    } else {
        self.parse_block();
    }
}
```
- Problem: The FE-6 fix guards `parse_expr`, `parse_block`, `parse_unary_expr`, `parse_pat`, and token trees — but **not** `parse_type` (deep `((((…T…))))` tuple types or `&&&&…&T` recurse directly) and not the `else if` chain (each link recurses `parse_if_expr` without any counter). ~20–50k-deep inputs (generated code/fuzzing) overflow the stack → ICE. Additionally, the corpus fixtures meant to catch this — `crates/glyim-test/tests/no-ice/{deep_if,deep_parens,deep_arrays}.g` — are all **0 bytes**, so the no-ICE corpus provides zero coverage (they pass trivially).
- Fix (3 steps):
  1. Add the standard guard at the top of `parse_type` (`if self.recursion_depth > MAX_EXPR_DEPTH { error; bump-one; return; }` with inc/dec around the body).
  2. Increment/check `recursion_depth` in the else-if branch of `parse_if_expr` before recursing.
  3. Restore real content in the three no-ice fixtures (e.g. 400-deep `else if` chain, 400 nested `(` in a type, deep array types) so the corpus actually exercises the guards.
- Verify: `cargo test -p glyim-frontend --lib depth_limit` after adding `deep_type_nesting_is_bounded` / `deep_else_if_chain_is_bounded` cases asserting a "nested too deeply" diagnostic instead of a crash; `cargo test -p glyim-test no_ice_corpus`.

---

### T079 [FE-112] [Medium] [STUB] `async` blocks, `comptime fn`, and `priv` have no grammar — `comptime fn` silently degrades to a plain fn

- File: `crates/glyim-frontend/src/parser/item.rs:37` (also stmt.rs:79-110, expr.rs:640, item.rs:444)
- Code:
```rust
if (self.current_kind() == SyntaxKind::KwConst
    || self.current_kind() == SyntaxKind::KwAsync)
    && self.next_non_ws_kind() == SyntaxKind::KwFn
{
    self.parse_fn_def();
    return;
}
```
- Problem: (a) `KwAsync` has no expression/statement arm at all — `async { … }` / `async move { … }` produce "unexpected token in statement: KwAsync" + cascade, even though `lower_async.rs` describes `async { 42 }` shapes and `.await` is fully wired; (b) `KwComptime` is routed nowhere: `comptime fn f() {}` emits "expected item, found KwComptime", the item-skip recovery bumps `comptime`, stops at `fn`, and parses a **plain `FnDef`** — the comptime-ness is silently lost with only one unrelated-looking error; (c) `KwPriv` (lexed, tested in `keywords.rs`, and `priv fn internal() {}` lexes fine per `programs.rs`) is likewise eaten by the item-skip loop and the privacy marker is dropped. Declared keyword → silent no-op is the definition of a stub path.
- Fix (2 steps):
  1. Add `KwComptime` and `KwPriv` to the modifier handling (route `comptime fn`/`priv fn` through `parse_fn_def`, bumping the modifier inside `FnDef` exactly like `const`/`async`, and thread an `is_comptime`/visibility bit through lowering).
  2. Either add an `AsyncBlockExpr`-style node plus a `KwAsync` arm in `parse_primary_expr`/`parse_stmt`, or make the parser emit an explicit "async blocks are not supported" diagnostic instead of an "unexpected token" cascade.
- Verify: unit tests: `comptime fn f() -> u8 { 1 }` parses to an `FnDef` carrying the comptime marker (zero diagnostics); `priv fn f() {}` likewise; `cargo test -p glyim-frontend --lib keywords programs`.

---

## 3.2 Macros, resolution, HIR

### T080 [DM-3] [Medium] [BUG] Nested use-trees: `use a::{b::c}` and `use a::{self}` are silently dropped

- File: `crates/glyim-def-map/src/lib.rs:652-654` (guard), :456-473 (`extract_path_from_syntax` returns `None` for bare `self`)
- Code:
```rust
if let (Some(base_mod), Some(inner_p)) = (base_module, inner_path)
    && inner_p.segments.len() == 1
{
```
- Problem: Inside a nested tree (`use std::io::{Read, Write}`), each inner `UseTree` is only handled when its extracted path has **exactly one** segment. `use a::{b::c}` (2 segments) matches nothing and is skipped without a diagnostic; the same happens for the `self` element — `extract_path_from_syntax` collects no segments for a bare `KwSelf` (only sets `PathKind::SelfPath`), returns `None`, so `use std::io::{self, Read}` imports `Read` but **not** `io`, despite the code comment at :659-660 claiming `{self, Read}` is handled. Trigger: `mod a { pub mod b { pub fn c() -> i32 { 9 } } } use a::{b::c}; fn main() -> i32 { c() }` → unresolved `c` with no import-level diagnostic.
- Fix (2 steps):
  1. Recurse instead of guarding — after building `inner_p`, if `inner_p.segments.len() != 1`, prepend `base` segments and re-dispatch through `process_use_tree`-equivalent logic (resolve module prefix, then final segment).
  2. Special-case a bare `self` inner tree: if the inner tree has a `KwSelf` token and no path node/segments, declare `base_mod`'s own `def_id` under its last base-segment name (or alias).
- Verify: `cargo test -p glyim-def-map` with `use a::{b::c};` and `use a::{self, f};` cases asserting both `c`/`io`-module bindings exist.

---

### T081 [DM-4] [Medium] [BUG] Duplicate `EnumDef` names silently overwrite — no duplicate-definition diagnostic

- File: `crates/glyim-def-map/src/lib.rs:844-908` (EnumDef arm; dup check exists only at :929-941)
- Code:
```rust
SyntaxKind::EnumDef => {
    let name_str = extract_ident(&child);
    let name = interner.intern(&name_str);
    ...
    modules[parent_module].scope.declare(
        name, enum_local, vis.clone(), span, Namespace::Types,
    );   // IndexMap::insert — overwrites, no `existing` check
```
- Problem: The `FnDef | StructDef | ...` arm checks `scope.<ns>.contains_key(&name)` and emits `duplicate definition of ...`; the `EnumDef` arm has no such check. `enum Color { Red } enum Color { Blue }` → the second silently overwrites the first in `scope.types`, pushes a **second** child module named `Color` into `parent.children` (`Color::Blue` resolves, `Red`'s variants orphan), and HIR keeps both `EnumDef` items — later phases see whichever name lookup hits first. Same for `mod Color {} enum Color {}` depending on order.
- Fix: At the top of the EnumDef arm, before declaring, check `modules[parent_module].scope.types.contains_key(&name) || modules[parent_module].children.iter().any(|(n, _)| *n == name)` and push `GlyimDiagnostic::parse_error(span, format!("duplicate definition of `{}`", interner.resolve(name)))` + skip, mirroring :929-941.
- Verify: `cargo test -p glyim-def-map` + new test `enum E { A } enum E { B }` asserting exactly one `duplicate definition of `E`` diagnostic.

---

### T082 [MAC-3] [Medium] [BUG] Unbound metavariable inside `$(...)*` in the template silently expands to nothing

- File: `crates/glyim-meta/src/expander/substitution.rs:84-88` (with :142-175 `find_all_metavars`)
- Code:
```rust
let repetitions: usize = var_names
    .iter()
    .filter_map(|name| bindings.get(name).map(|v| v.len()))
    .max()
    .unwrap_or(0);
```
- Problem: The module doc (:8-11) says unbound metavars are a hard error "because a `$(...)*` repetition tied to a metavar that never matched produces an expansion that cannot type-check" — but that error only fires for `$x` **outside** repetitions (line 44-47). Inside a repetition, `bindings.get(name) == None` is filtered out by `filter_map`, `repetitions` becomes 0, and the whole `$(...)*` vanishes with `Ok(vec![])`. Trigger: `macro_rules! m { () => { $( let _ = $q; )* } } m!();` compiles to an empty expansion instead of "unbound metavariable `$q`".
- Fix: after `find_all_metavars(inner)`, validate all names are bound:
```rust
if let Some(unbound) = var_names.iter().find(|n| !bindings.contains_key(n)) {
    return Err(unbound.clone());
}
```
before computing `repetitions` (propagates through the existing `?` at :100/:108 into the `mod.rs:499` diagnostic).
- Verify: `cargo test -p glyim-meta` + case `macro_rules! m { () => { $( $q )* } } m!()` asserting the "unbound metavariable" diagnostic fires.

---

### T083 [MAC-5] [Medium] [PERF] `consume_fragment` greedy longest-match re-parses every shrinking prefix — quadratic per fragment, per arm

- File: `crates/glyim-meta/src/expander/matcher.rs:363-400` (loop at :392-398)
- Code:
```rust
for take in (1..=terminator).rev() {
    let prefix = &remaining[..take];
    let src = to_source(prefix);
    if glyim_frontend::try_parse_fragment(kind, &src).is_some() {
        return Some((take, prefix.to_vec()));
    }
}
```
- Problem: For each flexible fragment (`$e:expr`, `$t:ty`, …) the loop tries the longest prefix first and re-runs a **full frontend parse** per attempt. When the fragment fails on the long forms, the work is O(n) parses × O(n) parse each = O(n²) tokens per fragment; with A arms tried before a match, O(A·n²) per invocation, and the whole pattern match re-runs per expansion.
- Fix: Parse once and memoize — have `try_parse_fragment` return the consumed length (or expose a `try_parse_fragment_prefix(kind, src) -> Option<usize>` that stops at the first token boundary where the parse cannot continue), then walk forward incrementally instead of re-parsing shrinking suffixes; alternatively cache `(kind, prefix-hash) -> bool` in a thread-local/LRU for the duration of one `match_pattern`.
- Verify: `cargo test -p glyim-meta` (all matching tests green) + a timing test: pattern with one failing arm then a matching arm over a 2k-token no-separator input completes < 100 ms (previously quadratic).

---

### T084 [MAC-6] [Medium] [STUB] `assert!`/`assert_eq!`/`debug_assert*` expand to `()` — condition and side effects silently discarded (old HIR-9, still present at HEAD)

- File: `crates/glyim-meta/src/expander/mod.rs:1009-1015` (Assert), :965-973 (Format), :990-993 (Matches)
- Code:
```rust
BuiltinMacro::Assert => {
    // assert!(..) → `()`.
    vec![
        TokenTree::Token(SyntaxKind::LParen, SmolStr::from("(")),
        TokenTree::Token(SyntaxKind::RParen, SmolStr::from(")")),
    ]
}
```
- Problem: `assert!(self.push(x));` expands to `()` — the push **never happens**; `format!("x={}", v)` yields `""` (wrong runtime values, silently); `matches!(e, pat)` yields `true` ignoring both operands. All are registered by default in `ExpanderImpl::new` (:160-165), so any stdlib/user code using them compiles clean and misbehaves. This is old-audit HIR-9; the code is unchanged at `6bab26ed` — re-verified, not fixed.
- Fix (mechanical): For Assert, expand to `if !(cond) { loop {} }` shape — parse the first `$e:expr` via the existing matcher (`match_pattern` with pattern `($e:expr)`) and emit `KwIf, !, Group($e), Group(loop{}), else Group(())` token trees; for Matches, emit an equivalent `match $e { pat => true, _ => false }` token stream; keep Format as a stub but make it **error** (`env!`-style diagnostic) when it has more than one argument so silent wrong values become loud.
- Verify: `cargo run -p glyim-cli -- prog.g --emit=exec` with `fn main() { let mut v = Vec...; assert!(v.push(1)); }`-style fixture asserting the side effect executes; `cargo test -p glyim-meta builtin`.

---

### T085 [HIRX-4] [Medium] [BUG] `?` on user-derived lowering inside item lowering silently deletes whole items

- File: `crates/glyim-hir/src/lower/lower_item.rs:399` (struct field type), :506 & :529 (variant fields), :593 (impl trait type), also :608/:772 (`?` on method-name extraction)
- Code:
```rust
let fty = lower_type_ref(ty, interner)?;
fields.push(Field { name: fname, ty: fty, span: node_span(node) });
```
and
```rust
} else if trait_ref.is_none() {
    // Before `for`: this is the trait.
    if let TypeRef::Path(p) = lower_type_ref(&child, interner)? {
        trait_ref = Some(p);
    }
}
```
- Problem: `lower_type_ref` legitimately returns `None` for recoverable/unhandled type nodes (it documents that at lower_type.rs:164-171). In these item contexts, one such node aborts the **entire item**: a struct with one field whose type fails to lower produces no `ItemKind::Struct` at all (every later mention becomes a misleading "unresolved"), a variant with one bad field vanishes from the enum, and `impl Trait for Foo` where the trait path fails to lower drops the whole impl (trait methods silently unregistered). No diagnostic is emitted — violates the crate's own §3.1 policy.
- Fix: Replace `?` with per-entry error recovery: on `None`, push
```rust
diags.push(GlyimDiagnostic::internal_error(format!(
    "failed to lower type of field in `{}`", interner.resolve(name))));
```
and either skip the field/variant (collecting what lowered) or use `TypeRef::Error` as the field type so the item survives: `let fty = lower_type_ref(ty, interner).unwrap_or(TypeRef::Error);`. For :593 use `match lower_type_ref(...) { Some(TypeRef::Path(p)) => trait_ref = Some(p), other => { if other.is_none() { diags.push(...) } } }`.
- Verify: `cargo test -p glyim-hir` + new test: struct with a deliberately unsupported type node (e.g. recovered parse) asserting the struct item still exists in `hir.items` and a diagnostic is present.

---

### T086 [HIRX-9] [Medium] [MISCOMPILE] Open-start ranges `..end` / `..=end` lower with start and end swapped (`..5` becomes `5..`)

- File: `crates/glyim-hir/src/lower/lower_expr.rs:2163-2190` (with parser `crates/glyim-frontend/src/parser/expr.rs:77-97`)
- Code:
```rust
let children: Vec<SyntaxNode> = node
    .children()
    .filter(|c| is_expr_node(c) || c.kind() == SyntaxKind::LitExpr)
    .collect();
let start = children.first().and_then(...);
let end = children.get(1).and_then(...);
let inclusive = ...DotDotEq...;
```
- Problem: The parser explicitly supports `..end` ("Range-from / inclusive-range-from: `..end` or `..=end` (no start)") and emits a `RangeExpr` whose **only** expr child is the end operand. Lowering assigns `children[0]` to `start` and `children[1]` to `end`, so `..5` lowers to `Range { start: Some(5), end: None, .. }` (i.e. `5..`) — for `for i in ..10 {}` the desugared iterator starts at 10 with no bound; `..=5` becomes `5..` non-bounded-above. `0..10` is unaffected (tests cover only that shape).
- Fix: Detect which side is present:
```rust
let leading_dotdot = node.children_with_tokens().next().is_some_and(|el|
    matches!(el.kind(), SyntaxKind::DotDot | SyntaxKind::DotDotEq));
// if leading_dotdot: the single child is the END; start: None
```
then branch on it before the `start`/`end` assignment.
- Verify: `cargo test -p glyim-hir lower_expr` + new test lowering `for _ in ..3 { count += 1; }` asserting `Expr::Range { start: None, end: Some(_), .. }` (and a run-pass counting exactly 3).

---

### T087 [PM-1] [Medium] [PERF] Every proc-macro token leaks a `CString` — `mem::forget` per token per invocation (old HIR-34, still present)

- File: `crates/glyim-proc-macro/src/lib.rs:354-370`
- Code:
```rust
let ctext = CString::new(text.as_str()).unwrap_or_default();
let pm_text = PmStr { ptr: ctext.as_ptr() as *const u8, len: text.len() as u32 };
pm_ts_push(&mut in_ts, PmToken { kind: *kind as u16, text: pm_text });
// ctext is leaked intentionally: the dylib reads it during
// the call; kept alive for the duration of the call.
std::mem::forget(ctext);
```
- Problem: The closure runs per proc-macro invocation; each input token leaks one heap allocation that is never freed (the comment's "duration of the call" is achieved by *never* freeing). Compiling with proc macros over many files/tokens grows RSS unboundedly. Re-verified unchanged at HEAD (old-audit HIR-34).
- Fix: Keep the CStrings alive until after the call instead of forgetting:
```rust
let mut owned: Vec<CString> = Vec::with_capacity(input.len());
// push each CString, build pm_text from owned.last().unwrap(),
// and simply drop `owned` after `let result = pm_to_tokens(&out_ts);`
// (the dylib only reads during `(entry)(...)`, which happens before the drop).
```
- Verify: `cargo test -p glyim-proc-macro load_cdylib_round_trip_compiles_and_expands` still green; add a 10k-token loop under a heap profiler (`/usr/bin/time -v`) showing flat RSS across invocations.

---

## 3.3 Type system

### T088 [SOLVE-26] [Medium] [BUG] Speculative deref/Vec coercions inside the Ref arm commit inference bindings on failure paths without rollback (SOLVE-4 still present)

- File: `crates/glyim-solve/src/infer.rs:483-523`
- Code:
```rust
if let Some((t, u)) = vec_a_to_slice_b {
    if self.unify_tys(ctx, t, u, span).is_ok() {   // may commit ?A:=u8, then fail deeper
        return Ok(constraints);
    }
}
...
constraints.extend(self.unify_tys(ctx, ty_a, ty_b, span)?);
```
- Problem: The three speculative `unify_tys(...).is_ok()` probes (Vec→slice both directions, `deref_ty` both directions) mutate `int_vars`/`ty_vars` and, if the probe ultimately fails, keep whatever was bound before the failure. The method-probe call sites snapshot (`check_expr.rs:2856-2862`), but ordinary statement unification (`FnCtxt::unify`, typeck/src/unify.rs:29-40) does not — e.g. `let x: &[u8] = &vec_of_tuples_mismatch;` leaves partially-bound int vars installed from the failed Vec→slice attempt, polluting all later inference in the body.
- Fix: wrap the whole speculative block:
```rust
let snap = self.snapshot();
if self.unify_tys(ctx, t, u, span).is_ok() { return Ok(constraints); }
self.rollback_to(snap.clone());
```
for each probe; only skip rollback when returning `Ok`. (`InferenceTable::snapshot`/`rollback_to` already exist.)
- Verify: unit test in `glyim-solve`: unify `&Vec<(?A, u8)>` with `&[(u8, bool)]` must leave `?A` unbound (probe `infer.probe_ty_var`); `cargo test -p glyim-solve`.

---

### T089 [SOLVE-27] [Medium] [BUG] Inference-var self type matches *any* impl (SOLVE-9 still present) while Param-self `where` bounds are hard-rejected — both directions wrong

- File: `crates/glyim-solve/src/solver.rs:276-278` (Infer arm), `434-435` (DefiniteNo), and `crates/glyim-typeck/src/lib.rs:2063-2073` (skip-list)
- Code:
```rust
if matches!(pred_kind, TyKind::Infer(_)) {
    return true;                                  // ?T: Clone proven against ANY impl
}
...
if matching_impls.is_empty() {
    return SolverResult::DefiniteNo;              // Param(T): Clone -> hard error
}
```
- Problem: (a) A trait obligation whose self is still an inference var is *proven* by the first matching impl — non-Clone finals are accepted silently (SOLVE-9 unfixed). (b) Conversely, `fn f<T>(x: T) where T: Clone` pushes `Param(T): Clone` via `process_where_clauses` (lib.rs:2094-2110; the skip list only covers `Fn|FnMut|FnOnce|Send|Sync` at :2070), no impl has a Param self, so the solver answers `DefiniteNo` and fulfillment turns it into a hard "trait bound not satisfied" — while the *inline* form `fn f<T: Clone>(x: T)` is only registered in `param_bounds` (lib.rs:723-741) and never checked. Asymmetric: identical programs accepted/rejected depending on syntax.
- Fix: In `prove_trait`, return `Ambiguous` when the predicate's self ty is `Infer(_)` or `Param(_)` (no impl matching); in `FulfillmentCtx::process_obligations`, treat `Ambiguous` for a Param-self trait predicate as proven-by-declared-bound; for Infer-self, record the obligation and re-check after zonk (or conservatively make it an error at zonk time if unresolved).
- Verify: `t1.g` = `fn f<T>(x: T) where T: Clone {}` must compile; `t2.g` = `struct S; fn g<T: Clone>(x: T) {} g(S);` must error; add solver unit tests for both.

---

### T090 [TCK-26] [Medium] [BUG] Struct patterns bind fields with the *declared* (unsubstituted) field type (SOLVE-12 still present)

- File: `crates/glyim-typeck/src/unify.rs:42-55`, called from `crates/glyim-typeck/src/check_pat.rs:366-373`
- Code:
```rust
pub fn lookup_field_ty(&mut self, adt_id: AdtId, field: Name, span: Span) -> Ty {
    if let Some(field_idx) = self.ctx.field_index(adt_id, field)
        && let Some(def) = self.ctx.adt_def(adt_id)
        && let Some(field_def) = def.fields.get(FieldIdx::from_raw(field_idx as u32))
    {
        let field_ty = field_def.ty;   // formal Param(T), no substitution
        return field_ty;
    }
```
- Problem: The tuple-variant path right above (check_pat.rs:323-354) substitutes the scrutinee's generic args, but the struct-pattern path does not: `let Wrapper { v } = w;` with `w: Wrapper<u64>` binds `v: Param(T)` (rigid). Downstream, `v + 1u64` errors spuriously, or worse, `Param` unifies by index only (infer.rs:404-415) so `v` silently unifies with an unrelated `T` elsewhere. The expression-side field access already has the fixed variant (`lookup_field_ty_with_substs`, check_expr.rs:3422) — only the pattern path was missed.
- Fix: In `check_pat.rs:366-373`, peel refs off `expected_ty`, extract `TyKind::Adt(_, substs)` into a `HashMap<u32, GenericArg>` (same code as :325-340), and call `self.ctx.subst_ty(self.lookup_field_ty(adt_id, *field_name, span), &subst)`.
- Verify: `struct W<T>{v:T} fn main(){ let w = W{v:1u64}; let W{v} = w; let _x: u64 = v; }` compiles; `cargo test -p glyim-typeck` green.

---

### T091 [CE-25] [Medium] [MISCOMPILE] Const `==` has no float/tuple/struct arm — `1.5 == 1.5` folds to `false`; `<` rejects floats (HIR-23 still present)

- File: `crates/glyim-const-eval/src/eval.rs:852-862` (compare_eq), `864-882` (compare_lt)
- Code:
```rust
let equal = match (lhs, rhs) {
    (ConstValue::Int(a, _), ConstValue::Int(b, _)) => a == b,
    (ConstValue::Uint(a, _), ConstValue::Uint(b, _)) => a == b,
    (ConstValue::Bool(a), ConstValue::Bool(b)) => a == b,
    (ConstValue::Char(a), ConstValue::Char(b)) => a == b,
    (ConstValue::Unit, ConstValue::Unit) => true,
    _ => false,        // floats, tuples, arrays, structs -> "not equal"
};
```
- Problem: `const X: bool = 1.5 == 1.5;` evaluates to `false` (and `1.5 != 1.5` → `true`) silently; `const X: bool = (1,2) == (1,2);` → false; `const Y: bool = 1.5 < 2.5;` is a hard error "requires matching numeric or char operands". Range patterns over float consts also misbehave (`compare_lt` error). This is the same site the old audit flagged; unchanged at HEAD.
- Fix: Add arms `(FloatBits(a, ta), FloatBits(b, tb)) => ta == ta && f64::from_bits(a) == f64::from_bits(b)` (after T035 fixes the convention), recursive equality for `Tuple`/`Array`/`Struct`, and a float arm in `compare_lt`.
- Verify: const-eval unit tests: `1.5 == 1.5` → true, `(1,2) == (1,2)` → true, `1.5 < 2.5` → true.

---

### T092 [SOLVE-28] [Medium] [BUG] Occurs check, `fully_resolve` and var-collection skip `Projection`/`Dynamic` (SOLVE-5 still present)

- File: `crates/glyim-solve/src/infer.rs:135-169` (occurs), `1238-1277` (`has_unresolved_non_ty_infer`), `1279-1316` (`collect_unresolved_vars`)
- Code:
```rust
fn occurs(&self, ctx: &dyn TypeLookup, var: TyVar, ty: Ty) -> bool {
    let ty = self.resolve_ty_shallow(ctx, ty);
    match ctx.ty_kind(ty) {
        TyKind::Infer(InferVar::Ty(v)) if *v == var => true,
        TyKind::Ref(_, inner, _) => self.occurs(ctx, var, *inner),
        ...                                    // no Projection / Dynamic arm
        _ => false,
    }
}
```
- Problem: `?T = <Vec<?T> as Iterator>::Item` (or a `dyn` predicate mentioning `?T`) passes the occurs check → a self-referential type is installed; likewise `fully_resolve` reports `Ok` for a `Projection` whose trait substs still contain unresolved vars, so raw infer vars can reach downstream passes.
- Fix: Add to all three walkers:
```rust
TyKind::Projection(proj) => /* walk trait_ref.substs args */,
TyKind::Dynamic(preds, _) => /* for pred in preds.skip_binder() { walk Trait predicate substs } */,
```
(Projection/Dynamic substitution machinery already exists in hrtb.rs:179-188 to copy from.)
- Verify: unit test `unify(?T, Projection(self=<Vec<?T>>))` must fail with "infinite type"; `cargo test -p glyim-solve`.

---

### T093 [TCK-27] [Medium] [STUB] Builtin-method fallback accepts `Option`/`Result` methods on *any* ADT receiver (LL-13 still present)

- File: `crates/glyim-typeck/src/check_expr.rs:2570-2586`
- Code:
```rust
let candidates: [u32; 4] = [1010, 1011, 1006, 1007];
let mut found = None;
for cand in candidates {
    if let Some(hit) = self.ctx.lookup_builtin_method(AdtId::from_raw(cand), method_name) {
        // Only accept if the receiver's type-shape ... matches the candidate's arity.
        if matches!(self.ctx.ty_kind(step_ty), TyKind::Adt(_, _)) {
            found = Some(hit);
            break;
        }
    }
}
found?
```
- Problem: For any user ADT missing from the builtin table, `.unwrap()/.map()/.expect()/.is_some()` … resolve to Option's (1010) or Result's (1011) intrinsic `FnDefId` with the receiver's own substitution — `my_struct.unwrap()` type-checks and dispatches to `Option::unwrap` codegen (the guard is `matches!(step_ty, Adt(_))`, i.e. always true for ADT receivers). Silent wrong dispatch.
- Fix: Replace the candidate probe with a name-keyed lookup: recover the receiver ADT's name via `adt_name_for_id(adt_id)` and only fall back when `BuiltinAdt::from_name(name)` maps to the probed id — or simply delete the fallback and register the stdlib's own ADT ids in the builtin table at canonicalization time.
- Verify: `struct S{a:i32} fn main(){ let s = S{a:1}; let _x = s.unwrap(); }` must error "no method `unwrap`"; full workspace tests must stay green (stdlib `Result`/`Option` resolve via canonical builtin ids).

---

### T094 [SOLVE-29] [Medium] [STUB] Fulfillment silently discards `WellFormed`/`TypeOutlives`/`RegionOutlives`/`Coerce` obligations (SOLVE-6 still present); `RegionOutlives` always "Proven"

- File: `crates/glyim-solve/src/fulfill.rs:164-167`; `crates/glyim-solve/src/solver.rs:579`
- Code:
```rust
Predicate::WellFormed(_)
| Predicate::TypeOutlives(_)
| Predicate::RegionOutlives(_)
| Predicate::Coerce(_, _) => {}
```
- Problem: Every obligation of these kinds pushed by typeck (e.g. `Predicate::Coerce` from `Expr::Cast` sites, region constraints collected by `unify`'s `Constraint::RegionEq`) is popped and dropped without evaluation — placeholder regions and coercion failures cannot surface. Together with SOLVE-11 (region constraints returned by `unify` discarded by most callers) the whole region layer is a no-op.
- Fix: Minimum viable: route `Predicate::Coerce(a,b)` through `can_coerce` and error when it returns false *and* both sides are fully resolved; keep WF/Outlives as `Ok(())` but document. Longer term: thread `Constraint`s from `FnCtxt::unify` into a region vector and verify no `Region::Var` escapes the body at zonk time.
- Verify: `cargo test -p glyim-solve -p glyim-typeck` green; new test that a registered `Coerce(String → i32)` obligation produces a diagnostic.

---

### T095 [SOLVE-30] [Medium] [BUG] HRTB module is dead code in production, and contains two latent bugs: bound-var/placeholder misalignment (SOLVE-10 still present) and unsound `'p : 'static` proof

- File: `crates/glyim-solve/src/hrtb.rs:57-83` (misalignment), `489-495` (unsound proof); no production caller of `check_hrtb`/`instantiate_hrtb_predicate` (only `crates/glyim-solve/src/tests/hrtb.rs`)
- Code:
```rust
BoundVariableKind::Region(_) => {
    if placeholder_idx < placeholders.len() {
        region_map.push(Region::Placeholder(placeholders[placeholder_idx].clone()));
        placeholder_idx += 1;      // ordinal, not the bound-var index
    }
}
```
```rust
match (&rp.a, &rp.b) {
    _ if rp.a == rp.b => ...Proven,
    (Region::Placeholder(_), Region::Static) => Proven,   // 'p : 'static "proven"
```
- Problem: (1) `for<'a>` bounds are never checked by typeck — `fn f<T>(g: fn(&T))`-style higher-ranked signatures are accepted without any region reasoning (STUB). (2) When the binder's `bound_vars` interleave a `Ty` binder before a region (`[Ty, Region]`), `LateBound(_, 1, _)` looks up `region_map[1]` which holds nothing (regions-only vec) → the placeholder leaks unsubstituted. (3) The comment "'static … is outlived by everything" is wrong: proving `Placeholder : 'static` accepts programs where an arbitrary region escapes to `'static`.
- Fix (3 steps):
  1. Build `region_map` as a full-length vec indexed by the *original bound-var index* (`region_map.resize(bound_vars.len())`, assign placeholders at their `idx`).
  2. Delete the `(Placeholder, Static) => Proven` arm (keep `(Static, Placeholder)`).
  3. Wire `check_hrtb` into `FulfillmentCtx` for `Binder`-wrapped predicates so HRTB is actually consulted.
- Verify: unit test with binder `[Ty, Region]` asserting the region is replaced by a `Placeholder`; `for<'a> fn(&'a i32)` predicate must not prove `'a: 'static`.

---

### T096 [TY-26] [Medium] [STUB] Send/Sync machinery: `impl !Send`/`unsafe impl Send` never registered; `&mut T: Sync` gated on `T: Sync` (should be `T: Send`); raw pointers not Send/Sync

- File: `crates/glyim-type/src/auto_trait.rs:163-175` (&mut Sync), 177 (RawPtr); `register_manual_impl`/`register_negative_impl` have zero production call sites (only tests); consumed via `solver.rs:401-414` for concrete `T: Send/Sync` bounds
- Code:
```rust
TyKind::Ref(_, inner, Mutability::Mut) => {
    let inner_flags = compute_auto_traits_recursive(...);
    let mut flags = AutoTraitFlags::UNPIN;
    if inner_flags.contains(AutoTraitFlags::SEND) { flags |= AutoTraitFlags::SEND; }
    if inner_flags.contains(AutoTraitFlags::SYNC) { flags |= AutoTraitFlags::SYNC; }  // wrong: needs SEND
    flags
}
TyKind::RawPtr(_, _) => AutoTraitFlags::UNPIN,   // Rust: *const/*mut are Send + Sync
```
- Problem: `&mut Cell<i32>` (Sync in Rust) is rejected; `*mut u8` is not Send so `thread::spawn`-style code holding raw pointers fails; and `impl !Send for X` / `unsafe impl Send for X` declared in `.g` files are silently ignored (coherence records them in `CoherenceChecker::negative_impls` which is dead, and `AutoTraitRegistry` is never populated) — negative reasoning is a stub.
- Fix: `&mut` arm: `if inner_flags.contains(SEND) { flags |= SYNC; }`; `RawPtr` arm: `AutoTraitFlags::SEND | SYNC | UNPIN`; in typeck's impl scan, when the trait name is `Send`/`Sync`/`Unpin`, call `ctx.register_manual_impl(self_adt, trait)` for positive polarity and `register_negative_impl` for negative.
- Verify: unit tests in `crates/glyim-type/src/tests/auto_traits.rs` for `&mut Cell<i32>: Sync` and `*mut u8: Send`; `.g` repro with `impl !Send for X {}` must make `thread::spawn(x)` fail.

---

### T097 [CE-26] [Medium] [BUG] Const shifts silently wrap and truncate the shift amount (HIR-27 still present)

- File: `crates/glyim-const-eval/src/eval.rs:976-1001`
- Code:
```rust
(ConstValue::Int(a, ty), ConstValue::Int(b, _)) => {
    Ok(ConstValue::Int(a.wrapping_shl(*b as u32), *ty))
}
```
- Problem: `const X: i32 = 1i32 << 33;` evaluates to 2 (wrapping), while `+`/`-`/`*` overflow correctly error in `apply_binop` — inconsistent policy silently produces wrong consts. A negative shift `x << -1` becomes `u32::MAX` (masked by `wrapping_shl`).
- Fix: check `b` first:
```rust
if *b < 0 || *b >= bits_of(ty) { return Err("attempt to shift with overflow".into()); }
```
then use `checked_shl/checked_shr`; keep a `wrapping_*` variant only if the language decides to support `wrapping_shl` intrinsics.
- Verify: const-eval test `1i32 << 33` must error; `1u8 << 1` = 2 stays green.

---

### T098 [CE-27] [Medium] [BUG] Const struct patterns match fields by position, ignoring names (HIR-25 still present)

- File: `crates/glyim-const-eval/src/eval.rs:1120-1134`
- Code:
```rust
Pat::Struct { fields, .. } => {
    if let ConstValue::Struct(vals) = value {
        if fields.len() != vals.len() { return Ok(false); }
        for ((_, pat_id), (_, val)) in fields.iter().zip(vals.iter()) {  // names discarded
            if !self.pattern_matches(pat_id, val)? { return Ok(false); }
```
- Problem: `match p { Point { y, x } => ... }` (name order ≠ declaration order) binds `x` to the value of `y` and vice versa during const evaluation — silent wrong bindings/const results. The pattern's field names (`fields[i].0`) are available but zipped positionally against the value.
- Fix: look each pattern field up by name in `vals`:
```rust
let matching = vals.iter().find(|(n, _)| n == pat_field_name);
let Some((_, val)) = matching else { return Ok(false) };
if !self.pattern_matches(pat_id, val)? { return Ok(false); }
```
- Verify: const-eval test with a 2-field struct pattern written in reverse order binds correctly.

---

## 3.4 Lowering, borrowck, opts, interpreter

### T099 [LOW-8] [Medium] [BUG] Dynamic range-slice bounds-check failures end in `Unreachable` instead of a panic

- File: `crates/glyim-lower/src/lower_rvalue.rs:2136-2138` (also :2067-2071 for the `..=` overflow arm)
- Code:
```rust
self.current_block = Some(check_end_le_len_bb);
self.terminate(TerminatorKind::Unreachable, span);
```
- Problem: Both runtime check failure paths (`start > end` and `end > len`, plus `end+1` overflow for `..=`) jump to blocks terminated with `Unreachable`. `&arr[5..3]` on a 3-element array reaches `Unreachable` → interpreter: `InterpError::Panic("reached unreachable terminator")` (an ICE-shaped crash); LLVM codegen: `unreachable` instruction = UB. `AssertMessage::BoundsCheck` exists in the MIR (`crates/glyim-mir/src/lib.rs:605`) but is never emitted by lowering.
- Fix: Replace the two `Unreachable` terminators with:
```rust
TerminatorKind::Assert {
    cond: <check>, expected: true, target: <done/cont bb>,
    cleanup: None, msg: AssertMessage::BoundsCheck,
}
```
— i.e. keep the existing check locals and use `Assert` instead of a two-target `SwitchInt` whose fail edge is `Unreachable`.
- Verify: run-fail fixture `&arr[5..3]` expecting the bounds panic message; `cargo test -p glyim-lower dynamic_range_slice` with an out-of-range case asserting the terminator is `Assert` with `BoundsCheck`.

---

### T100 [LOW-9] [Medium] [STUB] Async state machine: plan computed, never consumed; multi-await `Pending` still panics/hangs

- File: `crates/glyim-lower/src/async_state_transform.rs:168-185`; `crates/glyim-lower/src/lower.rs:173,179`
- Code:
```rust
pub fn transform_async_body(body: &Body) -> AsyncTransformPlan {
    // ... The complete MIR emission of the `match self.state { .. }` dispatch ...
    // is implemented in the pipeline wiring (M3) and cannot be runtime-verified
    plan_async_transform(body)
}
```
- Problem: `LowerResult::async_transform` is stored but **no code anywhere reads it** (grep: only lower.rs and async_state_transform.rs). The HIR desugar deliberately emits `Pending => panic!/loop {}` for multi-await bodies (glyim-hir/src/lower/lower_async.rs:85-112), so any async fn whose future actually returns `Pending` panics or hangs at runtime; the analysis half is dead weight. Also `split_at_suspend_points` treats *every* `Call` terminator as a suspend candidate (async_state_transform.rs:76-97), so the plan is wrong even as a plan.
- Fix: Either wire it (filter suspend sites by callee == `Future::poll`, then emit the state enum dispatch per `plan_resume_arm`) or delete `async_transform` from `LowerResult` and the module's callers, keeping the HIR-level single-await desugar; document multi-await as unsupported with a hard typeck error instead of a runtime panic.
- Verify: `cargo test -p glyim-pipeline -- async_multi_await` (currently passes only because the fixture never truly pends); add a fixture whose awaited future pends once and assert a clean diagnostic instead of a panic.

---

### T101 [LOW-10] [Medium] [STUB] Polymorphization dedup is never wired — and would be a Critical miscompile if wired as-is

- File: `crates/glyim-lower/src/mono.rs:447-459`; `crates/glyim-pipeline/src/mono_cache.rs:52-58`; pipeline call site lib.rs:591
- Code:
```rust
#[allow(dead_code)]
pub(crate) fn build_mono_cache(...) -> PipelineMonoCache {
    ctx.polymorphize_and_deduplicate(ty_ctx);   // never called anywhere
    PipelineMonoCache::from_items(ctx.items())
}
// pipeline/lib.rs:591 (production):
let cache = PipelineMonoCache::from_items(&mono_items);
```
- Problem: `build_mono_cache` is dead (`#[allow(dead_code)]`, zero callers), so polymorphization never runs. Worse, if someone wires it: `MonoItemData::body` is the **already-substituted** body (`make_mir_body_provider` → `substitute_body`, mono_cache.rs:381-388), so `analyze_used_params` finds no `TyKind::Param` → `used` all-false → `polymorphize_substs` rewrites every subst arg to `()` and `deduplicate` merges `f::<i32>` with `f::<String>` keeping the first body — wrong code for the second instantiation.
- Fix: Run `analyze_used_params` on the **pre-substitution** body (fetch from `mir_bodies_map` like `post_mono_checks::check_unused_generic_params` does, passing `pre_mono_bodies` into `MonoCtx`), then wire `build_mono_cache` into the pipeline; until then delete or `#[cfg(test)]`-gate `deduplicate` so it cannot be switched on unsafely.
- Verify: `cargo test -p glyim-lower polymorphize`; after wiring, `target/debug/glyim-cli generic.g --emit=exec` with two distinct instantiations of one generic must still print distinct results.

---

### T102 [LOW-11] [Medium] [MISCOMPILE] `substitute_body` does not substitute `AggregateKind::Array(Ty)` / `Adt` / `Closure` substs — `Param` survives monomorphization

- File: `crates/glyim-pipeline/src/mono_cache.rs:327-339`
- Code:
```rust
match rvalue {
    Rvalue::Cast(_, _, target_ty) => { *target_ty = substitute_ty(...); }
    Rvalue::Repeat(_, const_val) => { const_val.ty = substitute_ty(...); }
    _ => {}          // Aggregate(Adt(_,_,substs) | Array(ty) | Closure(_,substs)) untouched
}
```
- Problem: A generic fn constructing `[T; 2]` or a generic ADT aggregate (`Vec<T>`-shaped, `Some(x)` in `fn f<T>`) keeps `Param` inside the aggregate kind after monomorphization. That `post_mono_checks::rvalue_contains_param` explicitly checks exactly these three aggregate kinds (post_mono_checks.rs:264-279) is evidence they occur. Codegen/layout that reads the aggregate kind (rather than the dest local type) lays out with `Param` → `UnknownType` ICE or wrong layout.
- Fix: In the statement loop add:
```rust
Rvalue::Aggregate(kind, _) => match kind {
    AggregateKind::Array(ty) => *ty = substitute_ty(*ty, ...),
    AggregateKind::Adt(_,_,s) => *s = substitute_substitution(*s, ...),
    AggregateKind::Closure(_,s) => *s = substitute_substitution(*s, ...),
    AggregateKind::Tuple => {}
},
```
- Verify: `cargo test -p glyim-pipeline mono_cache` with a generic body containing `Rvalue::Aggregate(AggregateKind::Array(Param(0)), ..)`; assert no `TyKind::Param` remains post-`substitute_body`.

---

### T103 [BCK-1] [Medium] [BUG] Loan liveness = "reference local is live": moving the `&mut` elsewhere (or its last use) kills the loan, enabling unsound writes

- File: `crates/glyim-borrowck/src/lib.rs:404-414` (active-loan computation), `liveness.rs:214-241` (per-stmt kill)
- Code:
```rust
// A loan is active at this point if the local holding the
// reference is live (will be used later).
for local_idx in live_locals.ones() {
    for &loan_idx in &loans_by_dest[local_idx] {
        active_loans.push(&loans[loan_idx]);
    }
}
```
- Problem: `let r = &mut y; let holder = H { r }; y = 5;` — after the move of `r` into `holder`, `r` is dead ⇒ the loan is inactive ⇒ the write to `y` is accepted, while `holder.r` still aliases `y` (the reference's life is now carried by `holder`, which the analysis does not track). This is the NLL-lite hole: no region propagation through moves/copies of the reference.
- Fix (mechanical approximation): Kill loans only at `StorageDead` of `dest_local` (and at function exit), i.e. replace "dest is live" with "dest not StorageDead-ed and not reassigned on any path"; `move_analysis` already computes reassignment (`block_inits`) — feed `Assign(dest_local, …)`/`StorageDead(dest_local)` into loan deactivation instead of liveness.
- Verify: `cargo test -p glyim-borrowck` with a hand-built body: borrow `y` into `_2`, `Move(_2)` into an aggregate, then `Assign(y, ..)` — must be an error (today accepted).

---

### T104 [BCK-2] [Low→Medium] [BUG] Terminator writes are never checked against active loans (`Call::destination`, `Drop`)

- File: `crates/glyim-borrowck/src/visitor.rs:58-73` (`walk_terminator_reads` skips `Drop`, has no notion of writes); lib.rs:427-444
- Code:
```rust
TerminatorKind::Drop { .. } => {
    // Drop is a move-out (kill), not a shared read.
}
```
- Problem: `check_terminator_conflicts` only checks reads of `func`/args/discr. A `Call` whose `destination` is a place that overlaps an active loan (e.g. writing the result into a field of a struct that is shared-borrowed) is accepted, as is a `Drop` of a place that is still borrowed — both are write/kill effects rustc rejects ("cannot assign/drop while borrowed").
- Fix: In `check_terminator_conflicts`, after the read loop add: for `Call { destination, .. }` and `Drop { place, .. }`, run the same write-conflict loop used in `check_stmt_conflicts` case (b) (lib.rs:332-348) against `active_loans`.
- Verify: `cargo test -p glyim-borrowck terminator_conflict_tests` with a body whose `Call` destination is `Place{local: x, projection: [Field(0)]}` while `x` is active-shared-borrowed — must now error.

---

### T105 [OPT-2] [Medium→High] [MISCOMPILE] Drop elaboration treats a *may*-initialized union as "definitely initialized" → unconditional drop of possibly-uninitialized locals

- File: `crates/glyim-opt/src/drop_elaboration.rs:60-75` (union propagation) vs :405-449 (consumed as must)
- Code:
```rust
// propagation is a UNION (any predecessor that saw the local initialized):
for i in 0..num_locals {
    if cur[i] && !succ_entry[i] { succ_entry[i] = true; ... }
}
...
let definitely_init = analysis.is_definitely_initialized(old_bb, local);
if !definitely_init { /* flag-guarded Drop */ } else {
    TerminatorKind::Drop { place: place.clone(), ... }   // unconditional
}
```
- Problem: For `let s: S; if c { s = make(); }` the merge/exit block's entry has `s = true` because *one* predecessor initialized it, so `definitely_init` is true and the scope-exit `Drop(s)` is emitted **unconditionally** — on the `!c` path it drops an uninitialized local (garbage free in codegen; interp error when the flag path is skipped). The flag machinery built for exactly this case is bypassed precisely when it is needed. (Name says "definitely"; the dataflow is "maybe".)
- Fix: Make the entry state a **must**-analysis: initialize every block's entry to `true` for locals initialized at entry (params) and intersect (`cur[i] && succ_entry[i]` stays, otherwise set `false`) over predecessors; blocks with no predecessors other than entry start all-`false`. With intersection, the conditional-init case falls into the flag-guarded branch which is already correct.
- Verify: run-pass fixture `if c { s = make(); }` + exit path (MIR-11 scenario) — `target/debug/glyim-cli t.g --emit=mir` must show a `SwitchInt` on a drop flag guarding the `Drop`; `cargo test -p glyim-opt drop_elaboration` with a two-pred body where only one pred assigns.

---

### T106 [INT-3] [Medium] [BUG] Float division by zero panics in the interpreter but is IEEE ±inf in native codegen

- File: `crates/glyim-mir-interp/src/lib.rs:986-988`
- Code:
```rust
BinOp::Div => {
    if *r == 0.0 {
        return Err(InterpError::Panic("division by zero".into()));
    }
    *l / *r
}
```
- Problem: `1.0 / 0.0` is not a trap in Rust/IEEE semantics and the LLVM backend emits `fdiv` (→ `+inf`). A program computing `x / y` with `y == 0.0` behaves differently under `--emit=exec` interp vs native builds: interp errors out, native prints `inf`. (Integer div-by-zero correctly errors.)
- Fix: Drop the zero check for `(Float, Float)` Div and let `*l / *r` produce the IEEE result (Rust `f64` division already yields inf/NaN); keep `InterpError::DivisionByZero` only for integer ops.
- Verify: `cargo test -p glyim-mir-interp div_rem` with `f64 1.0 / 0.0` expecting `InterpValue::Float(inf)`; compare with the LLVM path on the same fixture.

---

### T107 [INT-4] [Medium] [BUG] `INT_MIN / -1` and `INT_MIN % -1` yield `0` in the interpreter while native `sdiv` traps (UB)

- File: `crates/glyim-mir-interp/src/lib.rs:893-908`
- Code:
```rust
BinOp::Div => {
    // Plan §11.2: signed division must not panic on `MIN / -1`; ...
    if *r == 0 { return Err(InterpError::DivisionByZero); }
    l.checked_div(*r).unwrap_or(0)
}
```
- Problem: `(-2147483648i32) / (-1)` — interpreter returns `0` (documented "language semantics require 0"), while the LLVM backend lowers `BinOp::Div` to `sdiv`, which faults (SIGFPE) on x86 for this operand pair. The same source program has two different observable behaviors depending on backend, and neither is a defined language semantics.
- Fix: Pick one semantics and enforce it in both places — cheapest: treat `MIN / -1` like an overflow panic: in the interpreter return `InterpError::Panic("attempt to divide with overflow")` when `l == i128::MIN && r == -1` (mirroring the `AssertMessage::Overflow` design), and have the LLVM backend emit a compare+branch to the overflow panic path (or `sdiv` guarded by the same check).
- Verify: run-fail fixture `exit(i32::MIN / -1)` executes identically (panic message, exit code) under interp and `--emit=exec` native; `cargo test -p glyim-mir-interp div_rem` updated accordingly.

---

### T108 [INT-5] [Medium] [BUG] `Call { target: None }` resumes at `bb_idx + 1` — arbitrary block / out-of-bounds index

- File: `crates/glyim-mir-interp/src/lib.rs:458-459`
- Code:
```rust
let next_bb = target
    .unwrap_or_else(|| BasicBlockIdx::from_raw((bb_idx.index() + 1) as u32));
```
- Problem: A diverging call (`panic()`, `abort`, any `Call` lowered with `target: None` — lowering never produces those today, but hand-built and future panic-path MIR does, cf. T099) stores `bb+1` as the frame's resume point. If `bb` is the last block, the callee's `Return` executes `body.basic_blocks[bb_idx]` with an out-of-range index → Rust index panic (process-level ICE, not an `InterpError`); if `bb+1` exists, execution silently resumes in an unrelated block after a call that was specified to diverge.
- Fix: For `target: None`, treat the call as diverging: do not push a normal continuation — set `frame.target_bb`/`unwind_target` to `None` and, when the callee returns, raise `InterpError::Panic("diverging call returned")` (or route to `unwind_step`).
- Verify: `cargo test -p glyim-mir-interp calls` with a body whose last block ends in `Call{target: None}` — today panics with an index-out-of-bounds Rust panic; after: clean `InterpError`.

---

### T109 [INT-6] [Medium] [PERF] Interpreter clones the terminator, every statement, and every projected value on each step

- File: `crates/glyim-mir-interp/src/lib.rs:293, 311, 1062-1091, 1189`
- Code:
```rust
let terminator_kind = body.basic_blocks[bb_idx].terminator.kind.clone();  // per block-step
let stmt = body.basic_blocks[bb_idx].statements[stmt_idx].clone();        // per statement
val = InterpValue::Aggregate(fields[1..].to_vec());                        // per Downcast
```
- Problem: The hot loop allocates per step: `SwitchTargets` (boxed slice) and `Vec<Operand>` are cloned for every block executed; every statement is cloned before execution; `read_place` clones the value at each projection step (aggregate fields deep-cloned per `Field`/`Index`). For the step-limit-bounded interp this dominates runtime on loop-heavy fixtures.
- Fix: Borrow instead of clone where possible: execute `&body.basic_blocks[bb].statements[i]` under a short-lived borrow (split `execute_statement(&Statement)` already takes a reference — clone only because of the `&mut self` overlap; restructure to index-based access); make `read_place` return `Cow`/take a sink callback for the final projection only; store `Aggregates` as `Rc<Vec<InterpValue>>` with copy-on-write at `write_through_projections`.
- Verify: `cargo test -p glyim-mir-interp` (all green) + a benchmark fixture (`while` loop 1e6 iterations) run-time before/after; step limit unchanged.

---

### T110 [LOW-12] [Medium→Low] [STUB] `MonoItem::Static` gets a dummy body — statics are never scanned/instantiated

- File: `crates/glyim-lower/src/mono.rs:144-147`
- Code:
```rust
MonoItem::Static { .. } => Arc::new(glyim_mir::Body::dummy(DefId::new(
    CrateId::from_raw(0), LocalDefId::from_raw(0),
))),
```
- Problem: Static items enter the mono set but their (never-lowered) bodies contribute nothing; functions called by a static initializer are never enqueued via the static path, and `discover_mono_roots` only picks `#[used]` statics. Any static-with-initializer support is a stub.
- Fix: Lower the static's initializer THIR body into MIR (reuse `lower_body` via the pipeline's lowered-body map) and feed it through `scan_body_for_refs` like `MonoItem::Fn`; or explicitly diagnose `#[used]` statics as unsupported.
- Verify: `cargo test -p glyim-lower mono_collect` with a `MonoItem::Static` asserting `body.owner` equals the static's DefId (today it is `LocalDefId(0)`).

---

### T111 [LOW-14] [Medium] [STUB] Two-phase-borrow machinery is dead code (no producer) and its activation scan ignores terminator reads

- File: `crates/glyim-borrowck/src/twophase.rs:44-69`; producers grep
- Code:
```rust
// twophase.rs:56 — activation detected ONLY in Assign rvalues:
if let StatementKind::Assign(_, rvalue) = &stmt.kind {
    walk_rvalue_reads(rvalue, &mut checker);
}
```
- Problem: No production lowering ever emits `BorrowKind::Mut { allow_two_phase_borrow: true }` (grep: only tests and borrowck itself set it; `lower_rvalue.rs:314-316` and the for-loop borrow at :602-604 hard-code `false`), so `ReservationAnalysis` never runs on real code. Additionally, if it ever runs, activation missed at a `Call` argument (the canonical two-phase use) — `walk_terminator_reads` is never consulted by the transfer, so the reservation survives past the activating call.
- Fix: Either remove the two-phase path until lowering emits it, or (a) make `ExprKind::Ref` used directly as a call argument emit `allow_two_phase_borrow: true`, and (b) extend `transfer` to also scan the block terminator's operand reads (`walk_terminator_reads`) for `dest_local`.
- Verify: `cargo test -p glyim-borrowck two_phase` after adding a lowering test where a two-phase ref is passed as a call arg — `is_reservation` must be false at the call point.

---

## 3.5 LLVM backend, layout, runtime

### T112 [LL-17] [Medium→High] [MISCOMPILE] `PassMode::Ignore` (ZST/Unit) params misalign MIR args → LLVM params (conflicts with tracker's LL-9 "fixed" note)

- File: `crates/glyim-codegen-llvm/src/lower.rs:3558-3562` (call loop), `4272-4311` (prologue); `crates/glyim-lower/src/lower_rvalue.rs:345-348`; `crates/glyim-lower/src/builder.rs:95,148`
- Code:
```rust
for arg_abi in &fn_abi.args {
    if matches!(arg_abi.mode, PassMode::Ignore) {
        continue;               // ← skips the `arg_idx += 1` at loop bottom
    }
    ...
    let arg_val = ... self.lower_operand(&args[arg_idx])? ...;
    arg_idx += 1;
}
```
- Problem: MIR keeps **all** THIR args (`for arg in args { mir_args.push(...) }`, lower_rvalue.rs:345-348) and `arg_count = thir.params.len()` (builder.rs:95), while the FnAbi drops size-0 args (`classify_arg`: `size == 0 → Ignore`) and `llvm_fn_type_from_sig_inner` skips them. For `fn f(u: (), x: i32)`, the call loop feeds the **unit** operand to the `i32` param and never consumes `5`; the prologue (`param_idx = i - 1`) stores `params[0]` (the i32) into `u`'s **null** ZST slot (`store … ptr null`) and leaves `x` uninitialized. audit-status says LL-9 was behaviorally verified — this contradicts the code as written; the verification presumably masked it (e.g. ZST-last ordering or no call). Reporting because the misalignment is mechanically present for ZST-first params.
- Fix: In `lower_call`, track MIR-arg consumption separately:
```rust
// use two indices: `abi_idx` over `fn_abi.args` and `mir_idx` over `args`,
// advancing BOTH per ABI arg even when the ABI arg is skipped.
```
In the prologue, iterate `fn_abi.args` (skipping `Ignore`) instead of `1..=arg_count` to map LLVM params to MIR locals.
- Verify: `target/debug/glyim-cli zst.g --emit=llvm-ir -o z.ll` with `fn f(u: (), x: i32) -> i32 { x } fn main() -> i32 { f((), 42) }`; grep `z.ll` for `store.*ptr null` (prologue) and check the call's argument order; `--emit=exec` should return 42.

---

### T113 [LL-18] [Medium] [MISCOMPILE] Oversized shifts emit unmasked LLVM `shl/lshr/ashr` (poison) while VM/interpreter wrap

- File: `crates/glyim-codegen-llvm/src/lower.rs:2224-2258`; contrast `crates/glyim-bytecode-vm/src/lib.rs:637-638`
- Code:
```rust
BinOp::Shl => { ... self.builder.build_left_shift(l.into_int_value(), r.into_int_value(), "shl") ... }
BinOp::Shr => { ... build_right_shift(l, r, signed, "shr") ... }   // no masking of the shift amount
```
- Problem: The language defines over-shifts as wrapping (HIR-27; MIR interp `trunc_to_ty`; VM `wrapping_shl`). LLVM `shl`/`lshr`/`ashr` with shift ≥ bitwidth is **poison**: `1i32 << 32` is `1` on the VM/interpreter, UB on LLVM (x86 masks to 5 bits by accident; AArch64 yields 0) — silent cross-backend divergence.
- Fix: Mask the shift amount to the operand width before shifting:
```rust
let bits = l.get_type().get_bit_width();
let amt = builder.build_int_and(r, r.get_type().const_int((bits - 1) as u64, false), "shift_mask");
// then use `amt` in the shift
```
(if the language wants the Rust-panic semantics instead, emit a check + `glyim_panic`).
- Verify: `--emit=llvm-ir` for `fn main() -> i32 { 1i32 << 33 }` — IR contains `shl i32 %x, %y` without `and …, 31` before fix; after fix the `and` mask appears; run the same program under `--backend=bytecode` and compare.

---

### T114 [LL-19] [Medium] [STUB] `MirConstKind::ConstRef` lowers to a zero-initialized global (LLVM) / raw DefId (bytecode) — named constants read as 0

- File: `crates/glyim-codegen-llvm/src/lower.rs:370-397`; `crates/glyim-codegen/src/lib.rs:1011-1015`
- Code:
```rust
MirConstKind::ConstRef(const_def_id, _substs) => {
    let global = module.add_global(llvm_ty, ...);
    global.set_initializer(&llvm_ty.const_zero());   // ← value is ALWAYS 0
```
```rust
MirConstKind::ConstRef(def_id, _) => { ... bc.extend_from_slice(&(def_id.to_raw() as i64).to_le_bytes()); } // value = DefId
```
- Problem: If any named `const` survives const-eval folding into MIR, the LLVM backend silently substitutes **0** (the global is never written with the real value), and the bytecode backend substitutes the constant's **DefId number**. Any non-folded const context (large aggregate consts, consts referenced through paths const-eval doesn't fold) miscompiles silently instead of erroring.
- Fix: Either (a) guarantee `ConstRef` never reaches codegen and turn both arms into ICE diagnostics, or (b) evaluate the const at codegen time via `glyim-const-eval` and emit `MirConstKind::Int/Uint/Aggregate` equivalents; for LLVM, initialize the global from the evaluated value instead of `const_zero()`.
- Verify: `--emit=llvm-ir` for `const N: i64 = 42; fn main() -> i64 { N }` (and a `const ARR: [i64; 3]` variant); check whether the returned value is `i64 42` or `i64 0` / a `__glyim_const_*` load.

---

### T115 [LL-20] [Medium] [BUG] Indirect (fn-pointer) calls use the C convention while Glyim-ABI callees are compiled `fastcc`

- File: `crates/glyim-codegen-llvm/src/lower.rs:3713-3731` vs `4113-4121`
- Code:
```rust
// callee definitions: Abi::Glyim => 8u32 (Fast)
function.set_call_conventions(cc);
// but the indirect call site never sets a convention (LLVM default = ccc/0):
self.builder.build_indirect_call(fn_type, func_val, &metadata_args, "call")
```
- Problem: Direct calls inherit the callee's convention (LLVM `CallInst::Create`), but `build_indirect_call`/`build_call` on fn-pointer values default to `ccc`. Calling a `fastcc` function through a `ccc` site is exactly the mismatch the file itself documents as broken on AArch64 ("calling a fastcc fn from a ccc site silently drops the return value") — fixed only for `main`, not for fn-pointer calls (used by the SEH test path, callbacks, etc.).
- Fix: Set the convention on the call site from the sig:
```rust
call.set_call_convention(if fn_sig.abi == Abi::Glyim { 8 }
    else if target.is_windows() { 64 } else { 0 });
```
(inkwell `CallSiteValue::set_call_convention`).
- Verify: `--emit=llvm-ir` for a program calling a glyim function through a `let f: fn(i32) -> i32 = g;` value; the IR call site shows `cc` absent (ccc) while `declare fastcc … @__glyim_fn_N` — mismatch visible in the IR diff before fix.

---

### T116 [LAY-1] [Medium] [BUG] `SimpleLayoutComputer::fn_abi_of` silently drops any argument whose layout fails — position shift instead of an error

- File: `crates/glyim-layout/src/lib.rs:890-905`
- Code:
```rust
let arg_abis: Vec<ArgAbi> = args.iter().filter_map(|arg| {
    if let GenericArg::Ty(t) = arg {
        let layout = self.layout_of(*t).ok()?;   // ← Err swallowed → arg removed
        ...
        Some(ArgAbi { ty: *t, layout, mode })
    } else { None }
}).collect();
```
- Problem: `fn_abi_of` returns `Result<FnAbi, LayoutError>`, but a param whose `layout_of` fails (e.g. a bare `Param`/`Slice`/`Infer` reaching the simple computer) is silently **filtered out** of `FnAbi.args`, shifting every later argument's ABI position for consumers of the simple computer (bytecode backend's provider paths, debug info). `FullLayoutComputer::fn_abi_of` does this correctly with `?`; the simple one diverges.
- Fix: Replace `filter_map` with a `for` loop using `?`:
```rust
let layout = self.layout_of(*t)?;   // inside the GenericArg::Ty arm; propagate the error
```
- Verify: unit test: build a `FnSig` with inputs `[Ty::ERROR-or-Param, I32]` and assert `fn_abi_of` returns `Err`, not `Ok(FnAbi { args: len 1 })`.

---

### T117 [LAY-2] [Medium] [MISCOMPILE] Signed ints are treated as niche-bearing — `0x80` for `i8` is a *representable* value

- File: `crates/glyim-layout/src/lib.rs:600-609`
- Code:
```rust
TyKind::Int(int_ty) => {
    let bw = int_ty.bit_width(&self.target);
    match bw {
        8 => Some((0x80, 1)),       // i8 CAN hold -128 (0x80)!
        16 => Some((0x8000, 1)),
        32 => Some((0x8000_0000, 1)),
        64 => Some((0x8000_0000_0000_0000, 1)),
```
- Problem: A niche is a bit pattern the type *cannot* represent. Plain `i8..i64` have no invalid values, so any niche-encoded enum over an `Int` field (SimpleLayoutComputer path — used by the bytecode `LayoutProvider`, debug.rs, and MIR-side consumers) reuses `i8::MIN` etc. as the discriminant: constructing `Variant(i8::MIN)` stores `0x80`, which `lower_discriminant`-style reads decode as the *other* variant. Only `Bool` (2..255), `Char`, and null-able refs/ptrs are genuine niches.
- Fix: Delete the `TyKind::Int` arm (or restrict it to wrapping newtype-like guarantees the language doesn't have); keep `Bool`/`Char`/`Ref`/`RawPtr`.
- Verify: unit test on `SimpleLayoutComputer`: `niche_info(i8_ty)` must be `None` after the fix (today it's `Some((0x80,1))`).

---

### T118 [LAY-3] [Medium] [BUG] Niche encoding leaves variants *after* the niche-holding variant unrepresentable (old RT-20, still present)

- File: `crates/glyim-layout/src/lib.rs:531-535` (build_niche_layout), `499-517` (try_niche_encoding)
- Code:
```rust
let niche_variants = if niche_variant_idx == 0 {
    1..=u32::try_from(variant_count - 1)...
} else {
    0..=u32::try_from(niche_variant_idx - 1).unwrap_or(0)   // variants > holder get nothing
};
```
- Problem: The niche-capacity check passes for middle-holder enums with sufficient capacity (e.g. `enum E { A, B(char), C, D }` with char's 0x110000 free values ≥ 3: `niche_variants = 0..=0` → **C and D have no encoding**; constructing them writes `niche_start + (vidx - start)` with `vidx > end`, which the read path (`in_range` check, codegen-llvm lower.rs:1634-1707) maps back to `untagged_variant`. Old RT-20, confirmed still present.
- Fix: Either reject niche encoding unless the holder is the first or last variant, or assign niche ordinals to *all* non-holder variants (in index order, skipping the holder) and make the range cover them; assert `niche_variants.len() == variant_count - 1` in `build_niche_layout`.
- Verify: unit test: lay out `enum E { A, B(char), C, D }` and assert every `variant_idx ∈ 0..4` round-trips through the encoding (write→read discriminant).

---

### T119 [VM-1] [Medium] [BUG] `set_mem` resizes memory to an unvalidated address (old RT-8, still present)

- File: `crates/glyim-bytecode-vm/src/lib.rs:685-692`
- Code:
```rust
fn set_mem(&mut self, addr: usize, v: Value) -> ExecResult<()> {
    let frame = ...;
    if addr >= frame.mem.len() { frame.mem.resize(addr + 1, Value::Int(0)); }
```
- Problem: `addr` comes from `base + offset` arithmetic on program-controlled values (index scaling, field offsets, T042's mismatches). A wild address (e.g. `a[bad_index]` pre-bounds-check, or a wrong offset) resizes the frame to `addr+1` slots — an attacker/bug-controlled host allocation (OOM) instead of a clean trap. Old RT-8, still present.
- Fix: Reject rather than grow:
```rust
if addr >= frame.mem.len() { return Err(VmError::LocalOutOfBounds(addr)); }
```
(the ZST-index bounds check already gates legitimate writes; legitimate aggregate unpacking fits within `n_locals*8`).
- Verify: `cargo test -p glyim-bytecode-vm` — hand-assembled `LoadLocalAddr 0; LoadConst 1<<40; Add; LoadConst 1; StoreField` expects `Err(LocalOutOfBounds)`, not growth.

---

### T120 [RT-35] [Medium] [BUG] `glyim_process_wait`/`wait_output` hold the registry mutex across the blocking wait (old RT-25, still present)

- File: `crates/glyim-runtime/src/lib.rs:709-731, 753-798`
- Code:
```rust
let mut registry = process_registry().lock().expect("process registry lock poisoned");
if let Some(mut child) = registry.children.remove(&handle) {
    match child.wait() { ... }     // blocks for the child's whole lifetime, lock held
```
- Problem: Any concurrent `spawn`/`wait`/`kill` from another glyim thread blocks until the waited child exits (`kill` can't run → deadlock in the documented kill-during-wait scenario). `wait_output` holds it across `wait_with_output` (which can block on full pipes too — see T121).
- Fix: Remove the handle from the map under the lock, `drop(registry)`, then `child.wait()` outside the critical section (the code already `remove`s, so just drop the guard first — note `wait_output` destructures `child` after removal, so only the guard needs dropping before the blocking call).
- Verify: spawn a `sleep 5` child from thread A; thread B `glyim_process_spawn` returns instantly (today: blocks 5 s); `cargo test -p glyim-runtime` with a two-thread wait/spawn test.

---

### T121 [RT-37] [Medium] [BUG] Children always get piped stdout/stderr but plain `wait()` never drains — pipe-full deadlock (old RT-26, still present)

- File: `crates/glyim-runtime/src/lib.rs:674-675` (spawn) vs `709-731` (`wait`)
- Code:
```rust
command.stdout(Stdio::piped());
command.stderr(Stdio::piped());
...
pub unsafe extern "C" fn glyim_process_wait(handle: usize, out_exit_code: *mut i32) -> i32 {
    ... match child.wait() { ... }   // never reads the pipes
```
- Problem: A child that writes > ~64 KiB to stdout fills the pipe and blocks; the parent blocks in `wait()` → classic deadlock for `proc.run()`-style usage that doesn't call `wait_output`.
- Fix: Only pipe when `wait_output` will be used — add a `pipe_output: bool` parameter to `glyim_process_spawn` (default `Stdio::null()` for `wait()`-only usage), or spawn a drain thread per pipe inside `glyim_process_spawn` that is joined by `wait`/`wait_output`.
- Verify: spawn `sh -c 'yes | head -c 1M'` and call `wait()` — must return (today: hangs); assert in a runtime test with a 5 s timeout.

---

### T122 [RT-36] [Medium] [BUG] `glyim_time_now_nanos` returns only sub-second nanos — resets every second (old RT-28, still present)

- File: `crates/glyim-runtime/src/lib.rs:1633-1638`
- Code:
```rust
pub unsafe extern "C" fn glyim_time_now_nanos() -> u64 {
    monotonic_base().elapsed().subsec_nanos() as u64
}
```
- Problem: The value is non-monotonic (saw-tooths 0..10⁹ each second), so `Instant`-style math in .g code (timeouts, elapsed measurements via `secs*1e9+nanos`) is wrong; `glyim_time_now` (line 1645) already implements the correct combined value.
- Fix: `monotonic_base().elapsed().as_nanos() as u64` (or reuse `glyim_time_now`'s body).
- Verify: `cargo test -p glyim-runtime` — call the FFI twice 100 ms apart after a 1.5 s sleep; assert `t2 > t1` and `t2 - t1 ≈ 10^8` (today t2 < t1 across a second boundary).

---

### T123 [LL-22] [Medium] [PERF] No memoization of layouts/LLVM types — `FullLayoutComputer` + `layout_of` recomputed per operand/projection/alloca

- File: `crates/glyim-codegen-llvm/src/types.rs:90-100`; `crates/glyim-codegen-llvm/src/lower.rs:496-497, 855-860, 237, 110-121`
- Code:
```rust
// per Field projection:
let layout_computer = FullLayoutComputer::new(self.ty_ctx, self.target_info.clone());
let field_offset_bytes = if let Ok(layout) = layout_computer.layout_of(current_ty) { ... }
// per ADT type query:
TyKind::Adt(_adt_id, _subst) => {
    let layout_computer = FullLayoutComputer::new(ctx, target_info.clone());
    if let Ok(layout) = layout_computer.layout_of(ty) { ... opaque_sized_type(...) }
```
- Problem: Every `lower_operand`, `place_ptr` projection, `alloc_local`, and `llvm_type_for_ty(Adt/Tuple/Closure/Opaque)` reconstructs a layout computer (cloning `TargetInfo`) and recomputes the full recursive layout with zero caching — O(type-graph) work per statement, quadratic overall on large bodies (task-listed hot spot).
- Fix: Add a `RefCell<HashMap<Ty, BasicTypeEnum>>`/layout cache on `LoweringCtx` (types are interned handles, so `Ty` is a valid key), or compute one `FullLayoutComputer` per `lower_body` and pass `&` down; memoize `layout_of` inside `FullLayoutComputer` with a `RefCell<HashMap<Ty, Layout>>`.
- Verify: `cargo build --release` then compile a large generated body (e.g. the stdlib pre-compile) with `--emit=llvm-ir` and compare wall time / `perf record` before vs after (should show `layout_of` frames disappearing).

---

## 3.6 Pipeline, CLI, LSP, glyip

### T124 [PIPE-4] [Medium] [STUB] `--codegen-units` / parallel CGU codegen is a no-op: every mono item has `source_module: 0`

- File: `crates/glyim-lower/src/mono.rs:166` (consumed by `crates/glyim-lower/src/partition.rs:29`; used by `crates/glyim-pipeline/src/lib.rs:594-603`)
- Code:
```rust
let id = self.items.push(MonoItemData {
    item: item.clone(),
    body,
    symbol,
    source_module: 0,   // never anything else
});
```
- Problem: `partition()` groups strictly by `item.source_module`; since `MonoCtx::collect` hardcodes 0, there is exactly one group regardless of `max_cgus` (which defaults to `available_parallelism().clamp(1,16)`), and `generate()` lowers all bodies sequentially into one module. The CLI help (glyim-cli/src/lib.rs:46-49) advertises "partition … for parallel code generation" — nothing is parallel and the flag has zero effect.
- Fix (2 steps):
  1. In `mono.rs`, set `source_module` from the item's defining module (`def_map` module of `def_id`; keep 0 for DropGlue).
  2. In `LlvmBackend::generate`, lower one module per CGU and either run `run_passes_on_module` per module on threads or feed the per-CGU modules to `run_lto(primary, secondaries, Fat, …)` (they are already collected).
- Verify: `target/debug/glyim-cli big.g --emit=obj --codegen-units=4 -o b.o && llvm-objdump -h b.o` before/after — today symbol count/inlining identical for `--codegen-units=1` vs `16`.

---

### T125 [PIPE-5] [Low→Medium] [BUG] mod_loader resolves `mod` inside an *inline* module against the file's directory, not the module path

- File: `crates/glyim-pipeline/src/mod_loader.rs:92,101` (and 268-278)
- Code:
```rust
let dir = source_path.parent().unwrap_or_else(|| Path::new("."));
...
match resolve_module_file(dir, &m.name) {
```
- Problem: The doc header promises "Rust's `dir/foo/mod.rs` convention". That holds only for file-backed modules. For an inline module in the parent text (`mod a { mod b; }` in `dir/parent.g`), `b.g` is looked up in `dir/` — Rust (and the documented convention) requires `dir/a/b.g`. Multi-file crates using nested inline modules silently load the wrong (or no) file.
- Fix: track a path suffix stack in `LoadCtx`: push `m.name` before recursing into that module's directory only for file-loaded modules; for inline modules push the name too and resolve `dir/<suffix…>/<name>.g`.
- Verify: `mkdir -p /tmp/ml/a && printf 'mod a { mod b; }\nfn main(){}\n' > /tmp/ml/main.g && printf 'pub fn x() {}\n' > /tmp/ml/a/b.g && glyim-cli /tmp/ml/main.g --emit=mir` — today: "cannot find module file for `mod b;` (looked for `/tmp/ml/b.g`…)".

---

### T126 [CLI-2] [Medium] [STUB] `--lto fat` is accepted but is bit-for-bit equivalent to `--lto off`

- File: `crates/glyim-codegen-llvm/src/lib.rs:330-346` (with passes.rs:36-51)
- Code:
```rust
// run_passes_on_module:
crate::passes::run_lto(
    module,
    &[],          // no secondary modules, ever
    self.lto, ...
```
- Problem: The CLI help says fat = "in-compiler module merge + optimize". Since `generate()` lowers everything into a single module and `run_lto` always receives an empty secondary list, the code comment itself concedes "Fat with no secondary modules is just a single-module pass run" — identical to `None`. Combined with T124 (CGUs never split) there is never a second module to merge, so the advertised fat-LTO cannot do anything.
- Fix: After implementing T124's per-CGU modules, pass the extra CGU modules into `run_lto(module, &secondaries, LtoKind::Fat, …)`; until then, reject `--lto fat` with the same explicit error used for `thin`+bytecode (glyim-cli/src/lib.rs:474-480 style) instead of silently no-op'ing.
- Verify: `glyim-cli x.g --emit=obj --lto=fat -o fat.o && glyim-cli x.g --emit=obj -o off.o && cmp fat.o off.o` — currently identical bytes.

---

### T127 [CLI-3] [Medium] [BUG] `--emit=mir|llvm-ir|asm` build the proc-macro registry and then throw it away

- File: `crates/glyim-cli/src/lib.rs:368-394` (with `crates/glyim-pipeline/src/lib.rs:1035,1069`)
- Code:
```rust
    // proc_registry built at line 368-385 ...
    if emit == EmitKind::Mir {
        return glyim_pipeline::emit_mir(&mut db, input, &object_path);
    } else if emit == EmitKind::LlvmIr {
        return glyim_pipeline::emit_llvm_ir(&mut db, input, &object_path);
```
```rust
// emit_llvm_ir:
let prepared = Pipeline::prepare_compilation(db, input, None, None)?;
```
- Problem: `emit_llvm_ir`/`emit_asm` hardcode `proc_registry = None`, so with `--proc-macro-deps foo.g --emit=llvm-ir` the costly two-stage build runs (cdylib compile + dlopen) and the produced IR is generated *without* proc-macro expansion — unresolved macro names, IR that doesn't match `--emit=obj` for the same input.
- Fix: Thread `proc_registry: Option<&Registry>` through `emit_llvm_ir`/`emit_asm`/`emit_mir` and pass `proc_registry.as_ref()` at the three call sites in `run_with_args`.
- Verify: proc-macro crate deriving a `fn generated_main()`; `glyim-cli app.g --proc-macro-deps pm.g --emit=llvm-ir -o a.ll` — before fix the generated fn is absent from `a.ll`.

---

### T128 [CLI-4] [Medium] [BUG] Incremental cache key omits proc-macro dependency inputs (and LTO kind) → stale objects

- File: `crates/glyim-cli/src/lib.rs:438-444`; key builder at `crates/glyim-db/src/cache.rs:102-147`
- Code:
```rust
let key = glyim_pipeline::Pipeline::compute_cache_key(
    &mut db, input, &target_triple, args.opt_level,
    matches!(emit, EmitKind::Exec) && entry_main.is_some(),
);
```
- Problem: The key covers source+target+opt+entry only. With `--proc-macro-deps pm.g`, changing `pm.g` (but not the main source) leaves the key unchanged and `CompileCache::lookup` serves the object produced by the *old* macro expansion. `lto` is also absent from the key (dormant today because fat==off per T126, but the moment fat becomes real the cache serves cross-config objects).
- Fix: In `run_with_args`, hash each proc-macro dep file's bytes (and the `lto` byte) into the key:
```rust
// extend Pipeline::compute_cache_key with an `extra: &[&[u8]]` param:
h.update(b"\x1fpm\x1f"); h.update(pm_bytes); h.update([lto as u8]);
```
- Verify: two builds with an edited proc-macro dep between them; before fix the second prints "cache hit — skipping compilation" (wrong), after fix recompiles.

---

### T129 [LSP-5] [Medium] [BUG] rename / goto-def / references are name-keyed with zero scope resolution

- File: `crates/glyim-lsp/src/rename.rs:86-127`, `goto_definition.rs:27-29`, `reference_graph.rs:784-789`, `symbol_index.rs:342-347`
- Code:
```rust
let references = ref_graph.find_references(symbol_name);   // global by NAME
...
let symbols = symbol_index.lookup_by_name(symbol_name);
let symbol = symbols.first()?;                             // arbitrary target
```
- Problem: Every `Reference` is stored with `def_id: None` (reference_graph.rs:110) and keyed purely by identifier text. Renaming a local `x` in `fn a` renames `x` locals/fields in `fn b` and in every other open file that shares the name; goto-definition on a use jumps to `symbols.first()` — an arbitrary same-named symbol from any file (locals are especially broken since their indexed spans are `Span::DUMMY`, symbol_index.rs:300).
- Fix: Minimum viable: in `rename_symbol` filter refs to the enclosing function body of the cursor (compute the containing body by span containment of the definition ref); better: resolve `Expr::Path` → binding via the def-map/typeck scopes already available in `AnalysisDatabase.hirs`/`typeck` and key refs by `(owner_def_id, name)`.
- Verify: two files each with `fn f() { let t = 1; use(t); }`; rename one `t` → after fix only that file's `t` changes (today both files' edits appear in `WorkspaceEdit.changes`).

---

### T130 [LSP-6] [Medium] [STUB] DependencyGraph is never populated and dependent files are never re-analyzed

- File: `crates/glyim-lsp/src/driver.rs:148-155` (with dep_graph.rs, fully wired but starved)
- Code:
```rust
fn extract_dependencies(
    &self,
    _path: &PathBuf,
    _hir: &CrateHir,
    _interner: &Interner,
) {
    // Placeholder for dependency extraction
}
```
- Problem: `analyze_file` calls `clear_deps` + `extract_dependencies` (no-op) and only re-analyzes the changed file. `affected_files()` (dep_graph.rs:59-72) has no caller. Editing a function signature in `a.g` leaves hover/completions/references in `b.g` stale until `b.g` itself is edited. (The empty placeholder is also what keeps T050's quick-fixes unreachable for cross-file items.)
- Fix: Implement `extract_dependencies` by walking the HIR for `Expr::Path`/`TypeRef::Path` names and matching them against `SymbolIndex.import_paths`/definitions of other files:
```rust
for (other, name) in … { dep_graph.write().add_dep(path.clone(), other); }
```
then in `analyze_file` enqueue `affected_files(&path)` for re-analysis.
- Verify: open two files where b.g calls a fn in a.g; change a.g's fn name; assert b.g's diagnostics/references update (today they don't until b.g is touched).

---

### T131 [LSP-7] [Medium] [BUG] One missing SourceMap aborts the entire references / workspace-symbols response

- File: `crates/glyim-lsp/src/navigation.rs:109-118` (and 214-224)
- Code:
```rust
for r in references {
    let sm = source_maps.get(&r.file_id)?;      // inside the loop
    ...
    let path = file_map.path(r.file_id)?;       // ditto
```
- Problem: `?` inside the collection loop returns `None` for the whole request when a single referenced file was closed (didClose removes `file_map` entries but not stale refs) or not yet analyzed. A symbol with 10 references where one file is closed returns "no references" instead of 9.
- Fix: `continue` on missing entries (collect what's resolvable):
```rust
let (Some(sm), Some(path)) = (source_maps.get(&r.file_id), file_map.path(r.file_id)) else { continue };
```
Apply the same to `workspace_symbols` (lines 215-224).
- Verify: open two files with a shared symbol, close one, request references on the symbol → today `null`; after fix the open file's references.

---

### T132 [LSP-8] [Medium] [BUG] `uri::offset_to_position` emits byte columns — INF-11 regression in the second offset→Position path

- File: `crates/glyim-lsp/src/uri.rs:23-48`
- Code:
```rust
} else {
    col += c.len_utf8();     // bytes, not UTF-16 units
}
```
- Problem: INF-11 converted `SourceMap` to UTF-16 columns, but this parallel helper (used by completion.rs:180 for auto-import `additional_text_edits` and code_action.rs:238 for match-arm insertion) still counts `len_utf8`. After any non-ASCII character earlier in the file (e.g. a `é` in a comment or string), the computed LSP `character` is too large, so `use` statements and match arms are inserted at shifted positions.
- Fix: `col += c.len_utf16();` (and count `\r\n` normally) — mirrors `SourceMap::offset_to_line_utf16`.
- Verify: unit test mirroring `multibyte_column_is_utf16` (database.rs:316-326) but for `offset_to_position("aé b\nlet x = 1;", offset_of_let)` — expects UTF-16 col 0 after the newline; today byte col is wrong for offsets after `é` on line 0.

---

### T133 [LSP-10] [Medium] [BUG] Folding ranges count `{`/`}` inside string literals and comments

- File: `crates/glyim-lsp/src/folding.rs:5-29`
- Code:
```rust
for (col, ch) in line.chars().enumerate() {
    if ch == '{' {
        brace_stack.push((line_idx, col));
    } else if ch == '}' && let Some((start_line, start_col)) = brace_stack.pop() {
```
- Problem: `"}"` or `// }` or `/* { */` unbalances the stack: a brace in a string pops the enclosing function's fold early, producing wrong folding regions and orphan ranges; nested `/* /* */ */` also miscounts. Additionally `col` is a char index while `start_character`/`end_character` are UTF-16 (drift after non-ASCII).
- Fix: Reuse the lexer: iterate `glyim_frontend::lex(source, file_id).tokens`, skip trivia, and compute ranges from `{`/`}` token spans via `sm.span_to_position`; or add the same string/comment state machine as T054's fix.
- Verify: folding request on `fn main() { let s = "}"; }` — today 1 bogus/0 miscounted ranges; after fix exactly one range spanning the fn.

---

### T134 [LSP-11] [Medium] [PERF] Full lex+parse+defmap+HIR+typeck per keystroke; overflow silently drops changes

- File: `crates/glyim-lsp/src/driver.rs:83-146`; `handler.rs:172-195` (`try_send`), state channel cap 16 (state.rs:36, server.rs:12)
- Code:
```rust
let _ = tx_open.try_send(AnalysisMessage::FileChanged { path, content, version });
```
- Problem: Every `didChange` triggers a complete re-analysis (lexer + parser + def-map + HIR + typeck) with a fresh `Interner`, no debounce, no coalescing, and no reuse of unchanged work. Worse, the channel is bounded (16) and `try_send` failures are discarded (`let _`), so during a fast typing burst the *latest* change can be dropped — the server then serves hover/completions/diagnostics for stale content with no error, and `version` is ignored end-to-end (`version: _` in driver.rs:70).
- Fix (3 steps):
  1. Coalesce in the driver: after `recv()`, `while let Ok(msg) = self.rx.try_recv() { latest = Some(msg); }` and analyze only the last per path.
  2. On `try_send` error fall back to storing the pending content in `AnalysisDatabase` (e.g. a `pending: Mutex<HashMap<PathBuf,(String,i32)>>` the driver drains), or grow the channel and log.
  3. Skip analysis when `version <= last_analyzed_version` per path.
- Verify: type 30 rapid characters in a large file with tracing on: before fix 30 `Analyzed file` logs and possible stale results; after fix ≤ a few, always matching the final buffer.

---

### T135 [GLYIP-4] [Medium] [BUG] Registry fallback silently locks `versions.first()` when no version satisfies the requirement

- File: `crates/glyip/src/dep.rs:624-630`
- Code:
```rust
let version = if let Some(req) = version_req {
    select_best_version(&entry.versions, Some(req))
        .or_else(|| entry.versions.first().cloned())   // <-- ignores the req
        .ok_or_else(...)?
```
- Problem: The local-index path errors on an unsatisfiable req (`select_best_version → None → DependencyNotFound`, dep.rs:94-99), but the remote path falls back to the first listed version — silently resolving `foo = "0.9"` to `foo 2.0.0`. That version then typically fails `check_version_conflicts` with a misleading "mutually unsatisfiable" error naming the wrong cause, and it depends on index listing order (plan §4.3 says order must not matter).
- Fix: delete the `.or_else(|| entry.versions.first().cloned())` so a non-matching req surfaces `DependencyNotFound { name, version: req }` exactly like the local path.
- Verify: unit test with a mock `RegistryClient` whose index has only `2.0.0`, req `"0.9"` → expect `Err(DependencyNotFound)`; today returns `Ok` locked at 2.0.0.

---

## 3.7 Stdlib, harness, span, diag

### T136 [STD-11] [Medium] [BUG] parse.g: leading `+` rejected for all integer types; f64 exponent accumulator overflows

- File: `crates/glyim-lang-core/lib/parse.g:26-33` (all unsigned), `129-136` (all signed), `339-347`
- Code:
```glyim
if ch < b'0' || ch > b'9' { return Result::Err(ParseIntError); }   // '+' rejected
...
exp = exp * 10 + ((ch - b'0') as i32);   // no overflow check, capped only AFTER accumulation
```
- Problem: Rust's `from_str_radix` accepts a leading `+` for signed *and* unsigned ints; Glyim rejects `"+5"` for every type (f64's parser does accept `+`, parse.g:284). Separately, for `"1e99999999999999999999"` the i32 `exp` wraps during accumulation → can become negative → no scaling applied → returns `Ok(1.0)` instead of `inf`.
- Fix: In each int impl accept `b'+'` when `i == 0` (`if bytes[0] == b'+' { i = 1; }` before the digit loop, keep the `i >= len` guard); in f64, bail to `exp = 400` as soon as `exp > 400` inside the loop.
- Verify: run-pass fixtures `assert_eq!("+5".parse::<i8>().unwrap_or(0), 5)` and `"1e99999999999999999999".parse::<f64>()` is `inf`.

---

### T137 [STD-12] [Medium] [BUG] `parse_ipv6_range` rejects valid IPv6 with empty tail (`::`, `fe80::`, `1::`)

- File: `crates/glyim-lang-std/lib/net.g:599-631` (esp. 627-631)
- Code:
```glyim
} else {                       // tail_start >= end  (nothing after "::")
    if head_count != 8 {
        return Option::None;   // rejects "::" and "1::"/"fe80::"
    }
}
```
- Problem: For inputs ending in `::` the tail is empty; the else-branch demands `head_count == 8`, so the all-zeros address `::` (Rust: valid) and forms like `fe80::` are rejected; only fully-populated 8-segment strings or `::`-with-tail parse.
- Fix: In the `tail_start >= end` branch accept when `head_count + tail_count <= 7` too (there must be room for the `::` gap); keep rejecting only `head_count + tail_count > 8` (already checked at :633).
- Verify: fixture asserting `parse_ip_addr_full("::")` and `parse_ip_addr_full("fe80::")` return `Some`.

---

### T138 [STD-13] [Medium] [BUG] `SystemTime::checked_add/checked_sub` don't normalize nanos

- File: `crates/glyim-lang-std/lib/time.g:177-186`
- Code:
```glyim
fn checked_add(&self, duration: Duration) -> Option<SystemTime> {
    let secs = self.secs.checked_add(duration.secs)?;
    Option::Some(SystemTime { secs, nanos: self.nanos + duration.nanos })   // ≥ 1e9, can wrap u32
}
fn checked_sub(&self, duration: Duration) -> Option<SystemTime> {
    let secs = self.secs.checked_sub(duration.secs)?;
    Option::Some(SystemTime { secs, nanos: self.nanos.saturating_sub(duration.nanos) })  // never borrows a second
}
```
- Problem: `checked_add` can produce `nanos >= 1_000_000_000` (and `u32` addition can wrap) — an invalid `SystemTime` that makes later `diff`/`elapsed` off by whole seconds. `checked_sub` drops the fractional borrow entirely: `2.0s - 0.5s` yields `{2, 0}` instead of `{1, 500_000_000}`.
- Fix: Mirror `Duration::checked_add` (time.g:90-99): carry `nanos / 1e9` into a second `checked_add(1)`; for `sub`, when `self.nanos < duration.nanos` borrow: `secs.checked_sub(1)?`, `nanos + 1e9 - duration.nanos`.
- Verify: unit test `UNIX_EPOCH.from_secs_nanos(2,0).checked_sub(Duration::from_millis(500))` == `{1, 500_000_000}`.

---

### T139 [STD-14] [Medium] [BUG] `write_all` silently succeeds on partial writes/`Ok(0)`; `read_exact` still uses the guard construct its own comment calls mistranslated

- File: `crates/glyim-lang-std/lib/io.g:162-182` (also 217-237, 456-476, 612-632), 22 and 45 and 130 and 510
- Code:
```glyim
match self.write(&buf[written..]) {
    Result::Ok(0) => break,          // partial write reported as success
    Result::Ok(n) => written += n,
    Result::Err(e) => return Result::Err(e),
}
Result::Ok(())                        // even though written < buf.len()
```
- Problem: `Stdout/Stderr/BufWriter/Sink::write_all` break on `Ok(0)` and return `Ok(())` regardless of `written < buf.len()` — silent data loss (Script 678 chose this to dodge guard/codegen bugs). Meanwhile `Read::read_exact`, `read_to_end`, `File::read_to_end`, `io::copy` still use `Result::Err(ref e) if e.kind() == …` match guards, which io.g:165-168 (Script 678 note) says the compiler mistranslates — so the error-retry paths are suspect.
- Fix: Return `Error::new(ErrorKind::WriteZero, …)` on `Ok(0)` and `Err` when `written != buf.len()` (once the guard mistranslation is fixed, or by rewriting the guard as nested `match e.kind()`).
- Verify: run-pass fixture writing to `sink()`-like writer returning `Ok(0)` must now produce an Err.

---

### T140 [STD-15] [Medium] [STUB] Bodyless "compiler intrinsic" functions have no intrinsic lowering anywhere

- File: `crates/glyim-lang-core/lib/str.g:16-18, 31-33, 48-51, 60-63`; `mem.g:18-25, 43-50, 90-102`; `ptr.g:19-26, 40-72`; `panic.g:18-20`; `hint.g:5-14`
- Code:
```glyim
fn trim(&self) -> &str {
    // compiler intrinsic
}
```
- Problem: `str::{trim, contains(&str), chars}`, `Chars::next`, `Lines::next`, `mem::{size_of, align_of, needs_drop, forget}`, `ptr::{read, write, copy, copy_nonoverlapping, drop_in_place, null, null_mut}`, `panic_any`, `MaybeUninit::*`, `NonNull::{new_unchecked, dangling}`, `black_box`, `spin_loop` all have empty bodies. The only intrinsic dispatchers are: the ty_ctx builtin tables (no entries for these names — verified by grep of `crates/glyim-type/src/ty_ctx_mut.rs`), the raw-pointer method table (`is_null/add/sub` — check_expr.rs:2708-2722, different thing), and LLVM's `try_lower_builtin_intrinsic` which handles only `as_bytes|as_ptr|as_mut_ptr|len|is_empty` (codegen-llvm/lower.rs:3241). Nothing lowers these; callers get a unit/garbage return (or a "missing return" style error).
- Fix: Either implement them in Glyim (e.g. `trim`/`contains` via `as_bytes` loops like net.g does) or register them as real builtins in `ty_ctx_mut.rs` + all three backends.
- Verify: `cargo run -p glyim-cli -- run prog.g` where `prog.g` prints `"  x ".trim().len()` — currently garbage/ICE; must print `1`.

---

### T141 [STD-16] [Medium] [BUG] `env::args()/vars()` ignore the returned length — NUL-padded strings

- File: `crates/glyim-lang-std/lib/env.g:94-106, 74-86`; `crates/glyim-runtime/src/lib.rs:514-539`
- Code:
```glyim
let rc = unsafe { glyim_env_vars_get(i, key_buf.as_mut_ptr(), key_buf.len(), ...) };
if rc >= 0 {
    let key = String::from_utf8_lossy(&key_buf).to_string();   // whole 256/4096-byte buffer!
```
- Problem: The runtime copies the key/value *without* a NUL terminator and returns `0`, not a length; env.g converts the entire zero-initialized array, so every key/value is padded to 256/4096 bytes of NULs. (`args_get` additionally has the T010 ABI mismatch.)
- Fix: After the ABI fix, slice to the returned length (`&key_buf[..n]`) or have the runtime return the length.
- Verify: fixture `env::args().len()` / arg equality against a known argv entry.

---

### T142 [STD-17] [Medium] [BUG] No `Drop` for `File`, `TcpStream`, `TcpListener`, `UdpSocket` — fd/handle leaks

- File: `crates/glyim-lang-std/lib/fs.g:12-15, 103-163`; `net.g:141-144, 360-362, 413-416`
- Code:
```glyim
struct File { fd: i32, path: String }   // no impl Drop for File anywhere in fs.g
```
- Problem: Nothing ever calls `glyim_fs_close`/socket close; every opened file/socket leaks its descriptor until process exit (long-running programs exhaust fd limits). Rust's std closes on drop.
- Fix: Add
```glyim
impl Drop for File {
    fn drop(&mut self) { unsafe { glyim_fs_close(self.fd); } }
}
```
(and the same for the three socket types, with a runtime `glyim_net_close(fd)`).
- Verify: loop fixture opening 10k files must not hit EMFILE.

---

### T143 [HARNESS-3] [Medium] [BUG] `compile-flags`, `aux-file`, `min-version` directives are parsed and then completely ignored

- File: `crates/glyim-test/src/harness/compiler.rs:153-159` (`_flags`), collector.rs:100-111, executor.rs (no min_version/aux_files use)
- Code:
```rust
fn compile(&self, source: &str, source_path: &Path, file_id: FileId,
           _flags: &[String]) -> CompileOutput {     // both compilers ignore flags
```
- Problem: `// compile-flags: --backend=llvm` (tests/codegen/llvm_rvalue_stubs.g) selects nothing; `// aux-file:` stages nothing; `// min-version:` gates nothing (grep shows zero consumers outside config.rs/collector.rs). Combined with `llvm_rvalue_stubs.g` being `test-mode: compile-pass` — where the strategy never runs the program, so `check-stdout: 2` is never evaluated — the fixture asserts nothing at all.
- Fix: Thread `flags` into `Pipeline` (`CrateConfig`/backend selection), implement aux-file staging next to the VFS source registration, and skip tests whose `min_version` exceeds the compiler version; make compile-pass evaluate check-stdout too (or forbid the combination in the collector).
- Verify: `cargo test -p glyim-test` with a fixture that only compiles under a flag — must change outcome when the flag is honored.

---

### T144 [HARNESS-4] [Medium] [BUG] Worker panics are reported as timeouts; timed-out interpreter threads keep spinning

- File: `crates/glyim-test/src/harness/executor.rs:329-346`; `interpreter_runner.rs:39-58`
- Code:
```rust
match rx.recv_timeout(timeout) {
    Ok(result) => Ok(result),
    Err(_) => Err(TimeoutError { timeout_secs }),   // Disconnected (panic!) == timeout
}
```
- Problem: `recv_timeout` fails with `Disconnected` when the worker thread panicked (e.g. `byte_offset_to_line` slicing panic on a mid-char offset, comparison/mod.rs:162-167 — `source[..offset]` panics unless `offset` is a char boundary); the test is misreported as `TimeoutExceeded`, hiding the real ICE. Additionally, on genuine timeouts the spawned thread is never stopped: an interpreter stuck in `loop {}` burns a core forever (one leak per timeout; rayon workers pile up).
- Fix: Match `RecvTimeoutError::Disconnected` → new `FailureReason::WorkerPanic`; add a step/instruction budget to `Interpreter` (old-audit PERF idea) so timed-out interps actually terminate.
- Verify: a fixture that panics inside `compare_diagnostics` (multi-byte char at the diag offset) reports a panic, not a timeout.

---

### T145 [HARNESS-5] [Medium] [BUG] stdin written before the timeout is armed — harness can hang outside timeout protection

- File: `crates/glyim-test/src/harness/runner.rs:96-102`
- Code:
```rust
if let Some(ref input) = self.stdin_input
    && let Some(mut stdin) = child.stdin.take()
{
    let _ = stdin.write_all(input.as_bytes());   // blocks if child never reads > 64 KiB
}
let result = run_child_with_timeout(child, timeout);   // timeout starts only here
```
- Problem: If the test program doesn't read stdin and the input exceeds the pipe buffer, `write_all` blocks forever *before* `recv_timeout` starts — the configured timeout never fires. Also on timeout, `RunResult.stdout` is blanked even though the child produced output.
- Fix: Write stdin on a thread (or after `run_child_with_timeout` spawns the monitor), and have the kill path drain the pipes into the returned `RunResult`.
- Verify: fixture that sleeps without reading stdin given 128 KiB input — must time out, not hang.

---

### T146 [HARNESS-6] [Medium] [BUG] `format_diagnostics` line mapping breaks on CRLF (all lines ≥ 2 shift by one byte)

- File: `crates/glyim-test/src/harness/strategy.rs:164-181`
- Code:
```rust
for (i, ln) in lines.iter().enumerate() {
    let end = acc + ln.len();
    ...
    acc = end + 1;    // assumes exactly one '\n' byte; '\r' of CRLF not counted
```
- Problem: `str::lines()` strips a trailing `\r`, so on CRLF sources each line consumes `ln.len() + 2` bytes but the accumulator advances by `ln.len() + 1` → every diagnostic from line 2 onward reports line-1/col off-by-one in UI snapshots and caret excerpts (Windows checkouts / CRLF fixtures).
- Fix: Compute offsets from `source.match_indices('\n')` or add `+ if ln.ends_with('\r') { 2 } else { 1 }`.
- Verify: round-trip test in `crates/glyim-test/src/tests` feeding a CRLF source with a diag on line 3 and asserting `--> file:3:…`.

---

### T147 [HARNESS-7] [Medium] [STUB] The 5 `probe_*.g.pending` fixtures are never collected or executed

- File: `crates/glyim-test/tests/run-pass/probe_{option_unwrap,println_function,println_macro,string_push_str,vec_push_len}.g.pending`; `harness/collector.rs:49-51`
- Code:
```rust
if path.extension().and_then(|e| e.to_str()) != Some("g") { continue; }   // ".g.pending" has ext "pending"
```
- Problem: No code anywhere renames or reads `.pending` files (grep over glyim-test shows zero references) — the five probes (Vec push/len with exit-code 3, `Option::unwrap`, both println forms, `String::push_str`) are dead coverage; their content suggests they were parked precisely because they fail (they exercise exactly the stdlib paths builtins shadow, cf. T013/T140).
- Fix: Decide per probe — rename to `.g` when the underlying bug is fixed, or delete them and note the blocker; optionally make the collector report `.pending` files as `Ignored (pending)` so they're visible.
- Verify: `cargo test -p glyim-test --test run_pass_corpus probe_` currently discovers nothing; after rename it must run.

---

### T148 [HARNESS-8] [Medium] [BUG] Six orphaned `.snap.new` files record one parser change: `PatIdent` spans now absorb trailing whitespace

- File: `crates/glyim-test/src/snapshot/snapshots/glyim_test__snapshot__snapshot_cst@{full_program, match_expr, index_expr, complex_match, error_handling, complex_program}.snap.new`
- Code (diff, identical shape in all six):
```diff
-         PatIdent@469..472
-         Whitespace@472..473 " "
+         PatIdent@469..473
+           Whitespace@472..473 " "
```
- Problem: Insta wrote these `.snap.new` files because the CST produced by the current parser differs from the committed `.snap`: pattern identifiers now *include* the following whitespace token (the Whitespace node became a child of `PatIdent` instead of a sibling, shifting every subsequent byte offset). This changes `PatIdent.span.hi` for every pattern binding (downstream span/line math, and anything that slices source by `PatIdent` range now picks up a trailing space). The change was never blessed (no `.snap` update) and never reverted — the six tests are either currently red or the files are stale debris. This is the same root cause as T002; complete T002 first, then clean up here.
- Fix: After T002, run `INSTA_UPDATE=always cargo test -p glyim-test snapshot` to bless if the new CST shape is intended (then audit PatIdent range consumers for the trailing-space inclusion), otherwise delete the six `.snap.new` files and confirm the committed `.snap` matches HEAD.
- Verify: `cargo test -p glyim-test --lib snapshot` — zero `.snap.new` files left behind and green.

---

### T149 [SPAN-1] [Medium] [BUG] `SyntaxContext::expn_id()` reinterprets a context id as an expansion id

- File: `crates/glyim-span/src/lib.rs:227-233`; `hygiene.rs:89-108`
- Code:
```rust
pub fn expn_id(self) -> ExpnId { ExpnId::from_raw(self.to_raw()) }
```
- Problem: `HygieneCtx` allocates `ExpnId`s and `SyntaxContext`s from two independent counters (`next_expn_id`, `next_syntax_context`, hygiene.rs:81-83, 100-101); `syntax_contexts[i].outer_expn` is the real association. `expn_id()` returns `ExpnId(ctx_raw)`, which indexes a *different* entity in `expansions` (e.g. context #3 may belong to expansion #1 while `expn_id()` claims expansion #3) — any consumer gets the wrong `ExpnData`/call site. Currently unused outside tests (latent trap).
- Fix: Return `self.syntax_contexts[raw-1].outer_expn` (needs `&HygieneCtx`) or delete the method.
- Verify: unit test in `crates/glyim-span/src/tests`: apply two marks for one expansion; `ctx.expn_id()` must equal that expansion, not the context index.

---

### T150 [DIAG-1] [Medium] [BUG] No diagnostic dedup/sort — and the pipeline comment that claims both is stale

- File: `crates/glyim-diag/src/lib.rs:438-451`; `crates/glyim-pipeline/src/lib.rs:611-613`
- Code:
```rust
// pipeline lib.rs:612
// caller can render it. Sorted + deduped for a stable order.
let warnings = sink_cell.into_inner().into_diagnostics();   // no sort, no dedup
```
- Problem: `DiagSink::emit` appends unconditionally (no dedup key on code+span+message), so duplicate diagnostics (e.g. the same expression diagnosed during expansion *and* re-check, or an error surfaced through two phases) reach users and the LSP twice, in nondeterministic phase order. The comment above `into_diagnostics` asserts sort+dedup that is simply not implemented.
- Fix: In `into_diagnostics` (or the pipeline):
```rust
diagnostics.sort_by_key(|d| (d.span.primary.lo.to_raw(), d.code.to_string(), d.message.clone()));
diagnostics.dedup_by(|a, b| a.span.primary == b.span.primary && a.code == b.code && a.message == b.message);
```
before returning.
- Verify: unit test in `crates/glyim-diag/src/tests.rs` emitting the same diagnostic twice through the pipeline returns exactly one.

---

## 3.8 glyim-pilot & extension (mediums)

### T151 [PILOT-9] [Medium] [BUG] Parse/apply/gate-infra errors never reach the AI — the agent hangs silently

- File: `tools/glyim-pilot/src/main.rs:341-345` (`handle_extension_message`), `src/orchestrator/turn.rs:153,157` (`?` propagation)
- Code:
```rust
Err(e) => {
    tracing::error!(?e, "orchestrator error");
    metrics_clone.increment_counter("orchestrator_error", &[("code", e.code())]);
}                                   // <-- no CliMessage::FeedbackSend
```
- Problem: `PilotError::Parse` (malformed ops), `ApplyError::FindNotFound/FindAmbiguous`, and `PilotError::Gate` (e.g. `cargo-llvm-cov not installed`) all return `Err` → only logged + counted. Spec REQ-FUNC-011 requires a structured error be **sent to the AI**; REQ-FUNC-049 requires done-gate infra failures to be fed back. The extension is left waiting for a `feedback.*` message that never comes — the turn deadlocks until the human intervenes.
- Fix: In the `Err(e)` arm map the error to feedback:
```rust
let fb = CliMessage::FeedbackSend {
    session_id, message: format!("Error {code}: {e}. Please fix your glyim-ops block and resend."),
    turn: turn + 1, trace_id: Some(trace_id), v: PROTOCOL_VERSION };
let _ = cli_sender_clone.send(serde_json::to_string(&fb).unwrap());
```
(only for recoverable classes: Parse/Apply/Limits/Gate; Escalate for others).
- Verify: send `ops.ready` with content `::WRITE src/x.rs\nno END` → extension receives `feedback.send` containing `E0100`.

---

### T152 [PILOT-10] [Medium] [STUB] Eight orphan `pub struct Stub;` files + whole spec subsystems dead

- File: `tools/glyim-pilot/src/{server,commit,gates,cli,context,orchestrator,session,dispatch}/stub.rs:1` (each is exactly `pub struct Stub;`)
- Code:
```rust
pub struct Stub;
```
- Problem: All 8 files are unreferenced — `grep "mod stub"` finds **no** `mod.rs` declares them, and nothing imports `Stub`. They are placeholders for spec capabilities that are implemented-but-unwired or absent: CAP-RETRY (`handle_rate_limit`, `ProviderPool::free`, cooldown-aware selection — zero callers; `detectError()` never invoked in TS), CAP-CONTEXT (`ContextAssembler`/`TokenBudget`/`smart_truncate` never called; tier 3/4 are `_`-prefixed ignored params in `assembler.rs:38-41`), session state machine (`TransitionValidator` zero callers → status stuck INIT), protocol framing (`extract_ops_blocks` dead server-side; `validate_version` dead).
- Fix — concrete plan per stub (each ≤ ~150 LOC, then delete the stub file):
  1. **server/stub.rs → `ConnectionRegistry`**: holds the single authorized extension conn + token check (T016); replace the per-connection `tokio::spawn` blob in `ws.rs::run` with `registry.register(stream, addr)?` / on drop deregister; provides `send_cli(msg)` so `main.rs` stops juggling the broadcast channel + dummy receiver (`main.rs:93-99`).
  2. **commit/stub.rs → `CommitPipelineRunner`**: move the gate→feedback formatting out of `engine.rs` (the `fmt_failed` retry block, lines 126-176) into `run_pipeline_and_commit(ctx) -> CommitDecision`; makes `engine.rs` testable without git.
  3. **gates/stub.rs → `GateFactory`**: `fn commit_gates(resolved: &ResolvedCommitGates, banned, arch) -> Vec<Arc<dyn Gate>>` and `fn done_gates(...) -> ...`; `commit_pipeline.rs`/`done_pipeline.rs` currently hand-build these lists — centralize so a missing gate in one list is a compile-time-visible table.
  4. **cli/stub.rs → `watch` command**: REQ-FUNC-065/066 real-time dashboard: `tokio::select!` loop printing `render_status_table` every 2s + counters (ops_ready_received, turn_processed from `metrics::names`); add `Commands::Watch` in `main.rs`.
  5. **context/stub.rs → `Tier3Extractor`**: implement REQ-FUNC-031 (pub-signature-only extraction — reuse `gates::contracts::extract_pub_name`) and wire `ContextAssembler::assemble` into `cli/agent.rs::assemble_prompts` so source context actually goes through `TokenBudget` instead of the current unbounded concatenation (`agent.rs:226-251`).
  6. **orchestrator/stub.rs → `TurnScheduler`**: REQ-FUNC-003 queue: per-stream mpsc holding pending `ops.ready`; if `processing` insert fails, push to queue and drain on completion (replaces the drop-on-floor `WaitForResponse` path that loses multi-block turns, cf. T158).
  7. **session/stub.rs → `SessionLifecycle`**: REQ-FUNC-062/063 — on startup, diff state-file sessions against live extension tabs (`session.ready` replay), mark unreachable ones `Paused`, expose `redispatch(session)` that resends `SessionStart`.
  8. **dispatch/stub.rs → `CooldownAwarePool`**: wrap `ProviderPool` so `most_slots_available()` filters `cooldown_until > now` (REQ-FUNC-006) and `free()` is called on stream completion (REQ-FUNC-043 — currently zero callers); wire `handle_rate_limit` to `error.detected` messages with `error_type == "rate_limit"`.
- Verify: `rg -n "struct Stub" tools/glyim-pilot/src` returns nothing; `cargo test -p glyim-pilot` green; `cargo run -p glyim-pilot -- watch` renders live table.

---

### T153 [PILOT-11] [Medium] [BUG] Banned-pattern gate: silent read failures + lifetime quoting let `.unwrap()`/`todo!()` through

- File: `tools/glyim-pilot/src/gates/banned_pattern.rs:60, 118-127, 183-193`
- Code:
```rust
if let Ok(content) = std::fs::read_to_string(&path) {   // read failure → file silently skipped → PASS
...
if c == '\'' {                       // lifetime 'a swallows rest of line looking for closing '
    while let Some(nc) = chars.next() { if nc == '\\' { chars.next(); continue; } if nc == '\'' { continue 'outer; } }
    continue;
}
```
- Problem: (a) If a changed `.rs` file is unreadable or non-UTF8, the gate passes without checking it — the agent can't accidentally but can systematically hide code from this gate (write a file with one invalid UTF-8 byte). (b) Any line containing an unmatched `'` (a lifetime: `let s: &'a str = x.unwrap();`) has its remainder consumed as an unterminated char literal, so `.unwrap()` after the lifetime is **not** flagged — and there is no G-3/KR2 enforcement left (`todo!()` KR in spec). (c) Block comments `/* todo!() */` are false-positived (only `//` lines are skipped).
- Fix (3 steps):
  1. Make read failure a gate error: `.unwrap_or_else(|e| { violations.push(format!("{rel_path}: unreadable: {e}")); String::new() })`.
  2. Distinguish lifetime from char literal: treat `'x` (single char + `'` or non-closing) as lifetime and just `result.push(c)`; simplest robust approach: if the char after `'` is alphanumeric and there's no closing `'` within 4 chars, treat as lifetime and push verbatim.
  3. Track a simple `in_block_comment` flag across lines in `check_content_for_violations` and skip.
- Verify: unit tests — (1) line `let s: &'a str = x.unwrap();` with pattern `unwrap` → violation reported; (2) file containing `0xFF` byte + pattern `unwrap` → gate returns `Err`/violation, not pass.

---

### T154 [PILOT-12] [Medium] [BUG] Backups read files as UTF-8 → binary files can never be Deleted/Replaced; no fsync before rename

- File: `tools/glyim-pilot/src/applier/mod.rs:186-196, 275-290`
- Code:
```rust
let original_content = if abs_path.exists() {
    Some(fs::read_to_string(&abs_path).map_err(|e| ... Io { operation: "read_for_backup" ...})?)  // fails on binary
...
fs::write(&tmp_path, content)?; fs::rename(&tmp_path, &abs_path)?;   // no sync_all before rename
```
- Problem: `create_backups` runs for **every** op including `Delete`, and `read_to_string` errors on non-UTF-8 (PNG, font, `.bin`) → deleting/replacing any binary file always fails with E0204 "I/O error during read_for_backup". Separately, tmp file is never `sync_all()`ed before rename, so on power loss the rename may be durable while the content isn't (torn file) — the tool's whole value is atomic-ish applies. (Old-audit INF-30 flagged the tmp-name collision; the fsync half remains.)
- Fix (2 steps):
  1. Store `original_bytes: Option<Vec<u8>>` using `fs::read`, and for Delete skip reading entirely (only stat existence).
  2. Before rename:
```rust
let f = fs::File::open(&tmp_path)?; f.sync_all()?;
fs::rename(&tmp_path, &abs_path)?;
```
- Verify: test — write `file.bin` with `[0u8, 1, 2, 255]`, run `apply_ops` with `FileOp::Delete` → Ok; fsync path reviewed.

---

### T155 [PILOT-13] [Medium] [BUG] WS forward loop dies on broadcast `Lagged`; undecodable messages silently dropped; `validate_version` never called

- File: `tools/glyim-pilot/src/server/ws.rs:96-101, 107-120`, `src/server/messages.rs:108-117`
- Code:
```rust
while let Ok(msg) = cli_rx.recv().await {     // Err(Lagged) → loop exits → extension deaf
...
Ok(Message::Text(text)) => { if let Ok(ext_msg) = serde_json::from_str::<ExtensionMessage>(&text) { ... } } // else: dropped silently
```
- Problem: (a) The broadcast channel holds 256 messages; if the extension reads slowly (tab throttled), `recv()` returns `Err(Lagged)` and the per-connection CLI-forward task **exits while the socket stays open** — from then on the extension receives nothing, with zero logs. (b) Any message that fails serde (unknown `type`, missing required field) is silently discarded — spec protocol negotiation is impossible to debug. (c) `ExtensionMessage::validate_version` (and the TS-side `validateMessageVersion`) are dead code — a `v=0` or `v=99` message is processed; protocol versioning (spec 5.1) is effectively absent.
- Fix (3 steps):
  1. `match cli_rx.recv().await { Ok(m) => {...}, Err(broadcast::error::RecvError::Lagged(n)) => { tracing::warn!("cli forward lagged, {n} dropped"); continue; }, Err(_) => break }`.
  2. Add an `else { tracing::warn!(peer=%addr, "undecodable message: {}", &text[..text.len().min(200)]) }` branch.
  3. After deserialize: `if let Err(ver) = ext_msg.validate_version() { let _ = event_tx.send(...); continue; }`.
- Verify: unit test pushing 300 messages into `cli_msg_tx` while receiver sleeps 1s → forward task still alive afterwards and receives the 301st; send `{"type":"ops.ready",...,"v":0}` → rejected with log.

---

### T156 [PILOT-14] [Medium] [BUG] State persistence: no fsync, shared `.tmp` race, panic on corrupt state, unbounded session growth, status machine dead

- File: `tools/glyim-pilot/src/session/persistence.rs:27-38`, `src/main.rs:101-105, 389-391`, `src/session/state.rs` (sessions map never pruned)
- Code:
```rust
let tmp_path = PathBuf::from(format!("{}.tmp", self.path.display()));
tokio::fs::write(&tmp_path, &content).await?;
tokio::fs::rename(&tmp_path, &self.path).await?;      // no sync_all / sync parent dir
// main.rs
StatePersistence::load(&project_root).await.expect("failed to load state");  // panics on corrupt JSON
```
- Problem: (1) Crash between rename and data flush can leave a torn state file (same durability gap as T154). (2) Two processes (`serve` + any `status`/`session`/HTTP merge in another process) use the same `.tmp` path — interleaved write/rename corrupts or cross-commits state; there's no file lock. (3) Corrupt/truncated state file (crash mid-write, the very scenario this is meant to survive, REQ-FUNC-061/063) makes the server **panic at startup** (old-audit INF-30, still live). (4) `sessions` HashMap grows without bound (every unknown `session_id` in `ops.ready` creates a session + worktree, T016) and is serialized **in full on every save** (`p.save()` inside `try_update_session`) — O(all sessions) JSON per message (PERF). (5) Status never transitions (see T070), so the dashboard + `wait_for_session` are wrong.
- Fix (5 steps):
  1. `let f = tokio::fs::File::create(&tmp).await?; f.write_all(...).await?; f.sync_all().await?; drop(f); tokio::fs::rename(...).await?;`
  2. `tmp = format!("{}.{}.tmp", path, std::process::id())` + `fs2::File::lock_exclusive` on the state file at load.
  3. Replace `.expect` with: on parse error, rename the bad file to `.glyim-pilot-state.json.corrupt-<ts>`, log `error!`, start fresh.
  4. Cap: if `sessions.len() > 200`, evict sessions with `pr_merged && last_activity > 30d` (log eviction).
  5. Wire `TransitionValidator` (T070/T152).
- Verify: `cargo test -p glyim-pilot persistence` (new tests: corrupt file → loads fresh + `.corrupt` sidecar exists; 250 sessions → evicted to 200).

---

### T157 [PILOT-15] [Medium] [MISCOMPILE] Prometheus metrics cache keyed by name only — per-call labels silently lost

- File: `tools/glyim-pilot/src/metrics.rs:81-97` (+ call site `src/main.rs:370`)
- Code:
```rust
fn increment_counter(&self, name: &str, labels: &[(&str, &str)]) {
    if let Some(counter) = cache.get(name) { counter.inc(); return; }   // labels ignored on 2nd call
    let opts = prometheus::Opts::new(name, name).const_labels(make_const_labels(labels));
```
- Problem: First call registers e.g. `extension_error{type="input_not_found"}`; every later call — `increment_counter("extension_error", &[("type", &error_type)])` with a *different* error type — increments that first series. All label dimensions collapse into whichever value arrived first: rate-limit vs dangerous-pattern vs network errors are indistinguishable in metrics (wrong-data). The `prometheus` crate also ignores duplicate registration (`let _ = register(...)`), hiding the conflict.
- Fix: Key the cache by `(name, sorted labels)`:
```rust
let key = format!("{name}|{}", labels.iter().map(|(k,v)| format!("{k}={v}")).collect::<Vec<_>>().join(","));
```
and store `HashMap<String, IntCounter>`. (Prometheus best practice: register an `IntCounterVec` once and fetch `get_metric_with_label_values` per call.)
- Verify: unit test — increment twice with different labels, `default_registry().gather()` shows 2 series with 1 each (not 1 series with 2).

---

### T158 [PILOT-16] [Medium] [BUG] glyim-ops parser wrong-data: unknown directives silently ignored, `::COMMIT` quotes kept, trailing blank lines stripped

- File: `tools/glyim-pilot/src/protocol/parser.rs:98-108, 120-141`, `src/cli/session.rs:16`
- Code:
```rust
} else if trimmed == "::APPROVED" { approved = true; }    // anything else: silently ignored (typo = lost directive)
let commit_msg = format!("stream-{stream_id}: {message}"); // message still has surrounding quotes from ::COMMIT "msg"
while content_lines.last().is_some_and(|l| l.trim().is_empty()) { content_lines.pop(); } // WRITE content silently loses trailing blank lines
```
- Problem: (a) A typo'd directive (`::COMMITE "x"`, `::DNONE`) is dropped without error — with T073 sending the *whole response*, any prose line that happens to look like a directive is executed and everything else ignored; spec NFR-MAIN-002 requires strict independent parsing. (b) The shipped system prompt tells the AI to write `::COMMIT "Add multiply function"`, and the parser stores the message **with literal quotes** → commits titled `stream-S01: "Add multiply function"`. (c) Trailing blank lines of `::WRITE` content and of `---FIND---`/`---REPLACE---` blocks are stripped, so a REPLACE whose FIND genuinely ends with an empty line can never match (mystery `FindNotFound`).
- Fix (3 steps):
  1. Replace the fall-through with `else { return Err(PilotError::Parse { line: line_num + 1, message: format!("unknown directive {trimmed:?}") }) }` (any line starting with `::`); keep ignoring non-directive prose.
  2. `let msg = msg.trim().trim_matches(|c| c == '"' || c == '\'')` before storing.
  3. Only strip the single trailing newline the protocol implies — i.e. strip at most one trailing empty line, not a loop (update tests).
- Verify: `cargo test -p glyim-pilot parser` + new tests: `::COMMITE x` → Err; `::COMMIT "hi"` → message `hi`; `::WRITE f\n\n\n::END` → content ends with one `\n`.

---

### T159 [PILOT-17] [Medium] [PERF] Server event loop blocks on worktree creation & git calls; per-connection backpressure stalls all peers

- File: `tools/glyim-pilot/src/main.rs:130-148, 251-297`
- Code:
```rust
Some(event) = event_rx.recv() => { match event { ... ServerEvent::Message { msg, .. } => {
    handle_extension_message(msg, ...).await;      // awaited inline in the select loop
```
with `create_worktree(...)` (up to `command_timeout`=120s of git) and `persistence.get_*` awaited inside.
- Problem: The main `tokio::select!` loop processes one `ServerEvent` at a time and `handle_extension_message` awaits git/`gh`/disk inline (only `process_turn_dispatch` is spawned, main.rs:321). A slow `git worktree add` (cold clone, network ref negotiation) freezes Pong handling, `error.detected` handling, and status reporting for every connected session; meanwhile each connection's `event_tx.send(...).await` backpressures when the 1024-slot channel fills → all WS connections stall together.
- Fix: In the `Message` arm, `let cfg=persistence…; tokio::spawn(handle_extension_message(...))` (all inputs are `Arc`/clonable — clone `persistence`, `cli_sender`, `processing`, `metrics`); keep ordering per stream via the existing `processing` set (T069's guard makes this safe). Move worktree creation into the spawned task.
- Verify: integration — start server, send `ops.ready` with a fake `git` shim that sleeps 30s, then send a `Pong` → pong is logged immediately (not after 30s).

---

### T160 [EXT-4] [Medium] [BUG] `StreamWatcher.resetForNewTurn` never called — turn stuck at 0, watcher half-dead after first completion; multi-block responses dropped server-side

- File: `tools/glyim-pilot/extension/src/stream_watcher.ts:78-83` (no callers), `background.ts:243-259` (turn bookkeeping), `src/orchestrator/turn.rs:122-128` (server dedup)
- Code:
```ts
resetForNewTurn(): void { this.turn++; this.previousResponseText=''; this.sentHashes.clear(); this.completed=false; }  // never invoked
// background: session.turn++ local only; ops.ready sent with watcher-internal this.turn (always 0)
```
- Problem: (1) `turn` in `ops.ready` is permanently `0`, so the server's `turn + 1` bookkeeping (event_handler.rs) and the extension's own counter diverge — feedback messages land with wrong turn numbers. (2) After the first `handleStreamComplete` sets `completed = true` and disconnects `copyObserver`, the watcher never signals completion again for that tab (feedback turns rely solely on `reinjectWatcher`'s separate page-side observer — T161). (3) When a model emits *two* ops blocks, background sends two `ops.ready`; the server's `processing` set (T069/T152) silently drops the second as "already processing" with no requeue — **data loss of an entire code block**.
- Fix (3 steps):
  1. Call `watchers.get(tabId)?.resetForNewTurn()` in `handleFeedbackSend`/`handleFeedbackContinue`/`handleRetryPrompt` before injecting; take the authoritative turn from the server's `feedback.send` message (`msg.turn`) instead of local increments.
  2. Server-side: replace drop-on-duplicate with the queued scheduler (T152 item 6) so the second block is processed after the first.
- Verify: two-block response fixture → both blocks applied (`ApplyResult` count 2 in logs, `turn` values 0 then 1).

---

### T161 [EXT-5] [Medium] [BUG] `reinjectWatcher` page observer fires immediately on the pre-existing assistant element — stale/partial responses re-sent

- File: `tools/glyim-pilot/extension/src/background.ts:229-237`
- Code:
```ts
const obs = new MutationObserver(() => {
  const responseElement = document.querySelector(asstSel);
  if (responseElement) {           // exists from the PREVIOUS turn → fires on the first mutation
    const full = responseElement.textContent || '';
    chrome.runtime.sendMessage({ type: 'stream.complete', sessionId: sid, turn: turnNum, fullResponse: full });
    obs.disconnect();
  }
});
```
- Problem: On feedback turns the assistant element from the previous reply already exists; the first unrelated mutation (typewriter span, cursor blink) triggers a `stream.complete` with the **stale** response. Background then extracts ops blocks from it and sends a duplicate `ops.ready` (the runtime.onMessage path has no dedup against `StreamWatcher.sentHashes`), causing re-applies of the previous block or application of a *partial* response mid-stream. Spec REQ-FUNC-058 requires not extracting while streaming — this path ignores `isStreaming()` entirely.
- Fix: Gate the observer on streaming state and on text growth:
```ts
obs.observe(document.body, { childList: true, subtree: true, characterData: true });
let baseline = document.querySelector(asstSel)?.textContent ?? '';
// inside callback:
const el = document.querySelector(asstSel);
if (!el) return;
const now = el.textContent ?? '';
if (adapter.isStreaming?.() || now === baseline || now.length <= baseline.length) return;
baseline = now; chrome.runtime.sendMessage({ type: 'stream.complete', ... fullResponse: now });
```
(and dedupe in background: keep a per-session `lastSentHash` checked in the `runtime.onMessage` handler too).
- Verify: manual — send feedback, observe `[bg] Sent ops.ready` logs: exactly one per completed reply; old content never re-sent (hash equality check in logs).

---

### T162 [EXT-6] [Medium] [PERF] No debounce on mutation checks (SHA-256 per DOM mutation) + `cycleTabs` steals focus every 10s forever

- File: `tools/glyim-pilot/extension/src/stream_watcher.ts:35-38, 85-88`, `background.ts:327-359`
- Code:
```ts
this.observer = new MutationObserver(() => { if (!this.adapter.isStreaming()) void this.serializedCheck(); });
...
setInterval(cycleTabs, 10000);   // activates EVERY session tab every 10s, always, even idle
```
- Problem: Spec REQ-FUNC-056 mandates "debounced extraction (500ms minimum interval)". As written, every streamed token triggers `serializedCheck` → `getAssistantText()` + `extractGlyimOpsBlocks` + **SHA-256 over the full response** (`crypto.subtle.digest`) — O(n²) work per reply and main-thread jank on the provider tab. Separately, `cycleTabs` reactivates all session tabs every 10 s unconditionally (not just while streaming), visibly flickering the user's window during long sessions; it also runs even when all watchers are `completed`.
- Fix (2 steps):
  1. Debounce: keep the raw mutation flag and process on a 500 ms trailing timer:
```ts
let debounce: ReturnType<typeof setTimeout> | null = null;
this.observer = new MutationObserver(() => {
  if (this.adapter.isStreaming()) return;
  if (debounce) return;
  debounce = setTimeout(() => { debounce = null; void this.serializedCheck(); }, 500);
});
```
  2. In `cycleTabs`, skip tabs whose watcher reports `completed`/`isStreaming() === false` and return early when no active watcher is mid-stream; raise interval to 1s only while any stream is live.
- Verify: instrument with a counter around `checkForCompleteBlocks` during a 60s stream → invocations ≈ response_length/500ms chunks, not per-mutation; user's active window no longer switches while streams idle.

---

### T163 [EXT-7] [Medium] [STUB] Rate-limit/error detection is implemented but never wired; protocol enums and version checks are warn-only

- File: `tools/glyim-pilot/extension/src/providers/adapter.ts:141-158` (`detectError` zero callers), `background.ts:74` (`errorType: 'dangerous_pattern'`), `types.ts:31-35` + `ws_client.ts:33-34` + `server/messages.rs:108` (version checks dead/warn-only)
- Code:
```ts
detectError(): ProviderError | null { ... }        // never invoked anywhere
...
(content, pattern) => ws.send({ type: 'error.detected', sessionId, errorType: 'dangerous_pattern', ... })
```
- Problem: CAP-RETRY (REQ-FUNC-033..040) is a stub end-to-end: the extension polls `isStreaming()` but never calls `detectError()`, so rate limits/server-busy states are never reported; the Rust `handle_rate_limit`/failover path has zero callers (see T152). The `error.detected` enum in spec 5.1 (`rate_limit|server_busy|capacity|server_error|network_error`) doesn't include `'dangerous_pattern'`, and the Rust side accepts any string — protocol enum drift unflagged. `validateMessageVersion` only `console.warn`s and processing continues; the Rust `validate_version` is never called at all — version enforcement is absent on both sides.
- Fix (3 steps):
  1. In the `pollingTimer` callback add `const err = this.adapter.detectError(); if (err) { this.onRateLimit(err); return; }` with a new constructor callback sending `error.detected` with the spec enum value (`recoverable: err.recoverable`).
  2. Extend the spec enum (and Rust `ErrorDetected`) with `'dangerous_pattern'` **or** send dangerous patterns as a distinct message type; validate on both sides with a shared list.
  3. Enforce version: TS `if (versionError) return;` before `messageHandler`, Rust as in T155(c).
- Verify: simulate a rate-limit by injecting the error selector DOM into the page → `error.detected {errorType:"rate_limit"}` appears on the WS; Rust unit test: `serde_json::from_str::<ExtensionMessage>("{\"type\":\"error.detected\",\"errorType\":\"banana\",...}")` → rejected.

---

**Wave 3 checkpoint:** `cargo build --workspace && cargo test --workspace && cargo test -p glyim-pilot; git add -A && git commit -m "fix(wave-3): T076-T163 mediums"`

---

# WAVE 4 — Low-severity & performance fixes

## 4.1 Frontend

### T164 [FE-109] [Low] [BUG] `x @ (tuple)` binding-with-subpattern errors — `@` calls a parser that has no `(` arm

- File: `crates/glyim-frontend/src/parser/pat.rs:180`
- Code:
```rust
// FE-9: `x @ subpat` — binding with a sub-pattern.
if self.current_kind() == SyntaxKind::At {
    self.bump(); // @
    self.parse_pat_inner();
}
```
- Problem: `parse_pat_inner` → `parse_pat_inner_impl` has no `LParen` arm (tuple handling lives in `parse_pat_single`), so `x @ (1, 2)` yields "expected pattern, found LParen", bumps the `(`, and cascades. `x @ Some(y)` works only because the `Ident` arm handles `Ident (`.
- Fix: Call `self.parse_pat_single()` instead of `self.parse_pat_inner()` after the `@` (it handles `(`, `&`, `ref`, `mut` without reintroducing top-level `|`, which Rust also rejects in bare `x @ a | b`).
- Verify: unit test on `fn main() { let x @ (a, b) = (1, 2); }` asserting zero diagnostics.

---

### T165 [FE-113] [Low] [BUG] Radix literals with no digits lex clean (old FE-16 — still open), downstream misreports

- File: `crates/glyim-frontend/src/lexer.rs:527`
- Code:
```rust
fn lex_number(&mut self) -> SyntaxKind {
    // Radix prefixes (no validation, just consume)
    if self.peek() == Some('0') {
        let next = self.peek_next();
        if next == Some('x') || next == Some('X') {
            self.advance();
            self.advance();
            while let Some(ch) = self.peek() {
                if ch.is_ascii_hexdigit() || ch == '_' { self.advance(); } else { break; }
            }
            self.lex_number_suffix();
            return SyntaxKind::IntLit;
```
- Problem: `0x`, `0b`, `0o` with zero digits produce a clean `IntLit` token and no lexer diagnostic. The error only surfaces later in HIR (`parse_int_with_prefix`, lower_expr.rs:1253) as **"integer literal is too large: \`0x\`"** — a wrong message at a stage where the precise token span is already available.
- Fix: After each radix loop, check that at least one digit was consumed (`self.pos > digits_start`); if not, push `GlyimDiagnostic::lex_error(self.span(start, self.pos), "expected digits after radix prefix")` before `lex_number_suffix()`.
- Verify: unit test in `lexer_tests.rs` asserting `lex("0x", id).diagnostics.len() == 1` and the message contains "radix"; `cargo test -p glyim-frontend --lib integers`.

---

### T166 [FE-115] [Low] [BUG] Trait-body associated type with a default (`type A = T;`) error-cascades

- File: `crates/glyim-frontend/src/parser/item.rs:684`
- Code:
```rust
SyntaxKind::KwType => {
    self.start_node(SyntaxKind::TypeAlias);
    self.bump(); // type
    self.bump_expected(SyntaxKind::Ident);
    if self.current_kind() == SyntaxKind::Colon {
        self.bump();
        loop { ... }
    }
    self.expect(SyntaxKind::Semicolon);
    self.finish_node();
}
```
- Problem: Only `:` bounds are handled; for `type Assoc = u8;` the `Eq` is left in place → "expected Semicolon, found Eq" (no bump), then the trait-body loop hits `Eq`/`u8`/`;` with "expected trait item" three more times. Valid Rust syntax, 4 spurious errors. (The impl-body variant at item.rs:784 correctly handles `Eq`.)
- Fix: Add the missing branch, mirroring the impl-body arm:
```rust
if self.current_kind() == SyntaxKind::Colon {
    self.bump();
    loop { self.parse_type(); if self.current_kind() == SyntaxKind::Plus { self.bump(); } else { break; } }
} else if self.current_kind() == SyntaxKind::Eq {
    self.bump();
    self.parse_type();
}
```
- Verify: unit test with `trait T { type A = u8; }` asserting zero diagnostics; `cargo test -p glyim-frontend --lib`.

---

### T167 [FE-116] [Low] [BUG] Unterminated byte literal reports a zero-width span

- File: `crates/glyim-frontend/src/lexer.rs:746`
- Code:
```rust
if !terminated {
    let end = self.pos;
    self.diagnostics.push(GlyimDiagnostic::lex_error(
        self.span(end, end),
        "unterminated byte literal".to_string(),
    ));
}
```
- Problem: Unlike `lex_string`/`lex_char` (which use `self.span(start, end)`), the byte-literal path spans `(end, end)` — a zero-width range at EOF — so editors/CLI underline nothing. `let b = b'abc;` points the diagnostic at the wrong place.
- Fix: Record `let lit_start = self.pos;` as the first line of `lex_byte_lit` and emit `self.span(lit_start, self.pos)`.
- Verify: unit test asserting the diagnostic span of `b'abc` starts at the `b` and covers `b'abc`; `cargo test -p glyim-frontend --lib strings` (or `lexer_tests`).

---

### T168 [FE-114] [Low] [PERF] Per-token `SmolStr` clone in the hottest parser loops (old FE-19 — still open)

- File: `crates/glyim-frontend/src/parser/mod.rs:140` (also `flush_trivia` at :77)
- Code:
```rust
if let Some(token) = self.tokens.get(self.pos) {
    let kind = GlyimLang::kind_to_raw(token.kind);
    let text = token.text.clone();
    self.builder.token(kind, text.as_str());
    self.pos += 1;
}
```
- Problem: Every `bump()`/trivia flush clones the token's `SmolStr` just to hand rowan a `&str` — `GreenNodeBuilder::token` takes `&str`, so the clone is pure overhead paid once per token of every parsed file (inline variants memcpy ≤23 bytes; long idents/comments take an Arc bump).
- Fix: Delete the clone — `self.builder.token(kind, token.text.as_str());` (the `&'a Token` borrow does not conflict with `&mut self.builder` because `self.tokens: &'a [Token]` is copied out). Same change in `flush_trivia`.
- Verify: `cargo build -p glyim-frontend && cargo test -p glyim-frontend --lib`; optionally benchmark parsing a large fixture (e.g. the 115 KB source mentioned in lexer.rs docs) before/after.

---

## 4.2 Macros, proc-macro, resolution, HIR

### T169 [MAC-4] [Low] [BUG] Metavariable used outside its repetition splices **all** iterations concatenated

- File: `crates/glyim-meta/src/expander/substitution.rs:38-43`
- Code:
```rust
} else if let Some(iterations) = bindings.get(name) {
    // Outside a repetition each metavar has exactly one
    // iteration; splice its tokens verbatim.
    for iteration in iterations {
        result.extend(iteration.iter().cloned());
    }
}
```
- Problem: For `$x` bound by `$( $x:expr ),*` and referenced bare in the template (not inside `$()`), the loop concatenates every iteration with no separator — `m!(1, 2)` with template `fn f() -> i32 { $x }` produces `1 2` (garbage token soup) instead of Rust's "attempted to repeat an expression containing no syntax variables" error. The comment ("exactly one iteration") is only true for depth-0 bindings.
- Fix: Record each binding's depth at match time (e.g. store `usize` alongside, or a parallel `HashMap<SmolStr, usize>` built in the Repetition arm); in the bare-reference branch, if depth > 0 return `Err` (or a dedicated diagnostic "metavariable `$x` used outside its repetition").
- Verify: `cargo test -p glyim-meta` + case asserting the diagnostic for `macro_rules! m { ($($x:expr),*) => { $x } } m!(1, 2);`.

---

### T170 [MAC-7] [Low] [MISCOMPILE] `env!`/`file!`/`option_env!` inject unescaped quotes/backslashes into the produced string literal

- File: `crates/glyim-meta/src/expander/mod.rs:552` (`file!`), :578-579 (`env!`), :610 (`option_env!`)
- Code:
```rust
Ok(val) => {
    let lit = SmolStr::from(format!("\"{}\"", val));
    vec![TokenTree::Token(SyntaxKind::StringLit, lit)]
}
```
- Problem: Unlike `include_str!` (:649 escapes `\` and `"`), the `env!`/`file!`/`option_env!` paths wrap the raw value in quotes. Trigger: `MYVAR='a"b' glyim-cli prog.g` with `let s = env!("MYVAR");` expands to `"a"b"` → the expansion reparse (mod.rs:259-263) turns it into garbage tokens / parse error; a backslash in a path or env value is likewise re-interpreted as an escape by the later string unescape (lower_expr.rs:1154-1176), producing wrong contents.
- Fix: Reuse the include_str escaping:
```rust
let escaped = val.replace('\\', "\\\\").replace('"', "\\\"");
let lit = SmolStr::from(format!("\"{}\"", escaped));
```
in all three arms (file! already escapes only `\\` — add the quote pass there too).
- Verify: `MYVAR=$'a"b\\c' cargo run -p glyim-cli -- envdemo.g --emit=exec` printing the variable reproduces it exactly; `cargo test -p glyim-meta builtin_macros`.

---

### T171 [MAC-8] [Low] [STUB] `include!` expands to a *string literal* of the file — cannot include code (duplicate of `include_str!`)

- File: `crates/glyim-meta/src/expander/mod.rs:747-798`
- Code:
```rust
BuiltinMacro::Include => {
    // include!("path") reads file content as a string literal.
    ...
    match fs::read_to_string(&resolved) {
        Ok(content) => {
            let escaped = content.replace('\\', "\\\\").replace('"', "\\\"");
            let lit = SmolStr::from(format!("\"{}\"", escaped));
            vec![TokenTree::Token(SyntaxKind::StringLit, lit)]
        }
```
- Problem: Rust's `include!` splices the file's **tokens** into the program (items/expressions). Here it is byte-identical to `include_str!` (string-literal result), so `include!("items.g");` at item position expands to a string literal in item position → parse failure, and there is no way to include code. Registered in the default set at :159.
- Fix: Read the file, then feed its text through the frontend in the appropriate context and re-emit green nodes — simplest mechanical path: build `TokenTree`s by lexing `content` (reuse `glyim_frontend::parse_to_syntax` on the content wrapped the same way `expand_node_recursive` wraps expansions, then copy the inner nodes), i.e. mirror the `build_expansion_green` + reparse flow used for declarative macros.
- Verify: `cargo run -p glyim-cli -- main.g --emit=mir` with `include!("inc.g");` containing `fn included() -> i32 { 7 }` and a call to it in main.

---

### T172 [PM-2] [Low] [BUG] Interior-NUL token text → `CString::new` fails, `unwrap_or_default` + stale `len` reads out of bounds

- File: `crates/glyim-proc-macro/src/lib.rs:355-359`
- Code:
```rust
let ctext = CString::new(text.as_str()).unwrap_or_default();
let pm_text = PmStr {
    ptr: ctext.as_ptr() as *const u8,
    len: text.len() as u32,
};
```
- Problem: If a token's text contains a NUL byte (a string literal carrying a raw NUL, or a synthesized token), `CString::new` returns `Err` and `unwrap_or_default()` yields the 1-byte `"\0"` allocation — but `len` is still the original `text.len()`. The dylib (and `pm_to_tokens` on output) reads `len` bytes from a 1-byte buffer → out-of-bounds read / UB.
- Fix: Compute the length from the buffer that is actually sent, and reject NULs explicitly:
```rust
match CString::new(text.as_str()) {
    Ok(c) => { owned.push(c); len = text.len() }
    Err(_) => { owned.push(CString::default()); len = 0 }
}
```
and use that `len` for `PmStr` (or replace the NUL with U+FFFD before interning). Requires the `owned` vec from T087.
- Verify: `cargo test -p glyim-proc-macro` + unit test registering a macro whose input contains `(SyntaxKind::StringLit, "a\u{0}b")` asserting no UB/correct round-trip under `cargo test --release` with debug-assertions on.

---

### T173 [DM-5] [Low] [PERF] Import fixed-point loop re-clones whole scopes per glob per pass

- File: `crates/glyim-def-map/src/lib.rs:542-566, 374`
- Code:
```rust
loop {
    for (node, module, vis) in &use_decls {
        process_use_decl(node, *module, &mut modules, &interner, vis.clone());
    }
    let new_count = scope_entry_count(&modules);
    ...
}
```
with `import_all_public_for_modules` starting at `let source_scope = modules[source].scope.clone();`
- Problem: Every pass re-processes **all** `use` decls; each glob clone-allocates the entire source scope (`IndexMap` clones) even when it settled in pass 1. Cost is O(passes × globs × scope-size); a crate with many globs over `std`-sized modules pays it per pass until the count stabilizes (guard at `modules.len() * 1024` allows many passes). Not unbounded, but pure churn.
- Fix: Track per-decl resolution success: keep `Vec<bool> settled` parallel to `use_decls`; skip a decl once its processing adds no new entries (compare scope count before/after that single decl), and only loop while any decl made progress.
- Verify: `cargo test -p glyim-def-map` (behavior unchanged — `u08_t13` must still pass) plus a `#[cfg(test)]` counter or timing on a many-glob fixture.

---

### T174 [HIRX-5] [Low] [BUG] `PatStruct` shorthand bindings take mutability from the **whole pattern node** — `let mut s = S { x, y }` marks every shorthand binding mutable

- File: `crates/glyim-hir/src/lower/lower_pat.rs:289-293` (with :20-47)
- Code:
```rust
} else {
    let binding_id = pats.push(Pat::Binding {
        name,
        mutability: pat_ident_mutability(node),   // ← node is the PatStruct
        subpattern: None,
    });
    fields.push((name, binding_id));
}
```
- Problem: `pat_ident_mutability` was written for `PatIdent` (scan own children, then the immediately-preceding sibling). Passing the parent `PatStruct` means: (a) any `mut` belonging to *another* field (`S { mut a, b }`) is found by the whole-node child scan, marking `b` `Mut`; (b) the preceding-sibling scan starts from the whole `PatStruct`, so `let mut s = S { x, y };` (KwMut before the pattern) marks **both** shorthand bindings `Mut` — immutable destructured bindings become mutable, letting `x = 5;` compile where it must not (or flapping the other way for `S { mut a }` fields).
- Fix: Call it on the field's own `PatIdent` node: in the `PatIdent` arm of `PatStruct`'s loop, pass `n` (the node already bound by the `SyntaxElement::Node(n) if n.kind() == SyntaxKind::PatIdent` match) — i.e. `mutability: pat_ident_mutability(n)` — and additionally *only* honor the has_mut_child half for struct fields (the preceding-sibling rule is only correct inside `LetStmt`).
- Verify: `cargo test -p glyim-hir lower_pat` + new test asserting `S { x, y }` under `let mut s =` lowers both bindings `Mutability::Not`, and `S { mut a, b }` lowers only `a` as `Mut`.

---

### T175 [HIRX-6] [Low] [MISCOMPILE] `PatIdent` starting with an uppercase letter is reclassified as a `Path` (variant/const) pattern — case-based, not resolution-based

- File: `crates/glyim-hir/src/lower/lower_pat.rs:59-67`
- Code:
```rust
if name_text.starts_with(|c: char| c.is_uppercase()) {
    let path = HirPath { segments: vec![PathSegment { name, generic_args: None }], kind: PathKind::Plain };
    Some(pats.push(Pat::Path(path)))
}
```
- Problem: Binding-ness is decided by capitalization. `let (A, b) = (1, 2);` or a match arm binding `Name => ...` turns the binding into a `Pat::Path` (unit-variant/constant pattern), so typeck resolves it as a variant/const and reports "no variant/const `A`" (or silently matches a same-named const instead of binding). Rust decides via resolution, not case.
- Fix: Keep the fast path as a *hint* only — lower uppercase idents as `Pat::Path` **and** have typeck fall back to treating a `Pat::Path` as a binding when the path fails to resolve to a unit variant/const (single segment, Plain kind). Mechanical version: in check_pat's `Pat::Path` arm, before erroring, check `path.as_name()` and if resolution failed, lower to a `Binding { name, mutability: Not, subpattern: None }` (single-segment only).
- Verify: `cargo test -p glyim-hir lower_pat` + run-pass `let (A, B) = (1, 2);` asserting both bind (or at minimum a precise diagnostic, not a variant-mismatch error).

---

### T176 [HIRX-7] [Low] [BUG] Trait generic parameters are hardcoded empty — `trait Iterator<T>` loses `T`

- File: `crates/glyim-hir/src/lower/lower_item.rs:857-865` (specifically :863)
- Code:
```rust
kind: ItemKind::Trait(TraitItem {
    associated_types,
    methods,
    generic_params: Vec::new(),
    where_clauses: collect_where_clauses(node, interner),
}),
```
- Problem: Every other item lowering uses `collect_generic_params(node, interner)`; traits hardcode `Vec::new()`, so a generic trait's parameter never reaches typeck — `trait Wrapper<T> { fn get(&self) -> T; }` leaves `T` unbound/unresolvable for the trait's method signatures and any `impl Wrapper<X> for Y` matching. `TraitItem.generic_params` exists as a field (lib.rs:234), so this is a one-line drop, not an infrastructure gap.
- Fix: Replace `generic_params: Vec::new(),` with `generic_params: collect_generic_params(node, interner),`.
- Verify: `cargo test -p glyim-hir` + new test `trait W<T> { fn get(&self) -> T; }` asserting `TraitItem.generic_params.len() == 1`.

---

### T177 [HIRX-8] [Low] [STUB] `static` items are never lowered into HIR — silently dropped everywhere

- File: `crates/glyim-hir/src/lower/mod.rs:254-361` (`_ => {}` in the item walk; no `SyntaxKind::StaticDef` arm), `lower_item.rs:983-1083` (module walk `_ => {}`); no production code constructs `ItemKind::Static` (only matched in `crates/glyim-lower/src/discovery.rs:53`)
- Code:
```rust
// Other item kinds (Trait, Use, Extern, etc.) are not yet lowered.
_ => {}
```
- Problem: The def-map declares `StaticDef` in the values namespace (lib.rs:911-917), but HIR has an `ItemKind::Static(StaticItem)` (lib.rs:117, :283-291) that nothing ever constructs — `static MAX: i32 = 10;` produces no HIR item, no diagnostic, and any use of `MAX` fails downstream with a misleading unresolved-name error. `lower_const_def` is the obvious template.
- Fix: Add a `SyntaxKind::StaticDef` arm in both walks calling a new `lower_static_def` cloned from `lower_const_def` (parse `is_mut` from a `KwMut` token sibling, build `StaticItem { ty, body, is_mut }`), pushing `ItemKind::Static` — then typeck/lower can register it like consts.
- Verify: `cargo test -p glyim-hir` + test `static MAX: i32 = 10;` asserting an `ItemKind::Static` item exists (currently panics/absent) and, after typeck wiring, `--emit=mir` shows the const read.

---

## 4.3 Type system

### T178 [TCK-28] [Low] [STUB] Object-safety check at `dyn Trait`: generic methods never detected (`has_generic_params: false` hardcoded) and receiver-less methods always a violation

- File: `crates/glyim-typeck/src/tyconv.rs:185-204`
- Code:
```rust
// Generic-parameter detection requires the trait
// method's own generic-param list, which is not
// recoverable from the interred `FnSig` substitution
// here. We conservatively assume no generic params; ...
has_generic_params: false,
```
- Problem: `trait It { fn next_of<T>(&mut self, x: T); }` used as `dyn It` passes object-safety (false negative → vtable for a monomorphization-impossible method → later codegen ICE). Conversely any trait with an associated function (`fn new() -> Self`) is rejected as non-object-safe even though Rust allows `dyn Trait` as long as the static method is not invoked (false positive).
- Fix: Thread `method.generic_params.len()` into `TraitDef::MethodDef` (available at `resolve_fn_sig` time) and use it for `has_generic_params`; treat `MethodSelfKind::None` as a violation only when the method lacks a `where Self: Sized` clause — or downgrade to a lint emitted only on call-through-dyn.
- Verify: `trait T1 { fn m<X>(&mut self, x: X); } let d: &dyn T1 = ...;` must error; `trait T2 { fn new() -> Self; fn run(&self); } let d: &dyn T2 = ...;` must compile.

---

### T179 [TCK-29] [Low] [STUB] Universal `.into()` / `.to_string()` / `.clone()` accepted on any receiver with no bounds

- File: `crates/glyim-typeck/src/check_expr.rs:2482-2507`
- Code:
```rust
if mn == "into" {
    let var = self.infer.new_ty_var(self.ctx);
    let out_ty = self.ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let fn_id = FnDefId::from_raw(u32::MAX - 1);
    return Some((out_ty, fn_id));
}
if mn == "to_owned" || mn == "to_string" {
    let out_ty = self.ctx.mk_ty(TyKind::String);
```
- Problem: Any receiver type gets `to_string() → str-typed`, `clone() → receiver type`, `into() → unconstrained` regardless of `ToString`/`Clone`/`From` impls — `Mutex.to_string()` type-checks; an unconstrained `.into()` silently defaults via zonk (`?T → i32` fallback or `Ty::ERROR`). Non-capturing-closure and non-Clone types are accepted where Rust requires bounds.
- Fix: Keep the synthetic ids only when a `ToString`/`Clone`/`From` impl (or structural Copy) exists for the receiver (consult `impl_method_fns`/coherence registry); otherwise fall through to the normal "no method" path. Pin `.into()` by requiring context (error if the var is unresolved at zonk).
- Verify: `struct X; fn main(){ let _s = X.to_string(); }` must error; stdlib `to_string` call sites stay green.

---

### T180 [TCK-30] [Low] [BUG] Explicit `i32`/`isize` literal suffix is silently ignored (SOLVE-14 still present)

- File: `crates/glyim-typeck/src/unify.rs:1056-1061` (with `crates/glyim-hir/src/lower/lower_expr.rs:1082-1086` which maps both unsuffixed and `i32`/`isize` suffixed literals to `Some(IntTy::I32)`/`Some(IntTy::Isize)`)
- Code:
```rust
Literal::Int(_, Some(IntTy::I32))
| Literal::Int(_, Some(IntTy::Isize))
| Literal::Int(_, None) => {
    let var = infer.new_int_var(ctx);
    ctx.mk_ty(TyKind::Infer(InferVar::Int(var)))
}
```
- Problem: `5i32` becomes an integer inference var indistinguishable from `5`, so `let x: u64 = 5i32;` and `fn f(x: u8){} f(5i32)` are accepted (Rust rejects; the suffix is a type annotation). Explicit suffixes on non-default types (`5u8`) do stay concrete.
- Fix: Make the HIR distinguish "unsuffixed" from "suffixed i32": change `lower_literal_with_diags` to emit `Literal::Int(value, None)` when there is no suffix and keep `Some(IntTy::I32)` only for a real `i32` suffix; then remove `Some(I32)/Some(Isize)` from the infer-var arm.
- Verify: `let x: u64 = 5i32;` must error; `let x: u64 = 5;` must still compile.

---

### T181 [TY-27] [Low] [BUG] Interior-mutability marking is registration-order dependent (Pass-1 empty defs make it permanent)

- File: `crates/glyim-type/src/ty_ctx_mut.rs:746-754 + 1367-1434`; order established by `crates/glyim-typeck/src/lib.rs:486-509` (Pass 1 registers every ADT with an empty `AdtDef`, Pass 2 re-registers in source order)
- Code:
```rust
self.adt_generation += 1;
self.interior_mutability_cache.clear();
if self.compute_adt_interior_mutability(id) {
    self.mark_adt_interior_mutable(id);
}
```
- Problem: `struct A { b: B }` declared before `struct B { c: Cell<u8> }`: at A's Pass-2 registration B is still the empty Pass-1 def (no fields, unmarked) → A computes `false` and is never re-marked (the cache clear doesn't recompute *parents*, and the arena's per-`Ty` `HAS_INTERIOR_MUTABILITY` flag is computed once at intern time and never invalidated).
- Fix: After typeck's Pass 2 completes, iterate `adt_def_ids()` and recompute + re-mark interior mutability to a fixpoint (or in `compute_adt_interior_mutability_rec`, treat "child registered" as unknown-and-retry later instead of caching `false`).
- Verify: unit test mirroring `ty_ctx_mut.rs:2967-3026` but registering A before B: `is_interior_mutable_adt(A)` must be true.

---

### T182 [TY-28] [Low] [BUG] `is_valid_cast` asymmetries and leniencies: `&T as i32` accepted but `&T as usize` rejected; ptr→any-width int; `as T` is an unchecked transmute via `Param`

- File: `crates/glyim-type/src/cast.rs:47-61, 73, 111-112`
- Code:
```rust
(RawPtr(_, _) | Ref(_, _, _), RawPtr(_, _) | Int(_)) => true,
...
(_, Param(_)) => true,
(Param(_), _) => true,
```
- Problem: `&v as i32` type-checks (Rust rejects ref→int entirely) while `&v as usize` is rejected (the first match's `Int(_) | Uint(_)` arm covers only `RawPtr` sources) — inconsistent; `ptr as i8` is accepted at any width; `(_, Param(_))` makes `x as T` an unchecked transmute for any generic `T` (documented as intentional for the stdlib, but it also silently permits `String as T` in user generic code).
- Fix: Restrict the ref/ptr→int arm to `Uint(Usize) | Int(Isize)` and require `RawPtr` (drop `Ref` from the source set — refs must go through an explicit `as *const T` step); gate the `Param` transmute arms behind `unsafe` blocks if the grammar can see them, otherwise at minimum document and keep.
- Verify: `let r = &5; let _x = r as i32;` must error; `let p = &5 as *const i32; let _n = p as usize;` must compile; `cargo test -p glyim-type` green.

---

### T183 [TCK-31] [Low] [PERF] Method resolution re-scans all HIR items per autoref/deref step per call, re-resolving impl self types each time; `std::env::var` in hot loops; snapshot deep-clones all four tables per probe (SOLVE-21/LL-16 still present)

- File: `crates/glyim-typeck/src/check_expr.rs:2763-2792` (`collect_for` per step), `2937-2946` (`std::env::var("GLYIM_DBG_CAND_PUSH")` per candidate push), `2856` (`infer.snapshot()` per impl probe); also `crates/glyim-typeck/src/unify.rs:952` (`GLYIM_DBG_VEXPR`), `crates/glyim-type/src/ty_ctx_mut.rs:714/772/2561/2653` (`GLYIM_DBG_*` in `register_adt`/methods); `InferenceTable::snapshot` (infer.rs:78-86) clones all `IndexVec`s
- Code:
```rust
for &step in steps.iter().chain(autoref_steps.iter()) {
    let found = collect_for(self, step);     // full HIR scan per step
    ...
}
```
- Problem: O(method_calls × steps × impls × resolve_type_ref) with `resolve_type_ref` re-interning impl self types every probe; five `env::var` syscalls in per-expression/per-registration hot paths; per-probe full-table clones.
- Fix (3 steps):
  1. Build once per `typeck_crate` a `HashMap<(AdtId|prim tag, Name), Vec<impl_idx>>` of method names → impls (SOLVE-20/LL-16's prescribed fix).
  2. Read debug flags once into `static DBG: OnceLock<DebugFlags>`.
  3. Make `snapshot` copy only entries written since a mark counter (or store `len`-based truncation points per table).
- Verify: `cargo test --workspace` green; time `--emit=mir` on the assembled stdlib before/after (expect a measurable drop in typeck time).

---

## 4.4 Lowering / opts / borrowck

### T184 [OPT-3] [Low] [BUG] Array drop loop drops `place.local[idx]`, losing parent projections of the dropped place

- File: `crates/glyim-opt/src/drop_elaboration.rs:556-559`
- Code:
```rust
let elem_place = Place {
    local: place.local,
    projection: vec![ProjectionElem::Index(idx_local)].into_boxed_slice(),
};
```
- Problem: For `Drop { place: Place{local: x, projection: [Field(0)]} }` where field 0 is `[String; N]` (shape emitted by `generate_struct_drop_glue`, mono_cache.rs), the loop drops `x[idx]` — indexing the *struct* as an array — instead of `x.0[idx]`. Latent today (user bodies only drop bare locals; drop-glue bodies don't run `elaborate_drops` in the current pipeline), but it fires the moment projected array drops are elaborated.
- Fix: Build `elem_place` by appending `Index` to the original place's projections:
```rust
projection: place.projection.iter().cloned()
    .chain(std::iter::once(ProjectionElem::Index(idx_local))).collect()
```
- Verify: unit test in `crates/glyim-opt/tests/drop_elaboration.rs`: `Drop{[Field(0)]}` on `[String; 2]` field → assert the emitted per-element `Drop` place is `[Field(0), Index(_)]`.

---

### T185 [OPT-4] [Low] [PERF] `cfg_simplify` re-scans the whole CFG every merge iteration (O(B²) on goto chains)

- File: `crates/glyim-opt/src/cfg_simplify.rs:9-44`
- Code:
```rust
while changed {
    changed = false;
    let mut preds = vec![Vec::new(); blocks.len()];   // rebuilt every iteration
    ...
    for i in 1..blocks.len() { ... }                  // full rescan
```
- Problem: Each single-block merge triggers a full predecessor rebuild and a full block rescan; a body that is one long `Goto` chain (typical after match lowering of large enums) degrades quadratically.
- Fix: Keep `preds` incrementally (update only the merged block's predecessors' targets when splicing), or process blocks in reverse post-order once and re-enqueue only predecessors of changed blocks (same worklist pattern as borrowck/liveness.rs:156-186).
- Verify: `cargo test -p glyim-opt cfg_simplify`; timing harness on a generated 5k-block goto chain (`--emit=mir` on a large match) before/after.

---

### T186 [LOW-13] [Low] [PERF] `devirtualize` clones every monomorphized body via `Arc::make_mut` even when nothing changes

- File: `crates/glyim-lower/src/mono.rs:203-208`
- Code:
```rust
fn devirtualize(&self, body: &mut Arc<glyim_mir::Body>) {
    let Some(ty_ctx) = self.ty_ctx else { return };
    let body_mut = Arc::make_mut(body);   // clones the whole Body (refcount > 1)
```
- Problem: `mir_bodies` returns shared `Arc`s; `Arc::make_mut` deep-clones every block/statement/operand for **every** mono item even though `VirtualMethod` constants are rare. On large mono sets this is a full-program-size copy per item.
- Fix: Pre-scan for `MirConstKind::VirtualMethod` (any `Call` terminator func) and only call `Arc::make_mut` when found:
```rust
let has_vm = body.basic_blocks.iter().any(|b| matches!(
    &b.terminator.kind,
    TerminatorKind::Call { func: Operand::Constant(c), .. }
        if matches!(c.kind, MirConstKind::VirtualMethod { .. })));
```
- Verify: `cargo test -p glyim-lower mono_collect` still green; time `target/debug/glyim-cli stdlib-heavy.g --emit=llvm-ir` before/after.

---

### T187 [BCK-3 + docs] [Low] [BUG] move_analysis block-0 special case + stale validate.rs doc

- File: `crates/glyim-borrowck/src/move_analysis.rs:414,425`; `crates/glyim-opt/src/validate.rs:18-22` vs `crates/glyim-opt/src/lib.rs:50-54`
- Code:
```rust
// move_analysis.rs — guards `if bi != 0` so block 0 never receives
// incoming move/dead state
```
- Problem: (a) The `if bi != 0` guard is correct for entry-only block 0 today, wrong if a future pass ever adds a back-edge into block 0; drop the special case and rely on empty `predecessors[0]`. (b) validate.rs's module doc says the validator is "intentionally not wired into `optimize()`" but `optimize()` calls it and **panics** on failure (lib.rs:50-54) — update the doc; the panic-on-invalid is intended ICE behavior.
- Fix: Remove the `bi != 0` special case (keep an `assert!(predecessors[0].is_empty())`); rewrite the validate.rs doc comment to say the validator runs at the end of `optimize()` and panics on invalid MIR (ICE by design).
- Verify: `cargo test -p glyim-borrowck -p glyim-opt` green.

---

## 4.5 LLVM backend, bytecode, glyip

### T188 [LL-21] [Low] [BUG] Debug info: compile unit and every subprogram hardcoded to "test.g", file 0, line 1

- File: `crates/glyim-codegen-llvm/src/debug.rs:52-56`; `crates/glyim-codegen-llvm/src/lower.rs:4191-4193`
- Code:
```rust
module.create_debug_info_builder(true, DWARFSourceLanguage::Rust,
    "test.g", ".", "glyim", ...);          // CU filename hardcoded
...
di.set_function(context, &function, &fn_name, FileId::from_raw(0), 1);  // every fn @ file 0, line 1
```
- Problem: With `--debug`, every function's DISubprogram claims `test.g:1` regardless of the real source file/line (only statement-level DILocations are correct). Backtraces/`llvm-symbolizer` point at the wrong file for every frame.
- Fix: Pass the body's primary `Span` (from `body.span` / first block's `source_info`) into `set_function` and create the CU from `source_map`'s real entry path instead of `"test.g"`.
- Verify: `glyim-cli prog.g --emit=exec --debug && llvm-dwarfdump --debug-line prog` — CU name is "test.g" before fix.

---

### T189 [LL-23] [Low] [BUG] Array count truncated to `u32` in `llvm_type_for_ty` (≥2³² wraps to a 0-element ZST array); negative `Int` counts accepted

- File: `crates/glyim-codegen-llvm/src/types.rs:70-82`
- Code:
```rust
let n = match &count.kind {
    glyim_type::ConstKind::Uint(n) => *n as u32,   // u64→u32 truncation
    glyim_type::ConstKind::Int(n)  => *n as u32,   // negative → huge u32
```
- Problem: `[u8; 4_294_967_296]` lowers to `[i8; 0]` (size 0 → ZST semantics, silent miscompile); a negative count (`ConstKind::Int`) casts to a ~4-billion element array. Layout (`layout_array`) uses u64 with checked_mul and would disagree with the LLVM type.
- Fix: Match the layout crate: `u64::try_from(*n)` → error on overflow/negative, and reject `n > u32::MAX` with a diagnostic (`LayoutError::SizeOverflow` equivalent) instead of truncating.
- Verify: `--emit=llvm-ir` for `fn main() { let a = [0u8; 4294967296]; }` — IR shows `[i8; 0]` before fix, a diagnostic after.

---

### T190 [BC-7] [Low] [PERF] `intern_string`/`intern_fn` are O(n) linear scans (old RT-16, still present)

- File: `crates/glyim-codegen/src/lib.rs:471-491`
- Code:
```rust
fn intern_string(&self, s: &str) -> u32 {
    let mut table = self.string_table.borrow_mut();
    for (i, existing) in table.iter().enumerate() { if existing == s { return i as u32; } }
```
- Problem: Every string/fn constant in every body rescans the whole table → O(consts²) codegen time on programs with many literals/fns.
- Fix: Maintain a side `RefCell<HashMap<String, u32>>` (and `HashMap<(FnDefId, Substitution), u32>`) next to the Vecs; insert and look up in O(1).
- Verify: compile a corpus with ~10k distinct string literals before/after and compare `--emit=bytecode` wall time (quadratic → linear).

---

### T191 [GLYIP-5] [Low] [BUG] Fingerprint "config files" watch the wrong filenames; `Glyip.lock` is never watched

- File: `crates/glyip/src/fingerprint.rs:294-315` (vs config.rs:8 `Glyip.toml`, lockfile.rs:11 `Glyip.lock`)
- Code:
```rust
let mut files = vec![
    dir.join("glyim.toml"),
    dir.join("glyim.lock"),
    dir.join("build.g"),
    dir.join("build.rs"),
];
```
- Problem: The real names are `Glyip.toml`/`Glyip.lock`. The listed four don't exist; `Glyip.toml` is only covered *accidentally* by the later `*.toml` glob (`p.file_name() != "glyim.toml"` is true), and `Glyip.lock` — not a `.toml` — is never fingerprinted, so plan §23.3 ("a manifest or dependency edit must invalidate incremental state") is only half-true and rests on the glob coincidence.
- Fix: List `dir.join("Glyip.toml")`, `dir.join("Glyip.lock")` (drop `glyim.*`/`build.*` or keep both spellings), and exclude `Glyip.toml` from the generic toml glob by exact name to avoid double-fingerprinting.
- Verify: build a project, `touch Glyip.lock` (or rewrite it via a `glyip update`), `glyip build` → before fix prints "skipping compilation"; after fix recompiles.

---

### T192 [GLYIP-6] [Low] [BUG] Locked registry URL is hardcoded `https://index.glyim.dev` regardless of the actual source

- File: `crates/glyip/src/dep.rs:604-606` and `651-653`
- Code:
```rust
source: CrateSource::Registry {
    url: "https://index.glyim.dev".to_string(),
    checksum,
},
```
- Problem: The URL is a constant even when the crate came from the *local* index directory (`resolve_registry_dep`'s first arm, no client involved) or from an `HttpRegistryClient::new(custom_url, …)` whose `base_url` differs. Lockfile provenance (and any future re-download keyed on that URL) is wrong.
- Fix: Store the resolved origin: for the local-index arm use e.g. `"local-index"` or the index dir; for the remote arm add `base_url()` to the `RegistryClient` trait and return it from `fetch_index` context (or construct `LockedCrate` with `client.base_url().to_string()`).
- Verify: unit test with a mock client pointing at `http://localhost:1` → locked source URL should be that, not index.glyim.dev.

---

### T193 [GLYIP-7] [Low] [BUG] Same-line `#[test] fn foo()` declarations are not discovered

- File: `crates/glyip/src/test_discovery.rs:57-64, 90-99`
- Code:
```rust
if trimmed == "#[test]" {
    pending_test_attr = true;
    continue;
}
```
- Problem: The attribute only registers when it is the *entire* trimmed line. `#[test] fn foo() { ... }` on one line fails both detectors (`extract_fn_name` sees the line starting with `#`, not `fn `), so the test is silently skipped by `glyip test`.
- Fix: Before the equality check, also test `trimmed.starts_with("#[test]")` and set `pending_test_attr = true` while letting the line fall through to `extract_fn_name` (which then needs to skip a leading `#[test]` prefix — strip any leading `#[…]` tokens before matching `fn `).
- Verify: file with `#[test] fn smoketest() {}` → `glyip test` reports total ≥ 1; today 0.

---

## 4.6 Stdlib, harness, span, diag

### T194 [STD-18] [Low] [PERF] `BufWriter::write` thresholds use `Vec::capacity()`, which drifts as the Vec grows

- File: `crates/glyim-lang-std/lib/io.g:478-488`
- Code:
```glyim
if self.buf.len() + buf.len() > self.buf.capacity() { self.flush()?; }
if buf.len() >= self.buf.capacity() { self.inner.write(buf) } else { self.buf.extend_from_slice(buf); ... }
```
- Problem: After the first flush + regrow, `Vec::capacity()` reflects RawVec's power-of-two growth (e.g. 512), not the configured 8192, so BufWriter flushes every few hundred bytes — exactly the anti-pattern it exists to prevent. Also `Vec::with_capacity` doesn't exist in vec.g (it's a compiler builtin, ty_ctx_mut.rs:2343), so the whole type depends on builtin wiring.
- Fix: Store `capacity: usize` in `BufWriter` at construction and compare against that.
- Verify: micro-benchmark/count of `inner.write` calls writing 1KB × 8192 — must be ~1 flush.

---

### T195 [STD-19] [Low] [STUB] `Option::copied/cloned` are identity no-ops

- File: `crates/glyim-lang-core/lib/option.g:192-200`
- Code:
```glyim
fn copied(self) -> Option<T> where T: Copy { self }
```
- Problem: In Rust these convert `Option<&T> → Option<T>`; here they take and return `Option<T>` unchanged, advertising semantics they don't implement (a `&T`-based impl would need a distinct self-type the language may not express yet — if not, they should be removed).
- Fix: Delete both, or implement on `Option<&T>` when the compiler supports it.
- Verify: grep corpus for `.copied()`/`.cloned()` uses; compile-fail or removal test.

---

### T196 [HARNESS-9] [Low] [BUG] Substring output oracle + no-op normalization

- File: `crates/glyim-test/src/harness/runner.rs:229-245`; `strategy.rs:112-113`; `comparison/normalize.rs:15-30`
- Code:
```rust
if let Some(expected) = &self.expected_stdout
    && !result.stdout.contains(expected.as_str()) { ... }        // substring oracle
...
crate::comparison::normalize::normalize_output(&text, test_path, &Default::default());  // all rules off
```
- Problem: `check-stdout: 1` matches `123` (partial-output false passes); exit codes use `unwrap_or(-1)`. `UiTestStrategy` invokes `normalize_output` with `Default::default()` — `normalize_slashes/normalize_line_endings/substitute_dir` all false, so the normalizer is a no-op and snapshot portability relies solely on `format_diagnostics`' `$FILE` substitution.
- Fix: Anchor output checks (`lines().any(|l| l == expected)` or exact-equals option), enable `normalize_line_endings: true` (and `$DIR` when paths are ever emitted) in `UiTestStrategy`.
- Verify: unit test in `crates/glyim-test/src/tests/comparison_tests.rs` asserting `"1"` does not match `"123"` after the change.

---

### T197 [SPAN-2] [Low] [BUG] Hygiene: no mark dedup, `HygieneKey` is a no-op, cross-context `remove_mark` silently mis-resolves

- File: `crates/glyim-span/src/hygiene.rs:99-128, 160-172`
- Code:
```rust
pub(crate) fn from_hygiene_key(_key: HygieneKey, raw: u32) -> Self { let _ = _key; SyntaxContext(raw) }
```
- Problem: Applying the same mark twice mints two distinct contexts (rustc dedups), so `adjust` walks farther than needed and context counts explode; the `HygieneKey` namespacing mechanism is ignored, so two `HygieneCtx` instances (macro-batch A and B) produce indistinguishable ids — `remove_mark` on the wrong instance silently returns an unrelated parent context instead of erroring.
- Fix: Dedup `(parent, expn_id, transparency)` in `apply_mark` via a HashMap; store the key in ids or debug_assert instance identity in `remove_mark`.
- Verify: unit test applying the same mark twice yields one context and `expn_data` lookups stay consistent.

---

### T198 [SPAN-3] [Low] [BUG] Span invariants are debug-only; cross-file merges and `lo > hi` survive in release

- File: `crates/glyim-span/src/lib.rs:75-78, 96-98, 107-118`
- Code:
```rust
debug_assert!(lo <= hi, "Span lo > hi");
debug_assert_eq!(self.file, other.file, "Cannot merge spans from different files");
```
- Problem: In release builds a `lo > hi` span yields `len() == 0` (saturating) and cross-file `to()` merges silently produce spans attributed to the wrong file — diagnostics then point at garbage locations in whichever file id is kept. `ByteIdx` is u32 with no checked construction, so macro expansions concatenating offsets can overflow silently on >4 GiB inputs.
- Fix: Make the file equality a real `assert` (or carry per-file merging into `MultiSpan`), and add a checked constructor used by macro span arithmetic.
- Verify: unit test calling `Span::to` with two files panics in release (`cargo test --release`).

---

### T199 [DIAG-2] [Low] [BUG] miette rendering drops suggestions and flattens severities

- File: `crates/glyim-diag/src/lib.rs:117-125, 208-212`
- Code:
```rust
fn help<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
    self.suggestions.first()
        .map(|s| Box::new(s.message.clone()) as Box<dyn fmt::Display>)
}
```
- Problem: Only the first suggestion is surfaced as `help` (the rest — e.g. multi-step quick-fix hints — are invisible in every miette-rendered output), `Note`/`Help` both collapse to `Advice` (severity info lost in terminal output), and `Display for GlyimDiagnostic` prints `@lo..hi` with no FileId, so two files' spans are indistinguishable in the human format.
- Fix: Join all suggestion messages (`suggestions.iter().map(|s| &s.message).join("\n")`), map `Help` → `miette::Severity::Advice` but keep `Note` distinguishable via a label prefix, and include `span.primary.file.to_raw()` in the Display.
- Verify: unit test rendering a diagnostic with two suggestions shows both.

---

## 4.7 glyim-pilot & extension (lows)

### T200 [PILOT-18] [Low] [BUG] CLI misc: `compute_waves` panics on unknown upstream; dead config knobs; hardcoded HTTP port

- File: `tools/glyim-pilot/src/cli/agent.rs:363` (`id_to_wave[u]`), `src/config/types.rs:83-92, 187-189` (`max_turns`, `auto_execute`, `retry_on_rate_limit`, `retry_max_wait`, `require_confirmation` — zero readers), `src/main.rs:122` + `agent.rs:104,149,396` (hardcoded `8421`)
- Code:
```rust
let max_upstream_wave = upstream.iter().map(|u| id_to_wave[u]).max().unwrap_or(0); // KeyError-style panic
```
- Problem: A `streams.json` entry listing an upstream id that doesn't exist (typo, stale brief) panics the CLI with a HashMap index instead of a readable error. Four config knobs (incl. `require_confirmation`, which spec NFR-SEC-005 turns into "dangerous patterns require human confirmation") are parsed and then **never read** — silent no-ops that mislead operators; `max_turns` is likewise unenforced (turns never counted toward it). The HTTP control port `8421` can't be configured and isn't in `.glyim-pilot.toml` schema; spec's `script_timeout` key is silently ignored by serde.
- Fix (3 steps):
  1. `let wave = upstream.iter().map(|u| id_to_wave.get(u).copied().unwrap_or_else(|| anyhow::bail!("stream {id} references unknown upstream {u}")))` (make the closure return `Result`).
  2. `#[serde(deny_unknown_fields)]` on `PilotConfig` (or a warn-on-unknown pass) so stale keys fail loudly; implement or delete `require_confirmation` (minimum viable: when `!= "never"`, have `apply_ops` results echoed through a CLI y/n prompt before `commit_all`); enforce `max_turns` in `turn.rs` via `s.turn >= config.defaults.max_turns → Escalate`.
  3. Add `[server] http_port` config, default 8421, used everywhere.
- Verify: `cargo test -p glyim-pilot compute_waves` with an unknown upstream → returns `Err`, no panic; config with `typo_key = 1` → startup error.

---

### T201 [EXT-8] [Low] [PERF/BUG] `WsClient` silently drops outbound messages when the socket is down; keepalive/ping plumbing is inconsistent

- File: `tools/glyim-pilot/extension/src/ws_client.ts:24, 7, 48-56`
- Code:
```ts
send(msg: ExtensionMessage): boolean { if (!this.ws || this.ws.readyState !== WebSocket.OPEN) return false; ... }
const PING_INTERVAL = 30000;                    // declared, never used (20000 hardcoded below)
this.send({ type: 'pong', timestamp: Date.now(), v: PROTOCOL_VERSION });   // 'pong' as keepalive
```
- Problem: Every `ops.ready`/`stream.complete`/`error.detected` produced while the WS is reconnecting (server restart, MV3 service-worker suspension) is **silently lost** — callers ignore the `false` return — so the server never learns a turn completed and the AI waits forever (compounding T151). The keepalive sends `pong` without a preceding `ping` (works, but only because the Rust log path tolerates it); `PING_INTERVAL` is dead; reconnect has no jitter; MV3 SW suspension kills `setTimeout`-based reconnect, and the top-level `ws.connect()` only re-runs on the next wake event.
- Fix: Add an outbound queue:
```ts
private queue: ExtensionMessage[] = [];
send(msg: ExtensionMessage): boolean {
  if (this.ws?.readyState === WebSocket.OPEN) { this.ws.send(JSON.stringify(msg)); return true; }
  if (this.queue.length < 50) this.queue.push(msg); return false;
}
// in onopen: for (const m of this.queue.splice(0)) this.ws?.send(JSON.stringify(m));
```
Use `PING_INTERVAL` for the timer, send `{type:'ping',...}`-style keepalive (or a WS-level Ping frame via server), add `+ Math.random()*250` jitter to the reconnect delay.
- Verify: with server stopped, run a watcher that completes a turn → start server → queued `ops.ready` delivered on reconnect (server log shows the message), no lost turn.

---

### T202 [X-1] [Medium] [BUG] Cross-crate: `mod_loader` ident scanner slices mid-UTF-8 — `mod café;` panics the compiler (ICE)

- File: `crates/glyim-pipeline/src/mod_loader.rs` `find_bodyless_mods` (lines ~221-235), `is_ident_boundary_before`
- Code:
```rust
// the ident scanner tests (bytes[j] as char).is_alphanumeric()
// and then slices &source[name_start..j]
```
- Problem: For a non-ASCII module name like `mod café;` the scan stops mid-UTF-8-codepoint and the str slice **panics** ("byte index is not a char boundary"). Same byte-as-char issue in `is_ident_boundary_before`. ICE on user input, found by agent 1-a as an out-of-scope observation and verified by reading the file.
- Fix: Iterate with `char_indices()` instead of raw bytes: collect the identifier by advancing over `char::is_alphanumeric()` chars, and slice only at returned char-boundary offsets.
- Verify: `printf 'mod café;\nfn main() {}\n' > /tmp/uni.g && glyim-cli /tmp/uni.g --emit=mir` — today panics; after fix a clean unresolved-module diagnostic (or successful load if `café.g` exists).

---

**Wave 4 checkpoint:** `cargo build --workspace && cargo test --workspace; git add -A && git commit -m "fix(wave-4): T164-T202 lows + perf"`

---

# Final regression checklist

After all four waves, run and record:

```bash
cargo build --workspace
cargo test --workspace                       # full suite
cargo test -p glyim-test --test run_pass_corpus
cargo test -p glyim-test --test no_ice_corpus
cargo test -p glyim-test --test compile_fail_corpus
cargo test -p glyim-bytecode-vm
cargo test -p glyim-pilot                    # tools workspace member
# If LLVM available, end-to-end smoke:
printf 'fn main() -> i32 { 4 | 1 & 3 }' > /tmp/smoke.g
target/debug/glyim-cli /tmp/smoke.g --emit=exec   # expect exit code 5 (tests T021 precedence)
```

Documentation to update once complete:
1. `docs/roadmaps/audit-status.md` — add this plan's IDs as verified-fixed (with evidence lines), and correct the wrong "HIR-10 verified FIXED" entry (see T004).
2. `README.md` "Tracked gaps" — remove items this plan closed (`?` operator T025, labels T048, struct field order T024, drop glue T005/T036/T037, glyip lockfile T057, LSP diagnostics T050).
3. Delete stale artifacts: the six `.snap.new` files (T002/T148), the `.pending` probe files once un-parked (T147).

Commit granularity: one conventionnal commit per file along the way for each fix
