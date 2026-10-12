# HANDOFF — session 10 (COMPLETE)

**HEAD**: `0bfae79f`
**Matrix**: 4599 passing / 0 failing / 2 skipped
**Working tree**: only `docs/roadmaps/better-testing.md` untracked (not mine)

---

## 0. Resume

```bash
cd /Users/adm/Documents/Repos/glyim-v2
cargo check --workspace --tests        # --tests is REQUIRED
cargo nextest run --workspace --no-fail-fast
cargo build -p glyim-runtime           # for any --emit=exec probe
```

Debugging rules that saved this session (carry forward):

1. **Always use a fresh `GLYIM_CACHE_DIR`** (`rm -rf /tmp/gc_X`). The CLI
   caches aggressively; stale results cost hours.
2. **Compare `--emit=mir` to runtime behavior.** Disagreement = the bug is
   between MIR and execution (codegen, opt, interp), not in lowering.
3. **`Place::ty(): ...` errors in stderr mean a place resolved to
   `Ty::ERROR`.** They don't fail compilation; they signal the bug.
4. **`[X0000]` prefixes are internal codegen errors.** Grep `crates/` for
   the literal to find the emitter.

---

## 1. What was fixed (session 10)

Everything from the previous handoff's §4 plus the downstream bugs it
unblocked. Seven commits:

| Commit | Fix |
|---|---|
| `d30b63d7` | **§4**: sequential arm testing for enum field patterns. Prior code compiled every enum `match` to a single discriminant-only `SwitchInt`, so `Ok(0)` vs `Ok(n)` silently picked the first arm. Now each arm gets a bool test block (`discr==V && field0==lit0 && …`) chained in source order. Plus supporting fixes: `Downcast(v)+Field(i)` for variant-field projection, resolved-place-ty fallback for binding/field-literal locals, arm-body diverge handling, `BinOp::And`/`Or` (not `BitAnd`/`BitOr`) for bool chains, and `expr.ty`-derived substitutions on `VariantCtor` aggregates. |
| `a2bef8c4` | **typeck**: unify a concrete-Adt formal against a same-Adt argument. `f(Result::Ok(0))` where `f(r: Result<usize, i32>)` left the ctor's `E` as an unpinned infer var that zonked to `Ty::ERROR`, so the aggregate substitution was `Result<usize, ERROR>` and codegen failed layout with `UnknownType(Ty(0))`. |
| `ce5ee8ae` | **typeck**: guard that unify on the argument actually containing a `TyKind::Infer(_)`. Without the guard, a generic fn passing `Foo<T>` to a formal `Foo<T_impl>` produced a spurious `mismatched types: Adt359<T> vs T` and broke `assembled_stdlib_compiles`. |
| `da6f4364` | **typeck**: route statement-shaped match-arm bodies through `check_stmt_to_thir`. `Ok(n) => written += n` was passed to `check_expr`, whose `Expr::Assign` arm type-checks then *discards* the assignment (THIR has no assignment-expression node). Every `written += n` arm was silently a no-op → `write_all` looped forever (261 KB of "hi" then SIGSEGV) and the 7 println fixtures failed. Mirrors the block-tail `TAIL-ASSIGN-STMT` fix. |
| `e12deb06` | handoff doc (superseded by this one) |
| `ff097360` | (pre-session) finish PtrArith dispatch — E0004 + E0502 |
| `83af7f35` | (pre-session) clean resume handoff for session 10 |

**Verified working**:
- `probe_println_function.g`, `probe_println_macro.g`, `stdout_after_loop.g`,
  `stdout_conditional.g`, `stdout_middle_line.g`, `stdout_two_lines.g` — all
  `exit=0` with correct stdout.
- `assembled_stdlib_compiles` — green.
- User-declared enums matching `R::Ok(0)` vs `R::Ok(n)` — correct.
- stdlib `Result<usize, i32>` matching `Result::Ok(0)` vs `Result::Ok(n)` — correct.

**Remaining known issue** (minor, not blocking):
- `probe_string_push_str.g` produces no binary under `--emit=exec`. It's a
  separate stdlib path (`String::push_str`), not related to the enum-match
  family. The harness `run_pass_corpus_passes` passes regardless (it uses
  the interpreter path, not exec).
- `Place::ty(): Field projection on non-tuple/ADT type` is still logged
  many times during stdlib compilation, but does not affect correctness
  of the paths we exercise. Worth investigating as a cleanup task: the
  logs are noise that will hide future real errors.

---

## 2. The bugs, in the order they were discovered

1. **Discriminant-only switch** (§4 root cause): the original miscompile.
2. **Missing `Downcast` before `Field`** in `bind_pattern` +
   `lower_pattern_test`: `Place::ty()` bailed, field locals got `Ty::ERROR`.
3. **`pat.ty` unreliable** for field literals/bindings inside variant
   patterns: fell back to resolved place type.
4. **`lower_arm_body` overwrote `Return`** with `Goto(merge)` for diverging
   arm bodies.
5. **Bool `BitAnd`/`BitOr` unsupported** by the interpreter: switch to
   `BinOp::And`/`Or`.
6. **`VariantCtor` aggregate substitution hardcoded empty**: for generic
   enums, layout computing couldn't resolve fields.
7. **Call-site arg unification skipped concrete-Adt formals**: the ctor's
   fresh infer vars were never pinned.
8. **Statement-shaped match-arm bodies were silently discarded**: the
   direct cause of the `write_all` infinite loop.

Each was verified with an isolated probe before the next was addressed.
The `.fix-log`-style TMP-eprintln probes (`GLYIM_TMP_CTOR`,
`GLYIM_TMP_AGG`) were the key debugging tools; keep them in mind for the
next similar bug.

---

## 3. Files touched this session

- `crates/glyim-lower/src/lower_rvalue.rs` — §4 match lowering, Downcast
  projections, resolved-place types, arm-body diverge, bool helpers,
  variant-ctor substitutions.
- `crates/glyim-lower/src/tests/{match,match_advanced,or_patterns,range_patterns,slice_patterns}.rs`
  — updated to the new bool-switch-chain MIR shape.
- `crates/glyim-typeck/src/check_expr.rs` — Call-arm unify, match-arm
  statement-body routing, `ty_contains_infer` helper.
- `docs/roadmaps/HANDOFF-session-10.md` — this file.

---

## 4. Conventions

Marker comments: `T###-PATCHED`, `T###-REVIEWED`, `T###-REVISED`,
`T###-DEFERRED`. Commit style:
`fix(wave-N): TIDs (brief)` / `feat(...)` / `perf(...)` / `chore(...)` /
`docs(session): ...` / `test(area): ...`.

---

## 4.1. Additional fixes landed (post-handoff)

Two more fixes landed after the initial handoff doc:

- `b7bcbc47` **lower**: auto-deref reference scrutinees in `lower_match`.
  `match self { .. }` inside an `impl` method (and any
  `match &x { Variant::A(..) => .. }`) has scrutinee type `&Adt`, but the
  per-arm pattern tests project `Field(i)` off the underlying Adt. Without
  a `Deref`, `Place::ty()` bailed with "Field projection on non-tuple/ADT
  type" 15x during stdlib compilation. Materialize the pointee into
  `scrut_local`.

- `0bfae79f` **codegen-llvm**: skip `Drop` on unit/error-typed base local.
  A drop-elaboration-synthesized `Drop` on a place whose base is `()` (e.g.
  `_0.Field(0)` when `_0: ()`) is spurious. Check the base local's
  declared type *before* calling `place_ty()`, branch past the drop.

Result: zero `Place::ty()` error noise on the hello-world and println
probes; matrix still 4599/0/2.

## 5. Other useful docs

- `docs/roadmaps/glyim-fix-plan.md` — original 202-task plan.
- `docs/roadmaps/audit-status.md` — prior audit status.
- `.fix-log/HANDOFF-session-4-archive.md` — session-4 handoff, preserved.
