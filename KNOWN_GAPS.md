# KNOWN_GAPS

Verified against the tree at `b91a585e`. Each entry names the code that
implements (or fails to implement) the behaviour, so a claim can be re-checked
rather than trusted. Anything not listed here either works or has a clean,
actionable diagnostic.

**Last audit:** this file was last regenerated from the working tree after a
session that closed ~20 gaps. The previous revision (2026-10-14) was stale:
three of its entries — `include!`, `--lto=thin`, and enum-variant value paths
— described behaviour that had since been implemented, and one
(`glyim-bytecode-vm` "referenced by nothing") was simply wrong. Treat an
entry here as a claim about *code*, not about intent.

---

## 1. Async: `.await` inside a loop body (category a — hard error)

`crates/glyim-hir/src/lower/lower_async.rs` rejects with `[T0060]`:

    `.await` inside a loop body is not yet supported by the async
    state-machine lowering (tracked: KNOWN_GAPS.md async-v2). Hoist the
    await out of the loop, or collect futures into a Vec and await them
    sequentially outside the loop.

This is a **deliberate, diagnosed gap**, not a silent miscompile. Single- and
multi-await sequential bodies both desugar; a loop needs a genuine
state-machine v2 (a suspension point that is the loop back-edge).

## 2. Windows SEH cleanup edges are unreachable (category b)

`crates/glyim-codegen-llvm/src/seh_ffi.rs` and `lower.rs`'s
`emit_seh_cleanuppad` implement the funclet path, and `Personality::Seh` is
selected for `x86_64-pc-windows-msvc` / `aarch64-pc-windows-msvc`. **Nothing
exercises it**, because *no lowering site ever sets `cleanup: Some(..)`* on a
`Call` or `Drop` terminator — MIR has the field and the backends handle it,
but lowering never populates it.

That is currently harmless: `glyim_panic` calls `process::exit(101)` rather
than unwinding, so no cleanup edge is ever taken. Verified — a panic inside a
`Drop` scope exits 101 and runs no destructor, on both targets.

If unwinding (`panic = "unwind"` semantics) is ever wanted, lowering must
populate `cleanup` edges first; until then the SEH code is dead but correct.

## 3. MIR interpreter: deeper writes through a `StaticPtr` — CLOSED (unreachable)

*(Historical note: the panic arm this described is still present, but the
only MIR shape that would reach it can't be produced from source in the
current compiler.)*

`write_projection_into_value` (`crates/glyim-mir-interp/src/lib.rs`) rejects
a `Deref` after a `StaticPtr` with an explicit panic. Reaching it needs a
`static mut` whose initializer contains a **reference** field (so that
writing through `*static.field = v` produces `Place{static, [Field, Deref]}`).

Verified unreachable:

- `static mut R: &'static mut i32 = &mut A;` — the const-evaluator rejects
  the initializer (`path expressions not yet supported in const eval for
  non-local paths`).
- `static mut S: S<'static> = S { p: &mut A };` — same rejection.
- A *local* `let s = S { p: &mut A }; *(s.p) = 9;` **does** produce
  `[Field, Deref]`, but on a local (not a `StaticPtr`), which takes the
  `Ref` arm of `write_place_frame`, not `write_projection_into_value`.

So no source program reaches the panic. If the const-evaluator ever gains
path-expression support for non-local paths, this entry becomes live again;
until then the reject arm is a correct guard against a shape that can't
occur.

## 4. `panic!` does not unwind (intentional, documented)

`glyim_panic` (`crates/glyim-runtime/src/lib.rs`) writes the message to
stderr and exits 101. It does not unwind, so destructors do not run on a
panic. This matches `panic = "abort"` and is what the `run-fail` corpus
asserts (`// exit-code: 101`).

The MIR interpreter has a cross-frame unwinding path (`panics_unwind`,
`tests/unwind_cleanup.rs`) that is **off by default**: enabling it changes the
top-level error variant every caller matches on (`Panic` → `Unwind`) and
breaks 14 tests that pin the current contract. That is a deliberate design
decision, not an oversight — see the doc comment on `with_panics_unwind`.

## 5. `include!` — implemented; proc-macro + pre-pass now aligned

`include!("path")` splices the target file's tokens at the call site, at both
item and expression position, resolving absolute paths and paths relative to
the calling file. Nested includes work, and **builtin macros inside an
included file are expanded** (verified: `include!("inner.g");` where
`inner.g` uses `concat!` in a function body compiles and runs).

The previous caveat — "the expander does not run proc-macro expansion inside
an included file" — was imprecise. The full pipeline *always* ran proc
expansion; the issue was the `entry_main_local_id` **pre-pass**, which
expanded with a builtin-only expander so its `LocalDefId` numbering could
diverge from the real pipeline's when a proc macro emitted items before
`main`. The pre-pass is what decides which body codegen treats as the entry
point, so a shifted id meant the wrong function ran.

Fixed by threading the proc registry into the pre-pass
(`entry_main_local_id_with_proc_registry`); the harness's compiler uses it.
The wrapper `entry_main_local_id(db, path)` keeps the old no-registry
behaviour for unit-test callers that don't have a registry to give. CLI and
`glyip` still call the wrapper (they build the registry themselves at a
layer above the pre-pass); if a user program combines an item-emitting proc
macro *and* `include!`, upgrade those two call sites too.

## 6. Deferred (category c — no code path, no diagnostic needed)

- **Windows SEH landingpads.** See §2: implemented, structurally unreachable.
- **`dyn Trait` niche layouts.** `needs_drop` returns `false` for types it
  cannot inspect (`dyn Trait`, generic params, projections, error/opaque),
  documented on `TyCtx::needs_drop`. Conservative and sound.
- **Fine-grained partial-move drop guards.** `drop_flags` models "should this
  whole local's `Drop` run", not per-field state. A partial move disables the
  parent's destructor entirely — sound (over-retains, never double-frees),
  but leaks the still-initialized siblings.

## 7. `for x in <slice-iterable>` — CLOSED

*(Historical note: this gap was open from session 15 until it was closed
by the lowering change described here. Kept in the file so a future reader
who remembers the symptom can trace the fix.)*

**Symptom.** The trait-path lowering of `for x in v.as_slice()` /
`for x in a.iter()` routed through the stdlib's `impl Iterator for &[T]`
(iter.g:332), whose `next` returns `&T` into its own transient frame:

    fn next(&mut self) -> Option<&'a T> {
        let cur: &[T] = *self;
        ...
        let first = &cur[0];        // Ref into `next`'s frame
        Option::Some(first)
    }

When `next` returned, its frame died, and the caller's `*x` panicked
with `deref of Ref from dead frame 1`. LLVM handled it (it doesn't care
about frames); the **interpreter did not**. A `run-pass` fixture could
not be added — the harness runs the interpreter, so the fixture would
fail on the interp path.

**Fix (this session).** `lower_rvalue.rs`'s for-loop lowering now takes
the *index-walk* path for any slice iterable, not just bare arrays:

- `take_array_path` fires for `TyKind::Ref(_, Slice(_))` (a `&[T]`,
  which is what `v.as_slice()` and `a.iter()` return), for bare
  `TyKind::Slice(_)`, and for the existing `TyKind::Array(_, _)`.
- The element read (`iter_local[Deref, Index(i)]`) uses the same
  `[Deref, Index]` projection shape that typeck emits for `s[i]`,
  after which codegen's `pointee_is_fat` path (lower.rs:664) and the
  interpreter's `ProjectionElem::Deref` arm handle the fat pointer
  identically. The item is then wrapped in a `Ref` to build the `&T`
  that `pat` binds — mirroring `impl Iterator for &[T]`'s `Item = &T`,
  but with the reference target in the caller's frame.
- The `Rvalue::Len` place also gets a `Deref` prepended for the
  ref-slice case, matching what `s.len()` emits.

Both backends now agree on every shape. The trait path for `Iterator`
on user-defined types (custom `impl Iterator for Counter`, etc.) is
untouched — only slice iterables route around it.

**Verified.**

- LLVM: `for x in v.as_slice()` → exit 3; `for x in a.iter()` → exit 6.
- Interp: same (harness covers both backends through the fixture).
- No regression to `for x in [1,2]`, `for i in 0..3`, `s[i]`, `s.len()`.
- Fixtures: `behavior/for_loop_over_slice.g`,
  `behavior/for_loop_over_slice_iter.g`.

**`IntoIterator for Vec<T>` — implemented in stdlib; for-loop wiring is
the only remaining piece.**

**Status as of this commit:**

- `crates/glyim-lang-alloc/lib/vec.g` now contains
  `pub struct VecIntoIter<T>` + `impl<T> Iterator for VecIntoIter<T>`
  + `impl<T> IntoIterator for Vec<T>`.
- **`v.into_iter()` works on both backends.** Verified by fixture
  `behavior/vec_into_iter_explicit.g` — a `let mut it = v.into_iter()`
  followed by a manual `match it.next()` loop returns 6 for `[1,2,3]`,
  matching LLVM and the interpreter.
- A first probe of `v.into_iter()` failed with "mismatched types:
  expected integer type, found `<?ty as Trait37>::Item`". That failure
  does not reproduce in the final tree — the associated-type
  projection (`<Vec<T> as IntoIterator>::Item = T`) resolves correctly
  once the impl is registered and the method-call return-type
  substitution runs. The earlier probe was against a stale binary.

**What still does not work: `for x in v` (the syntactic sugar).**
The for-arm in `check_expr.rs` looks up `Iterator::next` on `iter_ty`
directly; it never calls `IntoIterator::into_iter`. So a
`IntoIterator`-only type (which `Vec<T>` now is) is still rejected at
the for-head.

**Attempted this session (twice) and reverted.** The natural wiring is:

- `Expr::For` in `check_expr.rs`: resolve `IntoIterator::into_iter` on
  `iter_ty`; run the `Iterator::next` scan on its **return type**
  instead of `iter_ty`.
- Lowering: emit that `into_iter` call before the loop, re-bind the
  iteration local to its output.
- New THIR field `ForIteratorNext::into_iter: Option<ForIntoIter>` +
  matching `IteratorNextInfo::into_iter: Option<IntoIterInfo>`.

Both attempts produced a **codegen ICE** where the previous behavior
was a clean reject:

    fn type lowering failed: fn_abi_of failed: UnknownType(Ty(7292))

**Actual cause (verified by a `[DBG_RES]` probe in `check_expr.rs`
inserted right before the new `if iter_is_by_value_vec ...` block).**
The resolution block **never fires for the user's `for x in v`.** The
probe was set up to print `iter_kind` at every `Expr::For` typechecked
during the compilation, and every logged line was a stdlib-internal
loop:

    [DBG_RES] iter_is_by_value_vec=false iter_is_concrete=true iter_kind=Adt(AdtId(1000), …)   // Range
    [DBG_RES] iter_is_by_value_vec=false iter_is_concrete=true iter_kind=Ref(Erased, Ty(36), …) // slice ref
    …

Not a single `iter_kind=Adt(AdtId(1020), …)` line — the user's `Vec`
for-head never reaches our block at all. So the emitted MIR showed no
`Fn(419)` (the correct `VecIntoIter::into_iter`) call, the `into_iter`
local kept its pre-`into_iter` type (`Vec<i32>`), and the subsequent
`next` call — resolved against `VecIntoIter` from `iter_ty_for_next`,
which was computed inside the *never-entered* block — landed on an
inconsistent type. Hence `UnknownType`.

**Why the block never fires is not yet diagnosed.** Likely candidates,
in order of suspicion:

1. The user's `for x in v` may be typechecked **after** the stdlib's
   own items and, by the time it reaches `Expr::For`, `iter_ty` is
   already `Infer(...)` rather than the resolved `Adt(1020, …)`. Our
   probe fires on the raw `iter_ty` *before* any inference resolution,
   so if the user's `v` is bound to a `?T` that resolves to
   `Vec<i32>` later, the block's `matches!(TyKind::Adt(1020))` silently
   takes the `false` branch. Stdlib `for` loops bind their iterands to
   already-resolved types (Range literals, slices), so they all pass;
   the user's program is the only one that sees the `Infer`.
2. `main`'s `Expr::For` may be typechecked during a phase that our
   probe doesn't cover (the crash means typeck *did* run, but no
   `Adt(1020)` line appeared — the probe's own print is inside
   `check_expr`'s `Expr::For` arm, so if it didn't fire, that arm
   either ran with a non-`Adt(1020)` iterand or ran somewhere our
   instrumented copy is not visible). This is the second attempt to
   instrument it — the first (`DBG_INTO_ITER` above) *did* resolve
   `fn 419` correctly, which means the two probes are seeing *different*
   `Expr::For` nodes. The working resolution in the first probe likely
   came from a *separate* typecheck invocation (a pre-pass) where
   `iter_ty` was already concrete; the *final* pass that lowering
   consumes runs with `iter_ty = Infer`, which our concrete-only
   check skips.

**Next step for a future attempt.** Instead of gating on
`matches!(TyKind::Adt(1020))`, first resolve `iter_ty` through the
inference table:

    let resolved_iter_ty = self.infer.resolve_ty_shallow(self.ctx, iter_ty);

and match on `resolved_iter_ty`. That is exactly the pattern the
existing `is_unresolved` block uses (`check_expr.rs:707`). The gating
`iter_is_by_value_vec` should consult `resolved_iter_ty`, not the raw
`iter_ty`, for the same reason. This should make the block fire for the
user's `for x in v` — and, because `resolve_trait_method_fn` was
already verified (via `[DBG_INTO_ITER]` probe) to correctly return
`fn_def_id 419` for `Adt(1020, [i32])`, the rest of the wiring is
already known to work.

**Corrected misdiagnosis.** The earlier text in this entry said the
resolved fn id was `9162` (`to_vec`). That was a misreading of the MIR
dump: line 23 of the emitted MIR is the *source-level*
`[1,2,3].to_vec()` call, not the `into_iter` call. The actual
`into_iter` emission was **missing entirely** — see the `[DBG_RES]`
result above.

**Fifth attempt (also failed, reverted).** Following the
`resolve_ty_shallow` recommendation, the `iter_is_by_value_vec` gate was
changed to match on
`self.infer.resolve_ty_shallow(self.ctx, iter_ty)` instead of the raw
`iter_ty`. Same result: `for x in v` still ICEs with
`fn_abi_of failed: UnknownType(Ty(7292))`. So inference resolution was
not the (only) blocker either.

**Concrete facts established across five attempts:**

- `resolve_trait_method_fn(Adt(1020, [i32]), IntoIterator, into_iter)`
  returns `fn_def_id = 419` with output `Adt63<i32>` (VecIntoIter<i32>)
  — verified by a `[DBG_INTO_ITER]` probe. The resolution step *can*
  work.
- In one run, `[DBG_INTO_ITER]` fired on `Adt(1020, [i32])`. In another
  run (with the same gating shape in a different probe, `[DBG_RES]`),
  **no** `Adt(1020)` line was ever printed — only `Adt(1000)` (Range)
  and `Ref(Erased, Ty(36), …)` (slice). Two probes in structurally
  identical positions produced *different* `iter_ty` populations. That
  is the crux: the program is typechecked more than once, and at least
  one of those passes sees `Adt(1020)` (the pre-pass) while the pass
  whose THIR reaches lowering may not (or vice versa).
- The lowering's `[DBG_LOWER_II]` probe consistently reported
  `into_iter.is_some=false` — i.e. whatever THIR copy lowering actually
  consumes does not carry the field set by typeck's pass.

**What the next attempt must do first, before writing any wiring:**
determine **which typeck invocation lowering consumes**. Concrete
approach:

1. Add an unconditional `eprintln!` in `check_expr.rs`'s `Expr::For`
   arm (no gate, every invocation) printing `iter_ty` and
   `std::thread::current().id()`.
2. Add a matching unconditional `eprintln!` in lowering's `For` arm
   printing `next.is_some()`, `next.as_ref().map(|n|
   n.into_iter.is_some())`, and thread id.
3. Run `for x in v` once. Compare the thread ids / pass counts. If
   there are two typeck passes and lowering consumes the second, trace
   *why* the second pass discards the `into_iter` field — likely
   `zonk.rs` or a THIR clone/rebuild somewhere between typeck and
   lowering that doesn't preserve the new field.

Until that's done, further attempts will keep producing the same ICE.
There is no shame in leaving this as an honestly-rejected diagnostic —
`v.into_iter()` is the documented workaround, and it works on both
backends today.


**Already-working fallbacks** (unchanged): `for x in v.as_slice()`,
`for x in &v`, and `for x in v.into_iter()` (explicit).

## 8. Interp: `w.v.len()` on a `Vec<T>` field — CLOSED

*(Historical note. Verified working on both backends.)*

**Symptom.** `w.v.len()` where `v: Vec<T>` is a struct field returned the
wrong value in the interpreter (1 for a 3-element Vec):

    struct W { v: Vec<i32> }
    fn main() -> i32 {
        let w = W { v: [1, 2, 3].to_vec() };
        w.v.len() as i32         // LLVM: 3. Interp: 1 (before this fix)
    }

**Root cause.** `Rvalue::Ref` in `crates/glyim-mir-interp/src/lib.rs`
built `InterpValue::Ref { frame, local }` and **discarded
`place.projection`**. For `w.v.len()`, the MIR is:

    _6 = &mut _5.field0;          // Rvalue::Ref(place{5, [Field(0)]})
    _7 = <Vec<i32>>::len(move _6)

`builtin_deref` resolved the Ref to `frame_locals[5]` — the whole
`W { v, i }` struct (a 2-field `Aggregate`). `vec_len` then fell back to
`fields.len()` = **2**, which the previous exit-code fixture (1) showed
was off by the struct's field count.

**Fix.** `InterpValue::Ref` now carries `projection:
Box<[ProjectionElem]>`. `Rvalue::Ref` copies `place.projection` into it;
`builtin_deref` walks the projection (currently `Field` only) before
handing the result to the builtin. PartialEq on `Ref` deliberately
compares only `(frame, local)` — address identity, not structural
equality — so existing pointer-comparison tests are unaffected.

**Verified.**
- LLVM: `w.v.len()` → 3; `w.v.is_empty()` → 1.
- Interp: same (via `run_pass_corpus`, which uses the interpreter).
- Differential (both backends agree): green.
- Fixtures: `behavior/vec_field_len.g`, `behavior/vec_field_is_empty.g`.

## Verification

Two gates cover most of the above; run both before claiming a gap is closed.

```
# Full workspace suite (includes the run-pass corpus, which asserts exact
# exit codes end-to-end).
cargo nextest run --workspace --test-threads=2

# Differential: compiles every run-pass AND run-fail fixture once and runs
# both backends, asserting they agree on exit code and stdout. This is the
# gate that catches "works on LLVM, silently wrong in the interpreter".
GLYIM_TEST_REAL_LLVM=1 cargo nextest run -p glyim-test --test differential \
  --features real-llvm --test-threads=2
```

One-off reproducers (always bypass the CLI compile cache):

```
GLYIM_CACHE_DIR=$(mktemp -d) ./target/debug/glyim-cli --with-stdlib \
  --emit=exec /tmp/x.g -o /tmp/x.out && /tmp/x.out; echo $?
```

`--emit=mir` and `--emit=llvm-ir` write to the `-o` path, not stdout.

#### 3.4.x — s16 final verdict (revert-only)

**Diagnosis complete. Fix requires architectural change. Session 16 ends with
the 6th attempt fully reverted to `919575fa`.**

The immediate symptom was wrong self type in `VecIntoIter<T>::next`'s return:

    fn next(&mut VecIntoIter<T>) -> Option<<T as Iterator>::Item>   // WRONG
    fn next(&mut VecIntoIter<T>) -> Option<<VecIntoIter<T> as Iterator>::Item>  // RIGHT

`Self::Item` in the impl-method return type was resolved via the Script 63
fast path in `crates/glyim-typeck/src/tyconv.rs::resolve_path_type`, which
looks up `ctx.param_bounds_for(qname)`. That's a **crate-wide flat**
`HashMap<Name, Vec<(Name, TraitDefId)>>` **keyed only by the bare
parameter name**, so `"I"`, `"T"`, `"Self"`, `"U"` etc. accumulate a bound
from every impl/fn in the crate that declares a param with that name.

Three concrete collisions observed during s16 investigation:

1. `"Self"` was in `param_bounds` (registered by *some* impl's bound
   writer keyed by the literal name `"Self"`). This made the fast path
   fire for `Self::Item` and build `<Param(0, "Self") as SomeTrait>::Item`
   — a projection whose self type is a bare generic param, not the impl's
   declared self type. Guarding the fast path against `"Self"` fixed this
   specific case, but exposed problem 2.

2. **`Iterator` and `IntoIterator` both declare `type Item`.** In
   `impl<I: Iterator> Iterator for Skip<I>`, the resolver for the RHS of
   `type Item = I::Item;` saw two candidate bounds for `"I"`: the current
   impl's `I: Iterator`, and a *stale* `I: IntoIterator` registered by an
   earlier impl elsewhere. Heuristics tried and rejected this session:
   * `.first()` — picks whichever was registered earliest (wrong when
     IntoIterator came first).
   * `.filter(declares(aname)).last()` — fails because
     `trait_def(Iterator).associated_types` is empty during early passes
     (assoc types not yet registered), so the filter excludes the correct
     bound too.
   * "prefer declaring, else `.last()`" — combines both failure modes,
     introduced *new* regressions (`trivial_42` failed, `for x in &v`
     segfaulted, corpus went red on the LLVM path).

3. **Generic index hardcoding.** The fast path built
   `ParamTy { index: 0, name }` unconditionally. In
   `impl<T, U: TryFrom<T>> TryInto<U> for T`, `U` has index 1; hardcoding
   index 0 produced `<Param(0, "U") as TryFrom>::Error` which never
   unified with the correct `<Param(1, "U") as TryFrom>::Error`. This
   surfaced as `mismatched types: <U as Trait179>::Error vs
   <U as Trait178>::Error @66447..66464` in the assembled stdlib.

**The structural blocker:** any heuristic over `param_bounds` cannot be
sound, because the map is unordered w.r.t. the *current* impl scope, and
the correct bound is not identifiable from the name alone.

### Recommended fix (options)

* **A. Extend `param_map`** (currently `HashMap<Name, Ty>`) to carry
  per-scope trait bounds, e.g. `HashMap<Name, (Ty, Vec<TraitDefId>)>`.
  Threaded through `resolve_type_ref` / `resolve_path_type` /
  `resolve_fn_sig` / `check_body`. Small at each call site, but touches
  ~15 signatures.

* **B. Scope `param_bounds` by owner** — key on `(DefId, Name)` or use a
  scoped save/restore around each impl/fn walk. Same signature churn as
  A to preserve access to enclosing scopes.

* **C. Full trait solver.** Out of scope for a single session.

Preferred: **A**, because it makes the correct bound impossible to
confuse with a stale one — you can only see the bounds the current
scope declared.

### What must be reapplied once A/B land

1. `crates/glyim-type/src/ty_ctx.rs` — the
   `subst_impl_params_into_assoc` helper (shared by all three
   `resolve_associated_type*` resolvers). It substitutes the impl's
   generic params into its stored assoc type when the query self type
   supplies concrete arguments, so `<VecIntoIter<i32> as Iterator>::Item`
   normalizes to `i32` instead of leaving `Param(0)`.
   **Not committable alone** — s16 verified it regresses `for x in &v`
   (`[T0001] mismatched types: expected integer type, found &i32`) by
   enabling extra projection normalization in cases where the current
   resolver produces wrong-but-consistent types. Pairs with the resolver
   fix.

2. The `for x in v` wiring (spec'd in HANDOFF s16 §4): `thir::ForIntoIter`,
   `check_expr.rs`'s `Expr::For` arm (`into_iter` resolution + output_ty
   substitution), `lower::IntoIterInfo`, `lower_rvalue.rs`'s into_iter
   emission, `pipeline_context.rs` fallback.

### Test fixtures to add with the fix

* `for x in v` (Vec<i32> [1,2,3]) → exit 6
* `for x in v` (empty Vec) → exit 0
* `for x in v` (Vec<String>, sum lengths) → sum length
* Regressions to keep green: `for x in &v` → 6, `for i in 0..3` → 3,
  `v.into_iter()` explicit → 6, `for x in a.iter()` → 6,
  user `impl Iterator for Counter` → 3, Bug A `Wrap<i32>::v[w.i]` → 20.

### Do not repeat

Do not attempt further heuristic tweaks to `.first()` / `.last()` /
"prefer declaring" in the fast path or in the second 2-segment block.
The map is fundamentally unordered w.r.t. the current scope, and no
heuristic on it can be sound. Fix the scope first.


#### 3.4.y — s17 CLOSED

`for x in v` (v: Vec<T>) now compiles and runs correctly on both backends.
All ten targeted cases pass: sum-of-[1,2,3]→6, empty→0, [1,2,3,4,5]→15,
single element→elem, last-of-[1,2,3]→3. Plus all pre-existing regressions
(`for x in &v`, `for i in 0..3`, explicit `v.into_iter()`, `for x in
a.iter()`, `for x in [1,2,3]`) stay green.

**The three commits that closed it** (on top of `5a427776`):

1. `f17d262c` — `subst_impl_params_into_assoc` in ty_ctx.rs. Substitutes
   the query self type's concrete args into the impl's generic slots of
   its stored assoc type, so `<VecIntoIter<i32> as Iterator>::Item`
   normalizes to `i32` instead of leaving `Param(0, T)`. Standalone
   correctness fix; required by both later commits.

2. `c1f568df` — `Self::X` pre-check in `resolve_path_type`'s fast path.
   For `Self::X` where `param_map["Self"]` is the impl's declared self
   type, try `resolve_associated_type_by_self_ty(impl_self_ty, "X")` first
   — this returns the impl's registered assoc type, normalized through
   (1). Falls through to the projection-building path when the impl's
   own assoc entry isn't registered yet (during the impl's own
   `type Item = ...` registration) — same raw template either way.
   Without this, `Self::Item` resolved to `<Param(0, "Self") as
   Iterator>::Item`, which at monomorphisation became
   `<i32 as Iterator>::Item` and ICEd as `UnknownType`.

3. `55276722` — the `for x in v` wiring:
   * `thir::ForIntoIter` carried on `ExprKind::For.into_iter` (Option).
   * Typeck's for-arm resolves `IntoIterator::into_iter` for `Vec<T>` via
     **`resolve_into_iter_exact`** — exact receiver match (same `AdtId`,
     no autoref, no autoderef). The generic `resolve_trait_method_fn`
     walks `Vec<T> -> [T] -> &[T]` and matches
     `impl IntoIterator for &[T]` (Item = `&T`) before the correct
     `impl IntoIterator for Vec<T>` (Item = `T`), yielding a
     slice-iterator whose output (a 2-word fat pointer) was written into
     a `VecIntoIter<i32>` slot (3-word). Each iteration then read an
     `&i32` as `i32` — the s17 "sum = 12 for [1,2,3]" garbage. The exact
     matcher unifies the impl's self type against the receiver so the
     impl's inference vars bind to the Vec's concrete element; its stored
     output template substitutes to `VecIntoIter<i32>`.
   * The `Iterator::next` scan operates on that output type.
   * Lowering emits `into_iter(v)` into a fresh local, rebinds
     `iter_local` and `iter_ty` to it before the loop header.
   * Segfault guard (`is_vec_by_value` rejection) now fires only when
     `into_iter` was NOT resolved.

**Diagnostic trap caught during s17** — worth remembering: `FnDefId`s
must be looked up via `self.body_owner_map` (the id space
`pre_allocate_impl_method_ids` populated), NOT `hir.body_owners` (a
different HIR-side id space). The latter produced
`[X0000] MIR body not found for DefId LocalDefId(NNN)` because NNN is
an unrelated body. The working `resolve_trait_method_fn` already does
this correctly at check_expr.rs:~4586 — copy its pattern.

**Fixture:** `crates/glyim-test/tests/run-pass/behavior/for_loop_over_vec_owned.g`

#### 3.5 — Borrowck: loop-condition borrow outlives the condition (P1-C8, s18)

**Status:** OPEN. Blocks P1-C9 (delegating compile_file_to_mir to
prepare_compilation). Discovered while attempting that refactor: the
delegation is correct — it routes compile_file_to_mir through the
*same* strict pipeline --emit=exec uses — but the strict pipeline
reports 4 diagnostics on the full assembled stdlib that the old
compile_file_to_mir silently swallowed by skipping borrowck.

Of the 4, 2 are a real borrowck bug (the other 2 are Err-expression-in-
THIR from fs.g, tracked below).

**The bug.** A borrow created inside a while *condition* is treated
as live for the entire loop body:

    struct F<'a> { inner: &'a mut Inner, buf: &'a [u8], written: usize }
    fn go(&mut self) -> i32 {
        while self.written < self.buf.len() {
            let _n = self.inner.write(&self.buf[self.written..]);
            self.written = self.written + 1;
        }
        0
    }

Produces [B0001] use of partially moved value: local_1.* twice.
Real stdlib site: net.g WriteFuture::poll (line ~304).

**Narrowed test matrix** (each isolated, no stdlib):

| variant | result |
|---|---|
| while cond borrows self.buf AND body borrows self.buf[..] | FAIL |
| while self.written < 3 { ... &self.buf[r..] ... } | OK |
| while self.written < 3 { ... self.buf ... } (no range) | OK |
| (no loop) let _n = ...write(&self.buf[r..]); | OK |
| inner.two(&self.buf[0..1], &self.buf[r..]) (two slice args) | OK |

The trigger is specifically: the loop *condition* borrows the same
place the loop *body* borrows. Rust's NLL ends the condition's borrow
when the condition expression finishes. Glyim's move_analysis.rs / loan
model treats the loan as live while the *destination local* of the
reference is live — and for a condition expression lowered into the
loop header, that local's liveness spans the body.

**Fix shape.** Two candidates:

* A. Region-based: give each temporary its own Region, end it at
  the last use (real NLL).
* B. Local-scope heuristic: mark locals allocated by
  lower_expr_to_operand's temp path as "condition temp", and in
  move_analysis treat their loan as ending at the end of the enclosing
  expression rather than at local death.

B is the smaller change and unblocks P1-C9; A is the principled fix
and subsumes P1-C8 (loan-through-rebind unsoundness the original plan
also lists).

**Also blocking P1-C9:** two Err-expression-in-THIR diagnostics from
fs.g (File::metadata, create_dir_all-adjacent File method). These are
ExprKind::Err reaching lowering in non-raw-generic bodies. The
drop_raw_generic_err_warnings filter only clears them for raw-generic
bodies; the failing bodies are concrete. Root cause not yet isolated
— an isolated extern "C" { } inside a fn body lowers cleanly, so it
is specific to the fs.g shapes.

**Evidence.** At 489ece72, --emit=exec on the full assembled stdlib
reports 4 diagnostics:
  - two [X0000] Err expression in THIR during lowering @111277, @119668
  - two [B0001] use of partially moved value: local_1.* @131362, @131381

stdlib_full_probe passes only because it calls compile_file_to_mir,
which (at 489ece72) still skips borrowck.

#### 3.5.1 — P1-C9 blocker progress (post a97c1bef)

Since the §3.5 write-up, two of the four P1-C9 blockers are fixed:

* `489ece72` — auto-reborrow `&mut` field operands. Cleared the two
  `net.g` `WriteFuture::poll` / `ReadFuture::poll` `[B0001]` sites.
* `a97c1bef` — `Copy` (not `Move`) for `&T` shared-ref field operands.
  Same class, shared-ref shape.

Three diagnostics remain from `--emit=exec` on the full stdlib at
`a97c1bef`:

1. `fs.g:152` (`File::metadata`)         `Err expression in THIR`
2. `fs.g:447` (`impl FromStr for i16`)   `Err expression in THIR`
3. `thread.g:275` (`Option::None`)       `[B0001] partially moved`

(1)/(2) are specific to the `fs.g` shapes — an isolated
`extern "C" { }` inside a fn body lowers cleanly, so the failing
construct is something else in those methods.

(3) looks like a second Copy-vs-Move instance, this time for a struct-
literal field assignment `stack_size: Option::None`. `Option<usize>`'s
payload is Copy, but the field expression itself is lowered as `Move`
by the generic catch-all in `lower_expr_to_operand` — the same shape
`a97c1bef` fixed for `&T` fields, but for an `Option<usize>` payload.

The `compile_file_to_mir` → `prepare_compilation` delegation is ready
and correct; it is reverted at each attempt only because it would make
`stdlib_full_probe` fail on these three pre-existing diagnostics.

#### 3.5.2 — Partial-move analysis rejects disjoint field moves from owned self

Discovered while narrowing §3.5.1 blocker (3) (`thread.g:275`).

**The bug.** Moving one field out of an owned (consumed) struct, then
reading a *different* field of the same struct, is rejected:

    struct S { a: Option<usize>, b: Option<usize> }
    fn take(_x: Option<usize>, _y: Option<usize>) -> i32 { 0 }
    fn main() -> i32 {
        let s = S { a: Option::None, b: Option::None };
        take(s.a, s.b)         // <- [B0001] use of partially moved value
    }

Rust allows this: `s.a` moves field `a` and leaves `s.b` intact; the
second argument's `s.b` accesses a **disjoint** field of `s`, which is
still initialized. Same shape as `Builder::spawn` in `thread.g:275`:
`spawn_impl(self.name, self.stack_size)` inside `impl Builder { fn
spawn(self) -> ... }` — `self` is consumed, and moving two non-Copy
fields into the callee's parameters is legal.

**Current behaviour.** `move_analysis.rs` reports
`[B0001] use of partially moved value: local_1.*` on the second
argument. That diagnostic is emitted from `check_place_used` at line
~824-852: after `moved.contains(mp_idx)` for the root local `s`, *any*
field access is rejected. The check is at the level of the root
`MovePath`'s children list, but doesn't consider that a sibling child
(field `b`) is untouched.

**Fix shape.** The check has two levels today:

  * If `place.projection.is_empty()` → "use of moved value" (correct,
    whole-struct move).
  * If `place.projection` names a specific field → check whether that
    **specific field's move path** is in `moved`, not whether any child
    of the root is.

The relevant code is `check_place_used` at `move_analysis.rs:~800-860`.
`move_paths.find(place)` already produces a `MovePathIdx` for the exact
projection; the problem is the *fallback* branch that scans
`root_index[place.local].children` for any moved child and reports the
whole local. That branch should either (a) not fire when
`place.projection` is non-empty (the exact-projection check above is
authoritative), or (b) restrict the "any child moved" check to
`place.projection.is_empty()`.

**Consequence for P1-C9.** This is the last blocker for
`compile_file_to_mir -> prepare_compilation` other than the two
`fs.g Err-expression` sites (§3.5.1 items 1 and 2). Fixing this would
unblock the stdlib's `thread.g` module from the strict pipeline.

**Verification fixture.** `tests/compile-pass/borrowck/disjoint_field_moves.g`
(future) should assert the code above compiles; conversely
`tests/compile-fail/borrowck/field_after_field_move.g` should reject
`let a = s.a; s.a;` (using the *same* field twice).

#### 3.5.3 — CLOSED (P1-C9 unblocked)

All three blockers from §3.5.1 are fixed:

* `489ece72` — auto-reborrow `&mut` field operands (net.g sites)
* `a97c1bef` — `Copy` for `&T` shared-ref field operands
* `4deae722` — `count_fields` falls back to `adt_def.fields.len()` for
  user struct ADTs, giving per-field move paths (thread.g site)

The two remaining `fs.g Err expression in THIR during lowering`
diagnostics from §3.5.1 items (1)/(2) turned out to be
`DiagSeverity::Warning`, not `Error`. `prepare_compilation`'s
`has_errors()` correctly tolerates them, exactly as `--emit=exec`
already did. So `compile_file_to_mir`'s inline
`drop_raw_generic_err_warnings` filter has no analogue in
`prepare_compilation` — and doesn't need one.

**Landed:** `0a9f3e2d` — `compile_file_to_mir` now delegates to
`prepare_compilation`, closing P1-C9. `stdlib_full_probe` passes with
the delegation in place. Fuzz target (`fuzz_compile`) and glyip test
discovery now exercise the same pipeline as `--emit=exec` /
`--emit=obj`, including borrowck and optimizer passes.

#### 3.5.4 — P1-C8: `VarRef` lowering emitted `Copy` for every type, hiding all move-after-move

Attempted during this session (2026-10-25). Reverted — the fix is a
much larger change than it looked.

**The bug.** In `lower_expr_to_rvalue` (and `lower_expr_to_operand`)
the `thir::ExprKind::VarRef` arm unconditionally emits
`Rvalue::Use(Operand::Copy(place))`, regardless of whether the read
type is `Copy`. This is why every move-after-move in the compiler is
silently accepted:

    let s = S { a: Option::None, b: Option::None };   // S: !Copy
    let y = s;
    let z = s;          // BUG: accepted, both reads "copy" an unmoved s
    take(s); take(s);   // BUG: accepted
    let a = v; let b = v; // BUG: accepted for Vec<i32>
    let a = s; let b = s; // BUG: accepted for String

Compare with the `Field` read arm (lower_rvalue.rs ~1521), which does
the `is_copy` check and emits `Move` + `register_partial_move` for
non-Copy types. `VarRef` never got the same treatment.

**What was tried.** Add the same `is_copy` check to both `VarRef` arms:

    let is_copy = self.ctx.ty_ctx().is_copy(expr.ty);
    if is_copy { Copy(place) } else { Move(place) }

**Why it failed.** The fix exposed every latent move-after-move in the
stdlib itself. Every `with-stdlib` compile broke on:

    iterator::max (stdlib iter.g, ~line 1260):

        fn max(self) -> Option<Self::Item> where Self::Item: Ord {
            let mut result = self.next()?;
            let mut this = self;
            while let Option::Some(item) = this.next() {
                if item > result {
                    result = item;   // move of non-Copy `item`
                }
            }
            Option::Some(result)
        }

`result = item` is a genuine `Move` of a non-Copy `item` into
`result`. Each iteration rebinds `item` via the `while let` pattern, so
it *should* be fresh — but `move_analysis`'s dataflow unions the
previous iteration's `moved` set into the loop header, and pattern
bindings (`while let Some(x) = ...`) do not call `record_init` on the
bound local. So the second iteration trips `[B0001] use of moved value`.

**Scope.** Three intertwined sub-problems, each substantial:

1. **Pattern binding needs to register inits.** `while let` / `match` /
  `if let` all bind fresh locals each execution; the dataflow must
  model them as re-initializing. `move_analysis` has no
  pattern-bind statement kind — the pattern's `Assign(dest, _)` already
  exists in MIR, so this may just need the loop-header dataflow to
  forget the pattern-bound locals on each back-edge. Careful work.

2. **`MovePathArena` needs to be sized upfront.** My earlier on-demand
  child creation attempt (`Field` arm returning parent → creating new
  path) ICE'd because the move-dead/move-init bitsets are sized by
  `arena.len()` at `build_move_paths` time. Growing the arena mid-
  analysis panics. Enum variants need `Downcast` children created here,
  which is why §3.5.2 explicitly left enums on the old
  "collapse-to-root" behaviour.

3. **The stdlib contains latent move-after-move.** Even if (1) and (2)
  are fixed, `iterator::max` may still be a real problem in the
  current lowering (it may need `let item = match this.next() { Some
  (x) => x, None => break };` — a rebind — if `while let` doesn't
  produce a fresh binding in the current MIR shape).

**Follow-up.** This is a multi-session project, not a one-line fix.
Reverting the `VarRef` change restores the (unsound but working) status
quo. The soundness hole is real — `take(s); take(s);` and `let y = s;
let z = s;` compile and would produce UB at runtime if `noalias` were
emitted. It becomes a miscompile the moment alias attributes land in
codegen.

**Correct end state.** Soundness hole closed AND stdlib compiles with
strict move semantics. Sequence:
  a. Fix dataflow (1) — pattern-bound locals reinit at back-edges.
  b. Fix arena (2) — size `MovePathArena` to a conservative upper bound
     or grow the bitsets with it.
  c. Re-apply the `VarRef` is_copy check.
  d. Fix the stdlib (3) as needed.
  e. Full workspace + differential gate.

#### 3.5.5 — Struct-variant field binding in patterns is wrong (P1-C6, corrected)

Attempted P1-C6 this session (2026-10-25) and found the audit's
description inaccurate. The bug is not `tag_prefix` offset skew in
`codegen-llvm/lower.rs` — it is struct-variant **field name → index**
resolution in pattern lowering.

**Reproducer.**

    enum E { A { x: u8, y: u64 }, B }
    fn main() -> i32 {
        let e = E::A { x: 11, y: 42 };
        match e {
            E::A { y, x } => (y as i32) * 100 + (x as i32),
            E::B => 0,
        }
    }

Expected `42 * 100 + 11 = 4211`. Got **`118`**.

**Diagnosis.** The MIR for `match e { E::A { x, .. } => x }` uses the
projection `Downcast(VariantIdx(0)), Field(FieldIdx(0))` — correct for
`x`. But for `E::A { y, x }` and `E::A { x, y }`, the produced
projections bind to the wrong `FieldIdx` (garbage value 118 rather than
1142 or 4211 in either order — no arithmetic on the expected values
produces this, so the pattern binder is reading raw bytes at the wrong
offset / width rather than swapping the two fields).

**Where.** Pattern lowering of `PatternKind::Struct { fields: [(name,
subpat)], .. }` — the name-to-`FieldIdx` lookup for a struct *variant*
(as opposed to a struct *item*). Likely in `hir/lower/lower_pat.rs` (the
pattern lowering that turns a named field into a `FieldIdx`) or in
typeck's pattern checker where the HIR field name is resolved against
the variant's `AdtDef::variants[v].fields`.

Nothing to do with `tag_prefix` in codegen — the audit's `P1-C6`
heading ("enum bare-Field offset skew: niche encoding uses wrong tag
prefix; direct encoding mixed-alignment write/read mismatch") describes
a *different* case that:
  * the mixed-alignment enum `enum E { A(u8), B(u64) }` now handles
    correctly (verified: `E::B(42)` → 42);
  * the 2-field niche enum `enum E { A { x: u8, y: u64 }, B }` inherits
    by construction — its failure is field-name lookup, not byte offset.

**What works today** (verified this session):
  * `enum E { A(u8), B(u64) }` → 42
  * `Option<&T>` niche enum → correct
  * tuple variant `E::A(u8, u64)` → 42
  * unit-variant enums → correct
  * `Option<u64>` in a `match` → correct
  * nested `Outer::Wrap(Inner::X(42))` → correct

**What is broken:**
  * struct-variant pattern with named fields — wrong `FieldIdx`, wrong
    value.

**Next session.** Probe `hir/lower/lower_pat.rs` (or wherever
struct-variant patterns are lowered) for how `{ field: pat }` names map
to `FieldIdx`. Fix that one mapping. The `codegen-llvm/lower.rs`
`tag_prefix` logic audited as P1-C6 may still hide a real bug for
*unit + non-Copy* enums (`E { A, B(u64) }` — the audit's niche case),
but it is not what blocks user struct-variant patterns today.

**Update (2026-10-25, second probe).** LLVM IR examination narrows the
suspected cause to aggregate **alignment**, not field-name lookup.

For the failing case `enum E { A { x: u8, y: u64 }, B }`:

* Store side (correct offsets): `x = 11` stored at byte 8 with
  `align 1`; `y = 42` stored at byte 16 with **`align 4`** (should be
  `align 8` — `u64` on ARM64 has natural alignment 8).
* Aggregate load/store round-trips (`load { [3 x i64] }`,
  `store { [3 x i64] }`) all use `align 4`.
* Field reads at MIR level use the correct offsets: `Downcast(0),
  Field(0)` reads byte 8 (x), `Downcast(0), Field(1)` reads byte 16
  (y).

So the field indices and byte offsets are *correct*; the *alignment* is
wrong. Both pattern orders (`x*100+y` and `y*100+x`) produce the same
wrong value 118, which suggests the aggregate round-trip with `align 4`
corrupts the stored `i64` value (possibly through a poorly-optimized
misaligned 8-byte store that overlaps adjacent bytes, or through
spilling that assumes 8-aligned layouts while the actual local is only
4-aligned).

**Correct diagnosis.** The alignment computation in
`codegen-llvm`'s `store_to_place` / `load_from_place` /
`build_layout_aggregate` paths is passing `align 4` where the field's
natural alignment is 8 (u64) or the aggregate's is 8
(`{[3 x i64]}`). This is the audit's actual P1-C6 finding — "mixed-
alignment write/read mismatch" — just not for the niche-tag-prefix
reason the audit described.

**Next step (proper debugging).** Build with `-Z llvm-print-after-all`
(or equivalent) on the failing case and inspect the machine code
around the misaligned store. Or add `assert(ptr.is_aligned_to(natural))`
in `place_ptr` / `build_store` helpers in `codegen-llvm` to catch the
misalignment at the source. The fix is likely a one-line change in the
alignment lookup — the layout engine (`glyim-layout`) already knows the
correct per-field alignment; the codegen just isn't consulting it.

#### 3.5.6 — P1-C6 CLOSED (struct-variant patterns) + P1-C1 CLOSED (clone + into, all paths)

Two of the outstanding issues resolved this session:

**P1-C6 (struct-variant pattern field binding)** — `a71ce481`.

The audit's description ("enum bare-Field offset skew: niche encoding uses
wrong tag prefix; direct encoding mixed-alignment write/read mismatch") was
inaccurate. What was actually broken:

    enum E { A { x: u8, y: u64 }, B }
    match e { E::A { y, .. } => ... }   // y bound to x's value

Root cause: `check_pat.rs`'s `Pat::Struct` arm used a single `is_variant`
branch that assumed all variant patterns are **positional** (true for tuple
variants `E::A(x, y)`, false for struct variants `E::A { y, x }` — HIR
`Pat::Struct.fields` is `Vec<(Name, PatId)>` with real names). Positional
binding gave wrong field name, wrong index, wrong type — reverse-order and
`..`-only patterns silently bound the wrong field.

Fix: dispatch on `variant.style`:
  * `Tuple` → positional (`variant.fields[i]`), correct as before.
  * `Struct` → remap HIR field name via `remap_name`, then look up its
    declared index for both the binding name and field type.

Every shape verified at `a71ce481`:
  * `E::A { y, .. }`, `E::A { y, x }`, `E::A { x, y }` — all OK
  * `E::A(x, y)` (tuple), plain struct, `Option`/`Result` — unchanged
  * Fixture: `tests/run-pass/behavior/enum_struct_variant_pattern_names.g`

The mixed-alignment enum case the audit *also* mentioned
(`enum E { A(u8), B(u64) }`) was already correct at HEAD — verified.
The struct-variant bug was the entire remaining failure in that class.

**P1-C1 second half (Vec::clone for non-Copy elements)** — `0793fd61`.

`v.clone()` on `Vec<T>` where `T: !Copy` used to hit the P1-C2 clean
diagnostic (`builtin method \`clone\` is resolved by the type checker but
not implemented by the LLVM backend`) — no longer silent-wrong, but
unusable. Implemented in **both backends**:

  * **LLVM** (codegen-llvm/lower.rs): new `clone` arm. Reads ptr (byte
    0) + len (byte 16) from the Vec's 24-byte `{ptr, cap, len}`
    `{[3 x i64]}` header, gets element type from the Vec substitution,
    sizes memcpy from `FullLayoutComputer`, calls the existing
    `glyim_slice_to_vec(src, len, elem_size, dest)` runtime helper —
    same one the `to_vec` arm uses. Dispatch + arm gated on
    `_recv_adt == 1020` (Vec).
  * **Interpreter** (mir-interp/lib.rs): new `(1020, "clone")` arm.
    Interp Vec model is `Aggregate([Aggregate(elements), Uint(len)])`;
    clone is `InterpValue::clone()` of the dereferenced receiver (deep
    structural copy). Without this, mir-interp fell to
    `function not found: FnDefId(9068)` — the reason `vec_clone.g`
    passed LLVM but failed the run-pass corpus (which uses mir-interp).

Verified (both backends):
  * `v.clone()` length preserved (3 == 3)  → 0
  * clone independence (mutate original after clone → 4 vs 3)  → 0
  * `x.clone()` on Copy `i32`, `for x in v`, `for x in &v`,
    `[1,2,3].to_vec()` — unchanged
  * glyim-codegen-llvm: 302/302
  * run_pass_corpus, stdlib_full_probe — pass
  * full workspace: 4603/4603

Fixture: `tests/run-pass/behavior/vec_clone.g`

**P1-C1 third path — `into` on Copy primitives** — `5a39f52e`.

`let y: i32 = x.into();` on `x: i32` (with `--with-stdlib`) resolved
to the stdlib blanket impl `impl<T> Into<T> for T` in
`crates/glyim-lang-core/lib/convert.g` (FnDefId 477). That impl's
*body* is not collected into the mono set, so `--emit=exec` produced
an object with a `declare @__glyim_fn_477` and no `define`, and the
link failed with `Undefined symbols: ___glyim_fn_477`. Without
`--with-stdlib` the sentinel path was already correct (`u32::MAX - 1`
→ `lower_rvalue.rs:562`'s `is_copy_sentinel` arm → plain `Use` of the
operand), so the bug was invisible to every no-stdlib test.

Root cause was **ordering**, not lowering. `resolve_method_call`
(typing, `check_expr.rs`) has two calls into `try_builtin_method`:
a pre-check guard (for slice/array/str/builtin-ADT receivers) that
runs *before* the user-impl scan, and a fallback that runs only when
`candidates.is_empty()`. Primitive receivers fell through the
pre-check (no primitive arm), so the impl scan found the bodyless
blanket impl and returned it — `try_builtin_method` never saw the
call.

Two changes in `crates/glyim-typeck/src/check_expr.rs`:

  * `try_builtin_method`'s `into` arm now returns the receiver type
    as the output type when `is_copy(step_ty)` and the receiver is
    not behind a reference; the fresh-infer-var shape is preserved
    for `&T` and non-Copy receivers.
  * The pre-check guard now also matches `Int`/`Uint`/`Float`/
    `Bool`/`Char`/`RawPtr`/`Tuple`/`FnPtr`/`FnDef`, plus references
    to any of them, so `try_builtin_method` sees primitive receivers
    before the impl scan does.

Verification (both deltas are loud when broken):
  * `let y: i32 = x.into();` on `x: i32` → 42 (was a link error)
  * `let _y: u32 = x.into();` on `x: i32` → `[T0001] mismatched types:
    i32 vs u32` (would silently become 0 if the `is_copy` guard
    returned a fresh infer var)
  * `x.clone()` on `i32` → 7 (no regression)
  * `--emit=exec` with and without `--with-stdlib`, LLVM backend
  * Full workspace: 4605/4605

Fixture: `tests/run-pass/behavior/into_copy_primitive.g`

#### 3.5.7 — P1-C2 batch 1: unsigned bit-manipulation intrinsics

`count_ones` / `count_zeros` / `leading_zeros` / `trailing_zeros` /
`next_power_of_two` on the integer primitives were registered in
`TyCtxMut::register_builtin_methods` (`ty_ctx_mut.rs:2709..2723` and
`:2767..2781`) but had **no lowering** in either backend. Any call hit
the P1-C2 gate:

    [X0000] builtin method `count_ones` is resolved by the type checker
    but not implemented by the LLVM backend. This is a compiler gap
    (KNOWN_GAPS §6 P1-C2) ...

The registry entry is what makes them type-check and produce a real
`FnDefId`; the gate is the deliberate "clean diagnostic" for
registry-without-lowering. Batch 1 closes the four unsigned-integer
bit ops and the power-of-two round-up.

**LLVM** (`codegen-llvm/lower.rs::try_lower_builtin_intrinsic`):

  * The dispatcher allow-list gains the five method names.
  * New inline arm routes each to the LLVM intrinsic declared on
    demand per bit width — `llvm.ctpop.iN`, `llvm.ctlz.iN`,
    `llvm.cttz.iN`. `is_zero_poison` on ctlz/cttz is passed as
    `false` so `leading_zeros(0) == width` and `trailing_zeros(0) ==
    width`, matching Rust.
  * `count_zeros(x)` is `width - ctpop(x)`.
  * `next_power_of_two(x)` is `1 << (width - ctlz(x - 1))` with a
    `select` on `x <= 1` (so `x == 0` and `x == 1` both yield 1).
  * The intrinsic result is truncated or zero-extended to the
    destination integer type (e.g. `count_ones` on a `u8` receiver
    returns `u32` per the registry signature — the destination is
    `u32`, the intrinsic result is `i8`, so the arm inserts `zext`).

**Interpreter** (`mir-interp/src/lib.rs::try_call_builtin`):

  * New `(_, "count_ones" | "count_zeros" | "leading_zeros" |
    "trailing_zeros" | "next_power_of_two")` arms. `count_ones` is
    width-independent and delegates to `u128::count_ones`; the
    others need the receiver's bit width, so a new
    `operand_bit_width(Option<&Operand>)` helper reads it off the
    receiver place's local type via `self.local_decls`. Projected
    places return `None`, and the arms surface an explicit
    `InterpError::Panic` rather than silently miscomputing.
  * Zero semantics match Rust exactly: `leading_zeros(0) == width`,
    `trailing_zeros(0) == width`, `next_power_of_two(0) == 1`.

**Known follow-up (not in batch 1):** `next_power_of_two` overflow
(`x > 2^(width-1)`) currently *wraps to 0* on both backends, matching
the LLVM behavior of a poison-shift. Rust panics in debug and wraps
in release. The fixture does not exercise this case; a future batch
will either route through `llvm.umul.with.overflow` + a panic
intrinsic (debug-style) or document the wrap-to-0 contract.

Also not in batch 1: `count_zeros` / `leading_ones` / `trailing_ones`
for `u64` and `u8` are not registered (`ty_ctx_mut.rs` has
`count_zeros` only on `usize` and `u32`). Adding registry entries is
a one-line-per-type change when a caller needs them.

Verified (both backends, `--with-stdlib`):
  * `(5 as u32).count_ones()` → 2
  * `(0 as u32).leading_zeros()` → 32, `(1 as u32).leading_zeros()` → 31
  * `(0x100 as u32).trailing_zeros()` → 8
  * `(1000 as u32).next_power_of_two()` → 1024
  * `(0 as usize).count_zeros()` → 64, `(5 as usize).count_zeros()` → 62
  * Reproducer through `--emit=exec` and `GLYIM_LLVM=1 --emit=exec`
  * Full workspace: 4605/4605

Fixture: `tests/run-pass/behavior/bit_methods_batch1.g`

#### 3.6 — P2-B1 investigated: fallback is a defensive escape hatch, not a live miscompile

Investigated 2026-10-26 during session 19. The audit's P2-B1
(*"For-loop fallback executes body once with a dummy binding when
`next: None`"*) framed this as a silent miscompile reachable from
ordinary source. It is not.

**What the fallback actually fires for.** The `ExprKind::For` lowering
arm `next: None` (in `lower_rvalue.rs`) fires only when BOTH:
  * typeck did not attach a resolved `Iterator::next` to the `For`
    THIR node, AND
  * `LowerCtx::iterator_next_fn` returned `None` for the iterable.

For real user code, neither condition holds: typeck's "not iterable"
gate in `check_expr.rs` runs *before* lowering and rejects a concrete
non-iterable head with `[T0001] <ty> is not iterable` (verified:
`for x in 5` fails at typeck, never reaching lowering). When a real
`Iterator::next` exists, typeck threads it through the THIR node
(the "Phase 1 GLYIM_DESTUB_PLAN" wiring).

The fallback fires in exactly two situations:

  1. **Raw generic templates.** `prepare_compilation` lowers every
     body once (to catch type errors even in unused generics). A
     body like stdlib `Iterator::last(self)` → `for x in self` where
     `self: I` and `I: Iterator` reaches the fallback because `I`
     isn't monomorphised yet. The whole body is then *discarded* by
     the pipeline's `body_is_raw_generic` filter — the "silent body-
     once" never becomes MIR for codegen.

  2. **Mock test contexts** that construct a `thir::For` node by hand
     without a `Next` (the 3 tests in `glyim-lower::tests::for_loop*`).

**Attempted fix (reverted).** Adding `GlyimDiagnostic::internal_error`
in the fallback arm fires on case (1) above — the stdlib generic
`for x in self` bodies. That broke `stdlib_full_probe`,
`glyip run`, `with_stdlib_flag_is_accepted`, and `ui_corpus` in one
shot. Guarding the diagnostic to only concrete iterable types would
avoid case (1) but reintroduces the same false-positive risk elsewhere
in the pipeline wherever generics are lowered speculatively.

**Correct characterisation.** The fallback is a **defensive escape
hatch for a compiler-bug class**, not a live user-facing miscompile.
The audit's P2-B1 "silent miscompile" severity is inflated: no
reachable user program triggers it. If a future typeck regression
bypasses the not-iterable gate, the correct fix is in *typeck*
(assert that every `For` reaching lowering has a `next` or an array/
range/`as_slice` shape), not a lower-time diagnostic.

**What to do if the audit is re-applied.** Before wiring a diagnostic
into the fallback, prove that stdlib raw-generic bodies aren't
affected (`stdlib_full_probe` + `glyip run` + `ui_corpus` all still
green). The naive fix, tried and reverted this session, does not pass
that bar.

**Status.** Reviewed, no change needed. Documented to prevent a
future session from repeating the reverted attempt.
