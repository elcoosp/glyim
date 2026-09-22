# Glyim Compiler De-Stubbing Plan v2
### Execution roadmap for the exhaustive 24-crate audit — precise, mechanical, no hacks left behind

This plan turns the exhaustive inventory into concrete engineering work. Each entry gives: the
defect, the root cause, the exact fix (data structures + algorithm + code), and the test that proves
it's actually fixed. High-severity items get full architectural treatment since they block real
programs from compiling/running correctly; Medium/Low items get precise-but-tighter instructions.

---

## 0. Global Rules (apply throughout)

1. **Kill silent fallbacks.** Any function that "returns `false`/`None`/empty on missing data" where
   the missing data represents a genuine unhandled case must become `Result<T, E>` propagating a
   diagnostic, or the caller must guarantee the missing-data case is impossible (documented + assert).
2. **One canonical implementation per concept.** This report shows the classic failure mode again:
   `needs_drop` exists differently in `glyim-type::ty_ctx.rs` and is *also* implicitly relied on by
   `glyim-opt::validate.rs` (which explicitly says the consistency check is "intentionally NOT
   performed yet") and `glyim-pipeline::mono_cache.rs`'s broken array drop glue. Fix the source
   (§10.1) and every dependent falls into line.
3. **Severity-ordered execution**: fix all 6 **High** items first (they block correctness/usability
   outright), verified by dedicated regression tests, before sweeping the Medium/Low backlog.
4. **Every fix ships with a test** — unit test next to the function, plus an integration `.glyim`
   program run through the full pipeline where the defect is behavior-visible (not just
   AST/MIR-shape visible).

Suggested build order (dependency-driven):

```
glyim-core → glyim-span → glyim-db → glyim-def-map → glyim-frontend → glyim-hir →
glyim-type → glyim-solve → glyim-typeck → glyim-const-eval → glyim-mir → glyim-lower →
glyim-mir-interp → glyim-opt → glyim-layout → glyim-codegen / glyim-codegen-llvm →
glyim-pipeline → glyim-runtime → glyim-cli → glyim-lsp → glyim-meta → glyip → glyim-test
```

---

## 1. `glyim-core`

### 1.1 `Path::from_single` — no generic-argument support (`src/path.rs`)

The core `Path` type can't carry `generic_args`, forcing HIR to keep its own richer path type and
duplicate conversion logic everywhere a core `Path` is needed with generics attached (this is very
likely *why* several downstream "only single-segment" bugs exist elsewhere — the core type couldn't
express the general case, so call sites special-cased around it). Fix:

```rust
pub struct PathSegment {
    pub ident: Ident,
    pub generic_args: Option<GenericArgs>, // None = no `<...>` written
}
pub struct Path {
    pub segments: Vec<PathSegment>,
}
impl Path {
    pub fn from_single(ident: Ident) -> Self {
        Self { segments: vec![PathSegment { ident, generic_args: None }] }
    }
    pub fn from_segments(segments: Vec<PathSegment>) -> Self { Self { segments } }
}
```

Audit every call site constructing a `Path` (frontend parser, HIR lowering, `glyim-meta`
substitution) and migrate to `PathSegment { ident, generic_args }` instead of a bare `Ident` list.
This is small in isolation but unblocks precise multi-segment/generic path handling everywhere else
in the plan (§9's typeck method resolution, §13's const-eval, §21's macro path substitution). Test:
round-trip `foo::Bar<T, U>::baz` through `Path` and confirm each segment's generic args survive.

---

## 2. `glyim-span`

### 2.1 `HygieneCtx::adjust` — marks-only, no resolution integration (`src/hygiene.rs`)

Not itself a bug (hygiene-mark stripping correctly belongs at the span layer; resolution is
correctly a separate concern) — but audit the boundary: `adjust` must expose enough information
(the mark stack / syntax-context id) for the resolver (`glyim-def-map`, §4) to actually *use*
hygiene when resolving identifiers introduced by macro expansion, not just strip marks blindly. Fix:
add `HygieneCtx::syntax_context(span: Span) -> SyntaxContextId` (if not already present) and confirm
`glyim-def-map::process_use_tree` (§4.1) and `glyim-hir`'s `PathResolver`-equivalent both consult it
when two identifiers have identical text but different syntax contexts (hygienic macro-introduced
`let x` must not capture a use-site `x`). Test: a macro expanding to `let tmp = $e; tmp` used at a
call site that itself has a local named `tmp` — expansion's `tmp` and call site's `tmp` must resolve
to different bindings.

---

## 3. `glyim-db`

### 3.1 `Database::set_ty_ctx`/`mono_cache` — no real caching, mono cache is `Vec<String>` (`src/lib.rs`)

This is the incremental-compilation foundation and it currently does nothing. Implement a real
query-based incremental system:

```rust
// New: a minimal Salsa-style query database.
pub trait Query: Clone + Eq + Hash {
    type Value: Clone;
    fn execute(&self, db: &Database) -> Self::Value;
}

pub struct Database {
    revision: Revision, // bumped on any input change
    storage: HashMap<TypeId, Box<dyn Any>>, // per-query-type memo tables
}

struct MemoTable<Q: Query> {
    entries: HashMap<Q, (Revision, Q::Value, Vec<Dependency>)>,
}

impl Database {
    pub fn query<Q: Query>(&mut self, q: Q) -> Q::Value {
        if let Some((rev, value, deps)) = self.lookup::<Q>(&q) {
            if rev == self.revision || deps.iter().all(|d| !self.changed_since(d, rev)) {
                return value;
            }
        }
        let value = q.execute(self);
        self.memoize(q, value.clone());
        value
    }
}
```

- Convert `mono_cache` from `Vec<String>` (which can't even be looked up, only appended to — this is
  a "cache" in name only) into a real `Query` (`MonoItemQuery(InstanceId) -> MonoItem`) backed by the
  above, keyed by the monomorphization instance (`DefId` + substituted generics), invalidated when
  the source `DefId`'s HIR body or any transitively-used type's layout changes.
- Convert `set_ty_ctx` into an *input* (a Salsa-style `#[salsa::input]`), and make every
  type-checking/layout/codegen query above it a derived query keyed off that input plus the item
  being processed, so changing one function's body doesn't invalidate every other function's cached
  type-check result — this is the entire point of an incremental compiler and currently doesn't
  exist at all.
- Wire `glyim-lsp` (§20) to *use* this: incremental recompute on every keystroke is exactly the
  workload this unblocks; today the LSP almost certainly recomputes everything from scratch or
  works around the missing cache in ad hoc ways.

**Test:** touch one function's body in a 100-item test crate, re-run typeck+mono, and assert (via a
query-execution counter) that only the touched function and its transitive dependents were
recomputed, not all 100 items.

---

## 4. `glyim-def-map`

### 4.1 `process_use_tree` — nested/submodule imports, noted "NEW" in comments (`src/lib.rs`)

Not a stub per the report, but the "NEW:" comments signal recently-bolted-on logic without full
verification — treat as needing a hardening pass, since import resolution is a common source of
subtle bugs (glob imports, re-export cycles, shadowing). Concretely:
- Add explicit handling + tests for: `use a::{b, c::{d, e as f}};` (nested groups), `use a::*;` (glob
  imports — must defer resolution until all glob sources are known, since glob imports can introduce
  ambiguous names resolved only if exactly one glob provides it), `pub use a::b;` (re-export,
  changing the item's public path), and import cycles (`mod a { use crate::b::X; } mod b { use
  crate::a::Y; }` — must resolve via fixed-point iteration, not single-pass, since Rust-style import
  resolution requires it).
- Implement resolution as a fixed-point loop: repeatedly attempt to resolve every pending `use`
  until a pass makes no progress; anything still unresolved is a hard error (unresolved import,
  naming the failing path). This is the standard `rustc`/`rust-analyzer` def-map algorithm — don't
  attempt single-pass resolution, it cannot handle cross-module glob/cycle cases correctly.
- Test each pattern above individually, plus a stress test with 3+ modules importing from each other
  in a cycle via explicit (non-glob) re-exports, which must resolve deterministically.

---

## 5. `glyim-frontend`

### 5.1 `try_parse_fragment` — missing `lifetime`, `literal`, `vis`, `tt` fragment kinds (`parser/mod.rs`)

Add the four missing fragment specifiers to the match (each maps to an existing parser production):

```rust
match kind {
    FragmentKind::Lifetime => self.try_parse_lifetime().map(Fragment::Lifetime),
    FragmentKind::Literal  => self.try_parse_literal().map(Fragment::Literal), // must include unary-minus literals: `-1`
    FragmentKind::Vis      => self.try_parse_visibility().map(Fragment::Vis).or(Some(Fragment::Vis(Vis::Inherited))), // `vis` matches empty
    FragmentKind::Tt       => self.try_parse_token_tree().map(Fragment::Tt), // single token or single delimited group
    // ...existing arms unchanged
}
```

Note `vis` fragment semantics: it must successfully match *zero tokens* (private-by-default) as well
as `pub`/`pub(crate)`/`pub(in path)` — don't require a token to be present. `tt` must match exactly
one token tree (a single token, or one fully-delimited `(...)`/`[...]`/`{...}` group) — reuse
whatever raw-token-tree capture the macro tokenizer already uses for macro bodies. Test all four
against a macro_rules-equivalent definition using each fragment kind at least once, including the
`vis`-matches-nothing case and `tt`-matches-one-delimited-group case.

### 5.2 `parse_impl_def` — no `default` impls, no `#[derive(...)]` (`parser/item.rs`)

- **`default` impls**: add `is_default: bool` to the `ImplDef` AST node; consume a leading `default`
  keyword before `impl`. Thread through HIR and into `glyim-typeck::coherence.rs` — `default` impls
  participate in specialization priority (lower priority than non-default overlapping impls) rather
  than being flagged as a coherence violation; if the language doesn't support specialization yet,
  at minimum parse and *reject with a clear "specialization not supported" diagnostic* rather than
  silently dropping the keyword or misparsing.
- **`#[derive(...)]`**: this is parsed generically as any other attribute already (via the
  `MetaItem`/`#[attr(...)]` grammar), so `parse_impl_def` itself needs no special-casing — the gap is
  that *nothing consumes* the `derive` attribute afterward. Implement derive expansion as a
  `glyim-meta` builtin macro family (§21.5-adjacent): `#[derive(Clone, Debug, PartialEq, Eq, Hash,
  Default)]` on a struct/enum synthesizes the corresponding trait impl as HIR items, injected before
  typeck sees the rest of the crate. Each derive is its own small code generator:
  - `Clone`: `fn clone(&self) -> Self { Self { field: self.field.clone(), ... } }` (or, for enums,
    a match over variants).
  - `Debug`: emit a `fmt` calling `f.debug_struct("Name").field("field", &self.field)...finish()`
    (or `debug_tuple`/`debug_enum` shape as appropriate).
  - `PartialEq`/`Eq`: field-wise comparison; `Eq` derive requires every field's type to itself be
    `Eq` — check this and emit a clear error if not (mirrors Rust's derive bound requirements,
    auto-adding `where Field: Eq` bounds to the generated impl for generic structs).
  - `Hash`: field-wise `state.write(...)`/recursive `Hash::hash` calls in declaration order.
  - `Default`: requires every field to implement `Default`; for enums, requires exactly one variant
    marked `#[default]`, erroring clearly if zero or multiple are marked.
  Test each derive on a generic struct (bound-propagation correctness), an enum (variant-shape
  correctness for Debug/Hash), and a derive-bound-violation negative test (`#[derive(Eq)]` on a
  struct containing an `f64` field, which isn't `Eq` — must be a compile error, not a runtime issue).

---

## 6. `glyim-hir`

### 6.1 `lower_fn_def` — `async`/`const` fn not set despite fields existing (`lower/lower_item.rs`)

The struct fields (`is_async`, `is_unsafe`) exist but aren't populated for `async`/`const` — this is
a one-line-looking bug with real semantic depth once actually wired up. Fix in two parts:

- **Trivial part**: set `is_async`/`is_const`(add this field if missing) directly from the parsed
  keyword — the parser already recognizes `async fn`/`const fn` per the frontend grammar; if it
  doesn't yet, add it there first (`FnDef.is_async`/`is_const: bool` populated at parse time,
  identical pattern to `is_unsafe`).
- **Real part — `const fn`**: once `is_const` is set, `glyim-typeck` must enforce const-fn-body
  restrictions (only calling other `const fn`s, no heap allocation unless the allocator itself is
  const-capable, no trait-object dispatch, no floating point if the target const-eval doesn't support
  it — mirror whatever restriction set `glyim-const-eval`'s interpreter actually supports) at
  definition time, not just fail opaquely when someone tries to const-eval a call to it. Register
  `const fn`s so `glyim-const-eval`'s `Expr::Call`/`MethodCall` handling can check "is this callee
  actually const" up front and give a precise diagnostic instead of a generic evaluation failure.
- **Real part — `async fn`**: implement `async fn` desugaring at HIR-lowering time: an `async fn`
  becomes a regular `fn` returning `impl Future<Output = ReturnTy>`, with its body transformed into a
  state machine. This is a substantial feature; scope it explicitly:
  1. Define the `Future` lang item (add to `LangItems`, mirroring the registry pattern — if
     `glyim-type` doesn't yet have a lang-item registry, add one here since this feature requires
     it: `poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Output>`).
  2. Desugar the async body into a generator-shaped state machine: walk the body for `.await` points,
     assign each a state index, and lower into a `match self.state { 0 => ..., 1 => ..., }`-style
     `poll` implementation, storing locals that live across an `.await` point in the generated
     state-machine struct (borrow-checker-equivalent liveness analysis needed to determine which
     locals must be stored vs. can stay as MIR temporaries within one state).
  3. Desugar `.await` itself into `loop { match Future::poll(pinned, cx) { Ready(v) => break v,
     Pending => yield /* suspend this poll call, resume here next time */ } }`.
  This is large enough to warrant its own design doc if the language doesn't already have a
  generator/coroutine primitive in MIR to build on — if `glyim-mir` has no coroutine support, add a
  minimal one first (`TerminatorKind::Yield`/`GeneratorDrop`, mirroring rustc's MIR generator
  lowering) since async desugaring needs it as a building block; don't hand-roll ad hoc state-machine
  codegen bypassing MIR.
  Test: an `async fn` with two sequential `.await` points, driven by a trivial single-threaded
  executor (write one for testing — a `loop { match fut.poll(...) { Ready(v) => return v, Pending =>
  continue } }` busy-poller is sufficient for compiler tests), asserting correct value propagation
  and correct suspend/resume ordering (interleave two async fns manually polled to confirm state is
  preserved correctly across suspension).

### 6.2 `lower_type_ref DynType` — object safety deferred to typeck (`lower/lower_type.rs`)

Correctly deferred (HIR lowering shouldn't need full type information) — no fix needed here itself,
but cross-reference: confirm `glyim-typeck::tyconv.rs::resolve_type_ref` (§9.4) is the *single* place
object safety is actually checked, and that no code path can construct a `dyn Trait` MIR/layout
value bypassing that check (audit `glyim-layout::vtable.rs`, §10.2, and both codegen backends to
confirm they only ever receive already-validated trait-object types, never construct one from raw
`DefId`s without going through typeck's gate).

---

## 7. `glyim-type`

### 7.1 `needs_drop` returns `false` for unregistered cross-crate `Adt` (`ty_ctx.rs`)

This is a real correctness gap: silently treating a foreign type as non-drop when it might have a
`Drop` impl means MIR building/optimization skip its destructor — a memory leak or worse (skipped
`Drop` side effects) for any cross-crate droppable type. Fix:
- Ensure every dependency crate's ADT definitions are fully registered into `TyCtx` during crate
  *metadata loading*, before any query that could call `needs_drop` on a cross-crate type runs
  (this is a loading-order invariant, same pattern as the earlier report's `LowerCtx::adt_def` fix —
  the real fix is "never let an unregistered ADT be queryable", not "handle it gracefully when it
  happens").
- Change `needs_drop`'s signature to `Result<bool, TypeError::UnregisteredAdt>` during the transition
  period so any remaining ordering bug surfaces as a loud compile-time ICE rather than a silent
  `false`; once the loading-order invariant is verified (add an assertion in the crate-metadata
  loader that every ADT referenced in a loaded crate's exported item signatures is itself loaded),
  you may downgrade back to an infallible `bool` with a `debug_assert!` at the call site instead.
- Test: two-crate integration test (a "library" crate exporting a `Drop`-implementing type, a "app"
  crate using it) — instrument the `Drop` impl to record calls, run through `glyim-mir-interp`, and
  assert the destructor actually ran, which the current `false`-returning stub would fail.

### 7.2 `compute_auto_traits_recursive` — `Opaque`/`Projection` return empty flags (`auto_trait.rs`)

`Opaque` (i.e. `impl Trait` return-position types) and `Projection` (associated-type-projection
types like `<T as Iterator>::Item`) currently get zero auto traits, which is wrong whenever the
concrete underlying type *would* be `Send`/`Sync`/etc. Fix:
- **`Opaque`**: an opaque type's auto-trait set must be computed from its *defining-use* concrete
  type (the `impl Trait` return position type's actual hidden type, known at the definition site).
  Store the resolved hidden type on the `OpaqueTypeDef` (populated during typeck of the defining
  function) and have `compute_auto_traits_recursive` recurse into it exactly as if it were the
  concrete type, rather than stopping at the opaque wrapper.
- **`Projection`**: normalize the projection first (`<T as Iterator>::Item` → the concrete
  associated-type value, via the trait solver's projection-normalization, §8) — if it normalizes to
  a concrete type, recurse into that; if it's still unresolved (generic context where `T` isn't
  concrete), the auto-trait result must itself be *generic*, expressed as "auto-traits of `<T as
  Iterator>::Item` = auto-traits of whatever `T::Item` turns out to be", propagated as a where-clause
  obligation rather than assumed false. This requires auto-trait computation to be able to return
  "conditionally true, pending `X: AutoTrait`" rather than a flat bool in the generic case — extend
  `AutoTraitFlags` to `AutoTraitResult { definite: AutoTraitFlags, conditional: Vec<(Ty, AutoTrait)> }`
  wherever this matters (this mirrors how rustc computes auto-traits for generic/opaque types via
  where-clause obligations rather than eager booleans).
- Test: an `impl Trait` returning a type wrapping a channel-like non-`Send` type — must correctly
  report non-`Send`; a generic function returning `<T as MyTrait>::Output` where the bound guarantees
  `T::Output: Send` — must correctly propagate `Send` through the projection.

---

## 8. `glyim-solve` — **High severity item**

### 8.1 `SimpleTraitSolver::prove_trait` — no built-in/auto-trait support (`solver.rs`) — HIGH

This blocks *any* generic code with a `T: Send`/`T: Sync`/`T: Copy`/`T: Sized` bound, which is
extremely common — effectively this is a "generics barely work" bug. Fix:

```rust
fn prove_trait(&self, ctx: &TraitContext, goal: TraitRef) -> ProveResult {
    // 1. Auto traits (Send, Sync, Unpin, ...) — never impl-table-based; always structural.
    if let Some(auto_trait) = classify_auto_trait(goal.trait_def) {
        return match auto_trait_result_for(self.tcx, goal.self_ty, auto_trait) {
            AutoTraitResult { definite, .. } if definite.contains(auto_trait) => ProveResult::Yes,
            AutoTraitResult { conditional, .. } if !conditional.is_empty() =>
                ProveResult::Yes_If(conditional), // obligations the caller must also prove
            _ => ProveResult::No,
        };
    }
    // 2. Built-in structural traits: Copy, Clone (for trivially-Copy types), Sized, Unsize.
    if goal.trait_def == self.tcx.lang_items.require(LangItem::Copy)? {
        return prove_copy(self.tcx, goal.self_ty);
    }
    if goal.trait_def == self.tcx.lang_items.require(LangItem::Sized)? {
        return prove_sized(self.tcx, goal.self_ty); // recursively sized fields, no trailing DST unless goal is itself a DST context
    }
    // 3. Fall through to ordinary user-impl-table lookup (existing behavior), now via real
    //    unification (see prior-report §6.1) rather than structural equality only.
    self.prove_via_impl_table(ctx, goal)
}
```

- Implement `prove_copy`: `Copy` holds iff every field is `Copy` and the type has no `Drop` impl
  (Rust disallows `Copy` + `Drop` together — enforce this as a coherence-time error when someone
  tries to derive/impl both, not just silently in the solver).
  `Sized` holds for everything except: `str`, `[T]` (unsized slice), `dyn Trait`, and structs whose
  last field is itself unsized (recursive).
- Every downstream consumer of `prove_trait` (typeck bound-checking, `can_coerce`'s obligations,
  `glyim-lang-std`'s trait-bound-gated methods) must now handle `ProveResult::Yes_If(obligations)` by
  recursively discharging each nested obligation, not just checking `Yes`/`No` — this is what makes
  conditional auto-traits (§7.2) actually propagate correctly through generic code.
- Test: generic function `fn spawn_it<T: Send>(t: T)` called with a `Send` struct (passes), a
  non-`Send` struct containing a raw pointer marker type (fails with a clear "T is not Send"
  diagnostic naming the offending field, not a generic solver failure), and a generic wrapper
  `struct Wrap<T>(T)` where `Wrap<T>: Send` should hold conditionally on `T: Send` (tests the
  `Yes_If` propagation path).

### 8.2 `can_coerce` — missing tuple/struct/trait-object coercions (`fulfill.rs`) — Medium

- **Tuples**: Rust doesn't actually coerce between different tuple types even with covariant
  elements (tuples are invariant-shaped in coercion, not coercible field-wise) — verify this is
  correctly the target semantic (no fix needed if so; document why tuples are intentionally excluded
  to prevent a future "fix" from wrongly adding it).
- **Structs**: implement `CoerceUnsized`-style struct coercion (as in the previous report's §6.2) if
  not already present — `struct Wrapper<T: ?Sized>(T)` coercing `Wrapper<Concrete>` to
  `Wrapper<dyn Trait>` when the single field itself coerces.
- **Trait objects**: `dyn SubTrait` → `dyn SuperTrait` upcasting coercion — implement by building the
  supertrait's vtable as a *sub-slice or adjusted-pointer view* of the subtrait's vtable (requires
  vtable layout to place supertrait methods at a predictable prefix — coordinate with §10.2's vtable
  layout rework to lay out supertrait methods first, in supertrait-declaration order, specifically to
  make this upcast a simple pointer offset rather than requiring a secondary vtable).
Test: struct-field unsizing coercion end-to-end, and `dyn SubTrait` → `dyn SuperTrait` upcast calling
a supertrait method correctly after the coercion.

### 8.3 `check_hrtb` — naive, only static/reflexive cases (`hrtb.rs`) — Medium

Implement full HRTB checking as described in the prior report's §6.4/§6.5 (region unification with
placeholder instantiation and leak-checking, universe-aware structural equality) — this report
confirms the same gap independently; treat as the same fix, not a second design. If a shared
`interp-core`/`type-relate` module doesn't yet exist from a previous pass, create it now:
`glyim-solve::src::relate.rs` implementing a generic `Relate` trait (equate / subtype / HRTB-outlives
as three instantiations of the same structural-walk-with-binders algorithm).

---

## 9. `glyim-typeck` — **High severity item**

### 9.1 `check_expr::Expr::MethodCall` — HIR-only search, ignores trait solver & other crates (`check_expr.rs`) — HIGH

This is the most impactful bug in the report: any method call resolved via a trait impl that isn't
textually present in the current crate's HIR (blanket impls, foreign-crate impls, impls gated behind
a `where`-bound requiring the solver) simply fails to resolve. Fix — replace with the real algorithm:

```rust
fn resolve_method_call(&mut self, receiver_ty: Ty, method: Ident, args: &[Expr]) -> Result<ResolvedMethod, TypeError> {
    let mut candidates = Vec::new();
    for (deref_step, step_ty) in self.deref_chain(receiver_ty).enumerate() {
        // Inherent methods first, from the *global* method table (all loaded crates), not just local HIR.
        candidates.extend(self.method_table.inherent_methods(step_ty, method));
        if !candidates.is_empty() { break; }
        // Trait methods: for every trait currently in scope (imported, or auto-in-scope like the prelude),
        // ask the solver whether `step_ty: Trait` actually holds — this is the missing piece.
        for trait_def in self.traits_in_scope() {
            if let Some(item) = self.method_table.trait_method(trait_def, method) {
                let goal = TraitRef { trait_def, self_ty: step_ty, ... };
                match self.solver.prove_trait(&self.obligation_ctx, goal) {
                    ProveResult::Yes => candidates.push(Candidate::Trait(trait_def, item, deref_step)),
                    ProveResult::Yes_If(obligations) => {
                        candidates.push(Candidate::Trait(trait_def, item, deref_step));
                        self.pending_obligations.extend(obligations); // discharged after the call is resolved
                    }
                    ProveResult::No => {}
                }
            }
        }
        if !candidates.is_empty() { break; }
    }
    match candidates.len() {
        0 => Err(TypeError::NoMethodFound { receiver_ty, method, searched_traits: self.traits_in_scope() }),
        1 => Ok(candidates.remove(0).into()),
        _ => Err(TypeError::AmbiguousMethod { receiver_ty, method, candidates }),
    }
}
```

- This requires: (a) the global `MethodTable` from the prior report's §9.2 (build once at
  crate-graph-load time, across *all* loaded crates, not just the current one — index by
  `(receiver_type_head, method_name)` for O(1) candidate lookup before falling to the solver for
  trait methods), (b) `traits_in_scope()` computed from the current module's imports plus the
  language's prelude, and (c) the solver actually being able to answer "does `T: Trait` hold" for
  arbitrary `T` including generic type parameters with bounds (which requires §8.1's fix — these two
  High items are coupled; land §8.1 first).
- Diagnostics: on zero candidates, list which traits *were* checked and why each failed (e.g. "trait
  `Iterator` is in scope but `MyStruct: Iterator` does not hold because `MyStruct: IntoIterator` is
  not satisfied") — this is the single highest-value error-message improvement in the whole plan
  since method-not-found is one of the most common beginner error classes.
- Test matrix: inherent method on a local type (baseline, must still work), trait method from a
  blanket impl (`impl<T: Display> MyExt for T`) — previously invisible to a HIR-only scan, trait
  method from a dependency crate's impl, and an ambiguity case (two in-scope traits both providing
  `foo()` for the same receiver type with no inherent impl) producing the ambiguity diagnostic.

### 9.2 `check_pat::Pat::Slice` — length not validated until MIR lowering (`check_pat.rs`) — Medium

Move the length check earlier for better diagnostics (a MIR-lowering-time error is far harder to
report with a good span/message than a typeck-time one). Fix: when checking a slice pattern against
a fixed-size array type `[T; N]`, validate immediately:
- Pattern with no `..` rest: `pattern.len() == N` exactly, else `E-SLICE-PATTERN-LENGTH` naming both
  lengths.
- Pattern with a `..` rest: `fixed_len <= N`, else same error (too many fixed elements for the array
  size).
Against a dynamically-sized slice type `[T]` (not array), no length constraint is checkable at
typeck time — this must remain a deferred *runtime* check (correctly deferred to MIR lowering /
codegen as a runtime length comparison + panic), so only the fixed-array case moves earlier; keep
the slice case as-is and add a comment clarifying which case is which, so a future reader doesn't
"fix" the still-correctly-deferred slice case. Test: array pattern with wrong element count → typeck
error with correct expected/found counts in the message; slice pattern of any shape → still compiles
at typeck time, panics correctly at runtime.

### 9.3 `CoherenceChecker` — no negative impls / specialization (`coherence.rs`) — Medium

- **Negative impls** (`impl !Send for Foo {}`): if the language grammar doesn't yet parse these, add
  the parser support first (`!` before the trait name in an impl header), then have coherence
  checking treat a negative impl as removing an otherwise-auto-derived auto trait rather than as an
  ordinary impl participating in overlap checking — negative impls only apply to auto traits (mirror
  Rust's restriction: reject `impl !MyRegularTrait for Foo {}` with a clear diagnostic that negative
  impls are auto-trait-only, unless the language deliberately wants to generalize this).
- **Specialization**: given `default impl` support from §5.2, extend overlap checking: two impls that
  structurally overlap are *allowed* if exactly one of them is more specific (its self-type/where
  clauses are a strict subset — i.e. every type matching the specific impl also matches the general
  one, but not vice versa) **and** the more general one is marked `default`. Implement specificity
  checking as: impl A is more specific than impl B iff A's trait ref unifies against B's trait ref
  with A's generics as rigid and B's generics as existentials, but not vice versa (standard
  specialization partial-order check). Without this, keep rejecting overlaps as a hard coherence
  error — don't allow silent "last impl wins" behav6ior, which would be unsound.
Test: negative `Send` impl removing an otherwise-inferred auto-trait; specialization pair (`default
impl<T> Trait for T` + `impl Trait for ConcreteType`) resolving to the specific impl when called on
`ConcreteType` and the default impl otherwise; a genuine ambiguous-overlap case still correctly
rejected.

### 9.4 `resolve_type_ref DynType` — object safety doesn't consider supertraits/assoc-type constraints (`tyconv.rs`) — Medium

Extend the object-safety check (building on the earlier report's §1.5) to walk the **full supertrait
chain**, not just the immediate trait: a method is only excludable from the vtable via `where Self:
Sized` on itself, but a *supertrait's* non-dispatchable method (generic method, `Self`-by-value
return, etc.) makes the whole `dyn Trait` object-unsafe too, since a `dyn SubTrait` must also satisfy
`dyn SuperTrait`'s vtable requirements to support upcasting (§8.2). Also check associated-type
constraints: `dyn Iterator` (no `Item` binding) is invalid — a trait with associated types must have
every associated type bound when used as `dyn Trait` (`dyn Iterator<Item = u32>` is required, bare
`dyn Iterator` is a compile error) unless the language provides a default. Implement by recursively
collecting the full set of methods/assoc-types across the supertrait DAG (dedup by identity, not by
name, in case of diamond supertrait hierarchies) before running the existing per-method object-safety
predicate. Test: `dyn Trait` where a *supertrait* (not the trait itself) has a generic method → must
be rejected; `dyn IteratorLike` with an unbound associated type → must be rejected; correctly bound
`dyn IteratorLike<Item = u32>` → must succeed.

---

## 10. `glyim-layout`

### 10.1 `VTableComputer::vtable_of` — no supertrait/associated-type resolution, empty methods on missing trait (`vtable.rs`)

- **Missing trait ⇒ empty methods silently**: change to a hard `Result::Err(LayoutError::UnknownTrait)`
  — an unresolvable trait at vtable-construction time is a pipeline-ordering bug (should have been
  caught by typeck's object-safety check, §9.4, long before layout runs), not something layout should
  paper over with an empty vtable that would silently miscall through null/garbage slots at runtime.
- **Supertraits**: per §8.2/§9.4's coordination, lay out the vtable with supertrait methods first (in
  a canonical order: outermost-to-innermost supertrait, declaration order within each trait), so
  `dyn Sub` → `dyn Super` upcasting is a constant pointer offset rather than requiring a second
  vtable lookup table. Implement `collect_vtable_methods(trait_def) -> Vec<DefId>` recursing
  supertraits-first, matching exactly the order §9.4's object-safety walk uses (share the traversal
  code between the two — a mismatch here would be a real, hard-to-debug ABI bug).
- **Associated types**: associated-type bindings don't need vtable slots (they're resolved
  statically at the `dyn Trait<Assoc = X>` type level, not dynamically), but the vtable builder must
  *validate* every associated type is bound (delegating to §9.4's check, not re-implementing it) and
  use the bound concrete type when determining each method's calling convention (e.g. if a method
  returns `Self::Assoc` by value, the vtable-call trampoline needs to know its size/ABI, which comes
  from the bound concrete type).
Test: two-level supertrait hierarchy (`trait A`, `trait B: A`, `trait C: B`) — vtable for `dyn C`
must contain `A`'s methods first, then `B`'s, then `C`'s, in that stable order, and an upcast from
`dyn C` to `dyn A` must be a simple truncating pointer-offset view producing a working `dyn A`.

---

## 11. `glyim-mir`

### 11.1 `Place::ty` for `ProjectionElem::Subslice` on array base returns element type instead of slice type (`src/lib.rs`)

Straightforward but important correctness bug — this type is used throughout MIR-consuming passes
(borrow-check-equivalent analysis, codegen, the interpreter) to know what a subslice place actually
holds. Fix:

```rust
ProjectionElem::Subslice { from, to, from_end } => {
    let base_ty = self.ty(base_place, tcx);
    match base_ty.kind() {
        TyKind::Array(elem_ty, len) => {
            let new_len = compute_subslice_len(*len, from, to, from_end); // const-foldable when len/from/to are all constants
            match new_len {
                Some(n) => TyKind::Array(*elem_ty, n).intern(tcx), // fixed-size subslice of a fixed-size array stays an array
                None => TyKind::Slice(*elem_ty).intern(tcx), // dynamic-length subslice degrades to a slice type
            }
        }
        TyKind::Slice(elem_ty) => TyKind::Slice(*elem_ty).intern(tcx), // subslice of a slice is always a slice
        _ => bug!("Subslice projection on non-array/slice base"),
    }
}
```

This directly feeds correctness into the prior report's §8.6/§12.4 dynamic-slice/fat-pointer work —
make sure that code path calls this corrected `Place::ty` rather than any place that still assumes
"subslice of array = element type". Test: `let [_, rest @ ..] = arr;` where `arr: [i32; 5]` — `rest`'s
place type must be `[i32; 4]` (still an array, constant length), and a subslice with a
runtime-computed bound must correctly type as `[i32]` (slice).

---

## 12. `glyim-lower`

### 12.1 `MirBuilder::lower_closure` — dummy `DefId`, no capture analysis, captures passed as args (`builder.rs`)

Same defect class as the prior report's closure findings — implement per that plan's §5.1/§5.2 in
full (capture analysis producing a real environment struct, real synthesized `DefId`, `Fn`/`FnMut`/
`FnOnce` trait impls generated for the env type). The "captures passed as arguments" detail here adds
one more concrete requirement: audit every call site that invokes a closure value to make sure it's
updated in lockstep — once captures live in an env struct rather than being spread across the call's
argument list, every closure-invocation code path (direct calls, `Fn`-trait dynamic dispatch calls,
the const-evaluator's closure support) must be updated together, or a partial migration will produce
mismatched calling conventions between "closures built by the builder" and "closures called by X".
Treat this as one atomic change across `glyim-lower`, `glyim-mir-interp`, and both codegen backends,
landed together with a full closure test suite (direct call, stored-and-later-called, called through
a `dyn Fn` trait object, nested closures capturing another closure's environment) rather than
incrementally.

### 12.2 `lower_dynamic_range_slice` — confirmed complete per this report (uses `Subslice`, desugared later)

No action beyond confirming (via a test) that the desugaring pass it depends on (`glyim-opt::
slice_desugar`, prior report §12.4) is actually wired into the active pass pipeline — if that pass
integration isn't yet fixed, this "complete" code is only complete in principle; add an end-to-end
test slicing a `Vec` with a runtime-computed range and indexing the result, which will fail loudly if
the desugaring pass isn't actually running.

### 12.3 `MonoCtx::collect` drop-glue "stub" — confirmed correct delegation to `glyim-pipeline`, but see §16.1 below where the delegate target is itself broken

No change needed in `glyim-lower` itself; cross-reference only.

---

## 13. `glyim-const-eval`

### 13.1 `eval_expr::Expr::Closure` — "not a const value" (`src/eval.rs`) — Medium

Per the prior report's §4.3: allow closures in const context when immediately invoked or fully
non-capturing (`const fn`-eligible), rather than a blanket rejection. Once §12.1's real closure
lowering exists, a non-capturing closure is just an ordinary function value — const-eval can call it
like any other `const fn` (§13.2). A capturing closure remains rejected in const context (with the
existing clear diagnostic), since const evaluation has no runtime environment to close over.

### 13.2 `eval_cast::TypeRef::Path` — fixed primitive set only, no user types/generics (`src/eval.rs`)

Extend cast evaluation beyond the primitive allowlist:
- **User-defined types**: support `as`-casts that are actually legal for user types per the
  language's cast rules (typically: fieldless-enum-to-integer casts, and nothing else beyond
  primitives — verify against the language spec/typeck's existing cast-legality checker, which should
  already be the single source of truth for "is this cast even legal"; const-eval should *reuse* that
  legality check rather than maintaining its own separate allowlist that can drift out of sync).
  Refactor: extract `is_valid_cast(from_ty, to_ty) -> Result<CastKind, CastError>` from wherever
  typeck currently decides cast legality, share it with const-eval, so `eval_cast` becomes "assert the
  cast is legal via the shared checker, then evaluate `CastKind` accordingly" rather than
  reimplementing legality via a type allowlist.
- **Generic casts**: `x as T` where `T` is a generic parameter isn't generally const-evaluable unless
  monomorphized — ensure const-eval only runs post-monomorphization for generic const contexts (or,
  if it must run pre-mono for `const fn` bodies used as const-generic defaults, defer with a clear
  "cannot evaluate cast: generic type not yet resolved" error rather than silently miscomputing).
Test: fieldless enum → `i32` cast evaluated correctly at const time; illegal cast (e.g. struct → int)
rejected with the same diagnostic typeck would give at runtime-context cast-checking, proving the
two are unified.

### 13.3 `eval_for` — only `Range`/`Array`/`Tuple` iterables, no `Vec`/custom iterators (`src/eval.rs`)

Per the prior report's §4.4, once `Expr::Call`/`MethodCall` support lands (§13.1 unblocks this since
closures might appear in iterator chains, and general method-call support is needed for
`.next()`/`IntoIterator::into_iter()`), generalize `eval_for` to the real desugaring: call
`IntoIterator::into_iter` on the iterable (works for `Range`/`Array`/`Tuple`-via-their-lang-item-impls
*and* for `Vec`/any user iterator uniformly, since it's the same desugaring regardless of concrete
type), then loop calling `.next()` matching `Some`/`None` — delete the type-kind special-casing
entirely in favor of this uniform desugaring, bounded by the step-budget from the prior report's §4.2.
Test: const-eval a `for` loop over a const-constructed `Vec<T>` (if `Vec` supports const construction)
and over a small user-defined iterator type implementing `Iterator` manually.

---

## 14. `glyim-mir-interp`

### 14.1 `TerminatorKind::Drop` — debug-log-and-fallthrough, "correct because drop elaboration should have replaced it" (`src/lib.rs`) — Low, but verify the assumption

The comment asserts a pipeline invariant ("drop elaboration always replaces `Drop` terminators with
drop-glue calls before this interpreter sees them") without the interpreter *enforcing* it. Fix:
convert the debug-log-and-fallthrough into a hard assertion consistent with the "prove it, don't hope
it" pattern used elsewhere in this plan:

```rust
TerminatorKind::Drop { place, target, .. } => {
    debug_assert!(
        false,
        "Drop terminator reached the interpreter for place {place:?}; drop elaboration should \
         have lowered this to a drop-glue call. This indicates a missing/misordered optimization \
         pass — check that DropElaboration runs before MIR reaches the interpreter/codegen."
    );
    self.goto(target) // fail open only in release builds, loudly in debug/test builds
}
```

Add a MIR-validator check (prior report §8.8's pattern) asserting no `Drop` terminator survives past
the drop-elaboration pass, so this becomes provably true rather than assumed. Test: run the validator
against MIR immediately after drop elaboration in the standard test suite — any regression trips it
immediately instead of silently no-op'ing a drop at interpretation time.

### 14.2 `cleanup` blocks / `panics_unwind` no-op — unwinding doesn't execute cleanup (`src/lib.rs`) — Medium

This means any test relying on `Drop` running during a panic (a very common and important semantic —
"drop runs during unwind" is core to resource-safety in a Rust-like language) silently doesn't get
it. Fix: implement real unwinding in the interpreter:
- On a `panic!`-equivalent (or any operation the interpreter models as panicking — array
  out-of-bounds, integer overflow in debug mode, explicit `panic!` calls), instead of aborting
  interpretation immediately, walk up the call stack: for each frame, if the currently-executing
  block has a `cleanup` target on its terminator (or the frame's `Call` terminator that's currently
  in flight has one), execute that cleanup block (which itself may contain `Drop`/further calls) to
  completion before continuing to unwind to the next frame.
- Stop unwinding at a frame explicitly marked as a catch boundary (if the language has
  catch-unwind-equivalent semantics) or propagate all the way out (process abort / test-harness
  reports panic) if not.
- Guard against a panic occurring *during* cleanup execution (double panic) — the language spec
  should define this as an abort; implement that termination behavior explicitly rather than letting
  it recurse indefinitely or panic the host interpreter process.
Test: a function with a local holding a `Drop`-instrumented value, panicking after acquiring it —
assert (via the instrumented `Drop`'s call-count) that it still ran during unwind; a double-panic
scenario (panic inside a cleanup block) asserted to abort cleanly rather than hang/crash the test
harness itself.

---

## 15. `glyim-opt`

### 15.1 `validate.rs::validate_body` — drop/needs_drop consistency check explicitly not yet performed (`src/validate.rs`) — Medium

The comment names exactly what's missing. Implement it now that §7.1 gives a single canonical
`needs_drop`:

```rust
fn check_drop_consistency(body: &Body, tcx: &TyCtx) -> Result<(), ValidationError> {
    for bb in body.basic_blocks() {
        if let TerminatorKind::Drop { place, .. } = &bb.terminator {
            let ty = place.ty(body, tcx);
            if !tcx.needs_drop(ty)? {
                return Err(ValidationError::UnnecessaryDropTerminator { place: place.clone(), ty });
            }
        }
        // Conversely: any place whose type needs_drop must have a Drop terminator (or a proven-moved-out
        // flag guarding its omission) on every path out of its scope — this is the deeper half of the check.
    }
    check_every_droppable_local_is_dropped_on_every_exit_path(body, tcx)?; // graph reachability check
    Ok(())
}
```

The second half (every droppable local dropped on every exit path) is the substantive one — implement
as a backward dataflow analysis: for each basic block, compute the set of locals that *must* have
been dropped by the time control reaches a `Return`/unwind-exit; flag any droppable local missing
from that set at any exit. This is what actually catches drop-elaboration bugs like §16.1's array
drop-glue stub, rather than merely asserting a `Drop` terminator's *target type* looks droppable.
Wire this validator into the always-on debug/CI validation pass from the prior report's §8.8/§22.2.
Test: a MIR body with a deliberately-omitted `Drop` on one exit path (construct by hand or via a
temporarily-reintroduced bug) — validator must flag it; a fully correct body must pass cleanly.

### 15.2 `drop_elaboration::run` — array drop loops may drop uninitialized elements on partial init (`src/drop_elaboration.rs`) — Medium

Fix: array drop-glue must be gated by the *same* drop-flag mechanism used for partial struct moves
(prior report's §12.2), applied per-element for arrays constructed incrementally (e.g. via a loop
building up a fixed-size array, or via `[a, b, c]` array-literal lowering where elements are
initialized one at a time in MIR even though they appear atomic in surface syntax):
- Track, per array-typed place, either (a) a single "fully initialized" flag if the array is always
  initialized atomically (the common case — array literals lower to a single `Aggregate` rvalue, so
  this is usually sufficient and cheap), or (b) per-element flags only when MIR actually contains a
  code path that could leave the array partially initialized before a panic/early-return (e.g. an
  array built via a loop with an early-exit on error partway through) — detect this case specifically
  and only pay the per-element-flag cost when it's structurally possible, not unconditionally for
  every array (an unconditional per-element flag array would be a real, unnecessary performance
  regression for the common atomic-initialization case).
- The drop loop itself, when per-element flags are needed, becomes `for i in 0..N { if flags[i] {
  drop(arr[i]) } }` rather than an unconditional loop.
Test: an array built via a loop that panics partway through initialization (e.g. index 2 of 5 fails)
— instrumented `Drop` must show exactly 2 elements dropped (the successfully-initialized ones), not 5
(which would double-drop/read uninitialized memory) and not 0 (which would leak). Also test the
atomic-array-literal fast path doesn't regress to unnecessary per-element flags (assert via generated
MIR inspection that no flag locals are introduced for a plain `[a, b, c]` literal).

### 15.3 `constant_prop::evaluate_rvalue_to_const::Aggregate` — only empty tuples (`src/constant_prop.rs`)

Per the prior report's §12.1, generalize to non-empty tuples, arrays, and structs with all-constant
fields, reusing shared constant-arithmetic (`interp-core`) rather than a third bespoke implementation.
Same fix, confirmed independently by this report — implement once.

---

## 16. `glyim-pipeline` — **Two High severity items**

### 16.1 `mono_cache.rs::generate_drop_glue` — arrays/slices get a bare `Return`, no element loop — HIGH

This is a real memory-leak/resource-leak bug in every compiled program using array-of-droppable-type
fields — the collector correctly enqueues the element type's drop glue as needed, but the array's own
generated glue function never calls it. Fix:

```rust
fn generate_drop_glue(tcx: &TyCtx, ty: Ty, mono_ctx: &mut MonoCtx) -> Body {
    let mut builder = MirBodyBuilder::new(drop_glue_sig(ty));
    match ty.kind() {
        TyKind::Adt(adt_id, substs) => { /* existing per-field logic, presumably already correct */ }
        TyKind::Array(elem_ty, len) if tcx.needs_drop(*elem_ty)? => {
            // for i in 0..len { drop_in_place(&mut (*self_ptr)[i]) }
            let loop_var = builder.new_local(Ty::USIZE);
            builder.emit_assign(loop_var, Rvalue::const_usize(0));
            let (loop_head, loop_body, loop_exit) = builder.new_blocks(3);
            builder.set_terminator(loop_head, TerminatorKind::SwitchInt {
                discr: Operand::Copy(loop_var.into()),
                targets: SwitchTargets::if_less_than(*len, loop_body, loop_exit),
            });
            let elem_place = builder.place_index(self_place(), loop_var);
            let elem_glue = mono_ctx.enqueue_drop_glue(*elem_ty); // already happening per the report — now actually consumed
            builder.emit_call(elem_glue, [Operand::Move(elem_place)], /* next: */ increment_and_loop(loop_var, loop_head));
            builder.set_terminator(loop_exit, TerminatorKind::Return);
        }
        TyKind::Array(_, _) => builder.set_terminator(builder.entry_block(), TerminatorKind::Return), // element type genuinely doesn't need drop — Return is correct here, not a stub
        TyKind::Slice(elem_ty) if tcx.needs_drop(*elem_ty)? => { /* same loop shape, but length comes from the fat-pointer metadata, not a const */ }
        _ => unreachable!("drop glue requested for a type that doesn't need drop; caller bug — PROOF: mono_ctx.enqueue_drop_glue only called after needs_drop check"),
    }
    builder.finish()
}
```

- Coordinate the loop-bound source for `[T; N]` (compile-time constant `N`) vs `[T]` slices (runtime
  length from the fat-pointer metadata word, per the prior report's §8.6 fat-pointer representation)
  — these are genuinely different code shapes, don't conflate them.
- Integrate with §15.2's drop-flag work: if the array being dropped could be partially initialized
  (rare — drop glue normally runs on a fully-initialized value by construction, but partial-move-then-
  panic scenarios can reach here too), the drop loop must consult the same per-element flags rather
  than assuming full initialization.
Test: a struct containing a `[DropInstrumented; 5]` field — dropping the struct must show exactly 5
destructor calls, not 0 (the current bug). A struct containing a `Vec<DropInstrumented>`-backed slice
(via `Box<[T]>` or similar) — same assertion via the runtime-length loop path.

### 16.2 `pipeline_context.rs::PipelineLowerCtx::hir_body` always returns `None` — HIGH

This breaks `const { ... }` block evaluation during lowering entirely — any inline const block simply
can't be evaluated, presumably falling back to some other error path or (worse) silently producing a
wrong/default value downstream. Fix:
- Determine why it always returns `None`: almost certainly this function was stubbed pending a way to
  look up a HIR body by `DefId`/`BodyId` from within the pipeline's lowering context, which by this
  point in the plan exists (`glyim-hir` owns bodies, `glyim-db`'s new query system, §3.1, is exactly
  the mechanism to fetch one without threading raw references everywhere).
- Implement properly:
  ```rust
  fn hir_body(&self, body_id: HirBodyId) -> Option<&hir::Body> {
      self.db.query(HirBodyQuery(body_id)) // real incremental lookup, not a stub
  }
  ```
  Wire `HirBodyQuery` as a real query in `glyim-db` (§3.1) backed by the crate's parsed+lowered HIR,
  keyed by `HirBodyId`, invalidated when the owning item's source changes.
- Once this returns real bodies, confirm `const { ... }` block lowering (wherever it calls
  `hir_body` to fetch the const block's body for evaluation via `glyim-const-eval`, §13) actually
  exercises the fixed `glyim-const-eval` call/method/for-loop support end-to-end.
Test: a function containing an inline `const { compute_something() }` block where `compute_something`
is itself a small `const fn` doing arithmetic — must evaluate at compile time (verify by checking the
generated MIR/LLVM IR contains the folded constant, not a runtime call) rather than silently failing
or leaving a placeholder value.

---

## 17. `glyim-runtime`

### 17.1 `glyim_net_tcp_accept` — fixed 256-byte `addr_buf`, silent truncation (`src/lib.rs`) — Low

Fix: use `getpeername`'s actual required-size query first (or, on platforms where `accept` itself
reports the address length written, check that return value), and either (a) grow the buffer to the
reported size and re-fetch, or (b) if the FFI signature can't easily support variable-length output,
at minimum detect truncation (compare returned length against buffer capacity) and return an explicit
`GlyimIoError::AddressTooLong` instead of silently handing back a truncated address (a truncated IP/
socket address is actively dangerous to consume — e.g. an IPv6 address truncated could parse as a
different, valid-looking, wrong address). Test with an IPv6 loopback connection (longer textual/binary
representation than a typical IPv4 test would exercise) to make sure the non-truncating path is
actually taken.

### 17.2 `glyim_process_spawn` — no argument escaping/space handling (`src/lib.rs`) — Medium

This is a real bug, not a nice-to-have: on Windows, process arguments are passed as a single
command-line string that the child process re-parses, so improper escaping of spaces/quotes in
arguments changes how many arguments the child actually receives (a classic, security-relevant class
of bug). Fix:
- On Unix: arguments are already passed as a proper `argv[]` array (implied by "null-separated byte
  sequence" in the report) — spaces within a single argument are not a problem *if* the null-
  separation is correctly preserved end-to-end; audit the FFI boundary to confirm no intermediate step
  re-joins-and-re-splits on whitespace (a common accidental bug when shelling out via a string command
  line instead of an argv array internally) — if it does, fix it to pass argv directly to `execve`
  without ever forming a single string.
- On Windows: implement proper command-line quoting per the documented Windows argument-quoting rules
  (backslash-and-quote escaping algorithm — this is a well-specified, if fiddly, algorithm; implement
  exactly the `CommandLineToArgvW`-compatible quoting, e.g. as used by Rust's own `std::process`
  implementation as a reference for correctness) when constructing the single command-line string
  `CreateProcess` requires.
Test: spawn a child process (a tiny test helper binary that echoes back its received `argv`) with
arguments containing spaces, embedded double quotes, and (Windows-specific) trailing backslashes
before a quote — assert the child receives exactly the intended argument boundaries and content on
both platforms tested in CI.

### 17.3 `glyim_process_kill` on Windows ignores `signal`, always `TerminateProcess` (`src/lib.rs`) — Medium

Confirmed cross-referenced from the prior report's §16.4 — same fix: map the requested signal to the
closest Windows equivalent where one exists (there generally isn't a graceful-terminate equivalent to
`SIGTERM` on Windows without cooperating console-event handling — `GenerateConsoleCtrlEvent` for
console apps is the closest analog for a "polite" termination signal), and explicitly document in the
FFI doc comment which signals map to a hard `TerminateProcess` vs. which get the console-event
treatment, rather than silently flattening every signal to the same hard-kill behavior.

---

## 18. `glyim-cli`

### 18.1 `invoke_linker` — no cross-compilation linker flags, `detect_unix_linker` misses `ld` (`src/linker.rs`) — Medium

- Extend `detect_unix_linker` to probe for `ld`, `ld.lld`, `ld.gold`, `mold`, in addition to `cc`/
  `clang`/`gcc`, in a sensible preference order (prefer a C-compiler-driver invocation like `cc` when
  available, since it correctly supplies default system library paths/CRT objects that invoking `ld`
  directly requires the caller to supply manually — fall back to raw `ld` variants only if no compiler
  driver is found, and when falling back, explicitly add the CRT startup objects/system library paths
  the compiler driver would otherwise have supplied, or document that raw-`ld` mode requires the user
  to supply them via `-L`/explicit flags).
- Cross-compilation flags: when `opts.target` (from the prior report's §21.3 fix) differs from the
  host, pass the target-appropriate flags to the invoked linker — e.g. `--target=<triple>` for
  `clang`, or select a target-prefixed cross-linker binary (`<triple>-ld`) if using raw `ld`, or
  `-m <emulation>` for GNU `ld` (e.g. `-m aarch64linux` for an aarch64 Linux target from an x86_64
  host). Maintain a small `target_triple -> linker_flags` table covering at minimum the CI-tested
  target set, with a clear error for untested/unmapped targets rather than silently passing host flags
  to a cross target.
### 18.2 `UnixLinker::link` — no `-L`/`-l` handling, only user-supplied flags (`src/linker.rs`) — Medium

Add first-class support: accept a structured `LinkArgs { search_paths: Vec<PathBuf>, libs: Vec<String>,
objects: Vec<PathBuf>, user_flags: Vec<String> }` instead of a flat user-flags string, and emit
`-L<path>` for each search path and `-l<name>` for each library *before* appending free-form
user-supplied flags (so users can still override/append anything the structured API doesn't cover).
Wire this from wherever crate/dependency linking currently happens (this connects directly to the
prior report's §21.2 `cmd_build` dependency-compilation work — each compiled dependency's output
directory becomes a `-L` search path, and its crate name becomes an `-l` argument, or the artifact
path is passed directly as an object/archive file, whichever the toolchain's rlib format requires).

### 18.3 `run_with_args` — `--emit=asm` unsupported (`src/lib.rs`) — Low

Add the missing emit kind by routing through LLVM's `LLVMTargetMachineEmitToFile` with
`LLVMAssemblyFile` as the codegen file type (mirroring however `--emit=llvm-ir`/`--emit=obj` already
invoke the target machine, just changing the requested output kind) — this should be a small,
mechanical addition once the existing emit-kind dispatch is examined, not a new code path. Test:
`--emit=asm` on a trivial program produces a `.s` file containing recognizable assembly for the host
target.

---

## 19. `glyim-codegen-llvm`

### 19.1 `lower_call` invoke/landingpad — Linux-specific, untested on Windows/macOS (`src/lower.rs`) — Medium

- **macOS**: Itanium C++ ABI unwinding (same personality function family as Linux,
  `__gxx_personality_v0`-equivalent) generally applies — verify/adjust the existing Unix path works
  as-is on macOS (the report's "hardcoded for Unix" framing suggests it might already be shared;
  confirm with an actual macOS CI runner test rather than assuming).
- **Windows**: implement the SEH-based path fully — `__CxxFrameHandler3` personality (already
  selected per `lower_body`, §19.2) requires funclet-based landingpads (`cleanuppad`/`catchpad`/
  `cleanupret` LLVM IR constructs), which are structurally different from Itanium `landingpad`/
  `resume` — this is not a small tweak, it's a distinct code-generation path. Implement
  `emit_seh_cleanup_funclets` alongside the existing `emit_itanium_landingpad`, selected by target
  triple, and test on an actual Windows CI runner with a program that panics through nested `Drop`
  calls, asserting all destructors run in the correct order (matches §14.2's interpreter-level
  unwinding test, but now for real compiled-and-executed Windows binaries).
- **No-unwind targets** (e.g. `panic = "abort"` configurations, or bare-metal targets with no
  unwinding support at all): implement the fallback explicitly — when the target/profile has no
  unwinding, `Call` terminators with a `cleanup` target must lower to a plain `call` (not `invoke`)
  and the cleanup block must simply never be reachable (or, if the language's abort-on-panic mode
  still wants cleanup-free destructors to run before aborting, that's a different, simpler
  lowering — decide which semantic the language wants and implement it explicitly rather than leaving
  the fallback unimplemented).

### 19.2 `lower_body` personality function selection — no fallback for non-unwinding targets (`src/lower.rs`) — Medium

Same fix as 19.1's third bullet — make personality-function selection a proper three-way match
(`Itanium | Seh | None`) driven by target+profile, with `None` correctly omitting personality/
landingpad emission entirely rather than the current implicit "Unix or Windows, nothing else" binary
choice.

### 19.3 `opaque_sized_type` — `align > 16` workaround, alignment not carried by the type itself (`src/types.rs`) — Medium

Confirmed independently by both reports — same fix as the prior plan's §14.5: call
`LLVMSetAlignment` unconditionally (not just above a threshold) at every allocation/global site using
an opaque sized type, driven by the real computed alignment from `glyim-layout`, and add the
`#[repr(align(64))]` runtime-verified alignment test from that plan if not already present.

### 19.4 `run_llvm_passes` — only the built-in default pipeline string, no custom pass management (`src/passes.rs`) — Low

Expose a `--llvm-pass-plugin=<path>`/`--llvm-passes=<comma-list>` CLI passthrough (via `glyim-cli`) for
advanced users, and, more importantly, verify the *default* pipeline string is actually
optimization-level-correct (`default<O0>`/`default<O1>`/`default<O2>`/`default<O3>`/`default<Os>` per
`opts.opt_level`, not a single hardcoded string regardless of requested optimization level — this
detail isn't explicit in the report but is exactly the kind of thing worth auditing while already in
this function). Test: confirm `-O0` vs `-O3` builds of a benchmarkable function produce measurably
different (and correctly, not just differently) optimized IR.

---

## 20. `glyim-codegen` (bytecode backend)

### 20.1 `emit_place_address::ProjectionElem::Downcast` — doesn't adjust for tag offset in all encodings (`bytecode.rs`) — Medium

Fix: compute the downcast address offset from `glyim-layout`'s actual `TagEncoding`/variant layout
(from the confirmed-complete `direct_tag_encoding`/`build_niche_layout` machinery), not an assumption
that variant data starts at offset 0:

```rust
fn emit_downcast_address(&mut self, base: &Place, variant: VariantIdx, ty: Ty) {
    let layout = self.tcx.layout_of(ty);
    let variant_offset = match &layout.variants {
        Variants::Direct { tag, .. } => layout.field_offset(0) /* after tag */,
        Variants::Niche { .. } => 0, // niche-encoded variants: data and tag share the same bytes, no separate offset
        Variants::Single { .. } => 0,
    };
    self.emit_op(OP_ADD_CONST, variant_offset);
}
```

Audit every enum layout kind actually produced by `glyim-layout` (direct-tag, niche-optimized,
single-variant) and handle each explicitly rather than assuming one shape — this is exactly the kind
of bug that "works by accident" for the common single-variant/niche case (offset 0) but silently
miscomputes for a direct-tag multi-field-variant enum. Test: a direct-tag-encoded enum with a
non-zero-offset variant field (e.g. a variant with a leading `u8` tag byte followed by a `u32` field
needing alignment padding) — downcast address must correctly skip both the tag *and* any padding, not
just assume offset 0.

### 20.2 `emit_rvalue::Rvalue::Ref` — always `OP_LOAD_LOCAL_ADDR`, no shared/mutable distinction (`bytecode.rs`) — Medium

For a stack-based bytecode VM, the distinction mostly matters for aliasing-related VM-level
invariants/future optimizations (e.g. a VM that wants to enforce or exploit "no concurrent mutable
alias" at the bytecode level) rather than address computation itself (both compute the same address).
Fix: at minimum, thread `BorrowKind` through to the emitted instruction as a tagged variant
(`OP_LOAD_LOCAL_ADDR_SHARED` / `OP_LOAD_LOCAL_ADDR_MUT`) even if both currently execute identically in
the interpreter loop, so (a) any future VM-level invariant checking has the information available
without a larger instruction-format migration, and (b) debug/introspection tooling (e.g. a bytecode
disassembler) can display the actual borrow kind rather than an ambiguous generic "address-of". Test:
disassemble bytecode for a function taking both `&x` and `&mut y` and assert the two produce distinct
opcodes in the output.

---

## 21. `glyim-meta`

### 21.1 `expand_builtin BuiltinMacro::Env` — no `option_env!`, no custom fallback (`src/expander/mod.rs`) — Low

Add `option_env!` as a sibling builtin: identical lookup to `env!` but returns `Option<&'static str>`
(`Some(value)` if set, `None` if not) instead of `env!`'s hard compile error on missing variable.
Implement by factoring the shared "look up env var at compile time" logic into one helper used by
both `BuiltinMacro::Env` (errors on missing) and the new `BuiltinMacro::OptionEnv` (produces a
`None`-shaped expansion on missing). Test both the present and absent cases for each macro.

### 21.2 `expand_builtin BuiltinMacro::Include` — no `include_bytes!`/`include_str!` (`src/expander/mod.rs`) — Low

Add both as builtins alongside whatever plain `include!` currently does (report implies only a
generic include exists): `include_str!("path")` reads the file as UTF-8 and expands to a string
literal token (error clearly on invalid UTF-8, don't silently lossy-convert); `include_bytes!("path")`
reads raw bytes and expands to a byte-array-literal token. Resolve the path relative to the invoking
file's directory (standard behavior), and integrate with `glyim-db`'s incremental system (§3.1) by
registering the included file as a dependency of the expanding crate's compilation, so editing the
included file correctly invalidates the cached expansion/downstream typeck results — a real bug risk
if skipped, since `include!`-family macros are a classic incremental-compilation invalidation gap.
Test: `include_str!` on a real fixture file produces the exact expected string constant; touching that
fixture file between two incremental builds correctly triggers recompilation of the including crate.

### 21.3 `expand_builtin BuiltinMacro::Concat` — literals/idents only, no multi-expression or `concat_idents!` (`src/expander/mod.rs`) — Low

- `concat!`: per the language's actual spec (verify against whatever `concat!` is documented to
  accept — typically literals of any primitive type, not arbitrary expressions, is the *correct*
  Rust-matching behavior, not a gap) — if the intended semantic is genuinely "literals only," this
  finding may be a non-issue; if the language spec wants broader expression support, that would need
  const-evaluating each argument (via the now-more-capable `glyim-const-eval`, §13) and
  stringifying the result, which is a much larger feature — clarify the intended spec first, then
  implement accordingly, don't guess.
- `concat_idents!`: implement as a separate builtin producing a single new identifier token by
  textually joining each argument identifier's text — note this needs care around hygiene (the
  synthesized identifier should generally *not* inherit any one argument's hygiene context
  unambiguously; document the chosen behavior clearly, matching upstream Rust's own
  documented-as-unstable/quirky behavior if the language intends compatibility, or defining a clean
  new rule if not).
Test: `concat!` with a mix of string/integer/bool literals produces the correctly-formatted combined
string (verify exact formatting matches spec, e.g. `true` not `"true"` quoted-and-requoted); `concat_idents!("foo", "_", "bar")` produces a token that resolves as the identifier `foo_bar` when used in a subsequent item position.

---

## 22. `glyim-lsp`

### 22.1 `provide_code_actions` — only "remove unused import" (`src/code_action.rs`) — Medium

Add, at minimum, these high-value code actions (each maps to a specific, well-scoped diagnostic the
compiler already produces once the earlier fixes land):
- **"Add missing match arm(s)"**: triggered by the exhaustiveness-checking diagnostic from the prior
  report's §8.4/§9.10 — use the diagnostic's *witness patterns* (already computed by Maranget's
  algorithm to explain what's missing) to synthesize the missing arm(s)' source text directly,
  inserted before the closing `}` of the match.
- **"Generate impl"**: triggered by a "trait not implemented" diagnostic — synthesize a skeleton
  `impl Trait for Type { }` with stub bodies (`todo!()`-equivalent, or default-value returns where a
  sensible default exists) for every required method/associated type, sourced from the trait's
  definition (now resolvable precisely via §9.4's full supertrait-aware object/trait info).
- **"Fix error"**: scope this narrowly and honestly — implement targeted quick-fixes for a specific,
  enumerated set of diagnostics with unambiguous single fixes (e.g. §3.4's "missing struct field" from
  the prior report → insert the field with a placeholder value; a simple typo'd identifier close to
  exactly one in-scope name → rename to the suggested name), rather than a vague catch-all — a
  catch-all "fix error" invites exactly the kind of "reframe to make it seem to work" implementation
  this report is trying to eliminate.
Test each action against a fixture diagnostic, asserting the code action's proposed edit, when
applied, produces source that compiles cleanly.

### 22.2 `rename_symbol` — text-based fallback, no shadowing/cross-file handling (`src/rename.rs`)

Same defect and same fix as the prior report's §20.4: replace with HIR-binding-identity-based rename
(every occurrence resolved to a `HirId`, only matching occurrences renamed), extended here explicitly
to **cross-file**: when the symbol is a module-level item (function, type, const) rather than a local
variable, the rename must find and update every reference across every file in the workspace that
imports/qualifies it, using the `glyim-def-map` import graph (§4) to enumerate all files that could
reference it (not a blind text-search across the workspace, which would both miss shadowed-elsewhere
occurrences of the same name and over-match unrelated same-named items in unrelated modules). Delete
the text-based fallback entirely once this path is complete; for any residual "no HIR mapping" case,
refuse with a clear message rather than falling back to unsafe text matching (same principle as the
prior report's §20.4).

### 22.3 `workspace_symbols` — prefix/contains matching only, no fuzzy/semantic search (`src/navigation.rs`) — Low

Add fuzzy matching (implement or vendor a standard subsequence-based fuzzy scorer, e.g. the
Sublime-Text-style algorithm: consecutive-match bonus, word-boundary-start bonus, penalize gaps) on
top of the existing exact/prefix/contains matching, ranking exact matches highest, then prefix, then
fuzzy-score-ordered results — don't replace the existing fast paths, layer fuzzy matching as a final
fallback tier so common exact lookups stay fast and predictable. Test with a representative symbol set
asserting fuzzy queries like `"gsrbt"` surface `"get_something_related_by_type"` reasonably highly
ranked.

### 22.4 `convert_diagnostics` — no `cargo check`/external-linter integration (`src/diagnostics.rs`) — Low

Scope check: if `glyip` (§23) is the project's *only* build tool (i.e. there's no actual "cargo" in
this ecosystem), this item is likely a stray/misapplied note from a template — verify against the
actual toolchain; if genuinely relevant (e.g. the language interops with an external linter tool),
implement a generic "external diagnostic source" adapter trait (`ExternalDiagnosticProvider::run(&self,
workspace: &Path) -> Vec<Diagnostic>`) so any external tool's JSON/line-based output can be mapped into
the LSP's diagnostic format, rather than a bespoke one-off integration.

### 22.5 `provide_hover` — no "go to definition" preview (`src/hover.rs`) — Low

Add a definition-preview snippet (a few lines of source around the definition site, syntax-highlighted
if the LSP client supports it) alongside the existing type-signature/doc display, sourced via the same
symbol resolution `rename_symbol` (§22.2) and completion (§22.6) now share, keeping definition lookup
implemented exactly once across all three features.

### 22.6 `provide_completions` — no generic snippets, no auto-import (`src/completion.rs`) — Low

- **Generic snippets**: when completing a generic function/method/type, emit a snippet with
  tab-stops for each generic parameter (`foo::<${1:T}>(${2:arg})`) rather than a bare name, using the
  LSP snippet syntax.
- **Auto-import**: when a completion candidate isn't currently in scope (found via the global
  `MethodTable`/symbol index, §9.1/§22.5, searching *all* loaded crates' public items, not just
  in-scope ones), offer it as a completion whose accepted-edit includes inserting the necessary `use`
  statement (deduped/sorted correctly into the existing import block, using `glyim-def-map`'s import
  structure, §4, to find/create the right insertion point) alongside the completion text itself.
Test: complete a method only available via an unimported trait — accepting the completion must both
insert the call *and* add the trait's `use` statement in one edit.

---

## 23. `glyip` — **Two High severity items**

### 23.1 `cmd_test` — test bodies never actually executed, placeholder report only — HIGH

This means the entire test-running feature is currently theater — `resolve_test_def_id` finds tests
but nothing runs them. Fix: wire real execution through the compiled artifact:

```rust
fn cmd_test(opts: &TestOpts) -> Result<TestReport> {
    let test_def_ids = discover_and_resolve_tests(&opts.workspace)?; // existing resolve_test_def_id, presumably fine
    let compiled = compile_test_binary(&opts.workspace, &test_def_ids)?; // build a harness binary linking every #[test] fn as a callable entry point
    let mut report = TestReport::default();
    for test in &test_def_ids {
        let outcome = run_test_in_subprocess(&compiled, test, opts.timeout)?; // isolate each test in its own process: a panicking test must not kill the whole runner
        report.record(test, outcome);
    }
    Ok(report)
}
```

- **Compiled-binary execution (preferred for realism)**: generate a small `main`-equivalent harness
  crate at build time that, for each discovered `#[test]` function, exposes a callable entry (e.g. via
  a generated dispatch table keyed by test name, or one harness binary invoked with `--test-name=X` to
  run a single test) — link this harness against the real compiled program, and run each test as an
  actual subprocess (or the whole harness binary self-dispatches by argv, matching how `cargo test`'s
  generated harness works) so a segfault/abort in one test can't corrupt or kill the whole run, and so
  `#[should_panic]` tests can be verified by observing the actual process exit status/panic message.
- **Interpreter execution (faster iteration, useful for `glyim-lsp`'s "run test" inline action)**: as
  a secondary/faster path, also support running a test directly through `glyim-mir-interp` (§14) in
  process, for quick feedback without a full compile+link+subprocess round-trip — but this must not be
  the *only* execution path, since interpreter semantics for panics/unwinding (§14.2) and any FFI/
  runtime-boundary behavior (`glyim-runtime` calls) may not be fully faithful to compiled-binary
  behavior; use it for fast iteration, but the "real" `glyip test` (as opposed to a `--fast`/watch-mode
  variant) should run the compiled path.
- Implement `#[should_panic]`/`#[should_panic(expected = "message")]` handling (verify the child
  process's panic message, if the harness captures it, matches the expected substring), `#[ignore]`
  skip handling with a `--include-ignored` override flag, and parallel execution (thread/process pool)
  with per-test timeouts (reuse/fix `glyim-test::harness::runner.rs`'s timeout mechanism, §24.2, which
  has its own zombie-process bug to fix in tandem).
- Real, non-placeholder reporting: pass/fail/ignored counts, per-test duration, captured
  stdout/stderr on failure, and a non-zero process exit code when any test fails (for CI integration).
Test: a test suite with a passing test, a failing assertion, a `#[should_panic]` test (both matching
and non-matching expected message, to test both accept and correctly-still-fail cases), an `#[ignore]`d
test, and a test that panics via a genuine process abort (e.g. deliberately triggers a segfault) —
assert the runner correctly isolates and reports each without the whole run dying, and that the
process exit code is non-zero exactly when a non-ignored test failed.

### 23.2 `DependencyResolver` — no git/path dependency support, version resolution incomplete — HIGH

Extend the manifest schema and resolver to support all three standard dependency source kinds:

```rust
pub enum DepSource {
    Registry { name: String, version_req: VersionReq }, // existing
    Path { path: PathBuf },                              // new: local filesystem dependency, no version resolution needed — always uses the on-disk source as-is
    Git { url: String, spec: GitSpec },                   // new
}
pub enum GitSpec { Branch(String), Tag(String), Rev(String), DefaultBranch }
```

- **Path dependencies**: resolve by reading the target directory's own manifest directly, no
  network/registry involvement, no version matching (the path *is* the pin) — but still participate in
  the dependency graph/cycle-detection (prior report's §21.6) and the incremental fingerprinting
  (§23.3) exactly like any other dependency, keyed by canonicalized absolute path rather than
  name+version.
- **Git dependencies**: implement via shelling out to the system `git` binary (simplest, most
  compatible approach — avoid vendoring a full git-protocol implementation unless there's a strong
  reason to) into the global cache directory (prior report's §21.1, finally given a real purpose): `git
  clone --depth 1 <url> <cache_dir>/git/<url_hash>` for a branch/tag, or a full clone + `git checkout
  <rev>` when an exact revision is pinned (shallow clone can't fetch an arbitrary historical rev
  without knowing it's reachable from a shallow depth — detect this and fall back to a full clone when
  a bare `rev` is requested without a known-shallow-reachable branch/tag hint). Cache the resolved
  commit hash in the lockfile (§23.4) so subsequent builds don't need to re-resolve `DefaultBranch`/
  `Branch`/`Tag` refs to a commit every time — only re-resolve when explicitly updating
  (`glyip update`).
- **Full semver resolution**: implement per the prior report's §21.5 (real SemVer 2.0 parsing +
  range matching + a backtracking/conflict-driven resolver reconciling multiple dependents' version
  requirements for the same registry package) — this report confirms the same gap independently.
Test: a workspace with a path dependency on a sibling directory (must rebuild when the sibling's
source changes — verify via fingerprinting, §23.3), a git dependency pinned to a tag (must resolve
to a stable commit and not re-fetch on every build), and a registry dependency conflict requiring
real semver range intersection to resolve (same test as the prior report's §21.5).

### 23.3 `FingerprintStore::has_any_changed`/`update_all` — only scans `.g` files, ignores manifest/build scripts (`src/fingerprint.rs`) — Medium

Fix: include in the fingerprinted input set: the crate's `Glyip.toml` manifest itself (a changed
dependency version/feature flag must trigger rebuild even with zero `.g` file changes), any build-
script output (if the toolchain supports build scripts — fingerprint the script's own source *and* its
declared outputs), the resolved lockfile entry for each dependency (a dependency's *resolved version*
changing, e.g. after `glyip update`, must invalidate dependents even though no local file changed),
and environment variables the build is documented to be sensitive to (e.g. `RUSTFLAGS`-equivalent
target/codegen-option environment variables). Test: touching only `Glyip.toml` (no `.g` file changes)
between two builds must trigger a rebuild; touching neither must skip rebuild (regression-guard the
existing correct behavior while fixing the gap).

### 23.4 `Lockfile` — no validation against current manifest, conflicts undetected (`src/lockfile.rs`) — Medium

Fix: on every build, after resolving dependencies per the manifest (§23.2), diff the fresh resolution
against the existing lockfile's recorded versions:
- If the manifest's version requirements are still satisfied by the locked versions, use the locked
  versions as-is (standard "respect the lockfile unless it can't satisfy the manifest" behavior).
- If a manifest requirement no longer admits the locked version (e.g. the manifest was hand-edited to
  require a newer minimum version), re-resolve just the affected subtree and update the lockfile,
  printing a clear "X was updated from version A to B because the manifest now requires >=B" message
  — never silently use a lockfile-pinned version that violates the current manifest's stated
  requirement, and never silently rewrite the lockfile without telling the user what changed.
- Detect and clearly report genuine conflicts (two manifest requirements that together admit no locked
  version and can't be resolved even after a fresh attempt) rather than picking one arbitrarily.
Test: hand-edit a manifest's version requirement to conflict with the existing lockfile, run a build,
and assert it either correctly re-resolves-and-reports or correctly errors with a clear
unsatisfiable-constraint message — never silently proceeds with a stale, now-invalid locked version.

### 23.5 `Cache::global_cache_dir` — unused, no shared dependency storage (`src/cache.rs`) — Low

Fixed as a side effect of §23.2's git-dependency caching and the prior report's §21.1 registry-package
caching — confirm both actually route through this shared directory (keyed appropriately: git
dependencies by URL+resolved-commit-hash subdirectory, registry packages by name+version) rather than
each inventing its own ad hoc cache location, so a single `glyip cache clean` command (add this, per
the prior report's §21.1) genuinely clears everything.

---

## 24. `glyim-test` (compiler's own testing infrastructure)

### 24.1 `TestExecutor::run_parallel` — no progress reporting during parallel execution (`harness/executor.rs`) — Low

Add a progress callback/channel: as each `rayon` task completes, send `(test_name, outcome)` through an
`mpsc` channel to a reporting thread that prints incremental progress (`"12/340 passed, 2 failed..."`)
rather than only the final `Vec` summary. This is a UX improvement for the compiler's own long-running
test suite, not a correctness fix — implement with a bounded channel to avoid unbounded memory growth
if results are produced faster than the reporter consumes them.

### 24.2 `ProgramRunner::run` — timeout kills the reporting thread, not the actual process (zombie risk) (`harness/runner.rs`) — Low but real

Fix: on timeout, explicitly kill the *child process* (via its `Child` handle's `.kill()`, or, on Unix,
send `SIGKILL` to the process group if the test spawned further children that could otherwise survive
as orphans — spawn test subprocesses in their own process group specifically so a group-kill can clean
up any descendants too), *then* join/abandon the monitoring thread, rather than only signaling the
thread and leaving the actual OS process running. This is exactly the same underlying fix as `glyip`'s
test runner needing real process isolation (§23.1) — share the "spawn in own process group, kill on
timeout, reap the zombie via `wait()`" helper between `glyim-test` and `glyip` rather than
implementing it twice. Test: a deliberately-hanging test program with a short configured timeout —
assert (via a platform process-listing check, e.g. `/proc` on Linux) that no process from the test
remains alive after the timeout fires and the test harness reports the timeout.

### 24.3 `snapshot_mir` — no type info or debug info in snapshots (`snapshot/mod.rs`) — Low

Add an opt-in verbosity flag (`--snapshot-verbose`) that includes each place's resolved type
(annotated inline, e.g. `_1: i32 = ...`) and, where present, the originating debug-info variable name
for each local (`_1 /* x */`) — keep the default snapshot format terse (current behavior) for
readability in the common case, since most snapshot tests care about control-flow/operation shape, not
full type annotations, but make the richer format available for debugging snapshot-test failures that
are specifically type-related.

---

## 25. Summary — High-Severity Landing Order

Land these six in this order (each unblocks or is directly required by the next):

1. **`glyim-solve::prove_trait`** built-in/auto-trait support (§8.1) — unblocks generic code broadly.
2. **`glyim-typeck::check_expr` method resolution** via the solver + global method table (§9.1) —
   directly depends on #1.
3. **`glyim-pipeline::pipeline_context::hir_body`** real implementation (§16.2) — unblocks `const {
   }` blocks, depends on `glyim-db`'s query system (§3.1) landing first.
4. **`glyim-pipeline::mono_cache::generate_drop_glue`** array/slice element loop (§16.1) — a pure
   correctness fix, independent of the above three but should land before broad integration testing
   since leaked/skipped drops will otherwise mask other bugs in end-to-end tests.
5. **`glyip::cmd_test`** real execution (§23.1) — depends on #3/#4 being fixed first, since the test
   suite verifying all of this plan's other fixes needs a *working test runner* to actually prove
   anything; treat this as a bootstrapping dependency for the plan's own verification, not just
   another checklist item.
6. **`glyip::DependencyResolver`** git/path support + full semver (§23.2) — needed to build any
   multi-crate/external-dependency integration test used to verify the cross-crate-sensitive fixes
   throughout this plan (§7.1's cross-crate `needs_drop`, §9.1's cross-crate method resolution, etc.).

Everything else in this document can be parallelized across engineers/agents once these six are in,
each verified by its own test before being marked complete — per §0's rule, no item is done until its
test passes, and the test must exercise the *behavior*, not just the absence of a panic.
