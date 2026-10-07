# HANDOFF — Glyim compiler fix plan (current)

**Updated at**: end of Session 5
**HEAD**: `248a454a` (`fix(codegen): lower synthetic sentinel calls + accept Error const as zero`)
**Baseline for this session**: `bd571466` (the session-4 handoff doc commit)
**Session-4 history**: preserved verbatim in `.fix-log/HANDOFF-session-4-archive.md`
**Workspace state**: clean, 0 compile errors, **4569 passing / 2 failing** (2 skipped) across 65 test binaries.

> **Correction notice.** The archived session-4 handoff claimed "3,175 tests passing, 0 failing". That was wrong twice over: the real suite is 4,571 tests, and there were **six** pre-existing failures at `bd571466` that the doc's verification commands could not see. See §5 for why, and §3 for what remains.

---

## 1. What this session changed

Six commits on top of `bd571466`:

| Commit | Kind | What |
|---|---|---|
| `f3592a8d` | test(layout) | Fix `MethodDef` test initializers after T178 added `has_generic_params`. **Pre-existing breakage** — `cargo check --workspace` never compiles test targets, so it hid. |
| `114bb5f8` | fix(lower) | T121 follow-up: per-argument two-phase borrow flags. |
| `e5e0212f` | test(cli) | Pin `--lto fat` **error** contract. T126 made `--lto fat` a tracked gap; the test still asserted success. **Pre-existing stale test.** |
| `fed55588` | revert | Revert `114bb5f8` — based on a wrong hypothesis (see below). |
| `f35321a5` | reapply | Restore `114bb5f8` after confirming the async failure it was blamed for is pre-existing. |
| `248a454a` | fix(codegen) | Lower synthetic sentinel `FnDefId` calls to an `Error` constant; accept `MirConstKind::Error` in LLVM codegen as a zero of the destination type. **Fixes 4 `emit_modes` tests + `println` via `--with-stdlib`.** |

### 1.1 The T121 revert/reapply (honest record)

`114bb5f8` was reverted on the hypothesis that it caused `async_state_machine_runs_via_interpreter` to fail. It did not: the test fails identically at `bd571466`, before any of this session's work. The revert was based on a misread, and `f35321a5` undoes it. Both the revert and its reversal are in history deliberately — the log should show the correction, not hide it.

The reverted-then-restored change is a *precision* refinement (only mark `&mut x` as two-phase when a sibling argument reads `x`). It is sound: it never marks fewer borrows than necessary in a way that rejects a valid program, because the fallback for any unrecognised expression shape is "assume it reads the place" (`expr_reads_local`'s `_ => true`).

---

## 2. Corrected status of the session-4 "remaining items"

The archived handoff listed three items as remaining. Two of them were **already implemented** when it was written.

### 2.1 T100 (async state machine) — ALREADY IMPLEMENTED, not remaining

`crates/glyim-hir/src/lower/lower_async.rs` contains three working desugars:
- `desugar_one_async_fn` — single-poll (0 or 1 `.await`),
- `desugar_multi_async_fn` — sequential multi-`.await` state machine (`Start`/`S0`…/`Done`),
- `desugar_loop_async_fn` — a resumable machine for one `.await` inside a `while` body.

The MIR-level analysis half (`crates/glyim-lower/src/async_state_transform.rs`) also exists with unit tests. **The archived handoff's §3.1 "full design" describes work that is done.** What is *not* done is runtime verification: `async_state_machine_runs_via_interpreter` fails (see §3.1).

### 2.2 T121 (two-phase borrow) — DONE

Basic flag landed in session 4 (`687ef927`). The follow-up (precise per-argument activation) landed this session as `f35321a5`. No further work planned.

### 2.3 T140 (compiler intrinsics) — PARTIALLY done, remainder re-scoped

Done this session:
- Synthetic sentinel `FnDefId` calls (`into`/`to_string`/`clone`/`drop`) no longer ICE. `248a454a`.

Still open, with corrected reasoning:
- **`mem::size_of` / `mem::align_of`** — the archived handoff framed this as "needs layout fold". That understates it. `thir::ExprKind::FnRef(FnDefId)` carries **no substitution**, so the generic argument `T` in `size_of::<T>()` is not on the callee expression. `ExprKind::FnRef`'s `.ty` *is* `TyKind::FnDef(id, Substitution)`, so `T` is reachable — but identifying *which* `FnDefId` is `mem::size_of` needs a name-keyed registry that does not exist. **New task: add a `FnDefId`→name/kind registry**, then fold these calls in `lower_expr_to_rvalue`.
- **`ptr::read`/`write`/`copy`/`copy_nonoverlapping`/`drop_in_place`** — design unchanged from the archived §3.2.B (per-backend intrinsics).
- **`str::contains`/`trim`/`Chars::next`** — still blocked on `str` byte-range indexing returning `()`.

---

## 3. Genuinely remaining — the 2 failures

Both are **confirmed pre-existing** (they fail identically at `bd571466`).

### 3.1 `glyim-pipeline::async_runtime` — interpreter reads uninitialized local 0

```
Panic("read from uninitialized local 0 (owner=Some(DefId { local_id: LocalDefId(10) }))")
```

The generated state-machine `poll` body reads local 0 before the wrapper has initialized it. This is a real bug in the M4 async work. It needs MIR-convention recon: in the interpreter, `arg_count` locals are `1..=arg_count` (`lib.rs:663`) — local 0 is the return slot, and `poll`'s receiver binding must be established before the dispatch `match self.state` runs. **Do not "fix" this by loosening the uninitialized-read check** — the check is correct; the generated MIR is wrong.

### 3.2 `glyim-test::run_pass_corpus` — 5 probe fixtures

The harness (`TestRunner`) does **not** inject the stdlib. It has no `with-stdlib` annotation (see `harness/collector.rs:105`). So these probes fail for four *different* reasons:

| Fixture | Reason | Kind |
|---|---|---|
| `probe_println_function.g` | `unresolved name println` | harness scope — needs stdlib injection or relocation |
| `probe_println_macro.g` | `unresolved name println` | same |
| `probe_string_push_str.g` | `unresolved name as_str`, `println` | same, plus a missing `String::as_str` |
| `probe_vec_push_len.g` | `mismatched types: usize vs i32` | **fixture is wrong** — `v.len()` returns `usize`, `fn main() -> i32` requires a cast |
| `probe_option_unwrap.g` | exit 101, `TyKind::Error` reaches LLVM | **real `Option::unwrap` bug** |

The right fix for the first three is a **harness decision**: either teach `TestRunner` to inject `std_source_assembled_minimal` the way `inject_assembled_stdlib` does, or move the probes to `glyim-lang-std/tests/` where `println.g` already lives. `probe_vec_push_len.g` is a one-line fixture fix. `probe_option_unwrap.g` is a genuine compiler bug in the `Option::unwrap` lowering path.

---

## 4. Items reverted or rejected (unchanged from session 4)

### T010 (`STD-4`) / T011 (`STD-5`) — env.g / process.g FFI rewrite
Reverted; runtime FFI retained. Blocked on turbofish-on-generic-method (`ptr::null_mut::<u8>()`) hitting a solver limitation.

### T096 (`TY-26`) — Send/Sync realignment
Rejected as incorrect per Rust semantics. The compiler's current behavior is right; the plan's proposal was wrong. Genuinely useful remainder (user `unsafe impl Send`/`impl !Send`) not implemented — parser lacks `impl !Trait`.

---

## 5. Verification commands — CORRECTED

The session-4 handoff's commands were insufficient. They used `cargo check --workspace`, which **does not compile test targets**, so a broken test file (`glyim-layout/src/tests/vtable_layout.rs`) sat undetected, and `cargo nextest run` without `--no-fail-fast` stops at the first failing binary, hiding the rest.

```bash
# Compile sanity — INCLUDING test targets (this is the fix for the above):
cargo check --workspace --tests

# Full matrix — NO fail-fast, so every failure surfaces in one run:
cargo nextest run --workspace --no-fail-fast

# The two known-failing tests, for focused iteration:
cargo test -p glyim-pipeline --test async_runtime
cargo nextest run -p glyim-test run_pass_corpus_passes --no-fail-fast
```

**Expected**: `cargo check --workspace --tests` clean; `4569 passed / 2 failed / 2 skipped` (4,571 total).

---

## 6. Marker conventions (unchanged)

| Marker | Meaning |
|---|---|
| `T###-PATCHED` | Fix applied, verified. |
| `T###-PATCHED-XXX` | Sub-marker (`-DECL`, `-CALL`, `-LLVM-ERROR`, …). |
| `T###-REVIEWED` | Plan's suggestion rejected as incorrect; current code kept. |
| `T###-REVISED` | Fix applied, then revised after tests broke. |
| `T###-DEFERRED` | Not implemented; rationale + design in the comment. |

Commit style: `fix(wave-N): TIDs (brief)`, `feat(...)`, `perf(...)`, `chore(...)`, `docs(session): ...`, `test(area): ...`.

---

## 7. How to resume

1. `cd /Users/adm/Documents/Repos/glyim-v2`
2. `git log --oneline bd571466..HEAD` → **6** commits (listed in §1).
3. `cargo check --workspace --tests` → clean.
4. `cargo nextest run --workspace --no-fail-fast` → 4569/2.
5. Pick from §3. Suggested order:
   - **§3.2 `probe_vec_push_len.g`** — trivial fixture fix (`v.len() as i32`).
   - **§3.2 harness stdlib injection** — unblocks 3 probes at once; decide injection vs relocation.
   - **§3.1 async interpreter** — needs recon first; do not touch the uninit check.
   - **§3.2 `probe_option_unwrap.g`** — real compiler bug; highest difficulty.

The source of truth is `git log bd571466..HEAD` plus this document. The plan itself is `docs/roadmaps/glyim-fix-plan.md`.
