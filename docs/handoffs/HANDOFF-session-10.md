# Handoff -- glyim-v2, session 10 (continued from session 9)

## TL;DR

HEAD: `7b787335`. Suite: **4527/4528 pass** (2 skipped; the 1 failure is a
pre-existing arm64 linker issue, see below), clean tree.

Eleven commits landed: HIR-2, token fusion, SOLVE-1+SOLVE-3, HIR-31 (partial),
HIR-11, FE-20 (all Critical except FE-20 Medium), plus a batch of four
(FE-2, HIR-12, HIR-15, RT-7), HIR-16, and a revival of the dead
`glyim-solve` test suite (+258 tests).

## Commits landed this session

From `30f9820b` (session-9 wrap-up) forward:

1. `1c14a56f` fix(meta): HIR-2 -- depth-aware repetition bindings
2. `2e193f94` fix(meta): insert token separators in macro expansions
3. `804f6ef2` docs(handoff): session-10
4. `59a62211` fix(solve): SOLVE-1 + SOLVE-3 -- int-var binding cycles
5. `1016db75` fix(lower): HIR-31 (partial) -- closure capture operand wrong local
6. `bf0ac1a9` fix(hir): HIR-11 -- multi-await state machine dropped between-await statements
7. `a41ed261` fix(syntax): FE-20 -- is_keyword/is_node missed variants declared after ranges
8. `8edfcbd3` test(solve): revive the dead test suite (258 tests)
9. `d0bc46ed` fix: FE-2 + HIR-12 + HIR-15 + RT-7 (four findings)
10. `c349b296` fix(hir): HIR-16 -- replace reachable `unreachable!()`s
11. (this handoff)

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

## Later-session findings (FE-2, HIR-12, HIR-15, RT-7, HIR-16) -- FIXED

- **FE-2** (H): `(1,)` / `(1, 2,)` double-errored and swallowed the `)`. The
  tuple loop now breaks on a trailing comma before demanding another expr.
- **HIR-12** (H): `mut` on a `PatIdent` binding was hardcoded `Not`. The
  parser emits `mut` as a *sibling* of `PatIdent` inside `LetStmt`, so
  `pat_ident_mutability` scans children *and* the preceding non-trivia sibling.
- **HIR-15** (H): an int literal too large for `i128`/`u128` silently became
  `0`; now reports "integer literal is too large" via
  `lower_literal_with_diags`.
- **RT-7** (H): the bytecode VM's `Div`/`Rem` used `checked_*().unwrap_or(0)`;
  now traps (`VmError::AbnormalTermination`).
- **HIR-16** (M): two reachable `unreachable!()`s in expression lowering (block
  child / bin-op token) now emit diagnostics instead of ICE-ing.
- **HIR-14** (H) was already fixed on the tree (single left-to-right string
  unescape scanner present).

## Dead test suite revived (glyim-solve)

`crates/glyim-solve/src/tests/mod.rs` declared only 4 of 10 test files;
`solver.rs`, `unification.rs`, etc. had never compiled or run (same rot class
as the `glyim-syntax` module fixed earlier). Reviving needed: crate-root
re-exports of `BuiltinTrait`/`ImplDef`/`TraitDef`; `test_ty_ctx(|c| ..)` ->
`with_fresh_ty_ctx`; and refreshing ~10 expectations that had rotted against
intentional behavior changes (mutability is not a unify constraint; unbound
int/float default to i32/f64). `glyim-solve` went 53 -> 311 tests.

## HIR-31 remainder -- FIXED (7b787335)

`ByRef`/`ByRef(Mut)` captures now alias the enclosing binding. The full change
(the plan below, now landed) required four coordinated edits plus two latent
gaps it exposed:

1. **typeck** (`check_expr.rs` ~2163): the capture type recorded for a `ByRef`
   capture must become `Ref(_, T, mut)` (the environment field type). NOTE the
   closure *body* is type-checked at step 2, *before* capture classification at
   step 3, so body `VarRef`s are typed as `T` (not `&T`) -- the environment
   field is `&T` but the body expects `T`.
2. **lower** (`lower_rvalue.rs` closure arm): for a `ByRef` capture, emit
   `Rvalue::Ref(place_of_real_local, borrow_kind)` into a temp and capture
   `Move(temp)`.
3. **builder** (`builder.rs::lower_closure`): record which capture locals are
   by-ref; when lowering a closure-body `VarRef` to one of them, add a
   `ProjectionElem::Deref` (reuse the Field-on-ref machinery in
   `lower_expr_to_place`).
4. **interpreter**: the frame-relative `InterpValue::Ref { frame, local }` was
   NOT actually a problem -- the interpreter already resolves refs across frames
   (`locals_for_ref_frame` + `write_place_frame`'s generation checks). What it
   DID need: the closure value must be `[Fn(def_id), captures...]`, and
   `compile_file_to_mir` must register `lower_result.closure_bodies`. Both gaps
   were latent because closures had never executed at runtime before this.

**Note on the plan vs the result:** the handoff's step 1 assumed the body would
need a `&T` capture-field type AND body `VarRef`s typed `T`. In fact the body is
type-checked *before* capture classification, so its `VarRef`s are already `T`;
only the environment field type had to become `&T`, and lowering inserts the
deref. Also: assignment statements route through `check_stmt_to_thir`, whose
`Expr::Assign` arm (unlike `check_expr`'s) never set the capture `is_mut` flag --
so `c = c + 1` was classified `ByRef(Not)`. That was the crux.

Verified end-to-end: `closure_byref_runtime` (pipeline) interprets
`let mut c = 0; let mut f = || { c = c + 1; }; f(); f(); c` to `2`, plus a
shared-capture read-after-write case. Both fail pre-fix, pass post-fix.

## Recommended next steps (priority order)

1. Remaining High findings, e.g. SOLVE-4 (speculative coercions mutate the
   table with no rollback), SOLVE-5 (occurs check skips Projection/Dynamic),
   MIR-16 (`Rvalue::Ref` discards the projection), RT-1 (`resolve_target`
   indexes `block_offsets` unchecked).
3. Remaining Medium findings across the audit (FE-3 chained casts, FE-5
   negative-literal signs, HIR-13/20, ...).

## Pre-existing failure to be aware of

`glyim-cli::emit_modes::exec_binary_prints_hello` FAILS on this arm64 macOS
host: the linker cannot resolve `_glyim_stdout_write`. It fails **identically
on clean HEAD** -- not caused by any change in this or the prior session. It is
a native-codegen/link concern on non-Linux hosts; the in-process interpreter
tests are the authoritative on-host runtime proof.

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
