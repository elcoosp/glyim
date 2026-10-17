# HANDOFF — session 13 (final)

## 0. Status: GREEN, the last bug is fixed

The one remaining bug from §2 of the previous handoff
(nested `Option<Option<i32>>::unwrap` chains) is fixed. All 4599 tests
pass. The fixture `behavior/option_nested_unwrap.g` is un-ignored and
runs in the corpus.

    cargo nextest run --workspace    # 4599/4599

## 1. New commits on top of `26cbb7bd`

    665af582  test(behavior): cleanup option_nested_unwrap.g header + un-ignore
    521f7bd3  fix(typeck): resolve receiver through infer table when extracting callee substs
    89e278df  fix(codegen): thread mangled FnDef symbol through mono → codegen
    a12da7de  fix(pipeline): drop raw-generic bodies (return_ty = Param/Bound/Projection) from all_bodies

## 2. Root causes fixed this session

Nested unwrap was failing for **three** stacked reasons, each fixed in
its own commit:

### 2.1 Raw-generic bodies leaked into `all_bodies`
`MonoCtx::collect` enqueued the *unsubstituted* body of
`Option::<T>::unwrap` alongside its substituted instantiation. The raw
body's `return_ty` was `Param(0)`, which `fn_abi_of` → `layout_of`
cannot handle (`UnknownType(Ty(16))`).

**Fix** (`a12da7de`): `prepare_compilation` filters `all_bodies` with
`body_is_raw_generic` (return type is `Param`/`Bound`/`Projection`).
Checking `return_ty` alone (not locals) avoids dropping valid
error-typed bodies (`vec_is_empty.g`, `unsafecell_new.g`).

### 2.2 Symbol collision between instantiations
Even after the raw body is filtered, both `Option<i32>::unwrap` and
`Option<Option<i32>>::unwrap` mangled to the plain `__glyim_fn_362`.
The first definition wins, the second call site hits an undefined
symbol at link.

**Fix** (`89e278df`):
- New `glyim_type::mangle_fn_symbol(def_id, substs)` returns
  `__glyim_fn_<id>_<index>_<len>` for non-empty substitutions, the plain
  `__glyim_fn_<id>` for empty ones.
- `MonoItemData.symbol` is now derived from the item's
  `(FnDefId, Substitution)` pair.
- `PreparedCompilation.all_bodies` and `CompileArtifacts.mir_bodies`
  changed from `Vec<Arc<Body>>` to `Vec<(String, Arc<Body>)>`, threading
  the symbol through `CodegenBackend::generate` / `emit_assembly` /
  `emit_ir_to_string` / `emit_thinlto_bitcode_files` down to
  `lower_body`, which uses it as the LLVM function name.
- The two call-site mangle points (`MirConstKind::Fn` const operand and
  `lower_call`'s FnDef arm) compute the same name from their
  `(def_id, substs)`.

### 2.3 Typeck didn't give the inner call concrete substs
With (2.1) and (2.2) in place, link still failed because the *inner*
`.unwrap()` in the chain emitted `FnDef(362, [])` — empty substs —
while the definition was `FnDef(362, [Option<i32>])`. Probes showed:

    [GLYIM_MC] method=unwrap kind=Static recv_ty=Infer(TyVar(11747))
    [GLYIM_STATIC] fn_def=362 impl_part_len=0

`resolve_method_call` unified the receiver against `Option<i32>`, but
the `recv_ty` binding the dispatch arm reads is the *pre*-unification
`Infer` value; peeling refs off an `Infer` reaches no `TyKind::Adt`
arm, so `impl_part` came back empty.

**Fix** (`521f7bd3`):
- `MethodDispatch::Builtin` now derives the callee substs from the
  receiver's own ADT args (peeling refs/raw pointers), mirroring the
  `Static` arm. Previously it hardcoded `FnDef(id, [])` for *every*
  builtin, so a builtin fn that reached codegen with a generic impl
  substitution had no matching call-site name.
- `MethodDispatch::Static` calls
  `self.infer.resolve_ty_shallow(self.ctx, recv_ty)` before reading
  the ADT args, so a receiver that arrived as `Infer(TyVar)` (unified
  by `resolve_method_call` but not yet reflected in the local binding)
  yields the concrete `Option<i32>` substitution.

## 3. Verified working (all run via `--with-stdlib --emit=exec`)

    Option::Some(Option::Some(6)).unwrap().unwrap()         -> 6
    Result::Ok(Result::Ok(7)).unwrap().unwrap()             -> 7
    Option::Some(Result::Ok(8)).unwrap().unwrap()           -> 8
    Result::Ok(Option::Some(9)).unwrap().unwrap()           -> 9
    Option::Some(Option::Some(Option::Some(3)))....         -> 3
    let o: Option<Option<i32>> = ...; o.unwrap().unwrap()   -> 4
    Option::Some(5).unwrap()                                -> 5
    Option::Some(5).map(|x| x + 1).unwrap()                 -> 6
    Option::Some(3).and_then(|x| Option::Some(x+6)).unwrap()-> 9
    Option::None.or(Option::Some(8)).unwrap()               -> 8
    Option::Some(5).filter(|x| *x > 0).unwrap()             -> 5

## 4. Where the design lives

- `crates/glyim-type/src/substitution.rs` — `mangle_fn_symbol`
- `crates/glyim-pipeline/src/lib.rs` — `body_is_raw_generic`,
  `closure_body_has_unmonomorphized_ty`, and the `all_bodies` assembly
  that reads `MonoItemData.symbol`
- `crates/glyim-lower/src/mono.rs` — `MonoCtx::collect` symbol
  computation
- `crates/glyim-codegen/src/lib.rs` — `CodegenBackend::generate`
  signature
- `crates/glyim-codegen-llvm/src/lib.rs` — every `lower_body` caller
  now passes the symbol; the test helpers use `crate::tests::to_pairs`
- `crates/glyim-codegen-llvm/src/lower.rs` — `lower_body`'s
  `symbol: Option<&str>` parameter and the two mangle call sites
- `crates/glyim-typeck/src/check_expr.rs` — the `Static` / `Builtin`
  arms of the `MethodCall` dispatch now derive callee substs from the
  (infer-resolved) receiver

## 5. Known remaining work (not blocking)

The `unlinked-infer-var` class noted in the previous handoff had a
broader surface. This session fixed the slice that surfaced for nested
ADT unwrap. If future fixtures hit the same "plain `__glyim_fn_<id>`
referenced but not defined" link error, the same two-step diagnostic
applies:

1. `--emit=mir` and grep for `Fn(FnDefId(<id>), Substitution(..., len=0))`
   — an empty substitution where the receiver has concrete ADT args.
2. If the call site is `Static`/`Builtin`, check that the receiver was
   resolved through `infer.resolve_ty_shallow` before the ADT-args
   extraction.

## 6. Environment gotchas (still valid)

- `RUSTC_WRAPPER=kache` is set. Force rebuilds with `touch <file>`.
- `GLYIM_CACHE_DIR=$(mktemp -d)` bypasses the CLI compile cache.
- Read-only `.rmeta` in `target/`: `chmod -R u+w target`.
- Never remove probe code with `sed`/`python` line matching — the
  session-death-by-unbalanced-braces is still the easiest way to lose
  an hour. `git checkout -- <file>` for probe rollback.
- `strings target/debug/glyim-cli | grep -c 'GLYIM_TMP_XXX'` before
  trusting a probe's *absence* (0 = stale build).

---

## 7. Additional fix: type-annotation-needed diagnostic (rustc E0282)

The `Result::Ok(5).unwrap()` shape (un-annotated `E` generic on `Result`)
used to ICE the LLVM backend with `layout error building aggregate:
UnknownType(Ty(0))`. Not a regression from this session — identical at
base commit `26cbb7bd`. rustc rejects the same program with
`E0282: type annotations needed`.

**Fix**: after obligation fulfillment, typeck now walks every THIR body
and collects spans of `VariantCtor` calls whose result type still
contains an unbound `Infer(Ty(_))`. Spans are stored per-owner in
`TypeckResult.unresolved_ty_var_spans`. The pipeline emits the
diagnostic only for owners that actually reached the mono set — the
same set that would reach codegen — so unused stdlib template bodies
(which have unbound generics by design and are never monomorphized)
stay silent.

**Narrowing**: the walk is restricted to `Call { func: <VariantCtor> }`
expressions. A general "any unbound var in any expression type" walk
matched benign cases like `"3.14".parse::<f64>()`, whose return type
`Result<f64, <f64 as FromStr>::Err>` carries an unresolved projection
var that never reaches a value-construction site and does not ICE
codegen. Only the enum-construction shape is flagged.

---

## 7. Additional fix: type-annotation-needed diagnostic (rustc E0282)

The `Result::Ok(5).unwrap()` shape (un-annotated `E` generic on `Result`)
used to ICE the LLVM backend with `layout error building aggregate:
UnknownType(Ty(0))`. Not a regression from this session — identical at
base commit `26cbb7bd`. rustc rejects the same program with
`E0282: type annotations needed`.

**Fix**: after obligation fulfillment, typeck now walks every THIR body
and collects spans of `VariantCtor` calls whose result type still
contains an unbound `Infer(Ty(_))`. Spans are stored per-owner in
`TypeckResult.unresolved_ty_var_spans`. The pipeline emits the
diagnostic only for owners that actually reached the mono set — the
same set that would reach codegen — so unused stdlib template bodies
(which have unbound generics by design and are never monomorphized)
stay silent.

**Narrowing**: the walk is restricted to `Call { func: <VariantCtor> }`
expressions. A general "any unbound var in any expression type" walk
matched benign cases like `"3.14".parse::<f64>()`, whose return type
`Result<f64, <f64 as FromStr>::Err>` carries an unresolved projection
var that never reaches a value-construction site and does not ICE
codegen. Only the enum-construction shape is flagged.

---

## 8. Additional fix: `"3.14".parse::<f64>()` runtime segfault

`"X".parse::<T>()` on any primitive (u8/u16/u32/u64/usize/i8...i64/isize/f64)
compiled but **segfaulted at runtime** — verified pre-existing at base commit
`26cbb7bd`. Existing `primitive_trait_impl` tests only checked compilation, so
they did not catch it.

**Root cause** (in `check_expr::Expr::MethodCall` → `MethodDispatch::Static`):
the turbofish (`::<f64>`) was read as the *impl-level* substitution, but for
`impl str { fn parse<T>(&self) -> Result<T, T::Err> }` the impl has no
generics — the turbofish names the *method's* `T`. Result:
- Callee `FnDef` substs came out as `[f64, <fresh T>]` (arity 2) instead of
  `[f64]` (arity 1).
- The callee body's `return_ty` stayed `Result<Param(0), ...>`; `subst_ty`
  never saw the `f64`.
- The caller's destination local was `Result<Infer(..), <T as Trait6>::Err>`;
  zonk folded the unbound Infer to `Ty::ERROR`.
- At codegen the destination local's layout was the error-layout (opaque
  pointer-sized), but the callee wrote a `Result<f64, ...>` aggregate through
  it — the resulting IR `store { [2 x i64] } %sret_load, ptr null` wrote through
  a null pointer. Runtime SIGSEGV.

**Fix**:
1. `impl_part` now comes *only* from the receiver's own ADT args (peeling
   refs/raw pointers), the same way the `Builtin` arm was fixed. Never the
   turbofish.
2. A new `method_part` carries the turbofish args (or the fresh inference vars
   from `resolve_method_call` when there is no turbofish).
3. `all_substs = impl_part ++ method_part` — correct arity for both
   `impl<T> Foo<T> { fn m<U>(..) }` and `impl str { fn parse<T>(..) }`.
4. `ret_ty` is recomputed as `subst_ty(ret_ty, full_map)` where `full_map`
   covers both the impl portion and the (resolved) method portion. This folds
   both `T := f64` and the associated-type projection `<f64 as FromStr>::Err`
   into the destination local's type.

Verified working at runtime (all via `--with-stdlib --emit=exec` + `./out`):

    "255".parse::<u8>()                    -> ok
    "4294967295".parse::<u32>()            -> ok
    "18446744073709551615".parse::<u64>()  -> ok
    "-128".parse::<i8>()                   -> ok
    "-9223372036854775808".parse::<i64>()  -> ok
    "3.14".parse::<f64>()                  -> ok
    (all eleven primitive `FromStr` impls in one program) -> ok

Plus all previous fixes still hold (`Option<Option<i32>>` nested unwrap,
annotated `Result<Result<i32,i32>,i32>` unwrap, `.map`/`.and_then`, and the
E0282 rejection of un-annotated `Result::Ok(5)`).

---

## 9. Anti-regression fixtures added

Six behavior fixtures under `crates/glyim-test/tests/run-pass/behavior/`
lock in each distinct bug class fixed this session. The point of each is
that it would have *failed* before its corresponding fix:

| Fixture | Locks in | Failure mode if it regresses |
|---|---|---|
| `generic_fn_distinct_instantiations.g` | Per-`(FnDefId, Substitution)` symbol mangling (commit `89e278df`) | Link error: undefined `__glyim_fn_<id>` |
| `generic_fn_raw_body_filtered.g` | Raw-generic bodies filtered out of `all_bodies` (commit `a12da7de`) | ICE: `UnknownType(Ty(16))` in `fn_abi_of` |
| `generic_method_infer_receiver.g` | `MethodDispatch::Static`/`Builtin` resolve receiver through `infer.resolve_ty_shallow` (commit `521f7bd3`) | Link error, or wrong-ABI call |
| `generic_method_turbofish_all_primitives.g` | Method turbofish goes into the method's own generics, not the impl's (commit `e77fcc94`) | Runtime SIGSEGV (exit 139) |
| `nested_result_unwrap.g` | All of the above, at `Result<Result<i32,i32>,i32>` | Compile / link / runtime depending on which layer regresses |
| `mixed_adt_nested_unwrap.g` | Mangled key distinguishes `Option<X>::unwrap` from `Result<X, Y>::unwrap` at same substitution *length* | Link error or wrong-ABI call |

They are deliberately *behavioral* (they run and assert on the exit code)
rather than compile-only: the `FromStr` bug specifically slipped past the
existing compile-only `primitive_trait_impl` tests, because the miscompiled
program built fine but stored through a null pointer at runtime.

`crates/glyim-test/tests/ui/type_annotations_needed.g` (added earlier this
session, commit `5d022ec8`) locks in the E0282 diagnostic for un-annotated
`Result::Ok(..)` — the fixture is a real UI snapshot, not a compile-fail
marker, so the *text* of the diagnostic is pinned too.

### Known gap (documented, not fixed)

`crates/glyim-test/tests/ui/parse_result_used_in_match.g` is marked
`// ignore`. Using the parsed value in a `match`
(`match "42".parse::<i32>() { Result::Ok(v) => v, .. }`) hits
`internal compiler error: TyKind::Projection reached LLVM codegen – type
normalization incomplete`. The `Err` payload's type `<T as FromStr>::Err`
is not normalized before codegen. This is a distinct bug from anything
fixed this session (the discard-only use of `parse` works). The fixture
exists so the gap is *visible* and so the eventual fix has a place to
attach; removing the `// ignore` line makes it a live test.

---

## 10. Final fix: using the parsed value (pattern-match / unwrap)

The gap documented in §9 as "Known gap" is now closed. `let r =
"42".parse::<i32>(); match r { Ok(v) => v, Err(_) => -1 }` now runs and
returns 42; so does `"42".parse::<i32>().unwrap()`.

**Root cause**: `check_expr`'s `Expr::MethodCall` arm returns `(thir_expr,
ret_ty)`. The `thir_expr.ty` was recomputed by the `Static` dispatch arm from
the callee's final substitution (`Result<i32, ParseIntError>`), but the outer
`ret_ty` — which the return slot reported to the caller's `let` — still held
the pre-substitution shape from `resolve_method_call`
(`Result<T, <T as FromStr>::Err>` with a rigid `T`). So `r`'s binding was
`Result<Param(0), <Param(0) as FromStr>::Err>` while the call's destination
local was `Result<i32, ParseIntError>`.

The disagreement only became a compile-time failure when a downstream
`match r { Err(_) => .. }` tried to *lay out* the projection's field type:
the `subst_ty` Projection arm produced a self-referential substitution
`{0: <Param(0) as FromStr>::Err}` and the recursive substitution
overflowed before the type reached codegen — or, when the recursion guard
fired, the raw projection reached the backend and ICEd with
"TyKind::Projection reached LLVM codegen".

**Fixes**:
1. `Expr::MethodCall` now returns `thir_expr.ty` (the dispatch arm's
   precise type) as its second slot instead of the outer `ret_ty`. The
   dispatch arm is responsible for making `thir_expr.ty` the right shape —
   the outer binding is only a fallback for diagnostic paths, which set
   `thir_expr.ty` too.
2. `subst_ty` is now a thin wrapper around `subst_ty_depth` with an
   explicit `depth` argument. The guard fires at 64 levels and returns the
   type unchanged. This terminates the self-referential projection
   substitution that a call-site `T := <T as FromStr>::Err` shape can
   produce, without changing semantics for any non-cyclic map (real chains
   are a handful of levels deep).
3. `zonk::zonk_ty`'s Projection arm now attempts to normalize
   `Projection` types through `resolve_associated_type_by_self_ty_name`
   after resolving the projection's self type. This means a projection
   that *does* survive to zonk (e.g. one whose self is still a `Param` when
   zonk runs but a concrete ADT by the time a later substitution fills it
   in) still gets a chance to become the concrete defining type before
   codegen sees it.

**Verified working** (all via `--with-stdlib --emit=exec` + `./out`):

    let r = "42".parse::<i32>(); match r { Ok(v) => v, Err(_) => -1 }  -> 42
    let r = "abc".parse::<i32>(); match r { Ok(v) => v, Err(_) => -1 } -> 255 (exit)
    "42".parse::<i32>().unwrap()                                        -> 42
    let _ = "42".parse::<i32>();  (discard)                             -> 0
    "10".parse::<i32>().unwrap() + "20".parse::<i32>().unwrap()         -> 30
    mixed primitive types in one program                                -> 0

---

## 10. Final fix: using the parsed value (pattern-match / unwrap)

The gap documented in §9 as "Known gap" is now closed. `let r =
"42".parse::<i32>(); match r { Ok(v) => v, Err(_) => -1 }` now runs and
returns 42; so does `"42".parse::<i32>().unwrap()`.

**Root cause**: `check_expr`'s `Expr::MethodCall` arm returns `(thir_expr,
ret_ty)`. The `thir_expr.ty` was recomputed by the `Static` dispatch arm from
the callee's final substitution (`Result<i32, ParseIntError>`), but the outer
`ret_ty` — which the return slot reported to the caller's `let` — still held
the pre-substitution shape from `resolve_method_call`
(`Result<T, <T as FromStr>::Err>` with a rigid `T`). So `r`'s binding was
`Result<Param(0), <Param(0) as FromStr>::Err>` while the call's destination
local was `Result<i32, ParseIntError>`.

The disagreement only became a compile-time failure when a downstream
`match r { Err(_) => .. }` tried to *lay out* the projection's field type:
the `subst_ty` Projection arm produced a self-referential substitution
`{0: <Param(0) as FromStr>::Err}` and the recursive substitution
overflowed before the type reached codegen — or, when the recursion guard
fired, the raw projection reached the backend and ICEd with
"TyKind::Projection reached LLVM codegen".

**Fixes**:
1. `Expr::MethodCall` now returns `thir_expr.ty` (the dispatch arm's
   precise type) as its second slot instead of the outer `ret_ty`. The
   dispatch arm is responsible for making `thir_expr.ty` the right shape —
   the outer binding is only a fallback for diagnostic paths, which set
   `thir_expr.ty` too.
2. `subst_ty` is now a thin wrapper around `subst_ty_depth` with an
   explicit `depth` argument. The guard fires at 64 levels and returns the
   type unchanged. This terminates the self-referential projection
   substitution that a call-site `T := <T as FromStr>::Err` shape can
   produce, without changing semantics for any non-cyclic map (real chains
   are a handful of levels deep).
3. `zonk::zonk_ty`'s Projection arm now attempts to normalize
   `Projection` types through `resolve_associated_type_by_self_ty_name`
   after resolving the projection's self type. This means a projection
   that *does* survive to zonk (e.g. one whose self is still a `Param` when
   zonk runs but a concrete ADT by the time a later substitution fills it
   in) still gets a chance to become the concrete defining type before
   codegen sees it.

**Verified working** (all via `--with-stdlib --emit=exec` + `./out`):

    let r = "42".parse::<i32>(); match r { Ok(v) => v, Err(_) => -1 }  -> 42
    let r = "abc".parse::<i32>(); match r { Ok(v) => v, Err(_) => -1 } -> 255 (exit)
    "42".parse::<i32>().unwrap()                                        -> 42
    let _ = "42".parse::<i32>();  (discard)                             -> 0
    "10".parse::<i32>().unwrap() + "20".parse::<i32>().unwrap()         -> 30
    mixed primitive types in one program                                -> 0

---

## 11. Final fix: unwrap-then-cast on generic return types

The remaining gap noted in §10 ("`parse::<u8>().unwrap() as i32` trips
`invalid cast`") is closed. All of these now work:

    let x: Option<u8> = Option::Some(23); x.unwrap() as i32        -> 23
    "23".parse::<u8>().unwrap() as i32                             -> 23
    "42".parse::<i32>().unwrap() as i64 as i32                     -> 42
    let r: Result<i32,i32> = Result::Ok(5); r.unwrap() as i64 as i32 -> 5
    Option::Some(Option::Some(11u8)).unwrap().unwrap() as i32      -> 11

**Root cause**: `impl<T> Option<T> { fn unwrap(self) -> T }` has a fresh
`Infer(TyVar)` as its `FnSig.output` after `resolve_method_call`
instantiates the impl's param map. That var *is* bound by the receiver's
ADT unify (`Option<u8>` pins it to `u8`), but the `Static` dispatch arm
only ran `subst_ty(ret_ty, full_map)` — and `subst_ty` walks `Param(i)`
only; it never consults the inference table. So `ret_ty` stayed
`Infer(TyVar)`, and a downstream `as i64` failed `is_valid_cast` (which
has no `Infer(Ty)` source arm, only `Infer(Int)`/`Infer(Float)`).

**Fix**: after `subst_ty`, fold the remaining type through
`self.infer.resolve_ty_shallow(self.ctx, ret_ty)`. The bound var becomes
its concrete value (`u8`), the cast check sees `u8` and accepts.

This is exactly the same class as the earlier MethodCall-return-type fix
(`c6439a78`): the dispatch arm's `thir_expr.ty` (and the value it
returns to the caller) must be the *resolved* type, not a pre-inference
placeholder.

**Fixture**: `crates/glyim-test/tests/run-pass/behavior/generic_method_unwrap_then_cast.g`
(exit 140) covers the four shapes above with distinct primes so a
wrong-value regression produces a distinctly wrong exit code.

---

## 11. Final fix: unwrap-then-cast on generic return types

The remaining gap noted in §10 ("`parse::<u8>().unwrap() as i32` trips
`invalid cast`") is closed. All of these now work:

    let x: Option<u8> = Option::Some(23); x.unwrap() as i32        -> 23
    "23".parse::<u8>().unwrap() as i32                             -> 23
    "42".parse::<i32>().unwrap() as i64 as i32                     -> 42
    let r: Result<i32,i32> = Result::Ok(5); r.unwrap() as i64 as i32 -> 5
    Option::Some(Option::Some(11u8)).unwrap().unwrap() as i32      -> 11

**Root cause**: `impl<T> Option<T> { fn unwrap(self) -> T }` has a fresh
`Infer(TyVar)` as its `FnSig.output` after `resolve_method_call`
instantiates the impl's param map. That var *is* bound by the receiver's
ADT unify (`Option<u8>` pins it to `u8`), but the `Static` dispatch arm
only ran `subst_ty(ret_ty, full_map)` — and `subst_ty` walks `Param(i)`
only; it never consults the inference table. So `ret_ty` stayed
`Infer(TyVar)`, and a downstream `as i64` failed `is_valid_cast` (which
has no `Infer(Ty)` source arm, only `Infer(Int)`/`Infer(Float)`).

**Fix**: after `subst_ty`, fold the remaining type through
`self.infer.resolve_ty_shallow(self.ctx, ret_ty)`. The bound var becomes
its concrete value (`u8`), the cast check sees `u8` and accepts.

This is exactly the same class as the earlier MethodCall-return-type fix
(`c6439a78`): the dispatch arm's `thir_expr.ty` (and the value it
returns to the caller) must be the *resolved* type, not a pre-inference
placeholder.

**Fixture**: `crates/glyim-test/tests/run-pass/behavior/generic_method_unwrap_then_cast.g`
(exit 140) covers the four shapes above with distinct primes so a
wrong-value regression produces a distinctly wrong exit code.
