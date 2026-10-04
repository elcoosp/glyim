# Handoff -- glyim-v2, session 10 (continued from session 9)

## TL;DR

HEAD: `1016db75`. Suite: **4238/4238 pass** (2 skipped), clean tree.

Five commits landed. HIR-2 (Critical), a related non-audit Critical (macro
expansion token fusion), SOLVE-1+SOLVE-3 (Critical), and the operand-local
half of HIR-31 (Critical).

## Commits landed this session

From `30f9820b` (session-9 wrap-up) forward:

1. `1c14a56f` fix(meta): HIR-2 -- depth-aware repetition bindings
2. `2e193f94` fix(meta): insert token separators in macro expansions
3. `804f6ef2` docs(handoff): session-10
4. `59a62211` fix(solve): SOLVE-1 + SOLVE-3 -- int-var binding cycles
5. `1016db75` fix(lower): HIR-31 (partial) -- closure capture operand wrong local

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

## Recommended next steps (unchanged priority order)

1. **HIR-11** -- multi-await async desugar (`lower_async.rs`). 2-await async fn
   reports `unresolved name d`; 1-await hits "ambiguous method `poll`".
2. **HIR-31 remainder** -- the by-ref aliasing change above.
3. **FE-20** -- `SyntaxKind::is_keyword`/`is_node` numeric ranges exclude
   `KwAsync`/`KwAwait`/`Lifetime` and post-`Error` node kinds; replace with
   explicit `matches!` lists.
4. Remaining High/Medium findings across the audit.

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
| Async desugar | `crates/glyim-hir/src/lower/lower_async.rs` |
