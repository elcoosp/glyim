# Handoff — `glyim-v2` status + the param-bound-assoc-call bug

## Green state at this commit

`HEAD` = `fd247a85`. Working tree clean. Full suite **4176/4176 pass**.

- `glyim-cli --with-stdlib --emit=obj` on hello world -> valid 5152-byte
  Mach-O arm64 object.
- `glyim-cli --with-stdlib --emit=exec` -> 2.2 MB binary that prints `hello`,
  exits 0.
- **All five `--emit` modes work** (obj/exec/mir/llvm-ir/asm).

## Commits landed this session (2)

    fd247a85  fix(cache): invalidate on compiler rebuild; add per-emit-mode regression tests
    b63258ec  refactor(pipeline): share one front half across all emit modes
    9213df8d  docs: handoff -- hello world runs end-to-end, goal complete

### b63258ec -- pipeline unification (net -183 lines)

Extracted `Pipeline::prepare_compilation` (parse -> expand -> defmap -> hir ->
typeck -> lower -> borrowck -> opt -> monomorphize) and routed `emit_llvm_ir`
and `emit_asm` through it. Both previously re-implemented a *partial* chain
that stopped after `lower_body` and never ran `discover_mono_roots` /
`MonoCtx::collect`. Result: `--emit=llvm-ir` and `--emit=asm` ICEd on hello
world with `fn_abi_of failed: UnknownType`, because generic stdlib bodies
reached codegen with `TyKind::Param` in their locals. Now all five emit modes
succeed on the same program.

`emit_mir` and `compile_file_to_mir` still carry their own copies of the chain
(they have genuinely different semantics -- see "Follow-ups").

### fd247a85 -- cache + regression tests

**Cache bug (real, shipped):** `CompileCache::key` mixed in
`env!("CARGO_PKG_VERSION")` = `"0.1.0"` -- a *constant* across every dev build.
Two builds of the same version could emit different objects, so a
fix-and-rebuild cycle served the *pre-fix* object. Observed live: after fixing
the extern-fn symbol naming, `--emit=exec` still failed with `Undefined
symbols: ___glyim_fn_298` because the cache returned a pre-fix object. Fixed
by mixing in the compiler binary's path/length/mtime.

**Regression tests (`crates/glyim-cli/tests/emit_modes.rs`, 4 new):**
- `all_emit_modes_succeed_on_hello_world` -- the test that would have caught
  the `--emit=llvm-ir` monomorphization bug the moment it was introduced.
- `llvm_ir_output_looks_like_llvm_ir` / `asm_output_looks_like_assembly` --
  assert real content, not just a written file.
- `exec_binary_prints_hello` -- full end-to-end (link, run, assert stdout).
  Its linker-skip is deliberately narrow: a *failed* link fails the test,
  because an earlier draft that also skipped on `Undefined symbols` silently
  masked the stale-cache bug.

## The remaining latent bug: param-bound associated calls

`fn main() { let x = "42".parse::<i32>(); }` **ICEs**:

    fn_abi_of failed: UnknownType(Ty(16))   [Ty(16) = a Param]

This is the `T::method(args)` shape -- `str::parse<T: FromStr>` calls
`T::from_str(self)`, where the receiver is `&str` but the *impl* is selected
by the generic `T`. It is the one place the stdlib uses a param-bound
associated function, so it is the sole reachable instance.

### What was tried (all reverted -- do not re-derive)

I threaded a `self_ty: Option<Ty>` through the whole chain:
`thir::ExprKind::DynamicCall` -> `MirConstKind::VirtualMethod` -> codegen /
interp / mono.devirtualize / mono_cache.substitute_operand -> format. The MIR
then correctly emitted `VirtualMethod { self_ty: Some(T), .. }`.

**But the MIR still showed `self_ty: Some(Ty(16))` -- an unsubstituted
`Param` -- after monomorphization.** Instrumenting `enqueue`/`collect` in
`mono.rs` showed the root cause:

    [COLLECT] Fn(309) substs_len=0     # fn 309 = str::parse<T>

**`parse<T>` is enqueued with ZERO type arguments.** `substitute_body` is
therefore never given a substitution to apply (`substitute_body` was never
even called for it -- instrumentation confirmed 0 calls), so `T` stays a
`Param` and reaches codegen unresolved.

### Where the real fix lives

The bug is in **mono enqueue/discovery**, not in the `DynamicCall` shape. The
call `"42".parse::<i32>()` reaches MIR as a call to `parse` whose *operand*
carries no `[i32]` substitution, so `scan_terminator` enqueues
`MonoItem::Fn { def_id: 309, substs: <empty> }`. Two candidate causes:

1. **typeck does not record the call-site substitution** on the callee
   `FnDef(id, substs)` for this path. The turbofish `::<i32>` should give
   `substs = [i32]`. Check `check_expr`'s `Call` arm: for a normal generic
   call it builds `FnDef(def_id, substs)` from the args/turbofish; the
   param-bound path may bypass that.
2. **`discover_mono_roots` does not seed the generic** from the call site.

### Suggested next step

Dump the MIR of `main` for the parse program and look at the `Call`
terminator's `func` operand. If its `MirConstKind::Fn(def_id, substs)` has
`substs == []`, the fix is in `check_expr` (record the turbofish/inferred args
on the callee). If it has `[i32]` but mono still enqueues empty, the fix is in
`mono.rs`'s `scan_terminator` / `enqueue`.

This is a **monomorphization-completeness** issue, not a shape issue. The
`self_ty` threading I attempted is a *necessary* part of the eventual fix (the
devirtualizer needs the `Self` type once `T` is substituted), but it cannot
work until `parse<T>` is instantiated with `[i32]` in the first place. The
two changes should land together.

## Follow-ups (priority order)

1. **Param-bound associated call instantiation** (above) -- the last known
   reachable ICE. `"42".parse::<i32>()` must compile.
2. **`emit_mir` / `compile_file_to_mir` still duplicate the chain.** `emit_mir`
   is a *debugging dump* of pre-mono MIR (intentionally pre-mono); the
   interpreter path (`compile_file_to_mir`) monomorphizes on demand and needs
   drop-glue elaboration. Unify them onto `prepare_compilation` only if those
   semantics can be preserved via a flag.
3. **~10 silent `X0000 Err expression in THIR during lowering` warnings** per
   hello-world compile. Now traced: they come from `check_expr.rs:808` (the
   param-assoc-call path) and are the same root cause as (1). Fixing (1)
   should clear most of them; audit the rest against the 23 `thir::Expr::err`
   sites.
4. **`Place::ty`/`ty_mut`'s 12 `tracing::error!` sites** each return
   `Ty::ERROR` with no user diagnostic -- give them a diagnostic sink so a
   future type bug surfaces as an error at the span instead of an ICE far
   away.
5. **Runtime staticlib is ~26 MB (debug)** -- strip / release handling.

## Working-style constraints (still in force)

- **Never** lower `Ty::ERROR` at codegen -- `v15_t25_drop_error_type` is a
  `#[should_panic]` contract.
- Prefer fixing type resolution / registration once, everywhere.
- Every `thir::Expr::err(span)` should be paired with a
  `diagnostics.push(...)`.
- Conventional-commit prefixes; document remaining blockers.


## Update: turbofish + param-bound-assoc-call threading landed (`253c9794`)

Two real fixes landed since the previous handoff:

1. **Method turbofish is no longer dropped.** `x.parse::<i32>()` parses a
   `::<...>` list, but `hir::Expr::MethodCall` had no field for it — the
   `i32` was silently discarded. Added `generic_args: Option<Vec<TypeRef>>`,
   lowered it in `lower_method_call_expr`, and made
   `MethodDispatch::Static`'s callee-substs derivation prefer it over the
   receiver-derived substitution. `FnDef(parse, [])` became
   `FnDef(parse, [i32])`.

2. **Param-bound associated calls (`T::method(..)`) devirtualize.** Threaded
   `self_ty: Option<Ty>` through `DynamicCall` -> `VirtualMethod` (and all
   consumers). Previously the callee was an `Err` node that lowered to
   `MirConstKind::Error` and was never devirtualized.

Verified: a user-supplied `impl FromStr for MyInt` +
`"42".parse::<MyInt>()` compiles to a valid object. Full suite 4176/4176.

## Remaining: trait-bound enforcement at call sites (new, separate class)

`"42".parse::<i32>()` still ICEs — but NOT because of turbofish. The stdlib
declares `trait FromStr` with **no `impl FromStr for i32`**, so the bound
`T: FromStr` (with `T := i32`) is unsatisfiable. The same ICE fires for a
purely user-defined case:

    trait Foo { fn foo(&self); }
    fn needs_foo<T: Foo>(x: T) { x.foo(); }
    struct Bar;
    fn main() { needs_foo(Bar); }   // Bar has no `impl Foo`
    // => [X0000] expected function pointer or closure type for call operand

So the real gap is: **an unsatisfied trait bound at a generic call site is
not diagnosed; the un-resolvable call reaches codegen and ICEs.** The type
solver returns `DefiniteNo` (`glyim-solve/src/solver.rs`), and
`fulfill.rs::process_obligations` handles `DefiniteNo`, but nothing in
`check_expr`'s generic-call path *registers* the `T: Trait` obligation for
the call's inferred arguments, so no diagnostic is produced.

### Fix sketch

In `check_expr`'s `Call` arm, when the callee is a generic `FnDef(id, substs)`
whose signature has param bounds, register an `Obligation::Trait` for each
bound with the call's concrete substitutions and let
`FulfillmentCtx::process_obligations` reject `DefiniteNo` with a
`trait_not_implemented` diagnostic (the same one `resolve_method_call`
already emits at `check_expr.rs:3164`). That converts every unsatisfied-bound
ICE into a clean, spanned error.

The `trait_not_implemented` diagnostic constructor already exists and is
already used for the method-resolution analogue — this is wiring, not new
machinery. It is the highest-value next item: it closes the entire
"unsatisfied bound -> codegen ICE" class, not just `parse::<i32>`.


## Update: trait-bound ICE class closed + X0000 warnings eliminated

The two remaining items from the previous handoff are done.

### 1. Unsatisfied trait bounds are now diagnostics, not ICEs (`13c292e5`)

`check_unresolved_virtual_methods` was added to the post-monomorphization
checks (`glyim-lower/src/post_mono_checks.rs`) and wired into
`prepare_compilation`. Any `MirConstKind::VirtualMethod` callee that survives
monomorphization undevirtualized — an unsatisfied bound — is reported as a
clean, spanned `[T0001] the trait bound ... is not satisfied` diagnostic
instead of reaching codegen, which used to ICE with
`MirConstKind::VirtualMethod reached LLVM codegen`. Regression test:
`crates/glyim-cli/tests/trait_bound_diagnostic.rs`.

### 2. All 10 silent `X0000 Err expression in THIR` warnings eliminated

Three separate causes, each fixed:

- **`Expr::Let` / `Expr::Assign` in expression position** returned
  `thir::Expr::err` for a value that is really `()`. Now return a real
  `thir::ExprKind::Literal(thir::Literal::Unit)` (`c277a26b`). Killed 4.
- **`ErrorKind::InvalidInput` was referenced by `Error::invalid_input` (io.g)
  but never declared in the `ErrorKind` enum.** Fixed the stdlib source
  (`0c4f32ae`). Killed the last one.
- (The turbofish/devirt fixes earlier in the session killed the rest.)

A fresh `--with-stdlib --emit=obj` on hello world now emits **zero**
`X0000` warnings.

### Still open

- **`"42".parse::<i32>()`** produces the correct
  `trait bound \`i32: FromStr\` is not satisfied` diagnostic (because the
  stdlib has no `impl FromStr for i32`) — this is now *correct behaviour*,
  not a bug. If `parse::<i32>()` should actually work, the missing stdlib
  `impl` is the thing to add.
- **`glyim-runtime` reactor test is flaky** (`reactor_fd_registration_detects_readiness`
  fails intermittently on clean HEAD too; unrelated to compiler work).
  Worth stabilising: it makes a single `cargo nextest run` unreliable as a
  green/red gate.
- The remaining `emit_mir` / `compile_file_to_mir` chain duplication
  (documented above) — lower priority now that every emit mode works.
