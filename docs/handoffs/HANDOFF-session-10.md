# Handoff -- glyim-v2, session 10 (continued from session 9)

## TL;DR

HEAD: `a41ed261`. Suite: **4263/4263 pass** (2 skipped), clean tree.

Seven commits landed. HIR-2 (Critical), a related non-audit Critical (macro
expansion token fusion), SOLVE-1+SOLVE-3 (Critical), the operand-local half of
HIR-31 (Critical), HIR-11 (Critical), and FE-20 (Medium).

## Commits landed this session

From `30f9820b` (session-9 wrap-up) forward:

1. `1c14a56f` fix(meta): HIR-2 -- depth-aware repetition bindings
2. `2e193f94` fix(meta): insert token separators in macro expansions
3. `804f6ef2` docs(handoff): session-10
4. `59a62211` fix(solve): SOLVE-1 + SOLVE-3 -- int-var binding cycles
5. `1016db75` fix(lower): HIR-31 (partial) -- closure capture operand wrong local
6. `bf0ac1a9` fix(hir): HIR-11 -- multi-await state machine dropped between-await statements
7. `a41ed261` fix(syntax): FE-20 -- is_keyword/is_node missed variants declared after ranges

## HIR-2 (Critical) -- FIXED

`$($e:expr),*` matching `1 + 2, 3` recorded `e = [1, +, 2, 3]`, so
`$( let _ = $e; )*` expanded to four statements instead of two: the repetition
count was captured *tokens*, not matched *iterations*. Bindings are now
depth-aware (`HashMap<SmolStr, Vec<Vec<TokenTree>>>`, one inner Vec per
iteration). Regression tests fail pre-fix (6 `let`s), pass post-fix (2).

## Token fusion (Critical, non-audit) -- FIXED

Found validating HIR-2 e2e. `build_expansion_green` emitted tokens with no
separating whitespace and `expand_node_recursive` reconstructs source via
`temp_root.text()` before re-parsing, so `let` + `_` fused into `let_`
(`[T0001] unresolved name 'let_'`). Pre-existing on `30f9820b`.
Fix: `emit_token` + `token_boundary_needs_space` insert a `Whitespace` token
wherever two adjacent token texts would lex differently.

## SOLVE-1 + SOLVE-3 (Critical) -- FIXED

`unify_tys` bound an int/float var to the *raw* peer without resolving it, so
merging `(a, b)` with `(b, a)` installed `Ia := Ib`, `Ib := Ia` and every later
`resolve_ty_shallow_preserve_int` recursed forever -- `let t = if c { (a, b) }
else { (b, a) };` hung the compiler silently. The tuple arm passes element
types to `unify_tys` raw, so public `unify()` pre-resolution did not help.
Fix resolves the peer before binding, **scoped to the Int/Float arms only** --
no blanket chain-follow at the top of `unify_tys` (which broke stdlib cases in
session 9). SOLVE-3: added a visited set + depth guard to
`resolve_ty_shallow_preserve_int`, degrading to `Ty::ERROR` on a cycle.
Three regression tests SIGKILL pre-fix, pass post-fix.

## HIR-31 (Critical) -- PARTIALLY fixed (operand-local half)

`capture.local` is a THIR `LocalVarId`, but the closure-aggregate loop used
`LocalIdx::from_raw(capture.local.to_raw())`. The index spaces are not aligned:
an early-bound capture resolved to `LocalIdx(0)` -- the *return place* -- so the
closure captured the wrong storage. Fixed by resolving through `local_for_var`.
Regression test fails pre-fix with `LocalIdx(0)`.

**Remaining half of HIR-31 (still open):** `ByRef`/`ByRef(Mut)` captures are
still lowered as a `Copy` of the captured value, not a real reference -- no
aliasing, so `let mut c = 0; let mut f = || { c += 1; }; f(); f();` does not
observe `c == 2`. Needs the coordinated change:
1. typeck (`check_expr.rs` capture-field types): environment field type becomes
   `&T` / `&mut T` for `ByRef` captures.
2. lower (`lower_rvalue.rs`): emit `Rvalue::Ref(place, borrow_kind)` into a
   temp local and capture `Move` of that temp.
3. use sites: auto-deref when the closure body reads/writes the capture (the
   Field-on-ref machinery near `lower_expr_to_place` already exists).

## HIR-11 (Critical) -- FIXED

`desugar_multi_async_fn` split the body into `pre_segments[0..=n]` + a tail but
only emitted a subset:

(a) The Start arm Ready path skipped `pre_segments[1]` -- statements between
    await 0 and await 1 were dropped when the first future was Ready on the
    first poll (the common case).
(b) The last-await (`S_{n-1}`) Ready path never emitted `pre_segments[n]` --
    statements between the final await and the tail were never emitted anywhere.

Both produced `unresolved name` on
`let x = dep(a).await; let mid = x + 100; let y = dep(mid).await; ...`.
Fix emits `pre_segments[1]` (with `arm_rename(0)`) in the Start Ready body and
`pre_segments[n]` (with `arm_rename(n-1)`) in the last-await Ready body.
Regression test `hir11_statements_between_awaits_execute_runtime` (pipeline)
compiles + interprets the two-await program to 103; pre-fix it fails with
`unresolved name mid` / `unresolved name out`.

## FE-20 (Medium) -- FIXED

`is_keyword` used the range `KwFn ..= KwMacroRules` (excluding `KwMacro`,
`KwAsync`, `KwAwait`, `Lifetime`); `is_node` used `SourceFile ..< Error`
(excluding `Visibility`/`Vis*`, `WherePredicate`, `Bound`, `MetaVar`,
`MetaVarCrate`). Replaced with explicit `matches!` lists.

Root cause of the long-hidden status: `glyim-syntax`'s test module was **never
wired up** (`lib.rs` had no `mod tests;`), so `kind_tests.rs` -- with its own
incomplete copies of the same lists -- had never compiled or run. Now wired;
24 syntax tests run (previously 0). Both kind tests fail on the pre-fix ranges.

## Recommended next steps (priority order)

1. **HIR-31 remainder** -- by-ref capture aliasing (typeck `&T` field type +
   lower `Rvalue::Ref` + use-site auto-deref). Now the top open Critical.
2. **Audit follow-ups from this session's findings:** the `glyim-solve`
   `tests/unification.rs` (199 tests) is dead/rotted (not in `tests/mod.rs`,
   would not compile) -- either revive or delete. Same latent risk as the
   `glyim-syntax` dead test module fixed here.
3. Remaining High/Medium findings across the audit.

## Constraints (still in force)

- Never lower `Ty::ERROR` at codegen; pair `Expr::err(span)` with a diagnostic.
- Reproduce an audit finding against a freshly built CLI before fixing.
- **Regression tests must FAIL on pre-fix code** -- verify by restoring HEAD,
  appending ONLY the test, rebuilding, running. Beware: a failed pre-fix BUILD
  makes nextest silently run a STALE binary and report a false PASS. Guard the
  pre-fix run with explicit `grep -c` state checks (and abort on mismatch).
- Never `--emit=exec` a loop program without a kill guard; use `--emit=mir`
  (compile-only). `let _ = x;` fully DCEs, so a discarding-only macro yields an
  empty `.mir` -- use a value-consuming program to observe effects.
- Run the FULL workspace suite, not just the target crate.
- `nextest` filters on function names. `cargo fmt -p X` reformats unrelated
  pre-existing code -- restore those files (`git checkout --`) before committing.

## Tooling notes

- CLI binary is `target/debug/glyim-cli` (NOT `glyim`); input is positional:
  `glyim-cli <INPUT> --emit=mir`. Output goes to `<stem>.mir` next to input.
- `crates/glyim-solve/src/tests/unification.rs` (199 tests) is NOT declared in
  `tests/mod.rs` and would not even compile if added (10 errors) -- dead/rotted.
  Put new glyim-solve tests as top-level `#[test]` fns in `infer.rs`.
- Stale `.git/index.lock` / "unable to write new index file" is frequent this
  session; `rm -f .git/index.lock` and retry (the commit usually landed anyway).

## Where to look (quick index additions)

| What | Where |
|------|-------|
| Depth-aware macro bindings | `crates/glyim-meta/src/expander/{matcher,substitution}.rs` |
| Expansion token separators | `crates/glyim-meta/src/expander/mod.rs::emit_token` / `token_boundary_needs_space` |
| Int/Float unify + resolve guard | `crates/glyim-solve/src/infer.rs` |
| Closure capture operand | `crates/glyim-lower/src/lower_rvalue.rs` (Closure arm) |
| Closure body build | `crates/glyim-lower/src/builder.rs::lower_closure` |
| Async multi-await desugar | `crates/glyim-hir/src/lower/lower_async.rs::desugar_multi_async_fn` |
| SyntaxKind predicates | `crates/glyim-syntax/src/lib.rs::is_keyword` / `is_node` |
