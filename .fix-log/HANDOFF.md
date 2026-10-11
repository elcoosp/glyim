# HANDOFF — Glyim compiler, resume point (end of session 9)

**HEAD**: see `git log --oneline -1`
**Matrix**: 4599 passing / 0 failing / 2 skipped
**Tree**: clean (only `docs/roadmaps/better-testing.md` untracked — not mine)

---

## 0. Resume in 3 commands

```bash
cd /Users/adm/Documents/Repos/glyim-v2
cargo check --workspace --tests        # NOTE: --tests is REQUIRED
cargo nextest run --workspace --no-fail-fast
```

`cargo check --workspace` (without `--tests`) does NOT compile test targets and
will hide broken test files. Always use `--tests`.

For any `--emit=exec` / linker test, the runtime staticlib must exist:

```bash
cargo build -p glyim-runtime          # produces target/debug/libglyim_runtime.a
```

A `cargo check` alone does not build the `staticlib` crate-type.

---

## 1. Repo map (quick facts)

- Workspace root `Cargo.toml`; edition 2024, resolver 3.
- Pipeline: `frontend` → `hir` → `typeck`/`solve` → `lower` (THIR→MIR) →
  `opt` (const-prop, DCE, cfg-simplify, drop-elaboration) → `codegen`
  (LLVM + bytecode) + `mir-interp`; plus `lsp`, `runtime`, `pilot`, `glyip`.
- `.g` stdlib: `crates/glyim-lang-core/lib/*.g`,
  `crates/glyim-lang-alloc/lib/*.g`, `crates/glyim-lang-std/lib/*.g`.
- `.g` files are **embedded via `include_str!`** — editing one needs a
  `cargo build`, not just a re-link.
- Test harness `crates/glyim-test`: fixtures under `tests/run-pass/` (walked
  **recursively**), plus `run-fail/`, `compile-pass/`, `compile-fail/`.
  A fixture is run by `cargo nextest run -p glyim-test run_pass_corpus_passes`.
- A run-pass fixture header looks like:
  ```
  // test-mode: run-pass
  // exit-code: 3
  // compile-flags: --with-stdlib
  fn main() -> i32 { ... }
  ```

---

## 2. Two debugging rules that saved this session

1. **Dump what the interpreter ACTUALLY runs, then act.** Add a temporary
   `eprintln!` in `crates/glyim-test/src/harness/interpreter_runner.rs` (in the
   `for body in bodies { interpreter.add_function(..) }` loop) to print each
   body's `basic_blocks[].statements`. That is the *optimized* MIR the
   interpreter executes. Reasoning from source and patching forward led to many
   wrong-layer fixes; dumping found every real bug immediately.

2. **`--emit=mir` prints the UNOPTIMIZED MIR.** The harness runs `optimize()`.
   When they disagree, the optimizer is the suspect. Compare both:
   ```bash
   GLYIM_CACHE_DIR=/tmp/gc_x target/debug/glyim-cli FILE.g --with-stdlib --emit=mir -o /tmp/x.mir
   ```

Also useful: `GLYIM_DUMP_ITEMS=1` (item list), and grep the MIR for
`FnDefId(N)` to find which function a call targets.

---

## 3. Fixed and committed this session (7 commits)

| Commit | Fix |
|---|---|
| `1853419d` | Extern abort hooks (`glyim_panic`, `glyim_process_abort`) returned 0 instead of stopping. `try_call_extern`'s `_` fallthrough swallowed them. Now `InterpError::Panic`; `glyim_process_exit` carries its code. |
| `aa409971` | `mem::size_of`/`align_of`/`needs_drop` had no lowering (calls hit empty `.g` bodies). Added a `const_fns` name registry (`TyCtxMut`/`TyCtx`) populated in `pre_register_fn_sigs_in_module`, and folded the call in `lower_expr_to_rvalue` using the generic `T` from the callee's `FnDef` substitution + `SimpleLayoutComputer`. |
| `702ea7db` | (a) `lower_expr_to_place` had no `Unary(Deref)` arm → `*p = v` lowered as an rvalue (stored into a temp, never through the pointer). (b) A block **tail** that is statement-shaped (`unsafe { *p = 7 }`, `{ x = y }`) routed through `check_expr`'s `Expr::Assign` arm, which type-checks then **discards** the assignment. Both fixed. |
| `e7c13984` | Two optimizer aliasing bugs: (a) DCE's statement walk collected uses only from the RHS rvalue, never the LHS **place**, so a projected write (`p[Deref] = v`) let DCE delete the store feeding it. (b) const-prop removed only `place.local`'s constant, so `x=0; p=&x; *p=7; x` kept `x=0` folded — fixed in **both** the analysis and rewrite loops. |

Commits `2647142a`, `3c981214`, `b88742f0` are handoff docs (§4, §5).

**What works now** (fixtures under `tests/run-pass/`):
`str::len`/`contains`/`trim` (`str/`), `[T]::len` (`slice/`), `MaybeUninit`
(`core/`), `UnsafeCell::new` (`core/`), `mem::size_of`/`align_of` (`mem/`),
`ptr::read`/`ptr::write` + raw deref read/write (`ptr/`), extern abort hooks
(`core/abort_stops.g`), `hint::spin_loop`.

---

## 4. ROOT CAUSE FOUND, NOT FIXED — enum matches ignore field patterns

**Location**: `crates/glyim-lower/src/lower_rvalue.rs` — `lower_match` +
`collect_switch_values`.

**Bug**: an enum `match` compiles to a `SwitchInt` on the variant
**discriminant only**. `collect_switch_values`'s `PatternKind::Struct` arm
pushes `(variant_idx, arm_bb)` and **ignores the pattern's fields**. So:

```glyim
match r {
    Result::Ok(0) => 1,     // pushes (0, bb_a)
    Result::Ok(n) => n,     // pushes (0, bb_b)   <- same key
    Result::Err(_) => 9,
}
```

`SwitchInt { branches: [(0, bb_a), (0, bb_b), (1, bb_c)] }` — duplicate
discriminant `0`; the **first arm always wins** for any `Ok`. Every match that
discriminates on an enum **payload literal** is silently miscompiled.

**Why it matters**: `write_all` in `io.g` is
`match self.write(..) { Ok(0) => Err(WriteZero), Ok(n) => written += n, Err(e)
=> Err(e) }`. Arm 1 always wins, so **every successful write returns `Err`**.
`println` only "works" because `unwrap` on that `Err` calls `panic_any`, which
is a no-op stub. Enabling a real `panic_any` aborts — the observed failure.

**Design (implemented, then reverted — see below)**: *sequential arm testing*.
Each arm gets a test block and a body block; `test[i]` performs the
discriminant + field-literal tests (using `Discriminant(place)` and
`BinaryOp(Eq, field_place, literal)` in a chain of `SwitchInt` on `bool`), all
pass → `body[i]`, any fail → `test[i+1]` (or `otherwise`). This is O(arms)
instead of O(1) but correct, and matches are short. (Alternative: per-
discriminant arm chains — group arms by discriminant, then test fields.)

**Status**: an implementation of sequential arm testing **was written this
session and produced CORRECT MIR** — `Ok(0)` → `discr==0 && field0==0`,
`Ok(n)` → fallback — and three minimal probes passed (`Ok(0)`, `Ok(n)`,
`Some(0)`). **But it regressed all 7 `println` fixtures** with an abort traced
to `main`'s frame (fn 308) that could not be explained from the MIR. It was
**reverted** (`git checkout -- crates/glyim-lower/src/lower_rvalue.rs`).

**The open question for the next agent** (this is the whole task): *why does
sequential match lowering abort in `main`'s frame during `println`, when the
identical shape works for a standalone match?* The reverted implementation is
in `git log` — look for the commit before `2647142a` that touched
`lower_rvalue.rs` with a "MATCH-FIELD-TESTS" marker (grep the reflog / `git
log --all -- crates/glyim-lower/src/lower_rvalue.rs`). Re-apply it, reproduce
the abort, and trace it — the `TMP-*` instrumentation approach in §2 is what
will find it.

---

## 5. Remaining stubs — each needs a FEATURE, not a patch

| Item | Blocker / design |
|---|---|
| `ptr::add` / `sub` / `addr` / `is_null` | `check_expr.rs` (~line 2871) recognizes these on a `RawPtr` receiver but returns dispatch `None`; the `None` arm of `match dispatch` (in `Expr::MethodCall`) then emits an **indirect call** on the receiver → "indirect call through non-function value". **Design**: add a `MethodDispatch::PtrArith { op }` (or a Builtin id) so lowering emits `Rvalue::BinaryOp(Add/Sub, ptr, byte_offset)` scaled by `size_of::<T>()` (now available, see `aa409971`); interpreter handles pointer ± integer. ~half a day. **BLOCKS `ptr::copy`.** |
| `ptr::copy` / `copy_nonoverlapping` | byte-memcpy; either a pure-Glyim byte loop (needs `ptr::add` first) or an intrinsic. |
| `ptr::drop_in_place` | route to the pointed-to type's drop glue (backend already emits scope-drop glue). |
| `Chars::next` / `Lines::next` | unicode scalar decoding + `&str` iteration state. `Chars::next` currently fails with "invalid cast". |
| `UnsafeCell::get` (runtime use) | `&self.value` records `InterpValue::Ref { frame, local }` into the **callee** frame, dead on return ("deref of Ref from dead frame"). `Ref` carries **no projection**, so a reference to a *field* can't outlive the frame. Needs `InterpValue::Ref` to carry a projection path — a core representation change touching every `Ref` producer/consumer. |
| `panic_any` | **Blocked on §4.** Once `Ok(0)` vs `Ok(n)` is distinguished, `write_all` stops returning `Err`, `unwrap` stops panicking, and a real `panic_any` (route to `glyim_process_abort`) can be enabled. `glyim_process_abort` itself works — `tests/run-pass/core/abort_stops.g` proves it. |

---

## 6. Original plan context

The overarching plan is `docs/roadmaps/glyim-fix-plan.md` (202 tasks T001–T202,
4 waves). Waves 1–4 were done in sessions 1–4 (114 commits). Sessions 8–9 have
been working **T140 [STD-15]** — bodyless "compiler intrinsic" stubs.

Status markers in the tree: `-PATCHED` on ~111 files, `-REVIEWED` 2,
`-REVISED` 1, `-DEFERRED` on `str.g` (now largely done).

Other useful docs:
- `docs/roadmaps/audit-status.md` — prior audit status (different ID namespace).
- `docs/roadmaps/better-testing.md` — a test-coverage plan (untracked; not mine).
- `.fix-log/HANDOFF-session-4-archive.md` — the session-4 handoff, preserved.

---

## 7. Conventions

Marker comments in source: `T###-PATCHED`, `T###-PATCHED-<site>`,
`T###-REVIEWED` (rejected as incorrect), `T###-REVISED`, `T###-DEFERRED`.
Commit style: `fix(wave-N): TIDs (brief)`, `feat(...)`, `perf(...)`,
`chore(...)`, `docs(session): ...`, `test(area): ...`.

---

## 8. First recommended action

Re-apply the reverted enum-match fix (§4), reproduce the `println` abort, and
trace it. That single fix unblocks `panic_any` and fixes a genuine silent
miscompile. Everything else in §5 is downstream of it or independent.
