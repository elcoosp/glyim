# HANDOFF — session 13 (final)

## 0. FIRST: get back to green

The working tree currently has **uncommitted, broken** changes (a
`Body.symbol` threading attempt). Restore the last green state first:

    cd /Users/adm/Documents/Repos/glyim-v2
    git checkout -- crates
    cargo build -p glyim-cli
    cargo nextest run --workspace    # expect 4599/4599

- **Committed HEAD**: `26cbb7bd` — clean, 4599/4599 green.
- Only untracked file: `HANDOFF_SESSION_13_UPDATE.md` (older notes).

## 1. What is committed and fixed (16 commits from `6c528649`)

   26cbb7bd  test(behavior): un-ignore struct-pattern + tuple + ref fixtures
   7c59a2b9  fix(type): per-call-site variant-ctor FnDefId allocator
   7b1f38c8  fix(hir,typeck,codegen): struct-pattern fields, tuple types, fresh ctor ids
   3db58e69  test(behavior): anti-regression fixtures
   8e6af6f0  fix(lower,typeck): 'ref'/'ref mut' bind a borrow of the matched place
   118a18d1  test(behavior): gate ref-pattern fixtures
   9a4a4fdb  fix(hir,typeck): Pat::RefBinding variant
   23044196  fix(hir,typeck): closure param type annotations
   400a2653  fix(typeck): share ctor vars between sig input/output
   86e41177  test(behavior): document bug fixtures
   b2aa766d  test(behavior): regression fixtures
   8cdf704f  feat(typeck): closure-combinator support
   7c18ba49  fix(typeck): roll back receiver unify on no match
   b79eb98b  fix(solve): don't clobber a bound Infer var

**11 root-cause bugs fixed**, each with a passing behavior fixture under
`crates/glyim-test/tests/run-pass/behavior/`. Verified working:

    Option::Some(5).unwrap()                    -> 5
    Option::Some(5).map(|x| x + 1).unwrap()     -> 6
    Option::Some(3).and_then(|x| Some(x+6))     -> 9
    Option::None.or(Option::Some(8))            -> 8
    Option::Some(5).filter(|x| *x > 0)          -> 5
    Option::Some(ref v) => *v                   -> works (ref patterns)
    Option::Some(ref mut v) => *v = ..          -> works (write-through)
    P { x: a, y: b } => a + b                   -> works (struct patterns)
    (10, 20)  /  (ref a, ref b)                 -> works (tuples)
    |x: &i32| *x                                -> works (annotated closures)

## 2. THE ONE REMAINING BUG — nested ADT unwrap

    fn main() -> i32 {
        Option::Some(Option::Some(6)).unwrap().unwrap()
    }

Fails with:

    [X0000] fn type lowering failed: fn_abi_of failed: UnknownType(Ty(16))

Same for `Result<Result<i32,i32>,i32>` unwrap chains.

### Root cause (proven with probes)

`FnDefId 362` (stdlib `Option::<T>::unwrap`) is instantiated **twice**
and BOTH bodies reach codegen:

    [PL] all_bodies owner=362 symbol=Some("__glyim_fn_362_10433_1") return_ty=Ty(195)  # Adt(1010,[i32]) — GOOD
    [PL] all_bodies owner=362 symbol=None                          return_ty=Ty(16)   # Param(0)        — RAW GENERIC

`Ty(16)` is `Param(0)` (a rigid generic param). `fn_abi_of` cannot
`layout_of` a `Param`, so it ICEs. Two independent defects:

1. **The raw generic body is emitted alongside the substituted one.**
   Even after mangling, the raw body shares the plain name and ICEs.
2. **Symbol collision.** Before the raw body is dealt with, every
   instantiation emits `__glyim_fn_362`, so the first definition wins and
   later callers hit a mismatched ABI (`call void @__glyim_fn_362`).

### What I tried and why it failed

- **Mangle in `mono.rs` only** — codegen recomputes the symbol; it
  doesn't read `MonoItemData.symbol`.
- **Hash the concrete `FnSig`** — the definition and call sites compute
  *different* sigs (definition uses the substituted body's locals; the
  call site uses the receiver-resolved sig), so they hash to different
  names and the link breaks with `UnknownType(Ty(16))`.
- **`Body.symbol: Option<String>`** — correct in principle. Added the
  field, `mangle_fn_symbol` in `glyim-type`, `substitute_body` sets it,
  `lower_body` reads it, and call sites mangle from their `FnDef` substs.
  **The `(index, len)` of the substitution is the right mangling key.**
  But the workspace has ~200 `Body {` literals and my bulk-edit scripts
  kept mismatching (`basic_blocks:` vs shorthand `basic_blocks,`,
  HIR/THIR `Body` vs MIR `Body`), leaving the build broken.

### THE FIX (recommended, concrete)

The working-tree changes I left are 90% of the way. Two clean steps:

**Step A — deal with the raw generic body.** There is already a helper
`closure_body_has_unmonomorphized_ty(body, ctx)` in
`crates/glyim-pipeline/src/lib.rs` (~line 1159) that checks body
**locals** for `Param`/`Error`/`Projection`. Extend it to also check
`body.return_ty`, then filter `all_bodies` right after it is built
(after `let ty_ctx = db.get_ty_ctx()...`):

    fn closure_body_has_unmonomorphized_ty(body: &Body, ctx: &TyCtx) -> bool {
        ty_has_unmono(body.return_ty, ctx)
            || body.locals.iter().any(|l| ty_has_unmono(l.ty, ctx))
    }
    // ... after all_bodies is assembled:
    all_bodies.retain(|b| !closure_body_has_unmonomorphized_ty(b, &ty_ctx));

**Step B — mangle the symbol.** Add to `crates/glyim-type/src/substitution.rs`:

    pub fn mangle_fn_symbol(def_id: u32, substs: Substitution) -> String {
        if substs.is_empty() {
            format!("__glyim_fn_{}", def_id)
        } else {
            format!("__glyim_fn_{}_{}_{}", def_id, substs.index(), substs.len())
        }
    }

Then use it at three sites in `crates/glyim-codegen-llvm/src/lower.rs`:
  - `lower_body` (~line 7239): name from the body's *substs*. Note
    `lower_body` receives only `body` — the cleanest source of truth is
    to thread the symbol from the pipeline (see Step C), OR compute it
    from the substituted `return_ty`/locals via a `substs`-equivalent.
  - `lower_call` FnDef arm (~line 6640): `self.operand_ty(func)` gives
    `TyKind::FnDef(_, substs)` — mangle from that.
  - `MirConstKind::Fn` arm (~line 360): mangle from the const's `substs`.

**Step C (if threading from pipeline is preferred — more robust).**
Change `CodegenBackend::generate(&[Arc<Body>])` to
`generate(&[(String, Arc<Body>)])` in `crates/glyim-codegen/src/lib.rs`,
and pass `MonoItemData.symbol` (already `fn_symbol(def_id, substs)`)
through `all_bodies` in `crates/glyim-pipeline/src/lib.rs`. This avoids
recomputing the symbol in codegen entirely and guarantees the definition
and call sites agree.

## 3. Dead ends — DO NOT repeat

1. Threading `param_map` through `resolve_qualified_path` — no-op, wrong
   function (`Option<U>` is single-segment, resolved by
   `resolve_name_to_adt_ty`).
2. Widening `resolve_method_call` to return arg types — `i32 vs usize`
   stdlib regression.
3. Rigid `Param(i)` in ctor sigs — `T vs &T` / `parameter index
   mismatch` regressions.
4. Resolving the ctor output at the Call site or the `let` site — the
   `Some(8)` local is error-typed for a *different* reason (the shared
   ctor FnDefId; fixed in `7c59a2b9`).
5. Hashing the concrete `FnSig` for the symbol — def and call site
   compute different sigs; link breaks.

## 4. Environment gotchas (all cost real time)

- `RUSTC_WRAPPER=kache` is set. Force rebuilds with `touch <file>`.
- A **compile cache** in `glyim-cli` skips work and prints
  `cache hit — skipping compilation`; set `GLYIM_CACHE_DIR=$(mktemp -d)`
  to bypass.
- Read-only `.rmeta` in `target/` breaks builds: `chmod -R u+w target`.
- No `timeout(1)` on macOS; use the `setsid` + `killpg` runner pattern.
- **Never remove probe code with `sed`/`python` line matching** — it has
  deleted live code and left unbalanced braces multiple times this
  session. Use `git checkout -- <file>` to undo probe experiments.
- `strings target/debug/glyim-cli | grep -c 'GLYIM_TMP_XXX'` before
  trusting a probe's *absence* (0 = stale build).

## 5. Debugging method that worked

Env-gated `eprintln!` probes + `GLYIM_CACHE_DIR=$(mktemp -d)`. The
decisive probes this session:

    GLYIM_TMP_PL=1    # print every body in all_bodies with owner/symbol/return_ty
    GLYIM_TMP_LB=1    # print every body reaching lower_body
    GLYIM_TMP_FILT=1  # print how many un-monomorphized bodies were dropped

## 6. Test assets

- Canonical reproducer: `/tmp/p_map.g`, `/tmp/p_some.g` (may be pruned —
  recreate: `fn main() -> i32 { Option::Some(Option::Some(6)).unwrap().unwrap() }`).
- Ignore-marked fixture: `crates/glyim-test/tests/run-pass/behavior/option_nested_unwrap.g`.
  Un-ignore it once the bug is fixed.
- 67 behavior fixtures all pass at `26cbb7bd`.

## 7. Tree state at handoff

- HEAD: `26cbb7bd` (green base).
- Working tree: **DIRTY** — the `Body.symbol` attempt (broken build).
  Run `git checkout -- crates` to restore green before doing anything.
- Untracked: `HANDOFF_SESSION_13_UPDATE.md`, `HANDOFF_SESSION_13_FINAL.md`.
