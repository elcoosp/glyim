# Session 13 — handoff

## Landed: 16 commits, 11 bugs fixed

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

Workspace **4599/4599**. 67 behaviour fixtures pass, 0 fail.

## Fixed this session (11 root causes)

1.  `unify_tys` clobbered a bound Infer var with an unbound peer.
2.  Candidate loop leaked receiver-unify bindings from non-matching impls
    (the original `UnknownType(Ty(0))` layout ICE).
3.  Closure combinator: callee substs missing method generics; closure
    args unseeded.
4.  `variant_expr` ctor sig input/output referenced different vars
    (`Option::or`).
5.  HIR dropped closure param type annotations (`|x: &i32|`).
6.  `Pat::Binding` had no way to express `ref` (added `Pat::RefBinding`).
7.  `bind_pattern` stored a *copy* for `ref` bindings — `*v = ..` wrote
    into the copy. Now borrows the original place.
8.  `PatStruct` lowering emitted spurious fields named after bindings
    (`P { x: a }` → "no field `a`").
9.  Tuple LLVM type was opaque (single field) → const aggregate failed.
10. Shared variant-ctor `FnDefId` clobbered nested same-variant ctors.
11. `PatRef` had no lowering arm (tuple `ref` patterns).

## THE LAST OPEN BUG — symbol-name collision (fully diagnosed)

`Option<Option<i32>>::unwrap` (and `Result<Result<..>>`, and any nested
ADT unwrap chain) fails at codegen:

    [X0000] unexpected call return kind:
      call void @__glyim_fn_362(ptr byval({ [2 x i32] }) %arg9)

**Root cause, proven:** `FnDefId 362` is instantiated three times in one
compilation —

    FnDefId 362  Substitution([i32])          (unwrap::<i32>)
    FnDefId 362  Substitution([])             (generic / some other)
    FnDefId 362  Substitution([Option<i32>])  (unwrap::<Option<i32>>)

— but every one emits the LLVM symbol `__glyim_fn_362`. The first
definition wins; the others call it with a mismatched ABI, so the callee
is `void` while the caller expects an aggregate.

**Where it must be fixed** (`crates/glyim-codegen-llvm/src/lower.rs`):
the symbol is built as `format!("__glyim_fn_{}", def_id.to_raw())` at
FOUR sites, none of which account for the substitution:

  - line ~359  `MirConstKind::Fn` arm (callee resolution / declaration)
  - line ~6619 `lower_call` FnDef arm
  - line ~7239 `lower_body` (the definition)

**Fix shape:** introduce a shared `fn symbol_for(def_id, substs)` that
appends a hash of `substs`, and thread the substs:
  - `lower_body` currently receives only `body` (its
    `crates/glyim-codegen-llvm/src/lib.rs` caller passes
    `(context, &module, body, …)`). It has no substs — but the body IS
    already substituted (`mono_cache::substitute_body` rewrote
    `return_ty`/`locals`), so the instance identity is recoverable from
    the body's signature. Better: pass the `MonoItem`'s substs through
    from `Pipeline` (`lib.rs:143/256/317/…`), which already has the
    `MonoItemData`.
  - The two call-site arms have `substs` in scope.
  - `MonoItemData.symbol` is already `format!("{:?}", item)` — switch it
    to the mangled name and use it in `lower_body`.

**Not attempted** because it is a coordinated 4-site change spanning
`glyim-codegen-llvm` and the pipeline's `lower_body` call chain; two
prior multi-site scripts this session broke and had to be reverted. Do
it as a careful, `cargo check`-guided pass in a fresh session.

Fixture: `option_nested_unwrap.g` (ignore-marked).

## Tree state

Clean. HEAD `26cbb7bd`. Workspace 4599/4599.
