# HANDOFF — Glyim compiler fix plan (current)

**Updated at**: end of Session 5 (final)
**HEAD**: `17aacb46` (`chore: Cargo.lock for glyim-test -> glyim-lang-std dep`)
**Baseline for this session**: `bd571466` (the session-4 handoff doc commit)
**Session-4 history**: preserved verbatim in `.fix-log/HANDOFF-session-4-archive.md`
**Workspace state**: clean, 0 compile errors, **4570 passing / 1 failing** (2 skipped) across 65 test binaries.

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
| `b51f62bd` | docs(handoff) | Corrected this handoff. |
| `00949c26` | docs(handoff) | Restore the session-4 archive (it had been committed empty). |
| `80fe0fcd` | fix(hir/async) | **T100 real bug.** `rewrite_for_poll` searched for the fn-body root Block *after* rewriting; `rewrite_expr` allocates new Blocks (the `Pending => loop {}` arm), so the search found an empty inner Block, hit the silent `return` bail-out, and skipped the `Poll::Ready` wrap. The poll body was effectively empty. **Greens `async_state_machine_runs_via_interpreter`.** |
| `591aa394` | fix(interp) | Accept `MirConstKind::Error` as `Unit` (symmetric with `248a454a`'s LLVM change). |
| `7f57cf8e` | feat(harness) | `// compile-flags: --with-stdlib` — `PipelineCompiler` prepends the assembled minimal stdlib. |
| `71b900dc` | fix(interp) | **Builtin-method dispatch.** The interpreter had no handler for compiler-synthetic builtin methods (`FnDefId` 9000+); stdlib code calling `Vec::push`/`Option::unwrap`/`String::as_str` panicked `function not found`. Adds `try_call_builtin`. **Greens `probe_option_unwrap.g`.** |
| `17aacb46` | chore | `Cargo.lock` for the new `glyim-test` -> `glyim-lang-std` dep. |

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

## 3. Genuinely remaining — the last failure

One test still fails: `glyim-test::run_pass_corpus run_pass_corpus_passes`.
It reports **4 fixtures**, but they are **two distinct workstreams**, neither a
one-line fix.

Matrix today: **4,570 passing / 1 failing / 2 skipped** (was 4,565/6 at session start).

### 3.1 The interpreter cannot execute `vec.g`'s source `Vec::push`

`probe_vec_push_len.g` runs but returns `0` instead of `3`. Instrumentation shows:
- `Vec::len` reaches the builtin dispatch (`recv = Aggregate([])`).
- **`Vec::new` and `Vec::push` do NOT reach it** — they resolve to their *source*
  bodies in `crates/glyim-lang-alloc/lib/vec.g`, and those bodies fail silently
  in the interpreter.

`vec.g`'s `push` calls `RawVec::reserve`, `buf.as_mut_ptr`, `ptr::write`, and
updates `self.len`. The interpreter does not execute that allocation path
correctly, so the `Vec::push` arm added in `71b900dc` is dead code for this
fixture.

**This is the real remaining work.** Fix needs either (a) the interpreter to run
`RawVec`/`ptr::*` allocation, or (b) typeck to route `Vec::push` to the builtin
id instead of the source body. Either is a workstream, not a patch.

### 3.2 The `println` chain — remaining work, with design

`probe_println_function.g`, `probe_println_macro.g`, `probe_string_push_str.g`
compile but do not produce output. `println`'s body is
`stdout().write_all(s.as_bytes()).unwrap()`, whose byte path traverses
`str::as_bytes` -> `slice::as_ptr` / `slice::len` -> `extern "C"
glyim_stdout_write(fd, ptr, len)`.

**What was tried this session and why it is not the fix:** the interpreter
carries byte slices in `InterpValue::String`. Arms were added for
`Deref`/`Field`/`Index` projections and a `try_call_extern` dispatcher.
Each projection patch moved the failure one step (deref -> field -> binop),
ending at `unsupported binop types: String("hi") and Int(0)` — a String
meeting an Int in arithmetic. **This is the signal that the String stand-in
is the wrong shape.** A `String` cannot act as a `[ptr, len]` fat pointer;
every site that needs a real pointer reveals another site.

**The right design (not yet implemented):**

1. Add an interpreter-side byte arena: `byte_arena: Vec<Vec<u8>>`, with a
   new value variant `InterpValue::ByteRef(usize)` naming a slot in it.
2. `str::as_bytes` / `String::as_bytes` / `as_ptr` return a fat-pointer
   `Aggregate([ByteRef(id), Uint(len)])` — a real 2-field value that the
   existing `Aggregate` projection code already handles for `Deref`,
   `Field(0)`, `Field(1)`, and `Index`.
3. `try_call_extern`'s `glyim_stdout_write` reads `args[1]` as
   `ByteRef(id)` and copies `byte_arena[id]` into `stdout_buf`.
4. Delete the String projection arms added this session — they become
   dead once the value is a proper aggregate.

Estimated: ~1 day. Bounded, and removes the approximation rather than
layering on it.
### 3.3 Honest scope note

The session-4 handoff's claim of "two remaining failures" was an undercount:
there were **six** pre-existing failures at `bd571466`, of which four were test
files that did not compile (`--workspace` hides test targets) and two were real
bugs (async interpreter, `Option::unwrap`). This session closed four of those
six, plus the two genuine bugs, leaving one multi-fixture failure with the two
workstreams above.

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
