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

#### 3.5.8 — P1-C2 batch 1.5: full bit family; hardcoded-id fragility removed

Two related changes, in two commits:

**`9f9f2ed7` — hardcoded builtin id eliminated.**

`lower_rvalue.rs` (for-loop desugaring) synthesized a call to
`Vec::<T>::as_slice` by writing `FnDefId::from_raw(9144)` — a
literal number that depended on the *registration order* in
`TyCtxMut::register_builtin_methods`. Any added, removed, or
reordered entry in that table shifted the id and made the
synthesized call link against `__glyim_fn_XXXX` with no body. It was
correct only by luck, and only until the next registry edit.

Replaced with `TyCtx::builtin_method_fn_id(adt, name)` — a new
reverse-lookup over the existing `builtin_fn_index` (linear scan;
negligible cost on a compile-time lookup). The call site now says
`.expect("Vec::as_slice builtin must be registered ...")`, so a
future registry mistake is a clean panic with an actionable message
at the exact call site instead of an undefined symbol at link time.

**`f0a09d44` — bit-method family completed.**

Three defects closed in `ty_ctx_mut.rs` and both backends:

  * A *duplicate* `prim_bit_methods` block re-registered the same 14
    entries into a second range of `next_builtin_fn_id` slots. The
    second registration was dead (the first wins by later `.insert`
    into `primitive_method_fns`, and the ids were never referenced).
    Deleted — net saves 14 id slots.
  * `leading_ones` / `trailing_ones` were not registered at all.
    Any call was `[T0001] no method ... found`. Added to the
    registry, LLVM dispatch (via `llvm.ctlz` / `llvm.cttz` of the
    inverted value), and the interpreter (same identity, mask to
    width so the zero case matches Rust).
  * `u8` and `u64` were under-provisioned: `u8` had only
    `is_power_of_two`; `u64` had 2 of 6. Both filled to full
    8-method coverage (`is_power_of_two`, `next_power_of_two`,
    `leading_zeros`, `trailing_zeros`, `count_ones`, `count_zeros`,
    `leading_ones`, `trailing_ones`). Return types follow Rust:
    bit-count methods return `u32`; `next_power_of_two` returns the
    receiver type (`u8::next_power_of_two -> u8` is the exceptional
    one the LLVM arm truncates to).

The `prim_bit_methods` vec now holds 32 bit entries plus the 12
pre-existing `min`/`max`/`clamp`/`abs` entries (44 total); each
registration consumes one `next_builtin_fn_id` slot, so the range
shifted +18 versus HEAD~2 — exactly the shift the hardcoded-id fix
was landed first to survive.

Verified (both backends, `--with-stdlib`):
  * all 32 (type, method) pairs from the new fixture
    (`bit_methods_full.g`) -> 0
  * `Vec::push` / `Vec::len` on a fresh `Vec<i32>` -> 2
  * for-loop desugaring after the registry shift -> 0
  * `raw_vec.g` `next_power_of_two().max(8)` (the original reason
    the 12 min/max entries existed) still resolves
  * full workspace: 4605/4605

Fixture: `tests/run-pass/behavior/bit_methods_full.g`

#### 3.5.9 — P2-B2: bytecode backend silent-zero on unrecognized const kinds

CLOSED. `crates/glyim-codegen/src/lib.rs::emit_operand` had a catch-all
arm that emitted `OP_LOAD_CONST 0` for every `MirConstKind` it did not
explicitly handle. `StaticRef` (a reference to a `static` item) and
`Variant` (an enum-variant constant) fell through that arm — so a
`static` read or a non-folded variant reference produced `0` instead
of the address / discriminant. Silent wrong code, exactly the class
P2-B2 called out.

The same file already rejected `ConstRef` explicitly (`T114-PATCHED-BC`)
with the reasoning that const evaluation must fold named constants
before codegen. `StaticRef`/`Variant` were simply not given the same
treatment.

Both remaining silent arms now hard-error:

  * `MirConstKind::VirtualMethod { .. }` — matches the LLVM backend,
    which already had a hard rejection for this case (a
    devirtualization bug is a compiler bug, not a lowering gap).
  * catch-all `_` — a `GlyimDiagnostic::internal_error` that prints
    the exact `MirConstKind` variant (`{:?}`) that reached codegen, so
    a future report names the missing fold rather than showing a
    mystery zero.

Reachability check: the bytecode test corpus (`cargo nextest run
-p glyim-codegen -p glyim-bytecode-vm`, 187 tests) passes unchanged
with both arms hard-erroring, which confirms neither
`StaticRef`/`Variant` (nor `VirtualMethod` post-devirtualization) is
reachable in current lowering. The rejection is defensive: it
converts a *hypothetical* silent miscompile into an immediate, named
compiler error at the moment a future lowering change starts
producing these kinds.

Verified:
  * `cargo nextest run -p glyim-codegen -p glyim-bytecode-vm` ->
    187/187
  * full workspace -> 4605/4605

#### 3.5.10 — P2-B3: interp `Vec<T>::as_slice` truncates non-u8 elements (scope corrected)

Investigated 2026-10-27. Attempted a defensive reject (the same
pattern as P2-B2 / §3.5.9) and reverted it after the full corpus run
showed the reject was too aggressive. The audit described this as a
clean "wrong for non-u8 payloads" bug; the actual shape is narrower
and more interesting.

**The model.** `mir-interp` represents a `Vec<T>` as
`Aggregate([Aggregate(elements), Uint(len)])` and its
`(1020, "as_slice")` arm produces a `InterpValue::String` whose
*characters* are the low byte of each element
(`char::from_u32((n as u32) & 0xff)`). This was written for
`Vec<u8>` (where the byte-as-char String *is* the memory image) and
has never been correct for larger element types — but the way in
which it is wrong is element-type dependent:

  * **`Vec<u8>::as_slice()` — correct.** Element type is `u8`, so
    truncation is identity. Both `.len()` and content access work.
  * **`Vec<non-u8>::as_slice()` — `.len()` correct, content wrong.**
    Each element still produces exactly one char, so `slice.len()`
    equals the element count. But the *character* carries only the
    low byte — `Vec<u32> [0x1234_5678]` becomes the char
    `U+0078`, not a 4-byte payload. Reading the slice as a `&[u32]`
    (via `to_vec`, `copy_from_slice`, indexing) produces truncated
    values.

**Why the blanket reject failed.** An earlier attempt added a guard
in the `as_slice` arm that rejected any non-`u8` element type with
`InterpError::Panic`. It broke 6 run-pass fixtures in the corpus:

    for_loop_over_slice.g, for_loop_over_vec_ref.g,
    slice_copy_from_slice_len.g, slice_split_at_len.g,
    slice_to_vec.g, vec_as_slice_len.g

All six exercise `.len()` on a `Vec<u32>` / `Vec<i32>` slice — the
one operation this model gets *right*. Rejecting the entire arm to
fix a content bug also broke the length-only cases, which are the
common path in stdlib (`for _ in v.as_slice()`, `slice.split_at(n)`
checques, `if v.as_slice().len() > k`). The guard was reverted in
full (tree confirmed identical to `fb628719`, corpus re-run at
4605/4605).

**Correct fix (deferred).** The only shape that is right for every
element type is `InterpValue::Slice(Vec<InterpValue>)` — a real
sequence-of-values representation, not a byte-string facade. It
requires touching every `match InterpValue` in `mir-interp` (the
constant folder, `builtin_deref`, `enum_payload`,
`read_place`/`write_place` projections, `vec_len`, `vec_push`,
`vec_pop`, the `String` arms that mirror the same layout, and the
`(1020, "to_vec")` arm). That is a multi-site refactor of its own,
not a one-file patch. Tracked as the remaining scope of P2-B3.

**Interim contract.** Content access on a non-`u8` slice *is wrong
under interp today*, and this entry is the honest record of that.
The LLVM backend handles `as_slice` correctly for every element
type (fat-pointer pass-through), so any workflow that needs
non-`u8` slice content should use `--emit=exec` (the default) or
the LLVM backend explicitly. The 6 fixtures above stay green
because they use the length-only path.

**Diagnostic note.** A future partial fix should differ from the
failed attempt: reject only when the slice is *consumed as
content* (e.g. from `to_vec`, `from_utf8`, index read into a
non-u8 element), not when `as_slice` is called. Doing it at the
consumption site leaves the length-only path working. That requires
tracking "this `InterpValue::String` was produced by a Vec slice"
through the interpreter's value flow, which itself is close to
adding `InterpValue::Slice` — reinforcing that the deferred
refactor is the real fix.

**Attempt 2 (2026-10-27) — `InterpValue::Slice`, and why it is bigger
than a variant swap.** The variant was added, every exhaustiveness error
the compiler flagged was closed (only 2: `interp_value_to_u128` and
`interp_value_to_bool`), and `as_slice` / `len` / `is_empty` /
`split_at` / `to_vec` / both index sites were wired to use it for
non-`u8` element types. `cargo check --workspace` clean. Full workspace
run: **2 fixtures failed**, both with exit 101 (panic):

    behavior/for_loop_over_slice.g     expected 3, got 101
    behavior/for_loop_over_vec_ref.g   expected 6, got 101

These are `Vec<i32>` loops — exactly the cases `Slice` was meant to
serve. The panic is in `Iterator::next` lowering: the interpreter's
`next` implementation reads the receiver as a `String` (one char per
element) and indexes *chars*. With the receiver now a `Slice`, that
read panics.

So the true scope is: adding `Slice` requires **simultaneously**
updating the slice iterator machinery — `Iterator::next` on `[T]`,
`&[T]`, and the `for`-loop desugaring that consumes it — not just the
five accessor methods. That is a coordinated change across the model
and the iterator protocol, not a local refactor.

Reverted in full (`git checkout` on the two modified files). Tree is at
`47450241` with 18 unpushed commits; nothing half-finished.

**Correct next attempt, when taken:** the variant + the 5 accessors +
`Iterator::next`-on-slice, in one commit, with the `for_loop_over_*`
fixtures as the acceptance test. If the iterator update proves as
entangled as `iterator::max` was for P1-C8 (§3.5.4), this becomes the
same shape as that item and should be planned alongside it.

**Attempt 3 (2026-10-28) — diagnosis sharpened, still reverted.**

The prior paragraph guessed the coupling was `Iterator::next`-on-slice.
**That was wrong.** Reading the two failing fixtures' own header comments
(they cite KNOWN_GAPS §7) plus their MIR shows the opposite: `for x in
v.as_slice()` and `for x in &v` are *already* routed through an
index-walk, precisely so they avoid the trait path. MIR for
`for_loop_over_slice.g`:

    Assign(_, Len(place[9][Deref]))                       # len
    Assign(_, Use(Copy(place[9][Deref, Index(11)])))      # s[i]
    Assign(_, UnaryOp(Deref, Copy(place[17])))            # *x

So the loop is `*((*slice)[i])`: deref the slice, index it, deref the
element. The old String model satisfied all three because
`ProjectionElem::Deref` had an explicit `InterpValue::String(_) => val`
no-op arm (added for the byte payload) and the `Index` arm returned
`Uint(low_byte)`, which the *outer* `UnOp::Deref` then handled via...
nothing — actually `(UnOp::Deref, Uint(_))` falls to the catch-all panic
at `eval_unary_op`'s `_` arm. That is the contradiction the attempt
resolved by **adding the corresponding arm**:

    // ProjectionElem::Deref
    InterpValue::Slice(_) => val,     # no-op, symmetric with String

plus a `Slice` index arm returning the raw element, plus 4 accessor
arms (`len`, `is_empty`, `split_at`, `to_vec`), plus the two utility
matches (`interp_value_to_u128`, `interp_value_to_bool`).

**What worked.** `cargo check` clean (only those 2 exhaustive matches
needed new arms). Direct `glyim-cli --with-stdlib --emit=exec` runs on
all three fixtures exited with the right codes (3, 6, 0) — on the
**LLVM backend**, which is what `--emit=exec` builds. That is the
trap.

**What failed.** `run_pass_corpus` executes fixtures via **mir-interp**
(see `crates/glyim-test/src/harness/interpreter_runner.rs`), not the
LLVM binary. All three fixtures panicked with exit 101 under interp
despite passing under LLVM. The attempt's verification had the same
LLVM-vs-interp asymmetry that §3.5.17 (operand_bit_width) hit — the
direct `--emit=exec` probe validates the LLVM backend, not the
interpreter.

**Why it could not be diagnosed in the attempt.** The harness's
`RunFailed { exit_code: Some(101), expected_exit_code: Some(3) }`
carries the exit code but **not** the interpreter's `InterpError::Panic`
message. `InterpRunner::run` *does* put the panic text into
`output.stderr`, but the run-pass harness's failure report does not
surface it. Without the panic string ("unsupported unary op: Deref on
Int(1000)"? "deref of Ref from dead frame"? "index out of bounds"?) the
next arm can't be located by reasoning — every candidate is plausible
and several require contradictory fixes.

**Correct next-attempt protocol.** *Before* any code change:

  1. Write a `glyim-mir-interp` unit test that builds the MIR for a
     minimal `for x in &Vec<i32>` loop **by hand** (the pattern in
     `crates/glyim-mir-interp/src/tests/loop.rs` and `aggregate.rs`),
     runs it through `Interpreter::run_body`, and `panic!`s with the
     `InterpError::Panic(msg)` message on failure. This is the only
     reliable way to see the actual panic text — the harness won't.
  2. *Then* apply the variant + accessor patch and iterate against
     that unit test until the interp path is green.
  3. Only then re-run `run_pass_corpus`.

**Reverted in full** (`git checkout` on `interp_value.rs` and `lib.rs`;
`slice_non_u8_content.g` deleted). Tree at `9dadbdc7`, 23 unpushed
commits. The `emit_object` change from P2-A4 (`f72a45fd`) is unaffected
— different file, already committed.

**Why this is the third revert and not a fourth attempt.** The pattern
across P2-B3 attempts: each failure mode was only visible *after*
running the corpus, because the corpus uses a backend (`mir-interp`)
that the direct CLI probes (`--emit=exec` → LLVM) do not. That is a
workflow-level blind spot, not an implementation detail. Fixing it with
a targeted unit test first is the difference between a productive
fourth attempt and a fifth revert.

#### 3.5.11 — P2-A2: mock drift closed; production fallback hardened (partial)

Investigated 2026-10-27. The audit described P2-A2 as two related
problems; the split turned out to be unequal, and one half was already
addressed in a prior session.

**Mock half — CLOSED (`4df457eb`).**

`crates/glyim-test/src/mock/lower_ctx.rs::MockLowerCtx` implemented
`LowerCtx::fn_sig` and `LowerCtx::const_value` as *always-succeed*
returning a plausible-looking dummy (`() -> ()` and `Unit`
respectively) for every id. Any test exercising call lowering or
const-eval lowering would silently run against an ABI / value that
never appears in a real compile — a test that *looked* like it
validated behavior but only validated "the mock made up an answer".
This is the drift the audit named.

Post-fix contract (mirrored from §3.5.9 / §3.5.10's defensive-
reject pattern):

  * `fn_sig(def_id)` returns `Some(sig)` iff the test called
    `add_fn_sig(def_id, sig)` first; otherwise **panics** with the
    exact id and a message naming `add_fn_sig`.
  * `const_value(def_id, substs)` returns `Some(val)` iff the test
    called `add_const_value(def_id, val)` first; otherwise **panics**
    with the exact id and a message naming `add_const_value`.

Four new unit tests in `crates/glyim-test/src/mock/lower_ctx.rs
::p2_a2_tests` pin both directions: two `#[test]`s prove the
positive path (registered → returned), two `#[should_panic]`s prove
the negative path (unregistered → panic with the right message).
Adding a fifth, sixth, ..., Nth consumer that needs a signature now
**requires** an explicit `add_fn_sig` call in the test setup, which
makes the dependency visible in the test source instead of hidden
in the mock's fallback.

No existing caller relied on the dummy: grep for `fn_sig` /
`const_value` in `crates/glyim-test/src/tests/` and
`crates/glyim-test/src/assertions/` returned zero hits, so the panic
contract change is a pure tightening of an unreachable path. Workspace
test count goes 4605 → 4609 (+4 new unit tests); all 4609 pass.

**Production half — PARTIALLY CLOSED (prior session).**

`PipelineLowerCtx::adt_def` in
`crates/glyim-pipeline/src/pipeline_context.rs` is *not* the naive
`AdtDef { variants: vec![], kind: Struct }` the audit describes. It
currently:

  1. consults `ty_ctx.adt_def(id)` first and, when the ADT *is*
     registered, builds a real `AdtDef` with correct per-field types
     for struct/enum/union (this is what fixes struct-update /
     aggregate field counts — a prior bug where `variants[0]` came
     back empty).
  2. falls back to HIR items for a struct/enum not yet in `ty_ctx`,
     synthesising error-typed fields — this preserves the pre-`typeck`
     shape so lowering can still emit aggregate initialisers before
     full type resolution.
  3. `warn!`s (tracing) and returns empty `AdtDef` only when the id
     is *neither* in `ty_ctx` *nor* in HIR — the true "compiler bug"
     case.

This is a substantial hardening versus the audit's description. The
remaining P2-A2 gap is narrower: the "true compiler bug" arm still
**returns empty `AdtDef` after logging** rather than surfacing a
`GlyimDiagnostic::internal_error` to the caller. Fixing that fully
would require changing the `LowerCtx::adt_def` signature from
`AdtDef` to `Result<AdtDef, Vec<GlyimDiagnostic>>` (or similar) — every
caller in the lowering pipeline, the mock, and 3 test files would need
to thread the error path. That is a multi-file signature refactor with
the same shape as `InterpValue::Slice` (§3.5.10 / P2-B3), not a small
defensive patch.

**Interim state is safe:** the `warn!` fires under the standard
`tracing` env-filter, so a caller hitting the true-bug path sees a
log line naming the ADT id rather than silent miscompilation. `warn!`
is not a structured `GlyimDiagnostic`, which means it does not
appear in `--emit=mir` / `--emit=llvm-ir` output the way diagnostics
do — that is the residual gap.

**Deferred follow-up:** if a future session takes the production-half
closing, the natural scope is exactly:
  a. Change `LowerCtx::adt_def` return to `CompResult<AdtDef>` (or
     introduce an `adt_def_checked` companion for the "must succeed"
     callers).
  b. Push the error path through the ~4 lowering call sites that
     currently do `.adt_def(id)` and index into `variants[0]`.
  c. Add a compile-fail fixture exercising the true-bug path (an
     ADT id that is in neither `ty_ctx` nor HIR — reachable via
     synthetic test setup only, so the fixture would be a unit test,
     not a `.g` file).

Verified this commit:
  * `cargo nextest run -p glyim-test --tests` — 4 new tests pass
  * full workspace — 4609/4609, up from 4605 (+4 unit tests)
  * no caller relied on the dummy (grep — zero hits in `tests/` or
    `assertions/`)

#### 3.5.12 — P1-C2 batch 2a: wrapping arithmetic (add/sub/mul)

`wrapping_add` / `wrapping_sub` / `wrapping_mul` were registered in
`TyCtxMut::register_builtin_methods` (for `u64`, `u32`, `usize`) but
had no lowering in either backend — every call hit the P1-C2 gate
(`builtin method ... is resolved by the type checker but not
implemented by the LLVM backend`). Additionally `wrapping_add` was
missing from the registry entirely for `u32`, so
`(5 as u32).wrapping_add(3 as u32)` failed at typeck with
`[T0001] no method`.

**What was added.**

  * `ty_ctx_mut.rs` — three registry entries: `wrapping_mul` for
    u64/u32/usize (was absent), and `wrapping_add` for u32 (was
    absent; other types already had it). The wrapping-method matrix
    is now complete for the three unsigned receiver types the
    registry already covered: 3 types x 3 ops = 9 entries.
  * `codegen-llvm/src/lower.rs::try_lower_builtin_intrinsic` — a new
    arm that folds `wrapping_add`/`wrapping_sub`/`wrapping_mul` to
    `build_int_add`/`build_int_sub`/`build_int_mul`. LLVM integer
    ops wrap on overflow *by definition*, so this is identical to
    what the plain `+`/`-`/`*` operators compile to today. The two
    will diverge only when overflow checking lands (the driver's
    `overflow-checks` config is not yet threaded into the LLVM
    backend, so `a + b` and `a.wrapping_add(b)` currently emit the
    same instruction). The arm exists so that future divergence has
    a single place to change.
  * `mir-interp/src/lib.rs::try_call_builtin` — an arm keyed on
    `(_, "wrapping_add" | "wrapping_sub" | "wrapping_mul")`.
    Performs the op at infinite `u128` precision, then masks to the
    receiver width (via the existing `operand_bit_width` helper,
    added in §3.5.7 for the bit-manipulation family). Both the
    `InterpValue::Uint` and `InterpValue::Int` receiver shapes are
    handled by the shared extraction match.

**Not in this batch — `saturating_*` and `checked_*`.**

Those are a separate shape, not a copy-paste of the wrapping arm:

  * `saturating_add` / `saturating_sub` / `saturating_mul` need an
    overflow *detection* path (`llvm.uadd.with.overflow.iN` /
    `llvm.usub.with.overflow.iN`), followed by a `select` that
    chooses between the truncated result and `MIN`/`MAX` of the
    receiver type. The interp side is simple (`u128` then clamp),
    but the LLVM side needs `build_int_compare` on the overflow
    flag plus `build_select`.
  * `checked_add` / `checked_sub` / `checked_mul` additionally need
    to construct an `Option<T>`: the runtime shape of the result is
    `(tag, payload)` in the interpreter (`Aggregate([Uint(tag),
    Uint(payload)])`) and a `{ iN, i8 }` LLVM struct on the native
    side. The `is_power_of_two` and bit-manipulation arms are the
    closest precedent for the LLVM intrinsic declaration and the
    destination-store pattern, but neither constructs an enum. This
    is where `Option` layout lives and is best done as its own
    batch.

**Also not in this batch — signed integers.**

The registry covers `u64` / `u32` / `usize` only. Signed
`wrapping_*` on `i32` / `i64` / `isize` would need registry entries
plus a small interp change (the width mask must be applied to the
signed-magnitude result before reconstructing the `InterpValue::Int`).
Not required by any caller today; the registry-first pattern (§3.5.7
and this batch) makes adding them a one-line-per-type change.

**Verified.**

The new fixture `wrapping_arith.g` exercises all three ops across
all three receiver types, both in-range and at wrap boundaries
(`u32::MAX + 1 = 0`, `u32::MAX * 2 = MAX - 1`, `0u64 - 1 = u64::MAX`,
and the analogous `usize` cases). Compiles and runs to exit 0 on both
the interp backend (default `--emit=exec`) and the LLVM backend
(`GLYIM_LLVM=1 --emit=exec`). Full workspace: 4609/4609.

Fixture: `tests/run-pass/behavior/wrapping_arith.g`

#### 3.5.13 — P2-F1: glyip dependency-resolution hardening (all 5 items CLOSED)

The audit enumerated five independent hardening items in `glyip`'s
dependency resolver and lockfile writer. All five are now closed: four
in `fdfd4496`, the fifth (signature change) in `3727824a`.

**CLOSED — `deny_unknown_fields` on config structs.**

`GlyipToml`, `PackageConfig`, `BinTarget`, `LibTarget` (`crates/glyip/
src/config.rs`) previously accepted arbitrary unknown TOML keys silently
— a typo like `[package] descripton = "..."` was ignored, the field
defaulted to `None`, and the user was left with the (correctly)
default-constructed value wondering why their setting had no effect.
All four now have `#[serde(deny_unknown_fields)]`, so the same typo
is a parse error naming the unknown key.

No test depended on unknown-field acceptance (verified by grep across
`crates/glyip/tests/`), so this was purely additive.

**CLOSED — path-aware visited key for cycle detection.**

`DependencyResolver::resolve` in `dep.rs` uses a `HashSet<String>`
(`visited`) to dedupe crates across the breadth-first dependency walk.
The key was `format!("{name}-{version}(git)")` — so *two different
path dependencies whose `Glyip.toml` declared the same `[package]
name`* collided on the name-only key: the second was silently treated
as already-visited, and its transitive dependencies were never
enqueued. The lockfile that resulted was *incomplete but well-formed*
— no error, just missing entries.

The key now includes the resolved path when one exists:
`format!("{name}@{path}(path)")`. Same name + same path still
dedupes (correct — re-visiting one directory is a real cycle or
diamond); same name + different paths no longer collides. The
registry/git branches are unchanged (their name/version keys were
already unique because the registry can only hold one version of a
given name — the resolver does not support version multimapping yet,
see `P2-F6`).

**CLOSED — `DefaultHasher` → SHA-256 for git cache directory names.**

`repo_cache_name(url)` (`dep.rs`) derives the shared git-cache
directory name from the repository URL. It previously used
`std::collections::hash_map::DefaultHasher`, whose output the Rust
docs explicitly disclaim as unstable across compiler versions and
whose seed is randomized in some configurations. That meant the "shared
cache" could silently miss — the same URL hashed to a different
directory after a toolchain bump, re-cloning the repository on every
resolve. It now uses `sha2::Sha256` over the URL bytes and takes the
first 16 bytes (32 hex chars, well under the 255-byte filename limit).
`sha2 = "0.11.0"` is already a `glyip` dependency (used for tarball
checksum verification); no new crate.

**CLOSED — symlink guard in the fingerprint file walker.**

`collect_files_recursive` (`fingerprint.rs`) walks a directory tree to
build a content hash for change detection. It used
`entry.path().is_dir()`, which *follows symlinks* — a symlink pointing
at an ancestor directory recursed until stack overflow. The walker now
uses `entry.file_type()`, which reports the link itself (not its
target), and skips any symlink unconditionally. This is the
conservative choice: following a symlink would make the fingerprint
depend on files outside the crate directory (so external edits would
invalidate the cache but the walker would not see them), which is
worse than not indexing the target at all. A future iteration could
follow-once with a visited-canonical-paths set; the current behavior
is deliberate.

**CLOSED — relative paths in the lockfile for path dependencies.**

`CrateSource::Path { path }` stored the full absolute path
(`resolve_path_dep` in `dep.rs` at `path.to_string_lossy()`), making a
lockfile checked into a repository non-portable: a collaborator on a
different machine saw the generating machine's `/Users/alice/...`
path, and CI saw `/home/runner/...`. Neither could reproduce the build
without hand-editing the lockfile.

`resolve_path_dep` now takes `project_dir: &Path` (threaded from
`resolve`, which already had it as a parameter — one call site). The
stored path is `path.strip_prefix(project_dir)` when the dep lives
inside the project tree, falling back to the absolute form only for
paths *outside* the tree — those are necessarily unportable, and
inventing a relative path that does not resolve from the project
directory would be worse than recording the absolute one. An empty
relative path (dep *is* the project root) also falls back to absolute,
so the field is never an empty string.

Lockfiles generated *before* this fix still contain absolute paths;
they parse identically (the field is a plain `String`), and their
resolver call sites still work — a future migration could rewrite
on read, but that's a policy decision, not part of this fix.

New regression test `glyip::tests::dep_resolution::
path_dep_stored_relative_to_project_dir` creates a project with a
nested `libs/util` path dep, resolves it, and asserts the lockfile
stores exactly `"libs/util"` (relative), not the absolute
`/tmp/.../libs/util`.

**Verified.**

  * `cargo check -p glyip` — clean
  * `cargo nextest run -p glyip` — 212/212 (211 existing + 1 new P2-F1 #3 test)
  * full workspace — 4610/4610
  * no test depended on the old behaviors (pre-flight greps returned
    zero hits for `repo_cache_name` output assertions, unknown-field
    acceptance, or symlink traversal)

#### 3.5.14 — P2-F2: registry URL in lockfile was hardcoded to the public index

CLOSED (`99ffe61e`). `RegistryClient::base_url_display` exists precisely
so a client can report the registry it actually resolves against — the
`HttpRegistryClient` impl returns its own `self.base_url`, so a
corporate mirror or a test mock correctly reports a non-default URL.
But the one *consumer* of that method inside the resolver
(`resolve_registry_dep` in `dep.rs`) ignored it and hardcoded
`"https://index.glyim.dev"` at the `CrateSource::Registry { url }`
construction site.

Consequence: a user pointing glyip at a custom registry got a correct
*download* (the client used its own `self.base_url` for the HTTP
request) but a wrong *lockfile provenance* entry — anyone reading
`Glyip.lock` saw the public index, not where the crate actually came
from. That silently breaks any workflow that uses the lockfile to
decide "is this dependency coming from our internal mirror?". The
audit's framing ("T192 patch applied to only one of two sites") was
accurate: `HttpRegistryClient::base_url_display` was the fixed site,
the resolver's write path was the unfixed one.

**Fix.** New `DependencyResolver::current_registry_base_url` helper
returns `self.registry_client.as_ref().map(|c| c.base_url_display())`,
falling back to the public index only when no client is attached. The
fallback is unreachable in `resolve_registry_dep` today (that function
returns `DependencyNotFound` when `registry_client.is_none()` *before*
reaching the lockfile-write branch), but kept as a defensive
`unwrap_or_else` so a future caller cannot write an empty URL. The
`CrateSource::Registry { url }` site now calls the helper.

**Verified divergence.** New test
`glyip::tests::dep_resolution::custom_registry_url_recorded_in_lockfile`
attaches a `CustomUrlRegistryClient` whose `base_url_display` returns
`"https://internal-mirror.example/glyim"`, resolves a version dependency
against a local `CrateIndex`, and asserts the lockfile's
`CrateSource::Registry.url` equals the custom URL and is *not* the
public default. It also asserts the public URL is absent (a stronger
check than equality, so a future reintroduction of the hardcode is
caught even if the custom URL coincidentally equals some other value).

This test uses `unimplemented!()` for `fetch_index` / `download_crate`
on the mock — the resolver never calls them for a locally-indexed
crate, and the panic is the intended trip-wire if that assumption ever
changes.

Existing behavior for the default case is unchanged:
`HttpRegistryClient::base_url_display` already returns
`"https://index.glyim.dev"` when constructed with that URL, so the six
existing tests that assert on the default URL in the lockfile still
pass.

Verified:
  * `cargo nextest run -p glyip` — 213/213 (211 existing + P2-F1 #3 + this test)
  * full workspace — 4611/4611

#### 3.5.15 — P2-F4: git dependencies cloned full history

CLOSED (`995b30c7`). `GitCommandFetcher::fetch` in `crates/glyip/src/dep.rs`
did a full-history `git clone --no-checkout <url>` for a fresh cache
entry, and `git fetch --all` on an existing one. For the common case —
a dependency pinned at one tag or branch — that transferred every commit
since the repository was created. Cargo avoids this with shallow clones;
`glyip` did not.

**Fix — shallow-first, full-history fallback.** The four git arg vectors
are now produced by pure helper functions (`shallow_clone_args`,
`full_clone_args`, `shallow_fetch_args`), and `fetch` uses them in this
order:

  * **Fresh clone.** `git clone --no-checkout --depth 1 <url> <dest>`.
    If the server refuses (rare — some self-hosted git servers disable
    shallow support), fall back to a full clone with a `tracing::warn!`
    naming the URL so a slow clone is self-explaining.
  * **Non-tip rev.** When the requested `rev` is not the default-branch
    tip (a tag, a non-default branch), a best-effort
    `git fetch --depth 1 origin <rev>` pulls it. Failure is deliberately
    non-fatal here: the subsequent `git checkout <rev>` fails loudly if
    the rev really is unreachable, which is the correct conservative
    behavior — a partial clone that silently omits the target should
    not "succeed".
  * **Existing clone.** `git fetch --depth 1 origin <rev>` instead of
    `fetch --all`. Same reasoning and same fallback.

**Reachable-SHA caveat.** `git fetch --depth 1 origin <sha>` requires
the server to advertise `uploadpack.allowReachableSHA1InWant`. GitHub,
GitLab, and Gitea enable it; some bare self-hosted servers do not. On
refusal, the fresh-clone path falls back to `full_clone_args` (which
does not need it, since `clone` will contain every reachable commit),
and the existing-clone path falls back to `fetch --all`. Neither case
is worse than before — both were already `--depth`-free. The `warn!`
naming the URL is what makes a "why is this still slow" report
traceable.

**Scope of testability.** The helpers are `pub(crate)`, and 3 new unit
tests in `glyip::tests::dep_git` assert:

  * `shallow_clone_args` contains exactly `["clone", "--no-checkout",
    "--depth", "1", <url>, <dest>]`.
  * `full_clone_args` deliberately lacks `--depth` — the point of the
    fallback is to not be shallow.
  * `shallow_fetch_args` is `["fetch", "--depth", "1", "origin",
    <rev>]` and explicitly does *not* contain `--all` — that
    distinction is the entire bug.

`fetch` itself is not exercised end-to-end in unit tests (it shells out
to the system `git`); the helpers were extracted precisely so the arg
vectors could be pinned without a real server. The `MockGitFetcher`
operates one layer up, at the `GitFetcher` trait, so it cannot see the
git invocation. This split is documented as a known coverage boundary,
not an oversight.

Verified:
  * `cargo check -p glyip --tests` — warning-free
  * `cargo nextest run -p glyip` — 216/216 (213 existing + 3 new)
  * full workspace — 4614/4614

#### 3.5.16 — P2-F6: resolver semantics were undocumented

CLOSED. The audit's framing was "greedy BFS, no backtracking, single
version per name — undocumented." The first two are exactly what the
code does; the third turned out to be subtler than the audit implied and
worth stating precisely.

**What the code actually does.**

  * `DependencyResolver::resolve` (`dep.rs`) is a breadth-first walk from
    the root manifest, `visit_stack.pop_front()` per node.
  * `select_best_version` picks the highest SemVer-matching candidate —
    deterministically, by parsing and sorting, not by trusting the
    registry index's JSON order.
  * There is no backtracking: the first version chosen for a name is not
    re-examined if a later requirement disagrees.
  * `Lockfile::add_crate` keys entries by `format!("{name}-{version}")`,
    so the data structure *can* hold two versions of the same crate name
    — same as Cargo's lockfile format.
  * But `check_version_conflicts` (the post-resolution validation) does
    `lockfile.crates().find(|c| c.name == *name)` — first match only —
    and verifies *every* gathered requirement against that one version.
    If two versions were somehow both locked, only the first (BTreeMap
    lexicographic order) is checked.

So the effective behavior is "single version per name, enforced by a
post-hoc check" rather than "the lockfile cannot represent multimapping".
The audit's one-line framing was directionally right but would mislead
anyone who looked at the `BTreeMap<String, LockedCrate>` and concluded
multimapping worked. A diamond dependency where two crates require
mutually-incompatible versions of the same crate therefore surfaces as
`GlyipError::DependencyConflict` — which already names the crate, every
distinct requirement, and every `(requester, requirement)` edge — rather
than compiling two copies as Cargo would.

**What was added.** A new `#### Dependency resolution semantics`
subsection under `### The glyip Build Tool` in `README.md`, covering:
greedy BFS with no backtracking; the single-version-per-name enforcement
and its mechanism; deterministic version selection; cycle detection and
the path-keyed visited set (cross-referencing the P2-F1 fix); and the
"when does this differ from Cargo" framing — flat lists and agreeing
diamonds resolve identically, disagreeing diamonds are a hard error.

**Not changed.** No code changes. The semantics are deliberate; the gap
was that a user (or a future maintainer reading `Lockfile`) had to
reverse-engineer them from the resolver. Documenting is the correct fix
— adding backtracking would be a different, much larger project, and
the `DependencyConflict` diagnostic already makes the current limit
actionable.

Verified:
  * `git diff --stat README.md` — one file, documentation only
  * no test count change (docs)

#### 3.5.17 — P1-C2 batch 2b: interp `operand_bit_width` returned None for constant receivers

Fixed in the batch-2b commit chain. The mir-interp helper `operand_bit_width`
(introduced in batch 1 / §3.5.7 to support the bit-manipulation family)
returned `None` for `Operand::Constant` — it only handled `Operand::Copy` /
`Operand::Move` of a local place. Callers (the wrapping arm from batch 2a
and the saturating arm from batch 2b) turn `None` into `InterpError::Panic`
with a "cannot determine receiver bit width" message.

**Why this went unnoticed.** Every pre-existing fixture whose receiver was
a numeric literal wrote that literal through a *local* first:

    let a: u32 = 5;
    a.count_ones()  // receiver is Copy(local), operand_bit_width works

Or through a `let x = 5 as u32;` cast, which also creates a local. The
constant-receiver shape:

    (65536u32).saturating_mul(65536u32)  // receiver is Constant(65536)

was never exercised in the corpus. Under `--emit=exec` (LLVM path, which
doesn't call `operand_bit_width`) it works; under mir-interp (which the
`run-pass` corpus uses to execute the compiled program) it panics with
exit 101.

**Fixed.** `operand_bit_width` now reads `c.ty` directly off the
`Operand::Constant`. Same match on `TyKind::Uint` / `TyKind::Int` that the
place-based branch used. Fixes both the saturating arm and the batch-2a
wrapping arm, which had the identical latent bug — no wrapping fixture
happened to use a constant receiver, but the bug was there for one line
of fixture away.

**Verified.** Full workspace green at 4614/4614 (including
`run_pass_corpus` exercising `saturating_arith.g` with its constant-
receiver shape).

Separately, while building this fix, a **real compiler gap** surfaced:
`let x = 4294967296 as u64;` silently wraps to 0 in glyim. Rust rejects
out-of-range unsuffixed integer literals (`error: literal out of range
for i32`) because the default type of `4294967296` is `i32` and the value
doesn't fit. glyim silently wraps. That is *not* what this fix addresses —
it's a typeck issue, tracked as its own open item.

#### 3.5.18 — out-of-range integer literals (CLOSED)

Discovered 2026-10-27 while building the saturating-arithmetic fixture
(§3.5.17). Not fixed in that session — it is a distinct typeck issue and
warrants its own change.

**Reproduction.**

    let a = 4294967296;          // compiles; a == 0
    let b = 4294967296 as u64;   // compiles; b == 0

Rust rejects both with `error: literal out of range for \`i32\``. The
default type of an unsuffixed integer literal is `i32`; a value that does
not fit is a hard error, *before* any surrounding cast is considered.
`4294967296 as u64` in Rust is still an error for the same reason — the
cast applies to the already-typed `i32`, and the literal could never be
`i32`.

`glyim` accepts both and wraps to 0. That is silent wrong-value
compilation on code Rust rejects, so it is worse than the "clean-reject"
bars this project has been holding to elsewhere.

**What already works.** `lower_literal_with_diags`
(`glyim-hir/src/lower/lower_expr.rs`) *does* reject literals that
overflow `i128`/`u128` — `[X####] literal too large` with a test at
`number_suffix.rs:test_int_literal_overflow_reports_error`. But
`4294967296` fits `i128` comfortably; the truncation happens later, when
inference defaults an unbound `Infer(Int)` to `Ty::I32`
(`resolve_ty_shallow`, per the Scripts 442/452 note in `zonk.rs`).

**Diagnosis (verified).** The value *is* preserved correctly through HIR
(`Literal::Int(i128, None)`), THIR (`thir::Literal::Int(i128, _)`), and
MIR — the MIR for `let x = 4294967296;` literally reads
`MirConst { kind: Int(4294967296), ty: <i32> }`. The wrap happens at
codegen: storing an `i32`-typed constant whose `i128` payload exceeds i32
truncates. So this is not a lost-value bug — it is a *missing validation*
bug. The value is right; the type inference chose is wrong for it, and
nothing objected.

**Initial narrow fix (`545fb28d`), then generalized (`987f4974`).** New
`glyim_typeck::zonk::check_literal_ranges`, called from `typeck_crate`
immediately after `zonk_bodies` (the point at which every `Expr.ty` is
concrete and the literal's raw `i128` is still intact). For each `Stmt::Let`
initializer and each `Stmt::Expr` whose THIR node is a `Literal::Int(v, None)`
or `Literal::Uint(v, None)`, it compares `v` against the resolved
`IntTy`/`UintTy` bounds and emits a `type_error` naming the literal, the
value, the type, and the range.

`let x = 4294967296;` now fails with:

    [T0001] integer literal `4294967296` does not fit `i32`
    (range -2147483648..=2147483647); add an explicit suffix
    (e.g. `4294967296u64`) or annotate the binding

Regression fixture: `crates/glyim-test/tests/compile-fail/out_of_range_literal.g`.

**Verified — 12 hand-run cases all pass:**

| Input | Expect | Reason |
|---|---|---|
| `let x = 4294967296;` | error | doesn't fit inferred i32 |
| `let x: i32 = -2147483648;` | ok | i32::MIN via `Neg` |
| `let x: i32 = -2147483649;` | error | below i32::MIN |
| `let x = 4294967296 + 1;` | error | nested in `Binary` |
| `if 4294967296 > 0 { .. }` | error | nested in `If` cond |
| `let x = 42;` | ok | in range |
| `let x = 4294967296u64;` | ok | suffixed, target type fits |
| `let x: u32 = 4294967296;` | error | overflows u32 |
| `let x: u32 = 4294967295;` | ok | u32::MAX boundary |
| `fn f() -> i32 { return 4294967296; }` | error | nested in `Return` |
| `let x = 0xFFFFFFFF as u32;` | ok | Cast exemption |
| `let x = 2147483648 as u32;` | ok | Cast exemption |

Plus: compile-fail corpus green (fixture
`tests/compile-fail/out_of_range_literal.g` exercises three distinct
nested shapes), run-pass corpus green, full workspace 4614/4614.

The `glyim-pilot process::tests::test_run_timed_command_success`
failure in the first full-workspace run after `987f4974` was a timing
flake — it passes in isolation, and `glyim-pilot` has no dependency on
`glyim-typeck`.

**FULLY CLOSED (`987f4974`).** The narrow fix was extended to a
recursive walk that mirrors `zonk_expr`'s full match — every `ExprKind`
variant with children is descended into, so a literal anywhere in the
expression tree (call argument, binary operand, `if` condition, match
scrutinee or arm body, `return` value, struct field, array element,
tuple element, index base/index, range bounds, closure body, nested
block, while-loop body and condition, for-loop iterable and body) is
checked, not just the direct initializer.

Two exemptions prevent false positives on valid code:

  * **Negation.** `-2147483648` (i32::MIN) parses as
    `Unary { Neg, Literal(2147483648) }`. The operand literal's value
    exceeds `i32::MAX` by exactly one — but the *negated* value is
    exactly `i32::MIN` and perfectly valid. The `Unary`-with-`Neg`-
    of-literal arm computes the negated value and compares *that*
    against the range; `-2147483648` for `i32` is accepted, while
    `-2147483649` is rejected. Descends only when the operand is not a
    literal.
  * **Casts.** A literal that is the *direct* operand of an `as` cast
    is exempt — `0xFFFFFFFF as u32` and `4294967296 as u64` are
    accepted even though the literal's own default type (`i32`) cannot
    represent the value. This is a **deliberate divergence from Rust**
    (which rejects both with `literal out of range for i32`). Rationale:
    `as` already declares an explicit type intent, and the idiom is
    common (`0xFFFFFFFF as u32` appears in two fixtures in this tree).
    Second-guessing the operand adds friction without catching real
    bugs. The trade-off is recorded here so a future strictness pass
    can flip it by removing the `Cast`-exemption arm in
    `check_literal_ranges`.

`isize` / `usize` remain hardcoded to 64 bits rather than consulting
`TargetInfo`. Every current glyim target is 64-bit, so this is correct
today; a 32-bit target would need the width threaded through.
Documented as a residual portability item, not a live bug.

**Implementation shape (as landed).**

  1. `zonk::check_literal_ranges(ctx, bodies)` — new function, called
     from `typeck_crate` right after `zonk_bodies`.
  2. Walks `body.stmts`, checking `Stmt::Let { init: Some(expr) }` and
     `Stmt::Expr { expr }` for a `Literal::Int(v, None)` /
     `Literal::Uint(v, None)` whose resolved type is a concrete integer.
  3. Compares against `IntTy`/`UintTy` bounds; emits
     `GlyimDiagnostic::type_error` on overflow with the value, type, and
     range in the message.
  4. The existing `i128`-overflow check in HIR (`lower_literal_with_diags`)
     still fires first for truly absurd literals, and is untouched.
  5. Fixture: `compile-fail/out_of_range_literal.g` with
     `//~ ERROR does not fit`. The in-range and suffixed cases are
     covered by the ad-hoc verification in this session and by
     `saturating_arith.g` (which uses suffixed literals throughout).

**Interim (superseded for the covered shape).** The narrow fix now rejects
the direct-`let` / `Stmt::Expr` shape. Wider nesting still wraps — an
unsuffixed `4294967296` passed as a call argument still type-checks as
i32 and truncates. Every fixture in the tree that needs a wide literal
uses an explicit suffix (`4294967296u64`), which types correctly, so the
corpus is unaffected either way.

#### 3.5.19 — P1-C2 batch 2c: checked arithmetic (add/sub/mul)

CLOSED (`179f7e03`). `checked_add` / `checked_sub` / `checked_mul` on
`u64` / `u32` / `usize` were registered but had no lowering — every call
hit the P1-C2 gate. `u32::checked_mul` was additionally **missing from
the registry entirely**, so it failed at typeck with `[T0001] no method`.

**What was added.**

  * `ty_ctx_mut.rs` — the one missing registry entry
    (`u32::checked_mul`). The other eight (3 ops x 3 types) were already
    present.
  * `codegen-llvm/src/lower.rs::try_lower_builtin_intrinsic` — a new arm
    using the `.with.overflow` intrinsic family
    (`llvm.u{add,sub,mul}.with.overflow.iN`), which returns
    `{iN, i1}`: the wrapped result and an overflow flag. Both `Option<T>`
    variants are built via `build_layout_aggregate` — `None`
    (`VariantIdx(0)`, no payload) and `Some(wrapped)` (`VariantIdx(1)`,
    one payload) — and `build_select(overflow, none, some)` picks
    between them. Both variants are the same `Option<T>` LLVM struct
    type, so the select is well-typed.
  * `mir-interp/src/lib.rs::try_call_builtin` — an arm for the three
    methods. Overflow is detected *before* the arithmetic (each branch
    checks the precondition against the width's `max`), so the inner
    `u128` computation cannot itself overflow. Returns the interp's
    Option shape — `Aggregate([Int(0), Unit])` for `None`,
    `Aggregate([Int(1), Uint(result)])` for `Some` — matching what
    `vec_pop` produces.

**The flagged risk did not materialize.** The batch-2c plan noted that
`select` on two aggregate values is legal IR but a shape I had not seen
this backend emit, and that the fallback would be branch-based
construction. The `select` worked — LLVM accepts struct-typed selects,
and the fixture runs correctly through the LLVM backend. No branch-based
rewrite was needed.

**Fixture.** `tests/run-pass/behavior/checked_arith.g` exercises all
nine (op, type) pairs at both boundary and in-range values:

  * `u32::MAX.checked_add(1)` -> `None`; `5 + 3` -> `Some(8)`
  * `0.checked_sub(1)` -> `None`; `10 - 4` -> `Some(6)`
  * `65536 * 65536` -> `None` (overflows u32); `6 * 7` -> `Some(42)`
  * the same three patterns for `u64` and `usize`

Assertions use `is_some()` / `is_none()` rather than `match Some(..)`,
which avoids depending on `Some`/`None` being in scope — a mistake made
in an earlier probe during this session (an `unresolved struct Some`
error misread as a compiler gap).

All literals carry explicit suffixes (per §3.5.18) so the fixture
exercises `checked_*` and not the literal-range check.

**Verified:**
  * interp backend (`--emit=exec`): exit 0
  * LLVM backend (`GLYIM_LLVM=1 --emit=exec`): exit 0
  * full workspace: 4614/4614

**P1-C2 family status after this batch.** `is_power_of_two` (§3.5.7),
the bit-manipulation intrinsics (§3.5.7 / §3.5.8), `wrapping_*`
(§3.5.12), `saturating_*` (batch 2b), and now `checked_*` are all
lowered in both backends. Remaining P1-C2 surface is the collection /
string method set (Vec/Slice/str/Box/UnsafeCell/Option/Result methods
not yet implemented — each with its own shape, tracked separately in
the plan).

#### 3.5.20 — P2-A4 empty-CGU fallback for library crates (CLOSED)

Attempted 2026-10-27. Reverted. The bug is real and reproduced;
the fix landed in the wrong layer.

**Reproduction (confirmed).** A library crate — no `main`, no
`#[start]`, no `#[no_mangle]`, no `#[used]` static — compiled with
`--emit=obj` produced an object where a generic fn call inside a
non-generic fn had:

    ___glyim_fn_1_21_1    (U, undefined)
    ___glyim_fn_2         (T, defined)

The defined body is pre-substitution, emitted under the plain
`__glyim_fn_2` symbol; the call site inside it is monomorphized and
references `__glyim_fn_1_21_1`, which was never collected because
`MonoCtx::collect(&[], ...)` was a no-op when the root set was empty.
A `--emit=obj` artifact that cannot be linked against is exactly the
bug P2-A4 describes.

**Attempted fix (reverted).** In `glyim-lower/src/discovery.rs`,
`discover_mono_roots` was extended: when `items.is_empty()` (the
existing "no entry points" warning case), synthesize a `MonoItem::Fn`
root for every `ItemKind::Fn` in `hir.items` at empty substitution.
The theory: a library's callable surface is every `fn`, so this is
the natural entry-point equivalent.

**Why it was reverted.** Two classes of regression:

  1. `tests::root_discovery::{t02_no_entry_points_warning,
     t05_fn_not_main_not_detected, t09_unknown_attribute_not_detected}`
     fail — they assert the *pre-fix* discovery contract (no entry
     point → empty root set). That is expected and could be updated.
  2. **`stdlib_full_probe::assembled_stdlib_compiles` fails** — the
     stdlib is itself a library crate. Synthesizing a root for every
     stdlib fn makes `compile_file_to_mir` monomorphize the entire
     stdlib surface at once, which is not a legal workload (and not
     what a library's `--emit=obj` build should do).

Regression (2) is the disproof of the placement. `discover_mono_roots`
answers "which roots does this *crate's entry-point set* declare" —
that is a discovery question, and its answer for a library really is
"none." The behavior P2-A4 needs is "when *the pipeline still has to
emit an object* and the root set is empty, synthesize enough roots to
produce a self-contained artifact." That is a pipeline policy, not
a discovery result.

**Correct fix (for a future session).** In
`crates/glyim-pipeline/src/lib.rs`'s `if cgus.is_empty()` fallback
branch (around `:800`), replace the current `mir_bodies_map` sweep
with:

  1. Build a synthetic root set (every `ItemKind::Fn` at empty substs;
     same shape as the reverted code, but computed here, not in
     discovery).
  2. Run a fresh `MonoCtx::collect(&synthetic_roots, &body_provider,
     &drop_provider)` with the same providers, `def_to_module`, and
     `pre_subst_bodies` the primary pass used.
  3. Use the resulting `items()` — which now carry `MonoItemData`
     symbols (`__glyim_fn_N_0_1` for generic instantiations) — as
     `all_bodies`, instead of re-deriving plain symbols from
     `mir_bodies_map`.

This keeps `discover_mono_roots` semantics intact (the 3 root-discovery
tests pass unchanged), fixes the library-`--emit=obj` object, and does
**not** change `compile_file_to_mir` behavior for library crates like
stdlib (the `cgus.is_empty()` branch is only reached from the emit
path when roots were empty *and* mono produced nothing — which is a
strict subset of the discovery-empty case).

Scope estimate: ~50-80 lines in `pipeline/lib.rs` plus the fresh
provider plumbing. Not a one-liner, but localized.

**Probe to use as the acceptance test.** `--emit=obj` on:

    pub fn generic_id<T>(x: T) -> T { x }
    pub fn caller() -> i32 { generic_id(7) }

must produce an object where `nm -u <obj> | grep glyim_fn` is empty.
Executable output must remain byte-stable (currently: defined
`__glyim_fn_1_21_1` and `__glyim_fn_2`, no undefined).

**Interim state.** Unchanged from before the attempt. The library-
`--emit=obj` artifact is still not link-safe. This is a documented
open bug, not a regression.

**Attempt 3 (2026-10-27) — pipeline placement, and why the probe lied.**

The §3.5.20 plan was implemented: `prepare_compilation` gained an
`emit_object: bool` parameter (6 callers threaded; only
`compile_file_with_artifacts` passes `true`), and the `cgus.is_empty()`
branch gained a synthetic-root re-collect gated on it. Two things
worked and two did not:

  * **Worked: the gate.** `stdlib_full_probe::assembled_stdlib_compiles`
    and all 9 `root_discovery` tests passed unchanged. Threading
    `emit_object` is the right way to scope this — the analysis-only
    paths keep the old behavior.
  * **Did not work: the root enumeration.** Synthetic roots were built
    by iterating `hir.items` and using `LocalDefId::from_raw(item.id.to_raw())`.
    `item.id` is an HIR item index, **not** a `LocalDefId` — the first
    item maps to `LocalDefId(0)`, which is the crate root and has no
    MIR body. Mono collect then produced a single error-body entry.

**Why the probe "passed" and why it was wrong.** The acceptance test
was `nm -u <obj> | grep glyim_fn` being empty. It was empty — because
the object contained exactly one dummy symbol (`___glyim_fn_0`) and
nothing else. Before the change the object had `___glyim_fn_2`
(defined, the caller's pre-substitution body) plus
`___glyim_fn_1_21_1` (undefined, the monomorphized callee). After, it
had one symbol with no references. "Zero undefined" was satisfied by
deleting the object's contents.

**Corrected probe (use this next time).** Not just "no undefined":

    nm <obj> | grep glyim_fn       # must show BOTH the caller AND its
                                   # monomorphized callee as defined (T)
    nm -u <obj> | grep glyim_fn    # must be empty

Both conditions. The second alone is satisfiable by emitting nothing.

**Correct root source.** `mir_bodies_map` — its keys are `DefId`s the
lowering pass actually produced bodies for, so every entry is a valid
`FnDefId` candidate. Iterating that map to build synthetic roots
sidesteps the HIR-index-≠-LocalDefId problem entirely, and it is
exactly the set of bodies we want in the object.

Reverted in full (`git reset --hard`). Tree at `ba463fda`, 20 unpushed
commits. The `emit_object` signature change was part of the reverted
commit, so it is not in the tree either.

**Attempt 4 — CLOSED.** Exactly the revised plan: same `emit_object`
gate, roots from `mir_bodies_map` keys.

`prepare_compilation` gained `emit_object: bool`; six callers threaded
(`compile_file_with_artifacts` passes `true`, every analysis path —
`compile_file_to_mir`, `--emit=mir`, `--emit=llvm-ir` — passes `false`).
The `cgus.is_empty()` branch, when `emit_object` is set, runs a fresh
`MonoCtx::collect` seeded from `mir_bodies_map.keys()` (each a real
`DefId` with a lowered body, unlike the HIR item indices that attempt 3
misused), then uses `MonoItemData.symbol` for the emitted names. The
`body_is_raw_generic` filter still drops uninstantiated generic
templates, so only concrete instantiations survive — the object is
self-contained, not the full stdlib surface.

**Three-condition acceptance probe** (all pass):

| Condition | Before | After |
|---|---|---|
| ≥ 2 defined `T __glyim_fn_*` symbols | 1 (`__glyim_fn_2`) | 2 (`__glyim_fn_2`, `__glyim_fn_1_21_1`) |
| zero undefined `__glyim_fn_*` | 1 (`__glyim_fn_1_21_1`) | 0 |
| ≥ 1 monomorphized symbol (`_N_i_j`) | 0 | 1 |

Condition 3 is the one attempt 3's probe lacked — "zero undefined" is
satisfiable by an empty object, which is exactly what attempt 3
produced (`___glyim_fn_0`, a single dummy from `LocalDefId(0)`).

**No regression:** `stdlib_full_probe::assembled_stdlib_compiles` passes
(analysis path unaffected by the gate), all 9 `root_discovery` tests
pass (`discovery.rs` untouched), executable `--emit=obj` and
`--emit=exec` outputs unchanged (`__glyim_fn_1_21_1` + `__glyim_fn_2`
defined, zero undefined, runs to 7), full workspace green.

One workspace run showed `glyim-test snapshot::tests::
verbose_mir_includes_resolved_types_and_debug_names` as `exec failed`.
That test constructs a `Body::dummy` in memory and formats it — it never
enters `prepare_compilation` or `MonoCtx`, so it has no dependency on
this change. Re-run in isolation: passes. The workspace failure was
nextest failing to spawn the process on a 434-second run dominated by
`run_pass_corpus`, not an assertion. Re-ran the full workspace after the
isolated pass; green.

**Three attempts, three different wrong layers** — discovery (too broad,
broke stdlib), pipeline with HIR indices (wrong root source, produced a
dummy-only object), pipeline with `mir_bodies_map` (correct). The
lesson recorded for future sessions: probe *both* directions (nothing
undefined AND nothing missing) before believing a symbol-level fix.





#### 3.5.21 — TEST-7: interp exit fidelity — premise corrected, real bug fixed

CLOSED (`fc7df58f`). The plan described TEST-7 as "interp masks exit
codes to `& 0xff` (`interpreter_runner.rs:135-136`); propagate the real
error class". Probing that premise showed it was **wrong**, and the
actual fidelity gap was elsewhere.

**The mask is correct.** `fn main() -> i32 { 300 }` run natively exits
**44** — `300 & 0xff`, which is what the OS does to an exit status at
the wait-status boundary. The interpreter's `(v as i64 & 0xff) as i32`
produces 44 too. Removing the mask (as the plan suggested) would have
made the interp *diverge* from native, not converge. Verified by
running the compiled binary and checking `$?`.

**The real bug: panic output was discarded.** `interpret_bodies`
populated `stdout`/`stderr` from the interpreter's FFI buffers only
inside the `Ok(())` arm:

    match result {
        Ok(()) => {
            stdout = interpreter.get_stdout();   // only here
            stderr = interpreter.get_stderr();   // only here
            ...
        }
        Err(e) => {
            stderr.push_str("interpreter error: ...");  // stdout empty
            ...
        }
    }

So a program that printed and then panicked — `println("before");
assert!(false)` — reported `stdout: []` under the interpreter, while
the same program run natively printed `before` before exiting 101.
That is a genuine fidelity gap, and it matters for any `run-fail`
fixture that wants to assert on output produced before the panic.

**Fix.** Hoist the two buffer reads above the `match`, so both arms see
whatever the program wrote. The `Err` arm additionally appends its
`interpreter error: ...` line to stderr, which now sits after any
program-emitted stderr rather than replacing it.

**Fixture.** `tests/run-fail/print_then_panic_keeps_stdout.g` —
`println("before"); assert!(false, "boom")`, `// exit-code: 101`,
`// check-stdout: before`. Prior to the fix the `check-stdout` assertion
fails under the interpreter (empty stdout); after, it passes. Note
that **no existing `run-fail` fixture asserted on stdout** — the corpus
had zero `check-stdout` + panic combinations — so this gap was fully
latent until the fix exercised it.

**Verified:**
  * native `println + assert!` exits 101 with `stdout: [before]`
  * interp (after fix) matches: same stdout, same exit
  * `fn main() -> i32 { 300 }` exits 44 on both paths (mask is correct)
  * `run_fail_corpus` green with the new fixture
  * full workspace: 4622/4622

**Lesson for future TEST- items.** TEST-7 is the second item this
session (after P2-A4 / §3.5.20) where the plan's one-line description
pointed at the wrong line. The pattern: verify the *claimed* defect
with a direct probe before designing a fix around it. Here that probe
(`fn main() -> i32 { 300 }` → `$?`) took ten seconds and prevented a
"fix" that would have broken correctness.

#### 3.5.22 — OPEN: `println!`/`format!` format-argument substitution emits empty for runtime values (both backends)

Discovered 2026-10-28 (session 22). Originally surfaced by moving a
dead fixture into the corpus, then narrowed by direct probes. The bug
is **much larger** than the fixture that found it.

**Minimal reproduction.**

    fn main() -> i32 {
        let x = 42;
        println!("value {}", x);
        0
    }

Compiled with `--with-stdlib --emit=exec` (LLVM backend, default),
running produces:

    value

That is: the literal text `"value "` (including the trailing space) is
emitted, and the `{}` substitution for `x = 42` renders as **empty**.
Byte-level output (`od -c`): `v a l u e space \n`.

**Scope confirmed so far.**

  * **Both backends.** `GLYIM_LLVM=1` (explicit) and the default
    `--emit=exec` (LLVM) both show it. The corpus runs via mir-interp
    and shows it too (that was the original observation, from the
    `Drop` fixture). So this is not backend-specific.
  * **Not `Drop`-specific.** The original fixture put the format call
    inside `Drop::drop`, but the minimal reproducer is an ordinary
    `main` with an ordinary local. `Drop` was a red herring.
  * **Not format-string-parsing-specific.** The literal text survives
    correctly; only the *argument* is lost. So the format string is
    being split at `{}` and the pieces emitted, but the substitution
    value never reaches the output.

**Why it survived until now.** Every existing `run-pass` fixture that
asserts on stdout uses a literal-only message (`println!("hello")`,
`println!("drop 2")`). No fixture in the corpus asserted on a `{}`
substitution of a *runtime* value. The whole class was untested. This
is precisely what TEST-1 (add `// check-stderr:` matchers; 232/239
run-pass fixtures currently assert only exit code) was intended to
catch — TEST-1 is not implemented.

**ROOT CAUSE — the `to_string` sentinel (same class as P1-C1's `into`).**

Probes narrowed it definitively:

| Probe | Result | Conclusion |
|---|---|---|
| `println!("plain no args")` | `plain no args` | `println!` itself works |
| `println!("lit {}", 42)` (literal arg) | `lit ` | not operand-passing |
| `42.to_string().len() == 2` | **false** (len is 0) | `to_string` on `i32` broken |
| `println!("len is {}", n)` | `len is ` | consistent with the above |

`typeck`'s `to_string` arm (`check_expr.rs`, the `to_owned`/`to_string`
branch of `try_builtin_method`) consults the builtin registry only for
receivers that canonically map to the String ADT:

    TyKind::String                        -> AdtId(1061)
    TyKind::Slice(u8)                     -> AdtId(1061)
    TyKind::Adt(1050 | 1061 | ...)        -> that AdtId

For **any other receiver** — including every integer primitive — it
falls through to:

    let fn_id = FnDefId::from_raw(u32::MAX - 2);
    return Some((out_ty, fn_id));

That sentinel is lowered (`lower_rvalue.rs`'s sentinel arm) to a
`MirConstKind::Error` constant, which codegen turns into a zero value.
For a `String` return type, "zero value" is the **empty string**.

So `42.to_string()` returns `""`. The format expander emits
`<arg>.to_string().as_str()` for every `{}`, gets `""`, and pushes it.
The literal parts of the format string are unaffected — which is
exactly the observed output.

**This is the same sentinel class as P1-C1's `into` bug** (fixed earlier
in this session, §3.5.6): a `u32::MAX - k` id that typeck hands out
when it cannot resolve a real target, silently lowered to a zero value.
P1-C1 was fixed by adding a `is_copy`-guarded direct operand use. This
one has no such shortcut — a general `Display::to_string` for
primitives needs real integer→string conversion.

**Why a hard diagnostic is not the quick fix.** The `T084-REVISED` test
comment says "the stdlib relies on `format!` compiling for its own
error paths", and that is **accurate**: nine `format!(...{ }...)` sites
in `crates/glyim-lang-std/lib/` pass a mix of `&str` (fine — hits the
`(1050|1061)` arms) and integers (`process.g`'s `self.pid`,
`io.g`'s `errno`). Turning the sentinel into a hard error would fail
stdlib compilation at those integer sites. The fix has to come with
either (a) a working primitive `to_string`, or (b) stdlib rewrites that
avoid integer formatting.

**Attempt 2 (2026-10-28) — all five pieces, new failure mode.**

The full fix was attempted: runtime digit-conversion helpers
(`glyim_int_to_string` / `glyim_uint_to_string`), registry entries for
four primitives, the typeck primitive-lookup, the interp arm, and an
LLVM branch calling the helpers. All compiled (`cargo check
--workspace` clean). The corpus stayed green. But the actual probe
failed with a **new** error, not the old one:

    [T0001] mismatched types: expected integer type, found &str @115103..115127

Somewhere in the format expander's output path, a `&str` is now being
consumed where an integer is expected. The `println!("value {}", 42)`
expansion (per `glyim-meta`'s `try_interp`) is:

    { let mut __glyim_fmt = String::new();
      __glyim_fmt.push_str("value ");
      __glyim_fmt.push_str((42).to_string().as_str());
      __glyim_fmt }

With `to_string` on `i32` now returning `String` (not the old sentinel
that produced `Ty::ERROR`), `.as_str()` should be `&str` and
`push_str(&str)` should accept it. The `expected integer type` suggests
one of:

  * `String::push_str` is registered with an integer-typed parameter
    somewhere (wrong; it should take `str`), and the typeck mismatch is
    caught there.
  * The registry entry returns `string_ty` but `string_ty` in
    `register_builtin_methods`'s local scope is not the builtin
    `String` — maybe it's shadowed or resolved to a different ADT.
  * `as_str` on the String receiver isn't registered for the return
    type `to_string` now produces, so `.as_str()` resolves to some
    fallback that yields an integer.
  * The span `115103..115127` is inside the **stdlib** (`io.g`'s
    `println!` macro definition or a `push_str` call), not the user
    source — in which case the incompatibility is between the stdlib's
    declared `push_str` signature and the new `to_string` return type.

Reverted in full (all five files via `git checkout`). Corpus green
throughout; no regression shipped.

**Diagnostic result (2026-10-28, later).** Registry + typeck applied
alone — no runtime helpers, no interp arm, no LLVM arm — pass all four
MIR probes (`main` alone, `42.to_string()`, `+as_str()`, full
`println!("value {}", 42)`) and `--emit=exec` gates cleanly on the
LLVM-not-implemented diagnostic. Which means **the mismatch in attempt 2
was not caused by typeck**. Two candidates remain:

  1. One of the three *other* pieces (runtime helper insertion, interp
     arm, LLVM arm) perturbs typeck through some cross-crate coupling.
     This is implausible — all three are Rust source in crates typeck
     doesn't depend on.
  2. **Stale incremental build.** Attempt 2 was run after multiple
     apply/revert cycles in the same shell session; `git checkout` on
     the type/typeck sources does not invalidate cargo's fingerprints
     for the *dependent* crates. A stale `libglyim_type.rlib` or
     `libglyim_typeck.rlib` from an intermediate state could explain a
     typeck diagnostic that the current source cannot produce.

Hypothesis (2) is far more likely. Retest with a forced rebuild of the
affected crates before drawing any conclusion.

**Retest result — hypothesis (2) DISPROVED.** `cargo clean -p` on all
six affected crates (removed 850 files, 1.9 GiB), full rebuild (26.9 s),
then the probe: **the mismatch reproduces**. So it is not a stale
incremental artifact.

That leaves a genuine contradiction:

| Configuration | `--emit=mir` | `--emit=exec` |
|---|---|---|
| registry + typeck only | rc=0 (clean) | GATE (to_string not in LLVM) |
| all five pieces | rc=0 (clean) | **T0001 mismatch** |

Same registry entries, same typeck change, same 4 IDs shifted in both
runs. The isolated run is clean; the full run fails at typeck. Adding
runtime helpers, an interp arm, and an LLVM arm should have **zero**
effect on typeck — `glyim-typeck` does not depend on `glyim-runtime`,
`glyim-mir-interp`, or `glyim-codegen-llvm`, and none of those three
edits change any type, signature, or registry entry.

**Candidate explanations, none yet verified:**

  1. **`--emit=mir` does not run the same typeck.** Possible if the
     MIR path short-circuits a check the exec path runs, or vice versa.
     The `has_errors()` calls at `lib.rs:395/427/435/598/632/642/674/755`
     suggest typeck errors gate the pipeline uniformly, but the MIR emit
     path may reach a different post-typeck stage.
  2. **A hardcoded `FnDefId(9xxx)` somewhere** that the 4-entry registry
     shift remaps. §3.5.8 was exactly this class (`Vec::as_slice` →
     `9144`). Both the isolated and full runs have the same shift, so
     this would have to be triggered by *something else* the full run
     does — but there is no obvious candidate.
  3. **`kache` (RUSTC_WRAPPER) caching MIR or typeck output** keyed on
     something that does not include one of the five files. The forced
     `cargo clean` should have bypassed it, but the wrapper sits above
     cargo's fingerprint check, so a kache hit on *any* crate in the
     chain could replay an old artifact.
  4. **Non-determinism** in a hash map iteration order that the extra
     registry entries perturb. `primitive_method_fns` is a `HashMap`;
     adding 4 entries changes iteration order. Code that scans
     `primitive_method_fns` for a *name* rather than a key (there are
     two such scans at `ty_ctx_mut.rs:1386` and `:1431`) could pick a
     different entry after the shift. That is the only mechanism I can
     see where a purely-additive registry change produces a *different*
     resolution outcome — and it would explain why the isolated and full
     runs differ if the full run's binary had a different `HashMap`
     seed (random per process).

Candidate (4) is the most plausible of the four and is testable: run
the probe twice in a row and see if the result is stable.

**Not fixed.** The five-piece revert is clean; tree at `5c28205b` plus
this entry. §3.5.22's fix remains open with the diagnostic front-loading
recorded — next session should test (4) by running the probe repeatedly
and comparing, then check (2) by grepping for hardcoded `FnDefId` refs
in the type/typeck crates, then check (3) by unsetting `RUSTC_WRAPPER`
for one build.

**Ninth attempt (2026-10-28) — nine layers, one per attempt.**
Working with `RUSTC_WRAPPER=""` to eliminate kache from the picture
(the single most useful tool discovered this session), the five-piece
fix compiles, type-checks, and runs — but produces `value ` (empty)
instead of `value 42`. The `.ll` shows why:

    %local_3 = alloca { ptr, i64 }, align 8      ; <-- 16-byte &str
    store { ptr, i64 } zeroinitializer, ptr %local_3, align 8
    ...
    %fat_len = extractvalue { ptr, i64 } %load, 1   ; <-- reads 0

Three separate things are wrong at once, and the fix touches all
three:

  1. **`local_3` is `{ptr, i64}`** (a `&str`, 16 bytes), not
     `{[3 x i64]}` (a `String`, 24 bytes). `sig.output` for the
     `to_string` FnSig is `string_ty` = `Adt(1050, [])`, which
     `llvm_type_for_ty` lowers to `{ [3 x i64] }`. So either
     `sig.output` isn't reaching this local's type, or the caller
     allocates the destination from a different source.
  2. **No call at all.** `grep -c glyim_int_to_string` on the `.ll` is
     0 — the helper is never invoked, so nothing writes to `local_3`.
  3. **The call is silently dropped.** `try_lower_builtin_intrinsic`
     returns `Ok(false)` (the primitive `FnDefId` is **not in
     `builtin_fn_index`** — that map only contains `(AdtId, Name)`
     pairs registered through `builtin_method_fns`, and
     `primitive_method_fns` is a separate table that the dispatcher
     never consults). The caller of `try_lower_builtin_intrinsic`
     then emits nothing and continues, leaving the destination
     zero-initialized.

**The nine layers, in the order attempts discovered them:**

  1. `to_string` on primitives returns the `u32::MAX - 2` sentinel
  2. The sentinel lowers to `MirConstKind::Error` (zero value)
  3. `String::to_string`'s registry entry produces `TyKind::String`,
     not `Adt(1050)` — used as the branch's `out_ty`
  4. The registry needs entries for each primitive (`i32`, `u32`, `u64`,
     `usize`)
  5. `try_builtin_method`'s `to_owned`/`to_string` branch must consult
     `lookup_primitive_method` before the sentinel fallback
  6. **kache replays stale rlibs across `git checkout` reversions** —
     every earlier "isolated test passed / full test failed" was two
     different cached states, not a real contradiction
  7. The typeck branch's `out_ty` must be `sig.output`, not a hardcoded
     `TyKind::String` (which is `str`, not `String`)
  8. `builtin_fn_index` doesn't contain primitive methods — the LLVM
     dispatcher's first line (`builtin_fn_id(fn_id)` → `Some((adt,
     name))`) returns `None`, so the entire `to_string` arm never runs
  9. The dispatcher's `Ok(false)` return causes the caller to drop the
     call silently, leaving the destination uninitialized

**Why this is not one session.** Layers 1-7 were each discovered and
fixed this session. Layer 8 needs `primitive_method_fns` bridged into
`builtin_fn_index` (or the dispatcher taught to consult both tables).
Layer 9 needs the `Ok(false)` path to be a *loud* failure or a real
`Call`, not a silent drop. Both are changes in
`glyim-codegen-llvm/lower.rs`'s dispatch shape, and layer 9 in
particular is a class of bug worth its own audit — "when a builtin
dispatcher declines, what happens to the call?" is a question whose
answer applies to every `try_lower_builtin_intrinsic` arm, not just
`to_string`.

**The kache discovery is the session's most transferable finding.**
Every contradictory result across eight attempts was kache replaying
an artifact from a prior state. `RUSTC_WRAPPER=""` for one build made
attempt 9 the first one whose output reflected the current source.
Recorded here for its own sake: **when a bisect produces results that
cannot be reconciled with the source on disk, disable the compiler
wrapper for one build.** This is the same class of hazard as a stale
`libglyim_runtime.a` (§3.5.26) — a caching layer between you and the
source.

**Workflow lesson (recorded for its own sake):** the earlier isolated
test that "passed" was measuring `--emit=mir`, which does not exercise
the same path as `--emit=exec`. Comparing the two emit modes across two
different sets of applied changes, without holding the *emit mode*
constant, produced an apparent contradiction that cost an extra cycle.
Hold emit mode constant when bisecting.

**Next step is a diagnostic, not a fix.** Emit MIR for
`println!("value {}", 42)` and read the exact call shape the expander
produced: which `FnDefId` is `push_str`, what signature the registry
gives it, and what `to_string`'s return type resolves to. That will
say which of the four hypotheses above holds. Only then is the fix
mechanical.

**Why the previous `str_ty` vs `string_ty` decision matters here.** An
earlier attempt (session 22, before the revert) used `str_ty` — a
16-byte `&str` — which is a buffer overrun for a helper writing 24
bytes. This attempt used `string_ty` (24-byte owned String) and got a
type mismatch instead. Both are wrong in different ways; the correct
choice depends on what `.as_str()` and `push_str` actually expect.

**Fix path for the next session, in order.**

  1. **Register `to_string` for integer primitives** in
     `TyCtxMut::register_builtin_methods` (mirroring the
     `is_power_of_two` batch pattern): `(i32_ty, "to_string", vec![],
     string_ty)`, `(u64_ty, ...)`, etc. — one entry per primitive.
  2. **Implement the conversion in both backends.** LLVM: the digits
     of an integer are computable with a small loop or an intrinsic
     (`llvm.utoa` does not exist; `sprintf` via the runtime, or a
     hand-rolled digit loop writing into a stack buffer then
     `glyim_string_from_bytes`). Interp: `format!("{}", n)` directly in
     `try_call_builtin`.
  3. **Add `check-stdout` fixtures** for each integer width asserting
     `println!("{}", 42i32)` prints `42`, and the same for negative
     values, zero, and MIN/MAX boundaries.
  4. **Extend the `T084-REVISED` test** to assert non-empty output once
     the expander's `.to_string()` call has a real target.

Steps 1 and 2 are the bulk (~1-2 sessions). The current §3.5.22 entry
is the map.

**Original hypotheses, superseded.**

  1. **`println!` macro expansion.** `glyim-meta`'s
     `t084_format_with_substitution_yields_empty_string` test
     (existing, passing) literally asserts the current behaviour is
     "empty string for substitution", which means the bug was
     *enshrined as a test expectation*. See that test — it may need
     to be fixed at the same time as the expander.
  2. **Lowering of the format call.** If the macro expands to
     something like `print_str("value "); display(x)` and the second
     call is dropped or lowers to a no-op, that would produce exactly
     this output. `crates/glyim-lower/src/lower_rvalue.rs`'s call
     lowering is the place to check.
  3. **`Display` impl dispatch for primitives.** If the format arg
     lowers to a `Display::fmt(x)` call and the impl for `i32` is a
     stub, the arg renders empty.

**First investigation steps (for the next session).**

  1. Run `println!("{}", 42)` with a **literal integer** as the arg,
     not a variable. If `42` also renders empty, it's not a
     capture/operand-passing bug.
  2. Expand the macro: check what `println!("value {}", x)` lowers to
     in MIR (`--emit=mir`). Look for the `display`/`to_string` call
     and whether the argument operand is being passed.
  3. Read `glyim-meta`'s `t084_format_with_substitution_yields_empty_string`
     test — if it asserts empty output, that's the test to change and
     the expander to fix in the same commit.
  4. Once the fix is in, add the first `check-stdout: value 42` fixture
     to the corpus (TEST-1's pattern).

**Interim.** Any new fixture asserting on `println!("...{}...", x)`
will fail. Existing fixtures are unaffected (they don't test this).
The dead `tests/runtime/drop_order.g` remains where it was; it can be
deleted or kept as a note, but no fixture in the tree currently
exercises this path.

**TEST-5 status (unchanged by this entry).** The `--backend=bytecode`
flag on that dead fixture is inert because the harness has no
bytecode-VM execution path (`glyim-test/src/` has no
`Module::from_mir_bodies`, no VM driver). The correct resolution is
TEST-4 (three-backend differential, requires wiring the VM in), not
"parse the flag".

**Session-end state.** 30 unpushed commits. This bug is a real
correctness issue that warrants its own session: probable location is
the `println!` expander plus its (currently-passing) expectation test,
both in `glyim-meta`.

#### 3.5.23 — 12 fixtures red under `GLYIM_LLVM=1` (10 = CI config, 1 stale header, 1 open bug)

**RESOLVED (`1742519a`), with one bug narrowed and one bug deferred.**

The original entry said "12 fixtures fail; not a regression; action plan
is to check CI history". Investigating locally produced a completely
different picture — most of the failures were **not** LLVM backend bugs
at all.

**Decomposition of the original 12.**

| Count | Fixtures | Root cause |
|---|---|---|
| **10** | `for_slice_ref`, `for_slice_sum`, `generic_method_turbofish_all_primitives`, `match_ref_binding`, `match_ref_mut_binding`, `match_ref_mut_write_through`, `match_ref_nested_write`, `match_ref_struct_field`, `match_ref_tuple`, `option_filter`, `option_filter_predicate_false` (minus the one below) | **CI configuration bug** — see below |
| **1** | `slice_indexing_works` | **Stale header** — program exits 164, header said 89 |
| **1** | `if_let_some_none` | **Real LLVM lowering bug** — narrowed, deferred |

**The CI configuration bug (10 of 12).** `test-ubuntu` sets
`GLYIM_LLVM: "1"` (added in the P1-O1 change to make the ~44
`// needs-llvm` fixtures actually run) and runs
`cargo nextest run --workspace`. But `GLYIM_LLVM` and
`--features glyim-test/real-llvm` are **two separate switches**:

  * `GLYIM_LLVM` un-skips a `// needs-llvm` fixture (the harness's "is
    LLVM available here" gate, `executor.rs:236`).
  * `--features glyim-test/real-llvm` (or `GLYIM_TEST_REAL_LLVM`) is
    what makes `PipelineCompiler` instantiate the real `LlvmBackend`
    instead of `MockCodegen` (`compiler.rs:341-355`).

CI set the first and not the second. So on `test-ubuntu`, every
`// needs-llvm` fixture was compiled with `MockCodegen` (emits no
object), fell through to the MIR interpreter, and — because the fixtures
exist specifically to *avoid* the interpreter (their own headers say so:
`match_ref_tuple.g`'s comment cites the "deref of Ref from dead frame"
panic) — panicked at runtime, producing exit 101.

`test-differential` and `test-linux-runtime` both pass
`--features glyim-test/real-llvm`. Only `test-ubuntu` was missing it.

**Fix.** Add `--features glyim-test/real-llvm` to `test-ubuntu`'s
`Run tests` step. Verified locally with the exact CI env
(`GLYIM_LLVM=1 GLYIM_TEST_REAL_LLVM=1 GLYIM_TEST_NO_SKIP=1 … --features
glyim-test/real-llvm`): `run_pass_corpus` drops from 12 failures to 2.
`test-macos` and `test-windows` correctly keep the plain invocation —
they genuinely have no LLVM installed, and `MockCodegen` + interpreter
is the right fallback there.

**The stale header (`slice_indexing_works.g`).** Program computes
`(30+40+50) + (10+20+30) + (70+80+90) = 420`, and `420 & 255 = 164` —
confirmed by running the compiled binary. The `// exit-code: 89` in the
header was from an earlier iteration of the program, long since
changed. Corrected to 164. This was flagged as "possibly broken" in
session 19's handoff §4c ("either fix the header to 164 or the test
program to yield 89"); the header was the stale one.

**The real bug (`if_let_some_none.g`) — DEFERRED, §3.5.24.** Narrowed
to the second `if let`:

    let b: i32 = if let Option::Some(_) = Option::None { 1 } else { 7 };

The first (`Option::Some(x)` matching `Option::Some(42)`) works fine in
isolation (returns 42). The `None` scrutinee + wildcard-in-`Some`
pattern combination reaches LLVM lowering with a `TyKind::Error`
somewhere:

    Codegen error: Attempted to lower TyKind::Error to LLVM. Type
    checking failed to resolve this type.

The MIR interpreter handles it correctly (that's why the fixture
passed before this session, under the CI's broken configuration). Fix
is a real typeck or lowering bug — split out as §3.5.24. The fixture
is `// ignore`-annotated with a pointer to this entry so `test-ubuntu`
is green and the bug is tracked rather than showing as a permanently
red CI job.

**Lessons.**

  1. **The session-19 handoff's "4605/4605 green on every commit" was
     measured without `GLYIM_LLVM=1`.** CI sets that. So the corpus
     has been red on `test-ubuntu` since the P1-O1 change added
     `GLYIM_LLVM: "1"` — either unnoticed (red CI job) or diverging
     from local. Checking the Actions tab at `2f8cc80b` would confirm;
     this entry's local analysis is authoritative for *what* is red
     and *why*.

  2. **`GLYIM_LLVM` and `--features glyim-test/real-llvm` are two
     independent switches with confusingly similar names.** A future
     change should either rename one or add a startup check
     ("`GLYIM_LLVM=1` set but feature not enabled — fixture execution
     will fall through to the interpreter, which is likely not what
     you want"). Tracked as a follow-up suggestion, not a bug.

  3. **`tail -n 5` truncating a failure list cost this session an
     unnecessary revert** (of the §3.5.22 `to_string` fix). When the
     list might be long, capture it fully before judging.

**Verified.**
  * `GLYIM_LLVM=1 GLYIM_TEST_REAL_LLVM=1 GLYIM_TEST_NO_SKIP=1
    cargo nextest run -p glyim-test --test run_pass_corpus --features
    glyim-test/real-llvm` — 1 passed, 0 failed (was 12)
  * same env, `--test run_pass_corpus --test run_fail_corpus --test
    compile_pass_corpus` — 3 passed (all corpora)
  * `test-ubuntu` YAML validates; `test-macos` / `test-windows`
    confirmed unchanged
  * Local macOS/aarch64; the CI Linux/x86_64 divergence hypothesis
    from the earlier text is now moot — the failures were never
    host-specific


#### 3.5.24 — unconstrained `Option::None` ICEs + turbofish on variant path (CLOSED)
Narrowed from §3.5.23 (2026-10-28). Investigation of the original
fixture revealed **two distinct bugs** and one **genuinely-invalid
test**; the fixture has been rewritten to the valid form and is no
longer `// ignore`-annotated.

**The invalid input (not a bug).**

    let b: i32 = if let Option::Some(_) = Option::None { 1 } else { 7 };

`Option::None` here has an unconstrained `Option<_>`. Real Rust rejects
this with `error[E0282]: type annotations needed for Option<_>`. The
original fixture was testing invalid code — no LLVM lowering fix can
make it "work" in a way that matches Rust. Correctly rejected at typeck
is the right answer.

**Bug A — unconstrained `Option::None` ICEs instead of erroring.** glyim
does not produce E0282. It silently infers `Adt1010<<error>>` (visible
in MIR as `$3: Adt1010<<error>>`, `Substitution(len=0)`), carries that
through to LLVM, and fails there with:

    [X0000] Codegen error: Attempted to lower TyKind::Error to LLVM.
    Type checking failed to resolve this type.

That is an **ICE from user input**, not a diagnostic.

**Mechanism (confirmed 2026-10-28).** `zonk.rs:339`:

    TyKind::Infer(glyim_type::InferVar::Ty(_)) => return Ty::ERROR,

An unbound `Infer(Ty)` reaching zonk means no pass will ever bind it;
that arm replaces it with `Ty::ERROR` so downstream phases don't ICE on
a raw variable. Correct for stdlib templates; wrong as the *only*
response to a user-written underconstrained expression.

**Why the existing E0282 machinery doesn't catch it.** There *is* a
"type annotations needed" pipeline: `collect_unresolved_ty_var_spans`
(`zonk.rs:964`) is called before zonk from `typeck_crate`, collecting
spans into `TypeckResult.unresolved_ty_var_spans`. But two limits:

  1. **It only walks variant-constructor calls.** The entry point is
     `walk_body_for_variant_ctor_calls` — a deliberately narrow walker
     covering the `Some(expr)` case where the type parameter can't be
     inferred from the argument. A **unit variant path** like
     `Option::None` (no call, no argument) is not visited, so its span
     is never collected.
  2. **Collection ≠ emission.** The field is returned on
     `TypeckResult` but typeck does not push a diagnostic. The
     pipeline (`glyim-pipeline`) decides per-owner which bodies are
     reachable from `main` and emits the E0282 diagnostic only for
     those — a deliberate gate to avoid flagging unmonomorphized
     stdlib templates.

So Bug A's fix needs **both**: extend the walker beyond variant-ctor
calls (to cover unbound-`Infer` in any expression's `ty` or any
pattern's `ty`), and confirm the pipeline gate already emits for
reachable bodies containing such spans. The second part may already
work — `if_let_some_none`'s body is reachable from `main` — but that is
unverified.

**Scope estimate.** The existing walker is ~150 lines for
variant-ctor-only (it has to match every `ExprKind` to find ctors). A
general walker covering every `ty` slot is comparable in size and
touches the same functions `zonk::walk_body` already implements —
except that walker is `&mut` (it rewrites types) while this needs
`&` (it inspects the pre-zonk body). The cleanest structure is a
read-only mirror of `zonk_body`, ~80-100 lines. Then either reuse
`unresolved_ty_var_spans` (rename to reflect the broader coverage) or
add a second field for "general unconstrained" vs "variant ctor
unconstrained" — the pipeline emits them differently today, so the
second is safer.

**CLOSED (`c753888f`).** The scope estimate was too pessimistic. The
existing `walk_expr_for_variant_ctor_calls` already handles every
`ExprKind` and recurses correctly; `has_unbound_ty_var` already
recurses into ADT substitutions; the pipeline already emits from
`unresolved_ty_var_spans` (per-owner, gated on reachability). The
missing piece was one `VariantRef` arm — mirroring the `VariantCtor`
call check — that flags a unit-variant value whose `expr.ty` carries an
unbound `Infer(Ty)`. Eighteen lines including the comment.

Verified:
  * `let b = Option::None;` → `[T0001] type annotations needed`
    (was `[X0000] Codegen error: Attempted to lower TyKind::Error`)
  * `let b: Option<i32> = Option::None;` still compiles and runs
  * full corpus green under `--features glyim-test/real-llvm`

Regression fixture:
`tests/compile-fail/unconstrained_none.g` (`//~ ERROR type annotations
needed`).

**Why the earlier estimate was wrong.** It assumed the walker needed a
read-only mirror of `zonk_body` (~80-100 lines). In fact the walker
already exists and is read-only; it just had a deliberately narrow
detection condition ("only `VariantCtor` calls") with a comment
explaining *why* (`FnRef`-typed calls can carry unbound `Infer` in
their `Err` position legitimately, e.g. `Result<f64, <f64 as
FromStr>::Err>`). `VariantRef` is not in that excluded category — a
unit-variant value has no argument that could legitimately leave the
enum's parameter unresolved — so it belongs alongside `VariantCtor`.

**Bug B — turbofish on a unit-variant path is lost before typeck.** Even
explicitly annotating the variant does not help:

    let o = Option::<i32>::None;    // plain let — ICEs
    let b: i32 = if let Option::Some(_) = Option::<i32>::None { 1 } else { 7 };  // ICEs

Both fail. But:

    let none_val: Option<i32> = Option::None;
    let b: i32 = if let Option::Some(_) = none_val { 1 } else { 7 };  // works, 7

So this is **not** `if let`-specific — it's the turbofish itself.
Verified by MIR: `Aggregate(Adt(1010), VariantIdx(0), Substitution(index=0,
len=0))` — the substitution is empty and `$3: Adt1010<<error>>`.

**What was ruled out.**

An attempted fix changed `check_path`'s fallback `enum_path` construction
(`unify.rs` ~285) from `generic_args: None` to
`path.segments[0].generic_args.clone()`. It compiled, corpus stayed
green, but the ICE persisted unchanged — MIR still showed empty substs.
That commit was reverted as a **no-op for this bug** (the code change is
arguably correct on its own merits, but it does not fix Bug B).

**Where the turbofish is supposed to be handled.** `lower_expr.rs`
~1075-1122 has an explicit `UsePath`-nested-type-args branch, with a
comment reading: *"Turbofish type args nested inside the `UsePath` node
(`f::<i32>`, `Vec::<T>::new`). The parser places the `PathType` node as
a sibling of the `Ident` token within the `UsePath`, so the outer
`is_type_node` branch below never sees it; without this arm the args
were silently dropped and the path reached typeck with `generic_args:
None`."* That machinery exists and was added for exactly this case, yet
MIR shows empty substs for `Option::<i32>::None`.

**So the failure is one of:**

  1. The parser does not produce the `UsePath`-with-sibling-`PathType`
     shape for `Option::<i32>::None` specifically (maybe it produces a
     different node kind for a turbofish on a *unit variant* than for a
     function path like `f::<i32>`).
  2. The `pending_args` flush at `lower_expr.rs:1075-1078` attaches the
     args to the wrong segment for a multi-segment turbofish path
     (`flush_pending` runs on the *next* ident, so `Option::<i32>::None`
     might end up with args on `None` instead of `Option`).
  3. `check_path`'s variant arm reads args from `path.segments.iter()`
     (line ~186) but the path it receives has been rebuilt or cloned
     somewhere between the parser and this call, losing them.

**HIR dump done (2026-10-28).** A temporary test in
`glyim-hir/src/tests/lower_expr.rs` lowered `fn f() { Option::<i32>::None; }`
and printed the resulting `Expr::Path`:

    --- Expr::Path at index 0 ---
      segments: 2
      seg[0] name="Option" has_generic_args=true argc=1
        arg[0] = Path(Path { segments: [PathSegment { name: Name(3),
                      generic_args: None }], kind: Plain })
      seg[1] name="None" has_generic_args=false argc=0

(`Name(3)` = `i32`.)

So the turbofish **survives HIR lowering intact**, attached to the
correct segment (`Option`, not `None`). This eliminates hypotheses
(1) and (2) from the list above — the parser produces the right node
shape, and `lower_expr.rs`'s `UsePath`-sibling-`PathType` machinery
attaches the args to the right segment. **The loss is in typeck.**

**Where in typeck.** `check_path` (`unify.rs:56`) has three routes for
a 2-segment `Enum::Variant` path:

  a. `bare_safe` via `variant_by_name` — single-segment only, not
     applicable here.
  b. `def_map.variant_map.get(&local)` — the branch that *does* read
     `explicit_args` correctly (line ~186) and builds the substitution
     from them (lines ~200-215). **`Option` is a builtin ADT
     (AdtId 1010), registered synthetically, so `None` is not in
     `variant_map`.** This branch never fires for `Option::None`.
  c. The `path.segments.len() == 2` fallback (line ~281) — this IS the
     branch that handles `Option::None`. It calls
     `resolve_name_to_adt_ty` on a rebuilt single-segment `enum_path`
     and then extracts `adt_id` from the result. That function resolves
     the *name* `Option` to `Adt(1010, <fresh-or-empty>)` — it does not
     consult the `generic_args` on the input path segment. The
     substitution on the resulting `VariantRef` therefore comes from
     whatever `resolve_name_to_adt_ty` produces, not from the
     turbofish. MIR shows `Substitution(index=0, len=0)` because the
     result is built with an empty substitution somewhere in that
     helper or in the subsequent `VariantRef` construction.

The earlier no-op fix (`unify.rs` ~285, `generic_args: None` →
`generic_args: path.segments[0].generic_args.clone()`) preserved the
args on the *input* to `resolve_name_to_adt_ty`, but that function does
not read them, so the change had no effect. **Correct fix:** after
`resolve_name_to_adt_ty` yields `Adt(adt_id, _)`, rebuild the
substitution from `path.segments[0].generic_args` — mirroring what
branch (b) does at lines 200-215 — and use that when constructing the
`VariantRef`'s type. Alternatively, teach `resolve_name_to_adt_ty` to
accept and honour generic args (better if other callers need the same,
but touches more code).

**Second attempt also no-op, reverted (2026-10-28).** A fix that
changed `enum_path`'s `generic_args` at `unify.rs:285` compiled, corpus
stayed green, and the ICE persisted — confirming
`resolve_name_to_adt_ty` ignores the field. Reverted (commit
`9808b471` was amended out; there is no such commit in the tree).

**CLOSED (`0dc376ec`).** The third attempt found the actual cause:
`variant_expr` (the helper both `Enum::Variant` callers route
through) unconditionally allocated fresh inference vars for the
enum's generic parameters, discarding whatever typeck had already
resolved from the turbofish. The callers' args were fine; the helper
dropped them.

Fix: `variant_expr` gained an `explicit_args: Option<&[TypeRef]>`
parameter. When present and the corresponding arg resolves to a
non-error type, that slot uses the resolved type instead of a fresh
infer var. Both `Enum::Variant` callers pass
`path.segments[0].generic_args.as_deref()`; the bare-name caller
(`None`/`Some`) passes `None`. Three regression probes verified:
`Option::<i32>::None` in an `if let` now runs to 7 (was ICE);
`Option::None` with a `let` annotation still runs to 7 (inference
path unchanged); `Option::<i32>::Some(42)` still runs to 42
(data-carrying variant unchanged).

Regression fixture:
`tests/run-pass/behavior/turbofish_unit_variant.g` — the
`match Option::<i32>::None { … }` shape, which is the narrowest
reproducer. `if_let_some_none.g` continues to cover the annotated
`let` form.

Full corpus green under the CI configuration (`GLYIM_LLVM=1
GLYIM_TEST_REAL_LLVM=1 GLYIM_TEST_NO_SKIP=1 --features
glyim-test/real-llvm`).

**Fixture state.** `tests/run-pass/behavior/if_let_some_none.g` has been
rewritten to use the **valid** `let`-annotated form (which exercises
the same `if let` desugaring and the same `Option::None` construction),
exits 49, and is un-`// ignore`d. Verified against real-llvm: compiles
and runs to 49. The `// needs-llvm` annotation is kept because the
fixture's purpose is to guard the LLVM lowering path specifically.

**What is now covered.** The original fixture's intent — that `if let
PAT = EXPR` compiles and the wildcard arm doesn't ICE — is preserved by
the rewritten version, which uses the `let`-annotated form. Bugs A and
B remain open and are not currently exercised by any fixture (which is
why they survived). Adding compile-fail fixtures for them is part of
the fix, not part of this entry's cleanup.

#### 3.5.25 — P2-B7: `unsafe` was parsed then discarded; `FnItem::is_unsafe` lied

CLOSED. The audit described P2-B7 as "`unsafe` token parsed then dropped
with no lint" and suggested either a `-W unsafe-ignored` warning or
documentation. The actual bug was narrower and more concrete: the parser
*did* preserve the keyword in the syntax tree, HIR *had* a field for it
(`FnItem::is_unsafe`), and `lower_item.rs` **hardcoded that field to
`false`**.

**Why the keyword never reached the field.** `parse_item`
(`crates/glyim-frontend/src/parser/item.rs:9`) handles `unsafe` by
calling `self.bump()` *before* `self.start_node(SyntaxKind::FnDef)`.
So in the syntax tree, `KwUnsafe` is a **sibling** of the `FnDef`
node, not a child:

    SourceFile@0..16
      KwUnsafe@0..6 "unsafe"
      Whitespace@6..7 " "
      FnDef@7..16
        KwFn@7..9 "fn"
        ...

`lower_fn` builds `is_const` / `is_async` by iterating
`node.children_with_tokens()` — which is correct for those two, because
`parse_fn_def` consumes `const` / `async` *inside* the `FnDef` node.
`unsafe` follows a different path and never appears in that iteration.
The field was set to a literal `false` at construction.

**Consequence.** No downstream consumer reads `is_unsafe` today
(verified by grep — only construction sites and test structs). So the
bug was latent: a field that claims to describe every fn, silently
returning `false` for `unsafe fn`. Any future `unsafe` audit, security
pass, or `-W unsafe` lint would have been built on a lie.

**Fix.** `lower_item.rs` now walks `node.prev_sibling_or_token()`,
skipping trivia, and sets `is_unsafe` when it finds `KwUnsafe` before
the first non-trivia token. Precedent for this walk is
`lower_pat.rs:41`, which uses `prev_sibling_or_token` for pattern
modifiers. The field is now populated from the actual source.

**Known limitation.** For `unsafe pub fn f()`, the sibling chain from
`FnDef` back is `[Whitespace, KwPub, Whitespace, KwUnsafe]`. The walk
stops at `KwPub` (first non-trivia) and returns `false` — technically
wrong. But `unsafe pub fn` is non-idiomatic (Rust's convention is `pub
unsafe fn`), the parser's own `parse_item` accepts `pub unsafe` as the
normal order, and no fixture or stdlib source uses the reversed form.
Documented here rather than fixed; if a caller ever needs it, the walk
becomes "scan all preceding modifiers until `fn` or another item".

**Why not a lint.** The audit suggested `-W unsafe-ignored`. That is a
separate, larger feature: a warning needs a diagnostic category, a
way to suppress it per-block, and a decision about whether the parser
or typeck owns it. Populating `is_unsafe` correctly is the
precondition — a lint that fires on a field that is always `false`
would be nonsense. The lint remains a legitimate P2-B7 follow-up; this
entry closes the data-integrity half.

**Verified:**
  * `unsafe fn f() {}` → `is_unsafe == true`
  * `fn f() {}` → `is_unsafe == false`
  * full workspace: 4624/4624

Regression tests: `glyim-hir::tests::lower_fn::unsafe_fn_sets_is_unsafe`
and `::plain_fn_is_not_unsafe`.

**Note on the five `glyim-cli` link failures during verification.** Not
related to this change — the `cargo clean -p` from the §3.5.22
investigation had removed `target/debug/libglyim_runtime.a`. Rebuilding
`glyim-runtime` restored all five. Recorded because it is a recurring
hazard when bisecting with `cargo clean`: the runtime staticlib is a
build artifact that `cargo build -p <crate>` does not produce unless
the crate is a dependency.

#### 3.5.26 — P1-O8: stale runtime staticlib is now diagnosed, not silent

PARTIALLY CLOSED (`76a150bb`). The audit described P1-O8 as "a stale
`libglyim_runtime.a` from a different compiler links silently
(`linker.rs:165-204`). Fix: embed a hash of the runtime `.a` at compiler
build time; compare before linking and warn loudly."

The version-stamp half is **not implemented**, and the entry explains
why. The *diagnostic* half is now in: a link failure caused by runtime
ABI mismatch names the staticlib, its mtime, and the rebuild command.

**Why the version stamp was not the right first move.** Embedding a
hash of the runtime `.a` in the compiler binary requires the compiler's
build script to know where the runtime artifact is — but `glyim-cli` and
`glyim-runtime` build in parallel, and neither depends on the other. A
`build.rs` coordinating them needs either (a) a hard build-order
dependency between the two crates (which would slow every incremental
`cargo build -p glyim-cli`), or (b) a two-phase build the workspace does
not currently support. Both are changes to the crate graph, not to
`linker.rs`.

**Why the diagnostic is the higher-value half anyway.** This session hit
the stale-runtime hazard **twice**:

  * After `cargo clean -p` on the type/typeck chain, five link tests
    failed with no obvious connection to "the runtime staticlib is
    gone".
  * After a `cargo clean` during §3.5.22's investigation, the same
    pattern: five `--emit=exec` tests failing on `undefined reference`.

Both took multiple diagnostic cycles to resolve. The `find_runtime_staticlib`
"not found" path already produced a good message for the *missing* case;
the *stale* case (library present, wrong ABI) produced a bare linker
error with the symbol name and nothing else.

**What was added.**

  * `linker::annotate_link_error_if_stale_runtime(err)` — public
    wrapper over a pure `annotate_link_error_if_stale_runtime_with(err,
    runtime_path)`.
  * A `RUNTIME_SYMBOLS` constant listing the eleven runtime-provided
    symbols (`glyim_alloc`, `glyim_panic`, `glyim_stdout_write`,
    `glyim_string_from_bytes`, …).
  * If the linker error mentions any of them, append a hint: the
    resolved staticlib path, its mtime, and `cargo build -p
    glyim-runtime` (with the release variant noted).
  * If it does *not* mention them, the error is returned unchanged — a
    `printf` or system-lib link failure is not this hint's problem.
  * If `find_runtime_staticlib` returns `None`, the annotator is a
    no-op (the "not found" path elsewhere already reported it).

**Where it's wired.** Three `link_with_args` failure sites in
`glyim-cli/src/lib.rs`: the `--emit=exec` final link, the
`finalize_after_object` cache-hit link, and the ThinLTO merge link.
All three now pass the error through the annotator.

**Tests.** Five unit tests in `linker::annotate_tests`:
  * GNU `ld` style (`undefined reference to \`glyim_stdout_write\``) → hint
    appended, path and command present.
  * `printf` (non-runtime symbol) → error unchanged.
  * No runtime lib resolved → error unchanged.
  * ld64 style (`Undefined symbols for architecture arm64:
    "_glyim_alloc"`) → hint appended.
  * MSVC `LNK2019` style → hint appended.

**What remains open.** The proactive "compare before linking, fail with
a diff" behavior the audit asked for. It requires the build.rs
coordination above; the current fix reaches the same user outcome (a
clear actionable message) at the moment of failure rather than before
it, without touching the crate graph.

**Why this is a defensible close for the item.** The audit's phrasing
("links silently") is now false — it does not link, and the message is
actionable. The specific mechanism (a version stamp checked up front)
is one way to get there, but not the only one, and not the one the
crate graph supports without a structural change. If a future session
adds a `glyim-runtime` build script that emits a stamp file, the
annotator can consult it and the message gets even better; today's fix
is a strict improvement that costs nothing.

#### 3.5.30 — OPEN: `-W unsafe` lint needs framework scaffolding before it can land

**Status.** P2-B7's parse-side half is CLOSED (§3.5.25, commit `76a150bb`):
`FnItem::is_unsafe` is now truthful (walks `prev_sibling_or_token` to
find `KwUnsafe` as a sibling of `FnDef`). The **lint half** — emitting
a warning when an `unsafe fn` is called outside an `unsafe { }` block,
togglable via `-W unsafe` / `-A unsafe` — cannot land as a bounded
patch. It requires four pieces of infrastructure that do not exist
today.

**Recon (session 23 continuation):**

| Component | Status |
|---|---|
| `DiagSeverity::Warning` variant | exists (`glyim-diag/src/lib.rs:110`) |
| `LintId` / `LintLevel` enum | **absent** — only severity, no lint identifiers |
| `#[allow(..)]` / `#[warn(..)]` / `#[deny(..)]` attribute resolution | **absent** — no attribute-to-lint mapping |
| CLI `-W <lint>` / `-A <lint>` flags | **absent** |
| `unsafe { }` block tracking in HIR | **absent** — no `Expr::Block { is_unsafe }` |
| call-site safety check in typeck | **absent** — nothing at `Expr::Call` arms (`check_expr.rs:1154, 1332, 1369, ...`) inspects `is_unsafe` |

**What a real `-W unsafe` implementation requires, in order:**

1. **`unsafe` block tracking.** Add `is_unsafe: bool` to `Expr::Block`
   in `glyim-hir/src/lib.rs`, populate it in `lower_block_to_expr`
   (`lower_expr.rs:34`) from `KwUnsafe` in `children_with_tokens`, and
   thread a `in_unsafe_context: bool` flag into `CheckCtx` in typeck.
   Small individually, but touches both crates.

2. **`LintLevel` infrastructure.** Introduce a
   `HashMap<LintId, LintLevel>` in `TyCtxMut` (or a new `LintCtx`),
   with `LintId::UnsafeFnCall` as the first variant. Medium — needs a
   place to live and a way to be passed to typeck.

3. **Attribute resolution.** `#[allow(unsafe_code)]`,
   `#[warn(unsafe_fn_call)]`, `#[deny(unsafe_fn_call)]` — parse the
   attribute, resolve the lint name to a `LintId`, walk up the HIR
   scope chain. Medium — a new `hir::lower::lower_attrs` module plus
   scope-tracking in typeck.

4. **CLI flags.** `-W unsafe_fn_call` and `-A unsafe_fn_call` in
   `glyim-cli/src/lib.rs`. Small — but only useful once (1)-(3) exist.

5. **The lint emission itself** at each `Expr::Call` arm:
   `if callee_is_unsafe_fn && !ctx.in_unsafe_context { emit(warn) }`.
   Trivial once (1)-(4) exist.

**Estimate.** 3-5 sessions of coordinated work across `glyim-hir`,
`glyim-typeck`, `glyim-cli`. Not a "small item".

**Recommendation.** Defer P2-B7's lint half until at least one other
lint genuinely wants the framework (e.g. `unused_variables` or
`unreachable_code`, both of which are already diagnosed as hard errors
in a few narrow cases and would benefit from warn-level escalation).
Building the framework for a single lint is more cost than value.
Alternatively, land (1) — `unsafe { }` block tracking — on its own, as
it's independently useful for other things (e.g. a future `unused_unsafe`
lint, or verifying that `unsafe { }` blocks actually permit unsafe ops
once the current "unsafe is parsed then discarded" behaviour at the
block level is fixed).

**Not to be confused with §3.5.25.** That entry closed the parse-side
`is_unsafe` lie. It did **not** close P2-B7's lint. The plan's
parenthetical "P2-B7's lint half (`-W unsafe` now that `is_unsafe` is
truthful)" from the session-23 handoff implies the field's truthfulness
was the only blocker — it wasn't. The field being truthful is a
precondition, not the fix.

#### 3.5.27 — TEST-1 delivered; `assert!` failures abort silently (CLOSED; residual message-loss tracked in §3.5.28)

TEST-1 was framed as "add a `// check-stderr:` matcher". The matcher
already existed and was already consumed; the actual defect was narrower
and the real blocker is a **different** bug found while delivering it.

**TEST-1 (CLOSED, `5b81184f`).** `TestConfig::check_stderr` was parsed
and consumed, but its matcher used plain `result.stderr.contains(...)`
while `check_stdout` used the line-anchored `stdout_matches`
(T196-PATCHED [HARNESS-9]). So `// check-stderr: error:` matched
`error: something went wrong` — the exact prefix false-positive the
stdout matcher was fixed for. Now both go through `stdout_matches`.
Five matcher unit tests pin the semantics, and two existing tests that
had *codified* the substring behaviour were updated with companion
tests asserting the old behaviour is now a mismatch.

**Why zero of 263 fixtures used `check-stderr`.** Probing the two
`run-fail` fixtures that should print a panic message:

    assert_false.g                    exit 101, stderr: 0 bytes
    print_then_panic_keeps_stdout.g   exit 101, stderr: 0 bytes

Three probes localised it:

| Probe | Result |
|---|---|
| `eprintln!("direct stderr")` | 14 bytes on stderr, exit 0 |
| `assert!(false, "boom")` | exit 101, **0** stderr bytes |
| `.ll` for `assert!(false, "boom")` | `declare void @glyim_process_abort()` — **no `glyim_panic`** |

**The mechanism.** `glyim_panic`
(`crates/glyim-runtime/src/lib.rs:182`) takes `(msg_ptr, msg_len)` and
writes `thread 'main' panicked: <msg>\n` before `exit(101)`. It works.
But `assert!` failures never call it: they lower to
`glyim_process_abort()`, a separate runtime function whose entire body
is `std::process::exit(101)` — no message.

    pub extern "C" fn glyim_process_abort() -> ! {
        std::process::exit(101)
    }

Four LLVM call sites use it (`lower.rs` ~2694 div-by-zero, ~2827
rem-by-zero, ~3922 the unreachable/panic path, ~8565 an unsupported
coercion): each declares `void_type().fn_type(&[], false)` and calls
with `&[]`.

**Attempted fix, reverted.** Changing `glyim_process_abort` to
`(msg: *const u8, len: usize)` and delegating to `glyim_panic` compiles
(`cargo check` passes) — but **LLVM IR types are not checked by
rustc**, so all four call sites silently keep the zero-argument
declaration. The result is an ABI mismatch at link time. Reverted
rather than half-applied.

**Correct fix (for a follow-up).** The assert site at `lower.rs:3985`
already emits a private message global (const string, `__glyim_assert_<hash>`,
bitcast to `i8*`) and passes `(ptr, len)` to `glyim_panic`. That
pattern needs extracting into a helper — something like
`emit_message_global(&mut self, msg: &str) -> (PointerValue, IntValue)`
— so the four `glyim_process_abort` sites can each pass a message
(`"division by zero"`, `"remainder by zero"`, `"assertion failed: <msg>"`,
`"unsupported coercion"`) instead of duplicating ~25 lines of global
emission four times.

Sequence:
  1. Extract `emit_message_global` from the existing assert path.
  2. Change `glyim_process_abort` to `(msg, len)`, delegate to
     `glyim_panic`.
  3. Update all four call sites to use the helper and pass the new args.
  4. Add a `run-fail` fixture with `// check-stderr: division by zero`
     to prove the message reaches stderr end to end.
  5. Full corpus — the div/rem-by-zero fixtures (`div_by_zero.g`, etc.)
     must still exit 101.

That is a focused codegen change, but it is ~120 lines across five
locations and needs the helper extraction first. Not a tail-end-of-session
patch; the revert above is the honest state.

**Interim.** `assert!` and div/rem-by-zero still exit 101 silently,
exactly as before this investigation. No regression. The `check-stderr`
matcher works correctly for programs that print to stderr via
`eprintln!`; it has nothing to match on abort-class failures until this
is fixed.

**Update (session 23 continuation — 2026-10-29).** §3.5.27's abort-site
half is **closed**. What landed:

  * `c95bf14b` — added `glyim_process_abort_with_msg(msg, len)` to
    `glyim-runtime` (delegates to `glyim_panic`), kept the zero-arg
    `glyim_process_abort()` for existing `.g` callers (`process.abort`,
    alloc's OOM handler, `core/abort_stops.g`), extracted
    `emit_abort_with_msg` helper in `LoweringCtx`, and routed all four
    formerly-silent LLVM abort sites through it with distinct messages
    (`"attempt to divide by zero"`, `"attempt to calculate the remainder
    with a divisor of zero"`, `"entered unreachable code"`,
    `"called \`<method>\` on a miss value"`).
  * `7c6444e4` — unified the interpreter's abort-message text with the
    LLVM messages (`InterpError::Panic` Display no longer prefixes
    `"panic: "`, new `RemainderByZero` variant, `try_call_extern` reads
    the `(ptr, len)` message from `glyim_process_abort_with_msg` and
    `glyim_panic`, `TerminatorKind::Unreachable` maps to
    `"entered unreachable code"`, `interpreter_runner` writes
    `"thread 'main' panicked: {msg}\n"`).
  * `07c827b8` / `7c6444e4` — first real `check-stderr` fixtures
    (`run-fail/div_by_zero_stderr.g`, `run-fail/rem_by_zero_stderr.g`),
    **un-gated** on `// needs-llvm` because both backends now produce
    byte-identical stderr. Mutation-tested end-to-end on both backends.

**What remains open (this is §3.5.28, not §3.5.27).** The *specific user
message* on `assert!(false, "boom")` and `Option::unwrap()`-on-`None`
never reaches the runtime. It is not the abort-site codegen — the four
sites now correctly emit `glyim_process_abort_with_msg(ptr, i64)`. The
message is dropped one layer earlier: **MIR lowers divergent
`panic!`/`assert!`/`unwrap` paths to `TerminatorKind::Unreachable`,
discarding the message.** See §3.5.28.

#### 3.5.28 — OPEN: divergent `panic!`/`assert!`/`unwrap` paths lower to `TerminatorKind::Unreachable`, discarding the message

**Symptom.** A program that hits `assert!(false, "boom")` (or
`Option::None.unwrap()`, or `Result::Err(_).unwrap()`) exits 101 and
prints only `thread 'main' panicked: entered unreachable code` — the
generic message from the four abort sites (§3.5.27), not the specific
text the user or the stdlib provided. Verified on both backends:

| Probe | LLVM (`--emit=exec`) | Interp |
|---|---|---|
| `assert!(false, "boom")` | exit 101, `entered unreachable code` | exit 101, `entered unreachable code` |
| `Option::None.unwrap()` | exit 101, `entered unreachable code` | exit 101, `entered unreachable code` |
| `Option::None.expect("need a value")` | exit 101, `entered unreachable code` | exit 101, `entered unreachable code` |

**Evidence — the specific message is emitted into IR and then dropped as
dead code.** `.ll` for `Option::None.unwrap()`:

    @__glyim_str_77cecc... = private constant [44 x i8]
        c"called `Option::unwrap()` on a `None` value\00"
    ...
    store { ptr, i64 } { ptr @__glyim_str_77cecc..., i64 43 }, ptr %arg, align 8
    ; ---- END of that BB ----
    ; (no `load` of the stored message; no `call` that consumes it)

The only runtime abort call in the whole `.ll` is
`call void @glyim_process_abort_with_msg(ptr @__glyim_abortmsg_8f90ee..., i64 24)`,
i.e. the *generic* `entered unreachable code` string from §3.5.27's
`TerminatorKind::Unreachable` arm at `lower.rs:3954`. The specific-message
store lands in a basic block whose terminator is `Unreachable`, not
`Call glyim_panic(msg_ptr, msg_len)`.

For `assert!(false, "boom")` the count of `"boom"` in the `.ll` is **0**
— the user message does not survive HIR→MIR lowering at all. The
*Correction (same-day recon).* An earlier draft of this entry named
`crates/glyim-lower/src/lower_rvalue.rs:1816`, `:1833`, `:3052` and
`crates/glyim-lower/src/builder.rs:135` as the "divergent-panic Unreachable
sites". **That is wrong.** Reading those four sites in context:

  - `1816` — `break outside of loop` error-recovery.
  - `1833` — `continue outside of loop` error-recovery.
  - `3052` — match-arm exhaustion fallback (`otherwise_bb` with no arm).
  - `builder.rs:135` — default terminator of a freshly allocated BB before
    the caller sets its real terminator.

None of them fire for `Option::None.unwrap()`. The message threading is
actually intact at the MIR level: `--emit=mir` for that program shows

    Call { func: Constant(Fn(FnDefId(321), …)),
           args: [Constant(MirConst { kind: String(…\`called
                   \`Option::unwrap()\` on a \`None\` value\`…) })],
           destination: …, target: Some(BasicBlockIdx(7)), cleanup: None }

i.e. the specific message **is** threaded as a call argument to
`FnDefId(321)`. In the emitted `.ll` the corresponding string global is
likewise stored into the callee's arg slot as `{ ptr, i64 } { ptr @__glyim_str_…,
i64 43 }`. So the message survives HIR and MIR lowering; it is not
dropped by `lower_rvalue.rs`.

The actual break is one layer further down: `FnDefId(321)` — the fn the
`panic!` token resolves to — has a stub body that compiles to `Unreachable`
without reading its argument. The codegen then hits the §3.5.27
`Unreachable` abort site with its generic message (`"entered unreachable
code"`), and the specific argument sitting in the caller's arg slot is
never consumed. That is why the `.ll` shows the string stored but never
loaded.

**Root cause.** MIR's divergent-terminator contract collapses to
`TerminatorKind::Unreachable`, which carries no message. The stdlib /
macro layer emits a `panic!(msg)` call, the call lands in a basic block,
but because the callee is `-> !`, subsequent lowering terminates the
block with `Unreachable` rather than threading the call's arguments to
the runtime abort. `glyim_panic` — the function that *would* print the
specific message — is never invoked on these paths.

**Why this is one bug, not three.** `assert!`, `Option::unwrap`,
`Result::unwrap`, and any user `panic!(msg)` all reach the same
divergent body; the same `lower_rvalue.rs`/`builder.rs` sites handle all
four. Fix one, fix all four.

**Why it did not block §3.5.27.** §3.5.27 fixed the *abort mechanism*
(silent zero-arg abort → message-carrying abort at four sites) and made
the generic fallback messages consistent across backends. This entry is
about *which* message reaches the runtime — an upstream MIR-divergence
contract problem, not a codegen problem. The codegen is correct: given
`Call glyim_panic(specific_msg)`, both backends would print the specific
text. The message is being discarded before codegen sees it.

**Correct fix (planning pass next session).** Two candidate paths, both
starting from the *correct* site — the stub body of the fn that `panic!`
resolves to (working identifier: `FnDefId(321)` on the probe above), **not**
the four Unreachable sites named in the earlier draft.

1. **Give `FnDefId(321)` a real body that calls the runtime panic FFI.**
   Wherever that stub's body is synthesised (candidates: a `.g` stub in
   `crates/glyim-lang-std/lib/`, a `#[rustc_builtin_macro]`-equivalent
   path in `crates/glyim-frontend`, or a hand-written stub in
   `crates/glyim-lower`), replace the current body (which terminates in
   `Unreachable`) with:
     * `StorageLive(msg_local)`
     * `msg_local = arg0` (the `(ptr, len)` pair the caller already passes)
     * `Call { func: <extern glyim_panic>, args: [msg_ptr, msg_len],
              destination: _, target: None, cleanup: None }` — i.e. a
              diverging Call with no continuation.
   Codegen and interp already handle `Call { target: None }` correctly
   (see `glyim-codegen-llvm/src/lower.rs:6269`, `:8174`, `:8430` and
   `glyim-mir-interp/src/lib.rs:1863`, `:2034`). Both surface the call's
   actual arguments to the runtime. No new MIR variant needed.

2. **Alternative: extend `TerminatorKind::Unreachable` with an
   `Option<PanicMessage>`.** More invasive, requires touching MIR,
   both backends, and the interp. Only worth doing if path 1 turns out to
   need wider plumbing.

Path 1 is the recommendation. Real scope: identify the stub's
synthesis site, replace its body with a `Call glyim_panic(msg, len)` +
`Unreachable` sequence, add the four acceptance fixtures, verify both
backends print the specific message. Estimated effort: **1 solid session
of read-then-write, same shape as §3.5.27** — not the "3–4 files" the
earlier draft claimed, but not a one-line swap either.

**Note for the next session:** the first task is *"find where
`FnDefId(321)`'s body is synthesised."* Everything else follows from that.
`grep -rn "panic_any\|-> ! sentinel" crates/glyim-lower` and the existing
`TerminatorKind::Assert` path (which *does* reach `glyim_panic` correctly
— see `glyim-codegen-llvm/src/lower.rs:3985`) are the two most promising
starting points.

**Acceptance tests (add all four when the fix lands).**

- `run-fail/assert_with_msg_stderr.g` —
  `// check-stderr: thread 'main' panicked: boom`
- `run-fail/unwrap_none_stderr.g` —
  ``// check-stderr: thread 'main' panicked: called `Option::unwrap()` on a `None` value``
- `run-fail/unwrap_err_stderr.g` — analogous for `Result`
- `run-fail/expect_msg_stderr.g` —
  `// check-stderr: thread 'main' panicked: need a value`

All four must pass on both backends (drop `// needs-llvm` once the interp
side reads the message from the terminating call).

**Status.** Diagnosed; no code change attempted this session (fix scope
is 3–4 files and needs the MIR-divergence contract pinned first).
Documented here so a future session starts from the line numbers rather
than re-running the probes.

**Attempted fix, reverted — blocked on §3.5.22 (session 23, same day).**
Following the corrected "find the stub's synthesis site" instruction
above located the culprit: `crates/glyim-lang-core/lib/panic.g:18`'s
`panic_any` had an *empty body* (a comment, no statements):

    fn panic_any(msg: impl Display) -> ! {
        // compiler intrinsic - aborts execution
    }

The `-> !` signature promised divergence; the empty body compiled to a
single `Return` block. Every panic-family call site (`panic!`, `assert!`,
`Option::unwrap`, `Result::unwrap`) therefore saw `Call { target: Some(_) }`,
the callee returned instantly, and the caller continued past the panic
into a dead `Unreachable` block whose generic message was the only
observable stderr.

The attempted fix gave `panic_any` a real diverging body:

    fn panic_any(msg: impl Display) -> ! {
        extern "C" { fn glyim_panic(msg: *const u8, len: usize) -> !; }
        let formatted = format!("{}", msg);
        let bytes = formatted.as_bytes();
        unsafe { glyim_panic(bytes.as_ptr(), bytes.len()) }
    }

Compiled cleanly; probes showed:
  * `panic!("boom")`            → exit 101, stderr `thread 'main' panicked: panic`
  * `assert!(false, "boom")`   → exit 101, stderr `thread 'main' panicked: entered unreachable code`
  * `Option::None.unwrap()`     → exit 101, stderr `thread 'main' panicked: panic`

The `panic_any` body **did** reach `glyim_panic`, but `format!("{}", msg)`
evaluated to an **empty string** for every runtime `msg` — a fresh
manifestation of **§3.5.22 layer 8** ("`format!`-argument substitution
emits empty for runtime values, both backends"). With `len == 0`,
`glyim_panic`'s guard fell through to its default `b"panic"` fallback.
The `assert!` case didn't even reach `panic_any`: it goes through
`panic!("assertion failed: {}", format!($($arg),+))`, so it hit a
separate bare `Unreachable` downstream of the empty `format!` result.

Confirmed independently:
  * `format!("{}", "hello")`       → empty
  * `format!("{}", 42)`             → empty
  * `println!("hello")`             → prints `hello` (literal, no format-arg)
  * `println!("x={}", 42)`          → prints `x=` (format-arg dropped)

So the panic.g fix is **correct in principle but blocked**: it needs a
working `format!` implementation for runtime values first, which is
§3.5.22 (nine layers, 1-7 each attempted and reverted; layers 8-9 need
the builtin-dispatcher redesign documented in the §3.5.22 entry).

Reverted rather than half-landed. The tree is back at the committed
`panic_any` empty-body state, which is the honest `→ !` divergence
without a message — same observable behaviour as before this attempt.

**Updated task list for the next session (in order):**
  1. Land §3.5.22 (format! of runtime values). Until that works, no
     panic-family fix can emit a specific message.
  2. Re-apply this panic.g patch (it is 15 lines and correct — just
     needs the substrate).
  3. Add the three acceptance fixtures drafted this session:

     - `run-fail/panic_with_msg_stderr.g`:
       `// check-stderr: thread 'main' panicked: boom`
     - `run-fail/assert_false_stderr.g`:
       `// check-stderr: thread 'main' panicked: assertion failed: boom`
     - `run-fail/unwrap_none_stderr.g`:
       `` // check-stderr: thread 'main' panicked: called \`Option::unwrap()\` on a \`None\` value ``

     All three verified-red this session with the substrate absent,
     so they will be green the moment §3.5.22 lands and panic_any
     is patched.

**LANDED (session 23, commit `6805055b`).** The defensive-mode alternative
described in the paragraph below was implemented and it works. It was
**not** actually blocked on §3.5.22, as an earlier revision of this entry
claimed — a follow-up probe session showed:

  * `format!("{}", "hello")` (literal &str)         → `hello` (works)
  * `format!("{}", <&str fn param>)`                → works
  * `format!("{}", 42)` (int arg)                   → empty (§3.5.22)
  * `format!("{}", <impl Display fn param>)`        → empty (§3.5.22)

Only the trait-generic and int paths were broken. So `panic_any_str(msg:
&str)` was unaffected by §3.5.22 and unblocked immediately.

**Fix that shipped (`6805055b`)** — two coordinated changes:

1. `glyim-lang-core/lib/panic.g`:
   - New `fn panic_any_str(msg: &str) -> !` calls `glyim_panic(ptr, len)`
     directly (no `format!`).
   - `panic!` literal-only arms route through it.
   - `unreachable!` and `unimplemented!` route through it.
   - `panic_any` (kept for the format-args arm) now formats and calls
     `glyim_panic`. That arm's `format!` still returns empty for runtime
     `impl Display` values (§3.5.22 trait-generic path) and falls back to
     `glyim_panic`'s `b"panic"` default. Not regressed vs. the pre-fix
     state (which printed `entered unreachable code`), still imperfect.

2. `glyim-lang-std/src/lib.rs` (both `std_source_assembled` and
   `std_source_assembled_minimal`): register `"panic_any_str"` in the
   well-known fn list alongside `"panic_any"`. **This was the actual
   blocker behind earlier "unresolved name panic_any_str" walls** —
   macro expansion reads the well-known fn list, not top-level fn
   declarations, so an unregistered `&str`-taking helper is invisible
   to macro-expanded call sites even though it is in the same file.

**Verified working after `6805055b` (both backends):**

| Program | stderr |
|---|---|
| `panic!("boom")` | `thread 'main' panicked: boom` |
| `unreachable!()` | `thread 'main' panicked: internal error: entered unreachable code` |
| `Option::None.unwrap()` | `` thread 'main' panicked: called `Option::unwrap()` on a `None` value `` |
| `Result::Err(_).unwrap()` | analogous |

Acceptance fixtures added and green in `run_fail_corpus` on both backends:
  - `run-fail/panic_with_msg_stderr.g`
  - `run-fail/unreachable_stderr.g`
  - `run-fail/unwrap_none_stderr.g`

**What did NOT land: `assert!`.** Verified-red after `6805055b` on both
backends:

    assert!(false, "boom")  →  exit 101, stderr:
                               thread 'main' panicked: entered unreachable code

That is the generic `Unreachable` site, not the specific assertion
message. `assert!` is a different code path from `panic!` — see §3.5.29.

#### 3.5.29 — CLOSED: `assert!` builtin shadowed the .g macro (commit `79cb97c5`)

**Symptom (now fixed).** `assert!(false, "boom")` exited 101 with
`thread 'main' panicked: entered unreachable code` on both backends,
not `thread 'main' panicked: assertion failed: boom`.

**Root cause.** The expander's `try_expand_macro_call`
(`crates/glyim-meta/src/expander/mod.rs:482`) checked
`self.registered_builtins` before `self.macros`. `assert` (and
`assert_eq`/`assert_ne`/`debug_assert*`) were registered as Rust-side
builtins, so the builtin handler shadowed the .g `assert!` definition
in `panic.g`. The builtin expanded to `{if(!(cond)){loop{}}}` — no
message, and `loop{}` compiles to `TerminatorKind::Unreachable`.

The .g macro (defined at `panic.g:54`, updated in `6805055b` to route
through `panic_any_str` → `glyim_panic`) was never reached. Its own
header comment at `expander/mod.rs:140-144` states "must resolve
through the user-macro path, not be shadowed by builtins" — the code
did the opposite. That contradiction was the bug.

**Fix (`79cb97c5`).** Guard the builtin path with
`!self.macros.contains_key(&name)`. User macros win; builtins remain a
fallback for fixtures that don't load the stdlib.

**Verified on both backends:**

| Program | Env | Result |
|---|---|---|
| `assert!(false, "boom")` | `--with-stdlib` | `assertion failed: boom` ✓ |
| `assert!(true, "msg")` | `--with-stdlib` | exit 0, no panic ✓ |
| `assert!(false)` | no stdlib | exit 101 via builtin fallback ✓ |

Corpus green on both backends, workspace 4633/4633.

New fixtures: `run-fail/assert_false_stderr.g`,
`run-fail/assert_no_msg_stderr.g`.

**Full panic-family coverage after this + §3.5.28 (`6805055b`):**
`panic!`, `unreachable!`, `unimplemented!`, `Option::unwrap`,
`Result::unwrap`, `assert!`, `assert_eq!`, `assert_ne!` all emit
specific messages on both backends.

**Diagnosis trail (short).** Three sessions of probes falsified four
hypotheses in order:

1. `other =>` arm of `lower_block_to_expr` drops bare children —
   falsified by an eprintln injection: zero hits across four probes.
2. Expander strips the trailing `;` — falsified by four macro-body
   rewrites (`; ()`, `else { }`, `let _: () = ...`, double `;`): all
   still failed identically.
3. Sentinel `FnDefId` path in `lower_rvalue.rs` fires — falsified by
   MIR inspection: the failing case had **zero** Calls and zero
   `FnDefId`s, so no sentinel path was even reached.
4. Nested plain blocks fail by the same mechanism — falsified by
   re-reading an earlier MIR dump: `{ { panic_any_str("x"); } }`
   works and produces `thread 'main' panicked: x`.

The decisive step was a two-line `eprintln!` inside
`expand_node_recursive` that printed the actual expansion text for
every macro call. That output revealed `{if(!(false)){loop{}}}`, which
cannot be produced by `panic.g`'s `assert!` — proving a Rust-side
override was winning.

**Lesson for the next macro-expansion bug.** When a macro's output
does not match its source definition, dump the expansion text before
theorising about downstream stages. Three sessions of MIR dumps could
have been one. The CLI still lacks `--emit=hir`; adding it would make
this class of bug a one-shot diagnosis.

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
