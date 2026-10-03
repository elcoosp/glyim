# Handoff -- glyim-v2, session 8

## TL;DR

Suite: **4227/4227 pass** (2 skipped), clean tree.

This session worked the audit's "12 fixes that matter most" list to
completion, then began the next tier of Criticals. **A key finding: several
audit entries are stale** — the described bug is not reproducible on the
current tree (already fixed by an earlier commit, or the audit's line
anchors moved). Each fix landed with a regression test verified to fail on
the pre-fix code and pass after.

## Commits landed this session

    git log --oneline (session-8 range, newest first)
    a30ebaf8 fix(interp): read enum base type from the owning frame
    2982d490 fix(interp): enum field writes skip the discriminant tag
    71dfd401 fix(lower): short-circuit && and ||
    4c51c04e fix(lower): materialize loop break values
    358ae136 fix(meta): recurse into groups when collecting repetition metavars
    52e9f0d9 fix(interp): width-correct arithmetic and casts
    7165a208 fix(lsp): SourceMap uses UTF-16 columns, not byte offsets
    e6862ffa fix: audit batch 3 -- RT-13, LL-5, FE-6
    b3982574 fix: audit batch 2 -- MIR-21, HIR-1, INF-12
    e582702e fix(runtime): thread current_id is the ThreadStore id
    4ab298b4 fix(pipeline): slice drop glue emits a real terminating loop
    a516504d fix(opt): drop elaboration preserves old block indices
    17012798 fix: audit batch 1 -- SOLVE-7, FE-1, HIR-14, HIR-29, LL-1..4, RT-3, RT-4
    43c0ad86 fix(codegen): bytecode bool switch emits true/false targets inverted
    8f08df91 fix(opt): const-prop simulates blocks from entry state
    fb858abb fix(opt): DCE counts Drop terminators as uses
    eabdda89 fix: resurrect the .g test harness + fix the bugs it found
    ... plus the pre-audit session-7 work (turbofish, FromStr, byte literal)

## The 12 "fixes that matter most" -- all DONE

| # | Issue | Status |
|---|-------|--------|
| 1 | RT-11 bytecode bool branches | fixed |
| 2 | MIR-1 const-prop stale constants | fixed |
| 3 | MIR-6 DCE drop uses | fixed |
| 4 | MIR-10 drop-elab CFG corruption | fixed |
| 5 | LL-1..4 LLVM sign/unsigned | fixed |
| 6 | SOLVE-7 coercion precedence | fixed |
| 7 | HIR-29 match scrutinee twice | fixed |
| 8 | RT-21 async wake-up ID | fixed |
| 9 | INF-23 slice drop loop | fixed |
| 10 | FE-1 `%=` unparseable | fixed |
| 11 | RT-3/RT-4 VM wire format | fixed |
| 12 | HIR-14 string unescape | fixed |

Plus, from later batches: MIR-21, HIR-1, INF-12, RT-13, LL-5, FE-6, INF-11,
MIR-13, MIR-14, HIR-3, HIR-30, LL-10, MIR-17.

## IMPORTANT: audit entries that are NOT reproducible on the current tree

Verified by running the audit's own "Check" repro against a freshly built
CLI. **Do not re-investigate these without re-verifying.**

- **LL-6** (out-of-order struct literal writes wrong field): the MIR
  aggregate operands are ALREADY in declaration order (`[x, y]`). No fix
  needed; a pin test (`struct_field_order.rs`) guards it.
- **SOLVE-8** (blanket-impl recursion → stack overflow): `impl<T: Foo> Foo
  for T {}` produces a clean `T0001` diagnostic, no crash.
- **LL-9** (`PassMode::Ignore` / ZST args → store to null): `fn f(u: (),
  x: i32)` compiles and runs correctly.
- **LL-7** (alloca in loop): the loop case compiles fine (the stack-growth
  symptom needs a long-running binary to observe; not a compile failure).

The audit's line anchors are pinned to commit `eff1f1fa`; the tree has moved
~20 commits since, and several of these were fixed incidentally.

## Reverted / deferred (attempted, then backed out)

- **SOLVE-1 / SOLVE-2** (inference-var binding cycles / chain-following):
  the fix is correct in isolation (50/50 solve tests pass) but regresses
  stdlib type-checking (`T vs <Self as Trait>::Item`, `isize vs usize`).
  The audit itself notes these need the coordinated root fix with SOLVE-3.
- **MIR-24** (move analysis ignores terminator moves): the audit's fix
  produces FALSE-POSITIVE "use of moved value" errors on real programs
  (the `Drop`-as-move handling conflicts with legitimate scope-end drops;
  the async poll-loop pattern relies on `Call`-destination re-init without
  a matching move). Needs a more careful design.
- **HIR-31** (closure `ByRef` captures copied by value): reproduces
  (`Layout error: UnknownType`), but the fix requires threading a reference
  type through capture-field typing AND use-site auto-deref in the closure
  body — large and risky.

## Remaining audit work (priority order, with caveats)

1. **HIR-31** (closure `ByRef` captures) -- reproduces; large fix.
2. **MIR-24** (terminator moves) -- reproduces as a *gap*, but the naive fix
   over-reports; needs a "Drop of already-moved is OK" refinement.
3. **MIR-11** (`MaybeInitialized` MAY vs definite) -- not yet attempted.
4. **RT-12** (`block_offsets` table) -- needs the bytecode module
   serializer + `Module::deserialize`; large.
5. **INF-13** (rename whole-expr spans) -- LSP-only; needs identifier sub-
   spans recorded during HIR lowering (or re-lexed at rename time).
6. **INF-16** (LSP `FileMap` never populated) -- wire `build_router` to the
   shared `db.file_map` + register didOpen/didChange notifications.
7. Then the remaining High/Medium findings (FE-2..FE-20, SOLVE-1..23,
   MIR-2..31, RT-1..31, LL-6..11, INF-1..23, HIR-1..31).

## Working-style constraints (still in force)

- Never lower `Ty::ERROR` at codegen -- `v15_t25_drop_error_type` is a
  `#[should_panic]` contract.
- Every `thir::Expr::err(span)` should be paired with a diagnostic.
- Conventional commit prefixes; document blockers in handoff files.
- Do NOT `git stash pop` a stale stash without checking `git stash list`.
- **Reproduce an audit finding against a freshly built CLI BEFORE fixing.**
  Several claims are stale (see above). Rebuild + use a fresh
  `GLYIM_CACHE_DIR` to avoid the content-keyed cache serving stale results.
- **Regression tests must fail on the pre-fix code.** Verify by reverting
  the fix, rebuilding, and re-running. (Multiple times this session a
  "test" passed on both versions -- not a real regression test.)
- **Bounded test inputs.** A 5000-deep recursion test exhausted the user's
  machine memory (the parser can spin appending diagnostics if a depth
  guard bails without consuming a token). Use just-over-threshold inputs
  (e.g. 300 vs MAX_EXPR_DEPTH 256) and make guards consume a token on bail.
- **Watch for false positives** when strengthening an analysis (MIR-24):
  run the FULL workspace suite, not just the target crate's tests.
- `nextest` filters on test function names, not file names.

## Tooling notes

- CLI: `--emit=obj|exec|mir|llvm-ir|asm|cdylib`; `--with-stdlib` prepends
  the minimal stdlib. Use a fresh `GLYIM_CACHE_DIR` when verifying a fix.
- The MIR `ERROR glyim_mir: Place::ty(): ...` lines on every stdlib compile
  are pre-existing tracing noise; filter with `grep -v '^2026-'`.
- `docs/roadmaps/glyim-bug-and-performance-audit.md` is UNTRACKED (193
  findings). Decide whether to commit it.
- A stale `.git/index.lock` appeared repeatedly when background
  `cargo`/`git` calls raced; `rm -f .git/index.lock` clears it.

---

## SOLVE-1 / SOLVE-2 bisection (exact, this session)

Ran each half alone against a freshly built CLI + the full-stdlib probe.

**SOLVE-1 alone** (resolve the int/float peer before binding; refuse to bind
an int/float var to itself):

    let other_ty = self.resolve_ty_shallow_preserve_int(ctx, if a_is_int { b } else { a });
    let int_var_ty = if a_is_int { a } else { b };
    if other_ty == int_var_ty { return Ok(Vec::new()); }

- glyim-solve: 50/50 pass.
- full-stdlib probe: **PASSES** (no regression).
- Does NOT by itself fix the `if c { (a,b) } else { (b,a) }` repro.

**SOLVE-2 alone** (follow binding chains at the TOP of `unify_tys`, before
the `a_kind`/`b_kind` match):

    let a = self.resolve_ty_shallow_preserve_int(ctx, a);
    let b = self.resolve_ty_shallow_preserve_int(ctx, b);
    if a == b { return Ok(Vec::new()); }

- glyim-solve: 50/50 pass.
- full-stdlib probe: **FAILS, 3 cases**:
  - `option:L226` -- `mismatched types: T vs <Self as Trait37>::Item`
  - `iter:L308` -- `mismatched types: T vs <Self as Trait37>::Item`
  - `rc:L32` -- `mismatched types: isize vs usize`

**SOLVE-1 + SOLVE-2 together**: fixes the repro (`rc=0`), but inherits the
SOLVE-2 stdlib failures.

**Conclusion**: SOLVE-1 is safe and independently valuable (a latent
infinite-cycle guard), but it is not the fix for the int-var-cycle repro on
its own. SOLVE-2's blanket top-of-`unify_tys` chain-follow is too aggressive
for the stdlib's associated-type-heavy code (`<Self as Trait>::Item`,
`isize`/`usize` literal defaulting) and must be scoped (e.g. only follow a
chain when the *unresolved* side is a bare infer var, not for every
recursive element unification). This is exactly the "coordinated root fix
with SOLVE-3" the audit calls for. Deferred.

### Remaining reproducible Criticals (verified this session)

- **HIR-31** (closure `ByRef` captures): reproduces as
  `[X0000] Layout error: UnknownType(Ty(63))` for both `let mut f = || { c += 1; }`
  and the read-only `|| { let _ = c; }`. The MIR shows the closure aggregate
  copies the capture local (`Copy(Place { local: LocalIdx(0) })` for `_0` —
  the wrong local / by-value) and the closure body call has a `Closure<..>`
  receiver typed `Closure2000000<...>` whose layout fails. Fix needs:
  capture-field types become `&T`/`&mut T`, the aggregate operand becomes a
  `Ref`, and use-sites in the closure body auto-deref. Large.
- **SOLVE-1** repro (`if c { (a,b) } else { (b,a) }` with int literals) —
  see bisection above.

---

## SOLVE-1 / SOLVE-2 bisection (exact, this session)

Ran each half alone against a freshly built CLI + the full-stdlib probe.

**SOLVE-1 alone** (resolve the int/float peer before binding; refuse to bind
an int/float var to itself):

    let other_ty = self.resolve_ty_shallow_preserve_int(ctx, if a_is_int { b } else { a });
    let int_var_ty = if a_is_int { a } else { b };
    if other_ty == int_var_ty { return Ok(Vec::new()); }

- glyim-solve: 50/50 pass.
- full-stdlib probe: **PASSES** (no regression).
- Does NOT by itself fix the `if c { (a,b) } else { (b,a) }` repro.

**SOLVE-2 alone** (follow binding chains at the TOP of `unify_tys`, before
the `a_kind`/`b_kind` match):

    let a = self.resolve_ty_shallow_preserve_int(ctx, a);
    let b = self.resolve_ty_shallow_preserve_int(ctx, b);
    if a == b { return Ok(Vec::new()); }

- glyim-solve: 50/50 pass.
- full-stdlib probe: **FAILS, 3 cases**:
  - `option:L226` -- `mismatched types: T vs <Self as Trait37>::Item`
  - `iter:L308` -- `mismatched types: T vs <Self as Trait37>::Item`
  - `rc:L32` -- `mismatched types: isize vs usize`

**SOLVE-1 + SOLVE-2 together**: fixes the repro (`rc=0`), but inherits the
SOLVE-2 stdlib failures.

**Conclusion**: SOLVE-1 is safe and independently valuable (a latent
infinite-cycle guard), but it is not the fix for the int-var-cycle repro on
its own. SOLVE-2's blanket top-of-`unify_tys` chain-follow is too aggressive
for the stdlib's associated-type-heavy code (`<Self as Trait>::Item`,
`isize`/`usize` literal defaulting) and must be scoped (e.g. only follow a
chain when the *unresolved* side is a bare infer var, not for every
recursive element unification). This is exactly the "coordinated root fix
with SOLVE-3" the audit calls for. Deferred.

### Remaining reproducible Criticals (verified this session)

- **HIR-31** (closure `ByRef` captures): reproduces as
  `[X0000] Layout error: UnknownType(Ty(63))` for both `let mut f = || { c += 1; }`
  and the read-only `|| { let _ = c; }`. The MIR shows the closure aggregate
  copies the capture local (`Copy(Place { local: LocalIdx(0) })` for `_0` —
  the wrong local / by-value) and the closure body call has a `Closure<..>`
  receiver typed `Closure2000000<...>` whose layout fails. Fix needs:
  capture-field types become `&T`/`&mut T`, the aggregate operand becomes a
  `Ref`, and use-sites in the closure body auto-deref. Large.
- **SOLVE-1** repro (`if c { (a,b) } else { (b,a) }` with int literals) —
  see bisection above.

---

## HIR-31 investigation (not fixed — reverted)

The closure-capture bug (`[X0000] Layout error: UnknownType(Ty(63))` for any
non-`move` closure) was traced to **two** problems:

1. **Wrong aggregate operand local.** The `thir::ExprKind::Closure` arm in
   `crates/glyim-lower/src/lower_rvalue.rs` builds the closure aggregate's
   capture operands with `LocalIdx::from_raw(capture.local.to_raw())`, where
   `capture.local` is a THIR `LocalVarId` — NOT a MIR `LocalIdx`. For the
   first user local that is `LocalIdx(0)` (the unit return place). The correct
   value is `self.local_for_var(capture.local)` (the enclosing frame's MIR
   local for the captured var). Applying only this fix kept the test suite
   green (330/330 lower+typeck) but did **not** resolve the layout error, so
   it was reverted rather than left as a half-fix.

2. **A second, deeper source of `Ty(63)`.** `lower_closure` in
   `crates/glyim-lower/src/builder.rs` sets the closure `fn_const.ty` to
   `self.ctx.ty_ctx().error_ty()` and pushes the closure body under the
   closure `substs` — the `Ty(63)` reaching `llvm_type_for_ty` is an
   unresolved capture/closure type that survives to codegen. Fully fixing
   HIR-31 needs the capture-field types to be `&T`/`&mut T` (typeck
   `check_expr.rs` step 4), the aggregate operand to be a `Ref` (both
   `lower_rvalue.rs` and `builder.rs` sites), AND use-site auto-deref in the
   closure body — a coordinated typeck+lower change.

Deferred as a focused multi-crate task.

## Final session-8 state

    git log --oneline -1
    e7612a06 docs(handoff): record SOLVE-1/SOLVE-2 bisection + reproducible Criticals
    (plus this wrap-up commit)

    cargo nextest run --workspace
    Summary: 4227 tests run: 4227 passed, 2 skipped

Handoff files now live under `docs/handoffs/`.

### Highest-value next tasks (each sizable)

1. **HIR-31** closure captures (see above) — coordinated typeck+lower.
2. **SOLVE-1 + a scoped SOLVE-2** — SOLVE-1 alone is stdlib-safe; SOLVE-2's
   blanket chain-follow breaks 3 stdlib cases and must be narrowed.
3. **MIR-24** terminator moves — needs a "Drop of already-moved is OK"
   refinement (the naive fix false-positives on real programs).
4. **RT-12** bytecode `block_offsets` — needs a module serializer +
   `Module::deserialize`.
5. **INF-13** rename sub-spans / **INF-16** LSP `FileMap` wiring.

---

## HIR-31 investigation (not fixed — reverted)

The closure-capture bug (`[X0000] Layout error: UnknownType(Ty(63))` for any
non-`move` closure) was traced to **two** problems:

1. **Wrong aggregate operand local.** The `thir::ExprKind::Closure` arm in
   `crates/glyim-lower/src/lower_rvalue.rs` builds the closure aggregate's
   capture operands with `LocalIdx::from_raw(capture.local.to_raw())`, where
   `capture.local` is a THIR `LocalVarId` — NOT a MIR `LocalIdx`. For the
   first user local that is `LocalIdx(0)` (the unit return place). The correct
   value is `self.local_for_var(capture.local)` (the enclosing frame's MIR
   local for the captured var). Applying only this fix kept the test suite
   green (330/330 lower+typeck) but did **not** resolve the layout error, so
   it was reverted rather than left as a half-fix.

2. **A second, deeper source of `Ty(63)`.** `lower_closure` in
   `crates/glyim-lower/src/builder.rs` sets the closure `fn_const.ty` to
   `self.ctx.ty_ctx().error_ty()` and pushes the closure body under the
   closure `substs` — the `Ty(63)` reaching `llvm_type_for_ty` is an
   unresolved capture/closure type that survives to codegen. Fully fixing
   HIR-31 needs the capture-field types to be `&T`/`&mut T` (typeck
   `check_expr.rs` step 4), the aggregate operand to be a `Ref` (both
   `lower_rvalue.rs` and `builder.rs` sites), AND use-site auto-deref in the
   closure body — a coordinated typeck+lower change.

Deferred as a focused multi-crate task.

## Final session-8 state

    git log --oneline -1
    e7612a06 docs(handoff): record SOLVE-1/SOLVE-2 bisection + reproducible Criticals
    (plus this wrap-up commit)

    cargo nextest run --workspace
    Summary: 4227 tests run: 4227 passed, 2 skipped

Handoff files now live under `docs/handoffs/`.

### Highest-value next tasks (each sizable)

1. **HIR-31** closure captures (see above) — coordinated typeck+lower.
2. **SOLVE-1 + a scoped SOLVE-2** — SOLVE-1 alone is stdlib-safe; SOLVE-2's
   blanket chain-follow breaks 3 stdlib cases and must be narrowed.
3. **MIR-24** terminator moves — needs a "Drop of already-moved is OK"
   refinement (the naive fix false-positives on real programs).
4. **RT-12** bytecode `block_offsets` — needs a module serializer +
   `Module::deserialize`.
5. **INF-13** rename sub-spans / **INF-16** LSP `FileMap` wiring.

---

## Session 8 addendum: MAJOR non-audit bug found + Criticals status

### Compound assignment was completely broken (FIXED, commit a7617620)

Not from the audit -- found while reproducing LL-7. `lower_assign_expr`
(`crates/glyim-hir/src/lower/lower_expr.rs`) read the LHS and RHS but never
looked at the operator token, so **every** compound assignment lowered to a
plain assignment: `i += 1` -> `i = 1`, `x *= 3` -> `x = 3`. This made
`while i < 1000 { i += 1; }` loop forever and segfault programs reading the
result. Fixed by desugaring `lhs <op>= rhs` to `lhs = lhs <op> rhs`. This
was pervasive (any `+=` in the stdlib or user code).

### Criticals: what actually reproduces vs stale

Verified against a freshly built CLI this session:

| ID | Status |
|----|--------|
| RT-11, MIR-1, MIR-6, MIR-10, MIR-13, MIR-14, MIR-17, MIR-21, RT-3, RT-4, RT-13, RT-21, INF-11, INF-12, INF-23, HIR-1, HIR-3, HIR-29, HIR-30, LL-1, LL-2, LL-5, LL-10, FE-6, SOLVE-7 | **FIXED** |
| LL-6, SOLVE-8, LL-9, HIR-4, HIR-10, MIR-11, MIR-24, RT-12 | **NOT reproducible / already correct** on current tree |
| RT-31, LL-11 | **FIXED** this addendum |
| HIR-2, HIR-11, HIR-31, SOLVE-1, SOLVE-2, INF-13, INF-16 | **REPRODUCE, not fixed (large/risky)** |

### The remaining reproducible Criticals (each sizable)

- **HIR-2** (multi-token repetition fragments): `stmts!(1 + 2, 3)` with
  `($($e:expr),*)` mis-expands. Needs depth-aware bindings
  (`Vec<Vec<TokenTree>>`) in the matcher + substitution.
- **HIR-11** (multi-await drops statements): 2-await async fn reports
  `unresolved name d`. Also the 1-await case hits an unrelated
  "ambiguous method `poll` found in multiple impls". Deep async-desugar work
  in `lower_async.rs`.
- **HIR-31** (closure `ByRef` captures): `Layout error: UnknownType(Ty(63))`.
  Root cause traced (wrong aggregate operand local + unresolved closure type
  at codegen); needs coordinated typeck+lower + use-site auto-deref.
- **SOLVE-1 + scoped SOLVE-2**: SOLVE-1 alone is stdlib-safe but insufficient;
  SOLVE-2's blanket chain-follow breaks 3 stdlib cases. Needs scoping.
- **INF-13** (rename whole-expr spans), **INF-16** (LSP FileMap wiring):
  LSP-only, self-contained, but need sub-span recording / notification wiring.

Suite after the compound-assign fix: **4230/4230 pass** (2 skipped).

---

## Session 8 addendum: MAJOR non-audit bug found + Criticals status

### Compound assignment was completely broken (FIXED, commit a7617620)

Not from the audit -- found while reproducing LL-7. `lower_assign_expr`
(`crates/glyim-hir/src/lower/lower_expr.rs`) read the LHS and RHS but never
looked at the operator token, so **every** compound assignment lowered to a
plain assignment: `i += 1` -> `i = 1`, `x *= 3` -> `x = 3`. This made
`while i < 1000 { i += 1; }` loop forever and segfault programs reading the
result. Fixed by desugaring `lhs <op>= rhs` to `lhs = lhs <op> rhs`. This
was pervasive (any `+=` in the stdlib or user code).

### Criticals: what actually reproduces vs stale

Verified against a freshly built CLI this session:

| ID | Status |
|----|--------|
| RT-11, MIR-1, MIR-6, MIR-10, MIR-13, MIR-14, MIR-17, MIR-21, RT-3, RT-4, RT-13, RT-21, INF-11, INF-12, INF-23, HIR-1, HIR-3, HIR-29, HIR-30, LL-1, LL-2, LL-5, LL-10, FE-6, SOLVE-7 | **FIXED** |
| LL-6, SOLVE-8, LL-9, HIR-4, HIR-10, MIR-11, MIR-24, RT-12 | **NOT reproducible / already correct** on current tree |
| RT-31, LL-11 | **FIXED** this addendum |
| HIR-2, HIR-11, HIR-31, SOLVE-1, SOLVE-2, INF-13, INF-16 | **REPRODUCE, not fixed (large/risky)** |

### The remaining reproducible Criticals (each sizable)

- **HIR-2** (multi-token repetition fragments): `stmts!(1 + 2, 3)` with
  `($($e:expr),*)` mis-expands. Needs depth-aware bindings
  (`Vec<Vec<TokenTree>>`) in the matcher + substitution.
- **HIR-11** (multi-await drops statements): 2-await async fn reports
  `unresolved name d`. Also the 1-await case hits an unrelated
  "ambiguous method `poll` found in multiple impls". Deep async-desugar work
  in `lower_async.rs`.
- **HIR-31** (closure `ByRef` captures): `Layout error: UnknownType(Ty(63))`.
  Root cause traced (wrong aggregate operand local + unresolved closure type
  at codegen); needs coordinated typeck+lower + use-site auto-deref.
- **SOLVE-1 + scoped SOLVE-2**: SOLVE-1 alone is stdlib-safe but insufficient;
  SOLVE-2's blanket chain-follow breaks 3 stdlib cases. Needs scoping.
- **INF-13** (rename whole-expr spans), **INF-16** (LSP FileMap wiring):
  LSP-only, self-contained, but need sub-span recording / notification wiring.

Suite after the compound-assign fix: **4230/4230 pass** (2 skipped).
