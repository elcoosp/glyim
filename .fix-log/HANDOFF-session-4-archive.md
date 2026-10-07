# HANDOFF — Glyim compiler fix plan, sessions 1-4

**Generated at**: end of Session 4
**HEAD**: 86376644 (`docs(session): session 4 final handoff — 20 deferred items completed`)
**Baseline**: `af19152b` (start of Session 1)
**Commits applied**: 114
**Workspace state**: clean, 0 compile errors, 3,175 tests passing, 0 failing across 20 crates.

---

## 1. What was the objective

Implement the `glyim-fix-plan.md` (at repo root; also `.txt` form) — a
199-finding, 4-wave plan to fix bugs, stubs, miscompilations, and
performance defects across the entire Glyim compiler workspace (~207k
lines of Rust across 30+ crates plus the `.g` stdlib, the `glyip` build
tool, `glyim-lsp`, and `glyim-pilot`).

The plan is organised into:
- **Wave 1** (16 criticals, T001-T016) — miscompiles, ICEs, segfaults.
- **Wave 2** (highs) — silent wrong-code, dead advertised features.
- **Wave 3** (mediums) — edge cases, latent bugs.
- **Wave 4** (lows + perf) — minor, plus performance defects.

Every task in the plan has an ID (`T001`…`T202`), an old-audit
cross-reference (`OPT-1`, `FE-102`, `LL-15`, etc.), a severity, a
category (BUG/STUB/MISCOMPILE/PERF), exact file paths + line anchors,
and a suggested minimal patch.

---

## 2. What has been completed

### Sessions 1-4 total: 114 commits

**Wave 1 (16/16 handled)** — all criticals addressed:
- T001 (`OPT-1`) const-prop ignores Call terminators → kill constants after Call.
- T002 (`FE-102`) PatIdent absorbs trailing whitespace → deferred-open node.
- T003 (`TCK-24`) call args unchecked — minimal FnDef arity guard applied (full unify deferred, see §4).
- T004 (`HIRX-1`) async root-block forward search → reverse search.
- T005 (`LL-15`) drop_in_place declared 1 arg, runtime expects 2 → fixed both sides.
- T006 (`PIPE-1`) all drop-glue bodies collided on `__glyim_fn_0` → unique synthetic owner.
- T007 (`PIPE-2`) array glue used element index as local → `ConstantIndex`.
- T008 (`BC-1`) `OP_SWITCH_INT` wire mismatch (u128 vs u64) → truncate to u64.
- T009 (`INT-2`) interp Drop arm had `debug_assert!(false)` → no-op + warn.
- T010 (`STD-4`) env.g FFI rewrite → REVERTED, runtime FFI kept (see §4).
- T011 (`STD-5`) process.g FFI rewrite → REVERTED, runtime FFI kept.
- T012 (`STD-6`) fs.g metadata wrong ABI → fd-based fstat + real lstat.
- T013 (`STD-1`) Mutex/RwLock never stored the value → `value: UnsafeCell<T>`.
- T014 (`STD-2`) atomics were load-store sequences → real FFI.
- T015 (`STD-3`) Condvar never woke waiters → real FFI.
- T016 (`PILOT-1`) WS session_id validation → added `validate_id`.

**Wave 2 (28/28 applied)** — the full set of highs:
T017-T022 (parser: `&&pat`, `&&expr`, `&&mut T`, `Vec<u8>::new()`,
bitwise precedence, negative patterns), T024 (struct-literal field
order), T038/T040/T041 (bytecode), T049 (move closures), T051-T053
(LSP), T060-T063 (stdlib), T066/T067 (pilot), T084 (builtin macros),
T085/T086/T087 (HIR, proc-macro), T088/T090 (types), T092/T093/T095
(solve), T097/T098/T099 (const-eval, lowering), T102 (substitute_body
Aggregate), T104-T108 (borrowck, opt, interp), T112-T119 (LLVM,
layout, bytecode, runtime, pilot), T121 (two-phase borrow).

**Wave 3 (24/24 applied)** — including all the well-scoped mediums:
T076/T077/T078 (parser), T080-T082 (def-map, macros), T105-T108
(opt/interp), T112-T122 (LLVM/runtime), T131-T138 (LSP, stdlib),
T142-T155 (stdlib, harness, pilot), T159/T160/T162/T163 (pilot, ext).

**Wave 4 (34/34 applied)** — all lows + perf:
T164-T177 (frontend/HIR), T180-T182 (types), T184-T187 (opt/borrowck),
T188-T196 (LLVM/glyip/harness), T198-T202 (span/diag/pilot).

### Additional deferred backlog completed this session (20 items)

These were marked "DEFERRED" during Waves 1-4 (in
`.fix-log/WAVE2-SKIPPED.md` and inline `-DEFERRED` markers). All but
three were implemented this session:

| Task | Fix |
|---|---|
| T053 | HIR `Body.pat_spans` end-to-end → LSP rename edits correct positions |
| T093 | `TyCtx::adt_name_for_id` on frozen context |
| T096 | Plan's claims about `&mut T: Sync` and raw pointers rejected (wrong per Rust) |
| T101 | End-to-end polymorphization dedup (pre-subst body analysis) |
| T104 | Terminator write/Drop conflict checks |
| T109 | Removed per-statement `Statement` clone in interp hot loop |
| T110 | `MonoItem::Static` fetches real body via own `LocalDefId` |
| T121 | Two-phase borrow activation for call args |
| T123 | `FullLayoutComputer::layout_of` memoized |
| T124 | `CrateDefMap.def_to_module` → per-item `source_module` |
| T125 | Nested inline modules resolve against parent path |
| T126 | `--lto fat` errors clearly instead of silent no-op |
| T129 | Scope-aware rename / find-references |
| T130 | HIR dependency walk + re-analysis of dependents |
| T139 | Strict `write_all` (`Ok(0)` → `WriteZero`) |
| T140 | Partial: `ptr::null`, `ptr::null_mut`, `NonNull::{new_unchecked,dangling}`, `mem::forget` |
| T159 | Server event loop spawns handler (non-blocking) |
| T175 | Single-segment unresolved `Pat::Path` → `Binding` |
| T178 | `TraitMethod.generic_params` → object-safety |
| T179 | Universal `into`/`to_string`/`clone` gated on receiver shape |

Plus two follow-ups that unblocked T101:
- **T063/T141** cleaned up: replaced `Vec::resize` / `Iterator::position`
  / `Option::unwrap_or` with manual loops in io.g / env.g because those
  methods weren't in the builtin method tables.

---

## 3. What remains — proper designs

Three items remain. Each was attempted and blocked for a documented
reason. Below is the proper design for each.

### 3.1 — T100 [LOW-9]: multi-poll async state machine

**Current state.** The HIR `desugar_async` pass (`crates/glyim-hir/src/
lower/lower_async.rs`) rewrites `async fn f() { ... }` into a `FooFuture`
struct + `impl Future for FooFuture { fn poll(..) { ... } }` whose body
contains the original code with each `.await e` expanded to:
```glyim
match e.poll(cx) {
    Poll::Ready(v) => v,
    Poll::Pending => panic!("async suspension not supported"),
}
```
This is a **single-poll** desugar: it works when every awaited future
resolves on first poll. Real suspension requires a state machine.

**The MIR-level analysis exists** (`crates/glyim-lower/src/
async_state_transform.rs`): `plan_async_transform(body)` returns an
`AsyncTransformPlan { sites: Vec<SuspendSite>, live_after: Vec<FixedBitSet>, variant_count, ... }`,
correctly identifying every `Future::poll` call terminator as a suspend
site. This plan is stored in `LowerResult::async_transform` but **no
consumer reads it**.

**Full design.**

1. **HIR layer** (`crates/glyim-hir/src/lower/lower_async.rs`):
   - Replace the `Pending => panic!(...)` arm with a state-saving and
     `Poll::Pending` return. Concretely:
     - Give `FooFuture` a `state: u32` field (or an enum per the plan's
       `S0..S_{n-1}, Done` names).
     - Each suspend site gets a `state` value from `plan_async_transform`.
     - On the `Pending` path: store every live local into the
       `FooFuture` struct's saved-state fields (one field per live local
       per site), write the next state to `self.state`, return
       `Poll::Pending`.
     - At the top of `poll`, dispatch: `match self.state { S0 => poll_s0(),
       S1 => poll_s1(), .. Done => panic!() }`.
   - The plan's `live_after[k]` gives exactly the locals to save at
     site `k`; the `from_state`/`next_state` fields give the numeric
     state values.

2. **MIR layer** (`crates/glyim-lower/src/lower.rs::lower_body`):
   - The `async_transform` plan is already computed. Add a post-lowering
     pass that, given the plan, splits the CFG: for each suspend site
     `S_k`, the block ending in the `poll` call gets:
     - A `SwitchInt` on the discriminant of the `Poll` result.
     - A "Ready" successor that continues to `plan_resume_arm(k).next_state`.
     - A "Pending" successor that stores state + live locals into the
       future struct (via MIR projections) and `Return`s `Poll::Pending`.

3. **Codegen** (`crates/glyim-codegen-llvm/src/lower.rs`,
   `crates/glyim-codegen/src/lib.rs`):
   - The state enum is just an ADT; existing ADT layout/codegen handles
     it. No new work beyond making sure the state field is included in
     the future struct layout (which the HIR change does).

4. **Interpreter** (`crates/glyim-mir-interp/src/lib.rs`):
   - The interpreter already supports calls and returns. The
     `Pending`-return path is just a normal `Return`. No changes
     needed — the interpreter can already run the transformed MIR.

**Test plan.**
- Fixture: `async fn add_one(x: i32) -> i32 { let b = if x > 0 { 1 } else
  { 2 }; x + b }` driven by `block_on` must return the right sum (any
  x, not a fixed value).
- Fixture: two awaited futures that each pend once must resolve
  correctly under `block_on`.

**Estimated effort.** 2-3 days of focused work; touching HIR, MIR, and
one or two pipeline plumbing points.

---

### 3.2 — T140 remainder: compiler intrinsics

**Current state.** The following functions have empty bodies ("compiler
intrinsic" markers):

| Function | File | Blocker |
|---|---|---|
| `str::contains(&str)` | `crates/glyim-lang-core/lib/str.g` | byte-range indexing of `str` returns `()` (Unit) in this compiler |
| `str::trim` | same | same |
| `Chars::next` | same | same |
| `mem::size_of<T>` | `crates/glyim-lang-core/lib/mem.g` | needs layout fold |
| `mem::align_of<T>` | same | same |
| `mem::needs_drop<T>` | same | needs `TyCtx::needs_drop` fold |
| `ptr::read` | `crates/glyim-lang-core/lib/ptr.g` | needs backend-specific intrinsic |
| `ptr::write` | same | same |
| `ptr::copy` | same | same |
| `ptr::copy_nonoverlapping` | same | same |
| `ptr::drop_in_place` | same | same |
| `MaybeUninit::uninit` | `mem.g` | needs a representation decision |
| `MaybeUninit::new` | same | same |
| `MaybeUninit::assume_init` | same | same |

Already implemented in this session: `ptr::null`, `ptr::null_mut`,
`NonNull::new_unchecked`, `NonNull::dangling`, `mem::forget`.

**Full design.**

**A. `mem::size_of::<T>()` / `mem::align_of::<T>()` — fold at MIR
lowering time.**

The `LowerCtx` already imports `glyim_layout::SimpleLayoutComputer`
(see `crates/glyim-lower/src/lower_rvalue.rs:2243` for the pattern
used by `lower_dynamic_range_slice`). The right place to fold these is
in `lower_expr_to_rvalue`'s `thir::ExprKind::Call` arm:

1. Detect when `func.kind` is `ExprKind::FnRef(def_id)` and
   `def_id.to_raw()` matches `size_of` / `align_of`. Since these are
   top-level fns (not builtin methods), we need to identify them by
   name. Add a name-based lookup: in `MirBuilder::new`, register the
   `FnDefId`s of `mem::size_of` and `mem::align_of` by resolving their
   paths through `self.ctx`. Alternatively: lower these fns to a new
   `MirConstKind::SizeOf(Ty)` variant that codegen handles.

2. The cleanest approach: add `MirConstKind::SizeOf(Ty)` and
   `MirConstKind::AlignOf(Ty)` variants. When lowering a call whose
   callee is one of these fns, emit a `Rvalue::Use(Constant(MirConst {
   kind: SizeOf(arg_ty), ty: usize_ty, span }))`. Codegen reads the
   variant and emits `layout.size.0` / `layout.align.0` as an integer
   constant.

3. Alternative (no new MIR variant): recognize the callee path in
   `lower_expr_to_rvalue` and directly emit `Rvalue::Use(Operand::
   Constant(MirConst { kind: Uint(size), .. }))`.

**B. `ptr::read` / `ptr::write` / `ptr::copy` / `ptr::copy_nonoverlapping`
/ `ptr::drop_in_place` — per-backend intrinsics.**

These are the same pattern as the already-handled
`glyim_drop_in_place` FFI. Design:

1. Add a new `MirConstKind::Intrinsic { kind: IntrinsicKind }` variant
   (or extend the existing `MirConstKind::Fn` path with a "this is
   an intrinsic" flag).

2. In `check_expr.rs`'s path-call resolution, when the resolved callee
   is `ptr::read` etc., emit a synthetic `FnDefId` in the reserved
   9_000+ range and register it in `TyCtxMut::register_builtin_methods`
   so codegen recognizes it.

3. In `glyim-codegen-llvm/src/lower.rs::try_lower_builtin_intrinsic`,
   extend the `matches!(method.as_str(), ...)` list with these names
   and emit:
   - `read`: `builder.build_load(llvm_ty, src_ptr, "read")`.
   - `write`: `builder.build_store(dst_ptr, src_val)`.
   - `copy`: `build_memmove(dst, src, size)`.
   - `copy_nonoverlapping`: `build_memcpy(dst, src, size)`.
   - `drop_in_place`: recursive field-wise drops (see `generate_drop_glue`
     in `mono_cache.rs` for the shape).

4. In `glyim-codegen/src/lib.rs` (bytecode), emit corresponding
   `OP_LOAD_MEM` / `OP_STORE_MEM` / `OP_MEMCPY` opcodes. If those
   don't exist, add them.

5. In `glyim-mir-interp/src/lib.rs`, handle these FnDefIds directly:
   read/write through `InterpValue::Ptr`, copy via byte-level memory.

**C. `str::contains`, `str::trim`, `Chars::next` — blocked on `str`
byte-range indexing.**

The root cause: `self[start..end]` on a `str` typechecks to `()`
(Unit) in this compiler. Fixing this requires either:
- Extending typeck's `Index` handling for `str` with a `Range<usize>`
  index to return `&str`, plus codegen for it; or
- Providing dedicated intrinsics for these functions (follow the
  pattern in B above, with codegen emitting memchr-like loops).

**D. `MaybeUninit`** — likely to fall out of A/B once those land: the
representation can be a transparent struct with a single field.

**Test plan.**
- Fixture for `size_of::<i32>() == 4`.
- Fixture for `ptr::read` / `ptr::write` roundtrip.
- Fixture for `mem::forget(String)` (already works as no-op).
- Fixture for `str::trim` / `str::contains` once (C) lands.

**Estimated effort.** ~1 day per family (A, B, C).

---

### 3.3 — T121 follow-up: precise two-phase borrow activation

**Current state.** Every `&mut x` produced while lowering a call
argument gets `allow_two_phase_borrow: true`. This is conservative —
the two-phase flag is set even when the borrow is used immediately
(e.g. `f(&mut x)` where no `x` reference is read during the call).
The `crates/glyim-borrowck/src/twophase.rs` reservation scan already
models the precision rustc uses: it computes which blocks each
reservation is still active in and marks the activation points.

**Full design.**

1. In `lower_rvalue.rs::thir::ExprKind::Call`, replace the flag-based
   coarse detection with a check: only set `allow_two_phase_borrow` on
   an argument-ref if the *sibling arguments* read the same place. This
   is a syntactic check: walk `args`, collect places that appear in
   shared-borrow or read positions, and if the current arg's `Ref` of a
   place `p` collides with a sibling read, mark it two-phase.

2. Additionally, feed the borrow list into
   `borrowck::twophase::ReservationAnalysis::compute` so the borrower
   knows which reservation each loan is. The plumbing exists
   (`ReservationAnalysis` is computed and consulted by
   `check_stmt_conflicts` and `check_terminator_conflicts`).

3. Alternative (simpler): leave the coarse version. The over-approximation
   only admits *additional* valid programs; it never rejects valid
   ones. It could accept a program that rustc rejects if the two-phase
   borrow covers a conflicting write — but `check_stmt_conflicts`
   already runs against the activation-time state, and mismatches would
   surface. Low priority.

**Estimated effort.** ~2-4 hours for (1), ~1 day for (2).

---

## 4. Items reverted or rejected with rationale

### T010 (`STD-4`) / T011 (`STD-5`) — env.g / process.g FFI rewrite

**Status**: reverted, runtime FFI retained.

**Reason**: the .g rewrites used patterns that hit a pre-existing
solver limitation:
- `ptr::null_mut::<u8>()` (turbofish on generic method) — typeck
  error: "mismatched types: T vs u8".
- `slice::from_raw_parts::<u8>(p, n)` — same issue.

The runtime FFI additions (`glyim_env_var` with out-pointer ABI,
`glyim_process_*_handle`, `glyim_fs_metadata_fd`) are present and
correct; only the .g call sites remain in the old (broken) form. When
the solver is fixed (or when a workaround is applied — see T010's
turbofish anchor), the .g side can be re-patched in a few lines.

**Test for the fix**: `env::var("PATH")` must not segfault.

### T096 (`TY-26`) — Send/Sync realignment

**Status**: rejected as incorrect per Rust semantics.

The plan claimed two things:
1. `&mut T: Sync` iff `T: Send` — WRONG. Rust's stdlib has
   `unsafe impl<T: ?Sized + Sync> Sync for &mut T`, so `&mut T: Sync`
   iff `T: Sync`.
2. Raw pointers `*const T` / `*mut T` are `Send + Sync` — WRONG. Rust's
   stdlib has `impl<T: ?Sized> !Send for *mut T` and `!Send for
   *const T`.

The compiler's current behavior matches Rust. Reverting the plan's
proposal restored the correct behavior and re-greened 22 auto-trait
tests.

**Genuinely useful part of T096**: register user `unsafe impl
Send for X {}` and `impl !Send for X {}`. Not implemented yet — the
parser doesn't accept `impl !Trait`, and `ImplItem` needs an
`is_negative` flag. Documented inline in
`crates/glyim-type/src/auto_trait.rs`.

---

## 5. Verification commands

```bash
# Compile sanity
cargo check --workspace

# Full test matrix (20 crates)
for crate in glyim-frontend glyim-def-map glyim-meta glyim-hir glyim-typeck \
             glyim-solve glyim-lower glyim-opt glyim-mir-interp glyim-codegen \
             glyim-codegen-llvm glyim-bytecode-vm glyim-const-eval glyim-span \
             glyim-diag glyim-lsp glyip glyim-pilot glyim-lang-std glyim-runtime; do
  cargo test -p "$crate" --lib 2>&1 | grep "^test result" | tail -1
done

# End-to-end smoke (stdlib compiles)
cargo test -p glyim-pipeline --test stdlib_full_probe

# Assert snapshot contract intact
cargo test -p glyim-test --lib snapshot
```

Expected totals: **3,175 tests passing, 0 failing**. **0 compile
errors**.

---

## 6. Where to look for more context

- **`docs/roadmaps/glyim-fix-plan.md`** — the plan itself. Every task
  listed here is documented in full there with original line numbers
  at commit `6bab26ed`.
- **`docs/roadmaps/audit-status.md`** — status of the *prior* audit
  (uses a different ID namespace: `FE-1`, `HIR-10`, `SOLVE-7`, etc.).
- **`.fix-log/SESSION-4-FINAL.md`** — session 4 handoff (the summary
  you're reading now supersedes it).
- **`.fix-log/SESSION-3-DEFERRED-IMPLEMENTATION.md`** — session 3
  details.
- **`.fix-log/WAVE2-SKIPPED.md`** — early deferrals (mostly superseded
  by session 4).
- **`.fix-log/T00*.before/after.txt`** — evidence dumps for the first
  wave.

Grep for `-PATCHED` or `-REVIEWED` or `-DEFERRED` in any source file to
find the marker comment associated with each fix:
```bash
grep -rn "T053-PATCHED\|T101-PATCHED\|T140-PATCHED" crates/ tools/
```

---

## 7. Conventions used throughout

Every fix leaves one of the following marker comments in source:

| Marker | Meaning |
|---|---|
| `T###-PATCHED` | Fix applied, verified. |
| `T###-PATCHED-XXX` | Sub-marker for a specific site (e.g. `-DECL`, `-CALL`, `-NAV`, `-TEST`, `-RUNTIME`, `-G` for `.g` files). |
| `T###-REVIEWED` | Plan's suggestion reviewed and rejected as incorrect; current code kept. |
| `T###-REVISED` | Fix was applied, then revised after tests broke; documents the revised approach. |
| `T###-DEFERRED` | Not implemented; rationale + design in the comment. |

Commit messages follow `conventional-commits`:
- `fix(wave-N): TIDs (brief)` for bug/correctness fixes.
- `feat(wave-N): TIDs (brief)` for feature additions.
- `perf(wave-N): TIDs (brief)` for perf-only changes.
- `chore(wave-N): TIDs (brief)` for cleanup / doc updates.
- `docs(session): ...` for session wrap-ups.

---

## 8. How to resume

1. `cd` to the repo root (`/Users/adm/Documents/Repos/glyim-v2`).
2. `git log --oneline af19152b..HEAD | wc -l` → should be **114**.
3. `cargo check --workspace` → should be clean.
4. Pick a remaining item from §3 above.
5. Follow the plan's task format: locate by path+anchor, apply the
   minimal fix, verify with the indicated test, commit with the marker
   convention.

If anything is unclear, the source-of-truth is `git log
af19152b..HEAD` — every commit message references the task ID and
describes what landed. Cross-check against the plan document itself
at `docs/roadmaps/glyim-fix-plan.md`.
