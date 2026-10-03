# Handoff -- glyim-v2, session 9 (continued from session 8)

## TL;DR

HEAD is the commit shown by `git log --oneline -1`.

Suite at the point this handoff was written: **4231/4231 pass** (2 skipped),
clean tree.

This session continued the work of the audit
(`docs/roadmaps/glyim-bug-and-performance-audit.md`, now **committed**). It
found and fixed a **major non-audit bug** (compound assignment was entirely
broken) plus several Criticals, and identified that **several audit entries
are stale** (not reproducible on the current tree).

## Commits landed this session (chronological, newest last)

Run `git log --oneline origin/main..HEAD` for the authoritative list. From
`c878b7d2` (session-8 wrap-up) forward:

1. `8c8d9359` fix: RT-31 (proc-macro temp dir) + LL-11 (solver probe side effects)
2. `a7617620` fix(hir): compound assignment drops the operator (i += 1 became i = 1)
3. `6de793e6` docs(handoff): record compound-assign fix (major) + Criticals status
4. `f12f83f8` fix(lsp): wire document sync so requests see real files   [INF-16]
5. `ece05b43` fix(lsp): rename a `let` binding edits only the identifier [INF-13]

## THE BIG ONE: compound assignment was completely broken (a7617620)

**Not from the audit** -- found while reproducing LL-7.

`lower_assign_expr` (`crates/glyim-hir/src/lower/lower_expr.rs`) read the LHS
and RHS but **never looked at the operator token**, so EVERY compound
assignment lowered to a plain assignment:

    i += 1   ->   i = 1
    x *= 3   ->   x = 3

This made `while i < 1000 { i += 1; }` loop forever (i reset to 1 each
iteration) and segfault programs reading the result. It was pervasive --
any `+=` anywhere.

Fix: find the compound-assign operator token (a direct child of the
`AssignExpr` node; kinds `PlusEq`/`MinusEq`/`StarEq`/`SlashEq`/`PercentEq`/
`AndEq`/`OrEq`/`CaretEq`/`ShlEq`/`ShrEq`) and desugar `lhs <op>= rhs` to
`lhs = lhs <op> rhs` (synthesize an `Expr::Binary` for the RHS). Plain `=`
is unchanged. Regression test: `crates/glyim-cli/tests/compound_assign.rs`.

## Criticals: FIXED this session

| ID | Commit | What |
|----|--------|------|
| RT-31 | 8c8d9359 | proc-macro `TempDir` deleted before dlopen (`--proc-macro-deps` never worked) |
| LL-11 | 8c8d9359 | `resolve_trait_method_fn` probe left permanent inference side effects |
| INF-16 | f12f83f8 | LSP `build_router` used a throwaway `FileMap`, no didOpen/didChange -> all requests null |
| INF-13 | ece05b43 | rename a `let` binding edited the whole statement, clobbering the initializer |
| (non-audit) | a7617620 | compound assignment dropped the operator |

## Criticals: verified STALE (not reproducible on current tree)

Do NOT re-investigate without re-verifying against a freshly built CLI:

- **LL-6** (struct literal field order): operands already in declaration order.
- **SOLVE-8** (blanket-impl recursion): clean `T0001` diagnostic, no crash.
- **LL-9** (`PassMode::Ignore` / ZST args): `fn f(u: (), x: i32)` compiles+runs.
- **HIR-4** (macro args as one Group): `pair!(1, 2)` multi-metavar works.
- **HIR-10** (async wrong tail): the if/else async case compiles; the audit's
  `.rev()` fix BREAKS `async_state_machine_runs_via_interpreter`. Reverted.
- **MIR-11** (`MaybeInitialized` MAY vs definite): `if c { s = make(); }` with
  scope-end drop compiles fine.
- **MIR-24** (move analysis ignores terminators): the audit's fix produces
  FALSE-POSITIVE "use of moved value" on real programs (Drop-vs-move conflict).
  Reverted.
- **RT-12** (`block_offsets` table): control-flow programs compile; needs a
  VM-harness check to confirm the runtime symptom.

## Criticals: REPRODUCE, still open (each sizable)

1. **HIR-2** (multi-token repetition fragments): `stmts!(1 + 2, 3)` with
   `($($e:expr),*)` mis-expands. Needs depth-aware bindings
   (`Vec<Vec<TokenTree>>`) in `matcher.rs` + `substitution.rs`.
2. **HIR-11** (multi-await drops statements): 2-await async fn reports
   `unresolved name d`; the 1-await case hits an unrelated "ambiguous method
   `poll` found in multiple impls". Deep `lower_async.rs` work.
3. **HIR-31** (closure `ByRef` captures): `Layout error: UnknownType(Ty(63))`.
   Root cause traced: the `Closure` aggregate operand uses
   `LocalIdx::from_raw(capture.local.to_raw())` (a THIR `LocalVarId`!) instead
   of `self.local_for_var(capture.local)`; plus a deeper unresolved closure
   type reaching codegen (`lower_closure` sets `fn_const.ty = error_ty()`).
   Needs coordinated typeck (`check_expr.rs` capture-field types -> `&T`) +
   lower (Ref operand + use-site auto-deref).
4. **SOLVE-1 + scoped SOLVE-2**: SOLVE-1 (int/float self-bind guard) alone is
   stdlib-safe but insufficient; SOLVE-2's blanket chain-follow at the top of
   `unify_tys` breaks 3 stdlib cases (`T vs <Self as Trait>::Item` x2,
   `isize vs usize`). Repro: `let t = if c { (a, b) } else { (b, a) };` with
   int literals -> `Ty::ERROR` reaches codegen.

## Recommended next steps (in order)

1. **HIR-2** -- self-contained macro-expander fix; good starting point.
2. **SOLVE-1 + scoped SOLVE-2** -- fix the int-var cycle repro without the
   blanket chain-follow (only follow when the unresolved side is a bare infer
   var, not for every recursive element unification).
3. **HIR-31** -- the closure-capture fix; start with the operand-local fix +
   use-site auto-deref, then address the unresolved closure type at codegen.
4. **HIR-11** -- multi-await async desugar.
5. Remaining High/Medium findings across all sections of the audit.

## Working-style constraints (still in force)

- Never lower `Ty::ERROR` at codegen -- `v15_t25_drop_error_type` is a
  `#[should_panic]` contract.
- Every `thir::Expr::err(span)` should be paired with a diagnostic.
- Conventional commit prefixes; document blockers in this file.
- Do NOT `git stash pop` a stale stash without checking `git stash list`.
- **Reproduce an audit finding against a freshly built CLI BEFORE fixing** --
  several are stale (above). Rebuild + fresh `GLYIM_CACHE_DIR`.
- **Regression tests must FAIL on the pre-fix code.** Verify by reverting the
  fix, rebuilding, re-running. (Multiple times a "test" passed on both
  versions -- not a real regression test.)
- **Bounded test inputs.** A 5000-deep recursion test exhausted a machine's
  memory (a depth guard that bails without consuming a token lets the parser
  spin appending diagnostics). Use just-over-threshold inputs (e.g. 300 vs
  `MAX_EXPR_DEPTH` 256) and make guards consume a token on bail.
- **Never `--emit=exec` a loop-containing program without a hard kill guard**
  (macOS has no `timeout`): run `( sleep 10; pkill -9 -f BIN ) &` around it, or
  use `--emit=mir` (compile-only, cannot hang) to inspect.
- **Watch for false positives** when strengthening an analysis (MIR-24) --
  run the FULL workspace suite, not just the target crate.
- `nextest` filters on test function names, not file names.
- A stale `.git/index.lock` appears when background `cargo`/`git` race;
  `rm -f .git/index.lock` clears it. `git commit` sometimes reports
  `unable to write new index file` transiently -- just retry.

## Tooling notes

- CLI: `--emit=obj|exec|mir|llvm-ir|asm|cdylib`; `--with-stdlib` prepends the
  minimal stdlib. Use a fresh `GLYIM_CACHE_DIR` when verifying a fix.
- MIR `ERROR glyim_mir: Place::ty(): ...` lines on every stdlib compile are
  pre-existing tracing noise; filter with `grep -v '^2026-'`.
- Handoff files live in `docs/handoffs/`. The audit is committed at
  `docs/roadmaps/glyim-bug-and-performance-audit.md`.
- `tests/compile-pass/*.g` under `crates/glyim-typeck/` are NOT run (the
  harness was removed in `3ea72806`); use CLI tests instead.

## Where to look (quick index)

| What | Where |
|------|-------|
| Compound-assign lowering | `crates/glyim-hir/src/lower/lower_expr.rs::lower_assign_expr` |
| `Expr::Let` (has `pat_span` now) | `crates/glyim-hir/src/lib.rs` |
| LSP request routing + doc sync | `crates/glyim-lsp/src/handler.rs` |
| LSP rename | `crates/glyim-lsp/src/rename.rs`, `reference_graph.rs` |
| LSP source map (UTF-16) | `crates/glyim-lsp/src/database.rs` |
| Macro matcher/substitution | `crates/glyim-meta/src/expander/{matcher,substitution,mod}.rs` |
| Async desugar | `crates/glyim-hir/src/lower/lower_async.rs` |
| Closure capture lowering | `crates/glyim-lower/src/lower_rvalue.rs`, `builder.rs` |
| Inference / unify | `crates/glyim-solve/src/infer.rs` |
| Trait solver | `crates/glyim-solve/src/solver.rs` |
| Proc-macro deps | `crates/glyim-cli/src/lib.rs::compile_proc_macro_dep` |
