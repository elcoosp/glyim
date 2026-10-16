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
