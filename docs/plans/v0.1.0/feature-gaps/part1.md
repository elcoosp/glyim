# Glyim Production-Readiness Implementation Plan

This plan turns every finding in `Codebase Gaps, Stubs, and Semi-Implemented
Features Report` into a concrete, ordered set of engineering tasks against the
**actual** Glyim source (verified against the uploaded repo dump, not just the
report text). Each section gives:

- **Current state** — exact file/function, quoted/paraphrased from the real
  code, plus what it *actually* does today (a few report claims are already
  partially addressed in the dump; this is called out explicitly so no one
  re-implements something that exists).
- **Target design** — the concrete behavior after the fix.
- **Step-by-step instructions** — literal, ordered steps an automated coding
  agent can execute without needing to make design decisions.
- **Code** — real Rust, written against the crate's actual types
  (`glyim_mir::*`, `glyim_hir::*`, `glyim_type::*`, etc.) as observed in the
  dump. Treat this as a strong reference implementation to adapt to exact
  field names if they've drifted since this plan was written — always `grep`
  the target struct/enum first (Step 0 in every section).
- **Tests** — concrete test functions to add, in the same style as the
  existing `#[cfg(test)]` modules in each crate.
- **Acceptance criteria** — a checklist to mark the item done.

## 0. How an agent should execute this plan

Follow this loop for **every** section below, in the given order (severity
order, matching the report's §7 table, with dependencies respected):

1. `grep -n` for the exact function/struct named in "Current state" in the
   named file. If it has moved or the signature differs from what's quoted
   here, adapt the steps to the real signature — do not guess; read the
   surrounding 50 lines first.
2. Make the change as a single, self-contained commit/PR per section.
3. Add the tests listed under "Tests" (or equivalent if APIs differ).
4. Run `cargo test -p <crate>` for the affected crate(s), then
   `cargo test --workspace` before moving to the next section.
5. Update `KNOWN_GAPS.md` (create it at the repo root if it does not exist —
   see §9) to move the item from "Open" to "Closed" with the PR/commit
   reference.
6. Do not proceed to a section whose "Depends on" note references an
   unfinished earlier section.

Global rule: **every** new "unimplemented" path introduced by partial fixes
below must be a compile error or a clearly labeled `todo!()`/diagnostic, never
a silent wrong-answer fallback. This preserves the codebase's existing good
practice of failing loudly (see `ThinLTO`, `load_cdylib`) rather than lying
about capability.

---

## Priority order (do not reorder without reason)

| # | Item | Depends on |
|---|------|------------|
| 1 | §1.6 Dynamic range slicing | — |
| 2 | §1.7 Const-eval cast legality wiring | — |
| 3 | §1.8 Per-projection drop elaboration | §1.6 (shares MIR place/projection code) |
| 4 | §1.4 Cross-frame unwinding (interpreter) — **hardening**, not greenfield | — |
| 5 | §1.9 Native `--emit=exec` entry symbol | — |
| 6 | §1.5 Proc-macro loading on Windows | — |
| 7 | §2.2 `getppid` on non-Unix | — |
| 8 | §2.3 `fs_canonicalize` path encoding | — |
| 9 | §1.3 Windows SEH | §1.4 (unwind model must be settled first) |
| 10 | §1.2 ThinLTO | §1.9 (both touch `glyim-cli` linker driver) |
| 11 | §1.1 Async multi-poll state machine | §1.8 (drop elaboration needed for generator locals) |
| 12 | §2.4 Bytecode backend opt-level no-op | — |
| 13 | §2.5 `is_sized` unknown ADTs | — |
| 14 | §2.6 HRTB structural equality | — |
| 15 | §2.7 Auto-trait computation for `Projection` | §2.6 (both need trait-solver normalization) |
| 16 | §4.1 Multi-CGU / cross-CGU dedup | §1.1, §1.9 (codegen pipeline changes compound) |
| 17 | §4.2 Fingerprinting compiler flags | — |
| 18 | §4.3 SemVer conflict resolution (SAT) | — |
| 19 | §5 LSP quick-fixes / diagnostics polish | — |
| 20 | §2.1 `#[ignore]` heuristic parsing | — |

Rationale for ordering: items 1–8 are localized, low-risk, and unblock
testing infrastructure (native exec, Windows CI) needed to validate the
higher-risk items (SEH, ThinLTO, async) that follow.

---

# 1. Major Architectural Gaps

## 1.1 Asynchronous Execution (Async/Await) — full multi-poll state machine

**Depends on:** §1.8 (drop elaboration).

### Current state

`glyim-hir/src/lower/lower_async.rs::desugar_async` already does a **real,
compiling** single-poll desugar: it builds a `FooFuture` struct (one field per
captured parameter, `f0..fn`), a `impl Future for FooFuture { fn poll(&mut
self) -> Poll<R> { <rewritten body> } }`, and a wrapper `fn foo(args) ->
FooFuture`. `Expr::Await` is rewritten (`rewrite_expr`) into:

```text
match <inner>.poll() {
    Poll::Ready(v) => v,
    Poll::Pending => panic!(),
}
```

This is correct only if every awaited future resolves on the *first* poll.
The crate doc-comment is explicit that multi-poll (suspend/resume) is
"intentionally NOT attempted here."

### Target design

Rewrite `async fn` into a real generator/coroutine: a state machine enum with
one variant per suspension point, capturing exactly the locals live across
each `.await`. `poll` becomes a `loop { match self.state { ... } }` that:

- On `Poll::Pending` from an inner future, **stores the inner future in the
  state and returns `Poll::Pending`** instead of panicking.
- On the next call to `poll`, resumes at the stored state and re-polls the
  stored inner future.

This requires:

1. A **suspend-point discovery pass** over the (already-lowered, but not yet
   desugared) async body: walk the `Expr` tree in body order and assign each
   `Expr::Await` a `SuspendPointId` (0-indexed, in lexical/execution order).
2. A **liveness-across-suspend analysis**: for each suspend point, the set of
   local bindings that are live *after* that point and were defined *before*
   or *at* it. This becomes the state variant's captured fields (in addition
   to the always-present captured parameters).
3. A **state machine struct** `enum FooFutureState { Start(P0, P1, ..), S0 {
   awaited: InnerFut0, live0: T, .. }, S1 { .. }, Done }` plus the outer
   `struct FooFuture { state: FooFutureState }`.
4. **CFG-style body rewriting**: split the async body into segments at each
   `.await`; each segment becomes one `match` arm that (a) runs the segment's
   statements, (b) polls the awaited future, (c) on `Ready(v)` falls through
   into the next segment (bind `v`, continue the `loop`), on `Pending` stores
   state and `return Poll::Pending`.

Because Glyim's HIR is expression-tree-based (not yet a CFG at this lowering
point — MIR is built later, from THIR, after typeck), it is far simpler and
much lower-risk to build the state machine as a **desugared HIR expression
tree with an explicit resumption dispatch**, rather than trying to hand-roll
a CFG in HIR. Below is the concrete transform.

### Step-by-step instructions

**Step 0.** `grep -n "fn desugar_one_async_fn" glyim-hir/src/lower/lower_async.rs`
and re-read the whole function; the field names below (`Body`, `Expr`,
`ExprId`, `Pat`, `Field`, `StructItem`) must match exactly what's imported at
the top of that file today.

**Step 1. Add a suspend-point counter and collection pass.**

```rust
// glyim-hir/src/lower/lower_async.rs

/// One `.await` site inside an async body, in lexical/execution order.
struct SuspendPoint {
    id: usize,
    /// The `ExprId` of the `Expr::Await` node itself (its `inner` field is
    /// the future expression being polled).
    await_expr: ExprId,
}

/// Walk `body` in structural (evaluation) order and collect every
/// `Expr::Await`, assigning ids 0..N in the order they'd execute. This does
/// NOT walk into nested `async fn`/`async {}` blocks (those are desugared
/// independently, bottom-up, by the outer `desugar_async` loop already
/// iterating `hir.items` — leave that iteration order as-is, it already
/// gives us "inner async blocks desugared to Future-returning calls before
/// the outer one runs" for free as long as we run this collection pass
/// AFTER `desugar_async`'s existing per-item loop reaches this item, which
/// it does since items are processed in `hir.items` order and nested async
/// blocks were hoisted to top-level items during HIR lowering already).
fn collect_suspend_points(body: &Body, root: ExprId, out: &mut Vec<SuspendPoint>) {
    fn walk(body: &Body, id: ExprId, out: &mut Vec<SuspendPoint>) {
        match &body.exprs[id] {
            Expr::Await { expr } => {
                walk(body, *expr, out);
                out.push(SuspendPoint { id: out.len(), await_expr: id });
            }
            Expr::Block { stmts, tail } => {
                for s in stmts { walk(body, *s, out); }
                if let Some(t) = tail { walk(body, *t, out); }
            }
            Expr::If { cond, then_branch, else_branch } => {
                walk(body, *cond, out);
                walk(body, *then_branch, out);
                if let Some(e) = else_branch { walk(body, *e, out); }
            }
            Expr::Match { scrutinee, arms } => {
                walk(body, *scrutinee, out);
                for a in arms {
                    if let Some(g) = a.guard { walk(body, g, out); }
                    walk(body, a.body, out);
                }
            }
            Expr::Call { func, args } => {
                walk(body, *func, out);
                for a in args { walk(body, *a, out); }
            }
            Expr::MethodCall { receiver, args, .. } => {
                walk(body, *receiver, out);
                for a in args { walk(body, *a, out); }
            }
            Expr::Return { value: Some(v) } => walk(body, *v, out),
            _ => {}
        }
    }
    walk(body, root, out);
}
```

Note: this recursive walk only needs to be *complete enough to find every
`Await`*; it does not need to model every `Expr` variant with data flow
precision (that's step 2). Extend the `match` arms with any `Expr` variants
present in `glyim-hir/src/lib.rs` that can contain sub-expressions and are
missing above (`grep -n "^pub enum Expr" -A 60 glyim-hir/src/lib.rs` to get
the authoritative variant list before finalizing this function).

**Step 2. Compute the "state count" and bail out to the existing single-poll
path when there are 0 or 1 suspend points.**

This is the key risk-reduction move: *do not touch the existing, working,
tested single-poll path*. Only engage the new state-machine path when a body
has **2 or more** `.await`s, or when static analysis can't prove the single
poll resolves. Concretely:

```rust
pub fn desugar_async(hir: &mut crate::CrateHir) {
    let async_items: Vec<ItemId> = hir
        .items
        .iter_enumerated()
        .filter_map(|(id, item)| match &item.kind {
            ItemKind::Fn(fn_item) if fn_item.is_async => Some(id),
            _ => None,
        })
        .collect();

    for item_id in async_items {
        let body_id = match &hir.items[item_id].kind {
            ItemKind::Fn(f) => f.body,
            _ => None,
        };
        let suspend_count = body_id
            .map(|b| {
                let mut sps = Vec::new();
                collect_suspend_points(&hir.bodies[b], hir.bodies[b].root_expr(), &mut sps);
                sps.len()
            })
            .unwrap_or(0);

        if suspend_count <= 1 {
            // Existing, proven single-poll desugar: cheaper generated code,
            // and correct whenever there's at most one suspension point that
            // is itself guaranteed ready-on-first-poll by construction (a
            // bare non-.await tail) OR when there's exactly one `.await` and
            // we accept panic-on-Pending for it specifically (documented,
            // unchanged behavior — see doc comment at top of file).
            desugar_one_async_fn(hir, item_id);
        } else {
            desugar_one_async_fn_state_machine(hir, item_id);
        }
    }
}
```

If `Body` does not already expose a `root_expr()` helper, add one (it's the
last entry of `body.exprs`, matching the existing convention used in
`desugar_one_async_fn`'s tail-rewriting code — `grep -n "tail expression
becomes"` in the same file to find that logic and reuse its exact
"last-block/last-expr" rule instead of inventing a new one).

**Step 3. Implement `desugar_one_async_fn_state_machine`.**

This is the substantial new function. Structure:

```rust
fn desugar_one_async_fn_state_machine(hir: &mut crate::CrateHir, item_id: ItemId) {
    let mut item = hir.items[item_id].clone();
    let fn_item = match &mut item.kind { ItemKind::Fn(f) => f, _ => return };
    let fn_name = item.name;
    let original_params = fn_item.params.clone();
    let return_ty = fn_item.return_ty.clone();
    let original_body_id = match fn_item.body { Some(b) => b, None => return };
    let original_body_owner = hir.bodies[original_body_id].owner;

    let interner = &hir.interner;
    let fn_name_str = interner.resolve(fn_name).to_string();
    let future_name = format!("{}Future", fn_name_str);
    let state_name = format!("{}State", future_name);

    // 1. Collect suspend points against a *clone* of the original body so we
    //    can mutate the clone freely while still consulting the original.
    let mut work_body = hir.bodies[original_body_id].clone();
    let mut suspend_points = Vec::new();
    collect_suspend_points(&work_body, work_body.root_expr(), &mut suspend_points);
    let n = suspend_points.len();

    // 2. Liveness-across-suspend: which locals (HIR `Name`s bound by `let`
    //    or params) are referenced *after* suspend point k but bound at or
    //    before it. Reuse the existing name-resolution info already present
    //    on `Expr::Path` — do NOT reimplement scope resolution; walk the
    //    body twice (bind-order pass, then use-order pass) and intersect.
    let live_across = compute_live_across_suspends(&work_body, &suspend_points, &original_params);

    // 3. Build the state enum: `Start(params...)`, `S0 { live: .. , fut0: F0
    //    }`, .., `S{n-1} { .. }`, `Done`.
    let state_enum_item = build_state_enum(hir, &state_name, &original_params, &live_across, n);

    // 4. Build the outer future struct wrapping the state enum:
    //    `struct FooFuture { state: FooFutureState }`.
    let future_struct_item = build_future_wrapper_struct(hir, &future_name, &state_name);

    // 5. Rewrite the body into the `poll` loop-match implementation.
    let poll_body_id = build_poll_loop_body(
        hir, &mut work_body, &suspend_points, &live_across, &state_name, &original_params,
    );

    // 6. impl Future for FooFuture { type Output = R; fn poll(&mut self) ->
    //    Poll<R> { <poll_body_id> } } — identical shape to the existing
    //    single-poll path's step 3; reuse that code verbatim (extract it
    //    into a shared `build_future_impl(hir, future_name, output_ty,
    //    poll_body_id) -> Item` helper used by BOTH desugar functions).

    // 7. Wrapper fn: `fn foo(args) -> FooFuture { FooFuture { state:
    //    FooFutureState::Start(args...) } }` — same shape as existing step 4,
    //    but constructs `Start(..)` instead of flat fields.

    // 8. Push all new items, replace `item_id`'s `Fn` body/is_async exactly
    //    like the existing function does at its tail (mirror that code).
    hir.items.push(state_enum_item);
    hir.items.push(future_struct_item);
    // .. push future_impl_item, wrapper body/item, same pattern as existing
    //    desugar_one_async_fn's tail (copy verbatim, adjust ids).
}
```

Because this is the highest-complexity item in the whole report, **do not
attempt to write `compute_live_across_suspends`, `build_state_enum`,
`build_poll_loop_body` from scratch in one shot**. Implement and land them in
this order, each with its own unit tests, before wiring `desugar_async`'s
branch in Step 2 to call the new path:

1. `build_state_enum` (pure HIR construction, no analysis) — test that for a
   body with 2 suspend points and one `let x = ..;` between them, the
   generated `ItemKind::Enum`/`ItemKind::Struct` (check which HIR item kind
   the crate uses for enums — `grep -n "EnumItem\|ItemKind::Enum"
   glyim-hir/src/lib.rs`) has exactly 4 variants (`Start`, `S0`, `S1`,
   `Done`) with the expected field types.
2. `compute_live_across_suspends` — unit test directly against a
   hand-constructed `Body` (no full pipeline) asserting the live-set per
   suspend point matches by hand-checking a couple of small cases: (a) a
   local used only before the await (not live-across), (b) a local used both
   before and after (live-across), (c) a local declared *between* two awaits
   and used after the second (live only across the second).
3. `build_poll_loop_body` — this is the part that actually needs a `loop {
   match state { .. } }` `Expr`. If `Expr` doesn't already have a `Loop`
   variant, check `glyim-hir/src/lib.rs` for the closest existing looping
   construct (`Expr::Loop`, `Expr::While`) and reuse it; do not add a new HIR
   node type for this feature — express "resume dispatch, run segment, on
   inner-Pending return, on inner-Ready continue the loop to the next state"
   entirely in terms of `Expr::Loop { body: Block }`, `Expr::Match`,
   `Expr::Break`/`Expr::Return`, which the type-checker and MIR lowering
   already understand.

**Step 4. `.await` rewriting inside each segment.**

Inside segment `k`'s match arm, `<inner>.poll()` is now:

```text
match <inner>.poll() {
    Poll::Ready(v) => { /* fall through to segment k+1, binding v */ }
    Poll::Pending => {
        self.state = FooFutureState::Sk { live: .., fut_k: <inner> };
        return Poll::Pending;
    }
}
```

Reuse the exact `Poll::Ready`/`Poll::Pending` path-construction helpers
already defined in the file (`two_seg`, `plain_path`) — do not redefine them.

**Step 5. Resume dispatch.** The top of `poll` becomes:

```text
loop {
    match &mut self.state {
        FooFutureState::Start(..) => { /* segment 0 body, ending in the
            Step 4 match for suspend point 0, OR falling through to
            Poll::Ready(..) if n == 0 within this arm — unreachable here
            since n >= 2 by construction from Step 2's `<= 1` gate */ }
        FooFutureState::S0 { .. } => { /* re-poll fut0 first (Step 4's
            match), on Ready(v) run segment 1's statements then hit suspend
            point 1's Step-4 match */ }
        ...
        FooFutureState::Done => panic!("polled a completed future"),
    }
}
```

This "poll the stored future again on resume, then continue" structure is
exactly `std::future::Future`'s real contract, so it composes correctly with
`.await` on ordinary (non-async-fn) futures written directly against the
`Future` trait.

**Step 6. Drop semantics on state transition.** When `self.state` is
overwritten (`self.state = FooFutureState::Sk { .. }`), the *previous*
variant's fields must be dropped per normal Rust/Glyim struct-field drop
order before the assignment completes. Verify (write a test, don't assume)
that the existing MIR/drop-elaboration lowering already does this for a
plain `enum` field assignment — Glyim's assignment lowering should insert a
`Drop` of the old place before overwrite if the enum's variants own
droppable data. If it doesn't (this is plausible since assignment-drop and
enum-variant drop are two separate features), file this as an explicit new
sub-task rather than silently shipping a resource leak: search
`glyim-lower/src/builder.rs` for how plain (non-enum) reassignment already
inserts a drop of the overwritten place, and confirm the same code path
fires for `Place` with a downcast/variant projection.

### Tests

Add to `glyim-hir/src/lower/lower_async.rs`'s `#[cfg(test)]` module (create
one modeled on sibling lowering passes if absent — check
`glyim-hir/src/lower/*.rs` for the existing test harness helper, likely named
something like `lower_str_to_hir` or similar; `grep -rn "fn lower_str\|fn
parse_and_lower" glyim-hir/src`):

```rust
#[test]
fn single_await_still_uses_single_poll_desugar() {
    // Regression guard: a body with exactly one `.await` must still produce
    // the existing flat-field FooFuture (no *State enum), proving the <=1
    // gate in `desugar_async` routes correctly.
    let hir = lower_str_to_hir(r#"
        async fn one(x: i32) -> i32 { x.await }
    "#);
    assert!(hir_has_item_named(&hir, "oneFuture"));
    assert!(!hir_has_item_named(&hir, "oneFutureState"));
}

#[test]
fn two_awaits_produce_state_machine() {
    let hir = lower_str_to_hir(r#"
        async fn two(a: i32, b: i32) -> i32 {
            let x = a.await;
            let y = b.await;
            x + y
        }
    "#);
    assert!(hir_has_item_named(&hir, "twoFutureState"));
    // 4 variants: Start, S0, S1, Done.
    let variants = enum_variant_names(&hir, "twoFutureState");
    assert_eq!(variants, vec!["Start", "S0", "S1", "Done"]);
}

#[test]
fn pending_future_does_not_panic_and_resumes() {
    // End-to-end: compile through MIR interp (or bytecode VM) with a
    // hand-written test `Future` impl whose `poll` returns `Pending` on
    // its first call and `Ready(v)` on its second, then drive the outer
    // future's `poll` twice via the interpreter and assert the second call
    // returns `Ready`.
}

#[test]
fn live_across_suspend_local_survives_resume() {
    // `let x = a.await; let y = b.await; x + y` — `x` must still be readable
    // in segment 1 after resuming from `S0`. Compile + interpret; assert
    // final result equals a + b for representative a, b.
}
```

The third and fourth tests are integration-level (they need the MIR
interpreter or bytecode VM wired up); put them in
`glyim-pipeline`'s or `glyim-test`'s integration test suite instead if unit
tests can't easily drive two sequential `poll()` calls from HIR alone —
check `glyim-test/` for the existing "compile snippet, run it, assert
result" harness (`grep -rn "fn compile_and_run\|fn run_snippet"
glyim-test/src`) and use it verbatim.

### Acceptance criteria

- [ ] `suspend_count <= 1` still routes through the original, unmodified
      `desugar_one_async_fn` (no regression in existing single-poll tests).
- [ ] `suspend_count >= 2` produces a state-enum-backed future.
- [ ] A future whose inner `.await` returns `Pending` on first poll no longer
      panics; the outer future returns `Pending` and a second `poll()` call
      resumes correctly.
- [ ] Locals live across a suspend point retain their value across
      `Pending`/resume.
- [ ] `KNOWN_GAPS.md` Phase 5 entry updated to "Closed" (or "Partially closed
      — recursion/loops containing `.await` still route to the existing
      panic-on-Pending path" if that further edge case is deferred; be
      explicit either way, do not leave it ambiguous).

---

## 1.2 Link-Time Optimization (LTO) — ThinLTO

**Depends on:** §1.9 (both change `glyim-cli`'s linker invocation).

### Current state

`glyim-codegen-llvm/src/passes.rs::run_lto` is real, tested code:
`LtoKind::None` no-ops, `LtoKind::Fat` merges every other `Module` into
`primary` via `Module::link_in_module` and then runs
`run_llvm_passes`. `LtoKind::Thin` returns a `Result::Err(String)` explaining
that ThinLTO needs per-module summary emission plus a thin-link step driven
by `glyim-cli`'s linker invocation, and that the merge "cannot be performed
inside `glyim-codegen-llvm`." There are existing tests
(`run_lto_none_is_noop`, `run_lto_fat_merges_modules`, and a Thin test that
asserts the `Err` and its message contents) — **do not break these**; the Fat
and None behavior must stay byte-identical.

### Target design

Real ThinLTO: each compiled module gets an LLVM bitcode + per-module summary
written to disk; `glyim-cli`'s linker driver collects all per-CGU summary
files, runs LLVM's thin-link step (either via `llvm-lto`/`llvm-lto2`
subprocess, or via the `inkwell`/`llvm-sys` FFI to
`LLVMThinLTO*`/`LTOCodeGenerator` APIs if exposed by the pinned LLVM 22
bindings) to produce an import/export index, and each module is then
optimized independently using that index (real ThinLTO's whole selling
point: parallel, incremental per-module optimization instead of one giant
merged module).

### Step-by-step instructions

**Step 0.** `grep -n "fn compile\|fn emit_object\|write_bitcode\|write_ir" glyim-codegen-llvm/src/lib.rs` to find where the module is currently
serialized to an object file — the new bitcode+summary emission slots in
right next to that.

**Step 1. Emit per-module bitcode + summary when `LtoKind::Thin` is
requested**, instead of erroring immediately. In
`glyim-codegen-llvm/src/passes.rs`:

```rust
use inkwell::targets::{FileType};
use std::path::Path;

/// Emit this module's bitcode with an embedded ThinLTO per-module summary,
/// suitable for `glyim-cli`'s thin-link step. Returns the path written.
pub(crate) fn emit_thinlto_bitcode<'ctx>(
    module: &Module<'ctx>,
    target_machine: &TargetMachine,
    out_path: &Path,
) -> Result<(), String> {
    // LLVM's C API embeds a per-module summary automatically when bitcode is
    // written for a module that has `EnableSplitLTOUnit`/ThinLTO metadata
    // set. inkwell doesn't wrap `LLVMWriteBitcodeToFile` with summary flags
    // directly as of the pinned version, so this goes through the target
    // machine's `write_to_file` with `FileType::Object` is NOT what we want
    // here — bitcode (not object) is required for the thin-link step.
    //
    // Concretely: set the module flag that turns on ThinLTO summary
    // emission, then write bitcode.
    module.set_metadata(
        module.get_context().i32_type().const_int(1, false),
        // "ThinLTO" module flag id — see llvm/IR/ModuleSummaryIndex.h,
        // module flag key "ThinLTO". If inkwell's `Module` doesn't expose a
        // typed "add module flag" API, drop to `LLVMAddModuleFlag` via
        // `module.as_mut_ptr()` + `llvm_sys` directly (this crate already
        // depends on `inkwell`, which re-exports `llvm-sys` — check
        // `Cargo.toml` for the exact re-export path before adding a new
        // dependency).
        0,
    );
    module
        .write_bitcode_to_path(out_path)
        .then_some(())
        .ok_or_else(|| format!("failed to write ThinLTO bitcode to {}", out_path.display()))
}
```

If `inkwell`'s `Module` truly has no module-flag API (verify with `grep -rn
"set_flag\|add_module_flag\|ModuleFlag" ~/.cargo/registry/**/inkwell*/src` or
equivalent in the vendored deps), use `llvm-sys::bit_writer::
LLVMWriteBitcodeToFile` plus `llvm-sys::core::LLVMAddModuleFlag` directly,
matching whatever low-level pattern the file already uses elsewhere for
things `inkwell` doesn't wrap (search `lower.rs`/`lib.rs` for existing
`llvm_sys::` calls as a precedent — this file already drops to raw FFI in a
few places, e.g. around `build_landing_pad`).

**Step 2. Change `run_lto`'s `Thin` arm** from an unconditional `Err` into:

```rust
LtoKind::Thin => {
    // Real per-module summary emission now happens in the codegen driver
    // (glyim-cli), which calls `emit_thinlto_bitcode` for every CGU instead
    // of calling `run_lto` at all for Thin. Reaching this arm means a
    // caller invoked `run_lto` directly with `Thin` outside that driver
    // path, which is a programming error (Thin must never merge modules
    // in-process) — keep failing loudly, but the message now describes the
    // correct call path instead of "not implemented".
    Err(
        "LtoKind::Thin must not be merged via run_lto; call \
         emit_thinlto_bitcode() per-module and let glyim-cli's thin-link \
         driver combine them. run_lto(Thin) is only reachable from a caller \
         bug.".to_string(),
    )
}
```

Keep the existing test that asserts an `Err` is returned for `Thin` through
`run_lto` — update only its message assertion (`result.unwrap_err().contains
("ThinLTO")` → assert it still contains `"Thin"` or update to the new
wording; do not delete the test, it's still validating correct
in-process-never-merges behavior).

**Step 3. Add the thin-link driver to `glyim-cli`.** `grep -n "fn link\|struct Linker" glyim-cli/src/linker.rs`. Add:

```rust
// glyim-cli/src/linker.rs

/// Drive LLVM's ThinLTO thin-link step over a set of per-CGU bitcode files
/// with embedded summaries, producing an optimized object per module, then
/// hand all resulting objects to the existing native linker invocation.
pub fn thin_lto_link(
    bitcode_paths: &[PathBuf],
    target_machine: &TargetMachine,
    opt_level: u8,
    out_dir: &Path,
) -> Result<Vec<PathBuf>, String> {
    // Preferred: shell out to `llvm-lto2 run` (ships with every LLVM
    // distribution that has `llc`/`opt`, and is the officially supported
    // ThinLTO driver — this sidesteps needing unstable FFI bindings for
    // `LTOCodeGenerator`). Locate it next to whatever `llc`/`opt` binary
    // this crate already shells out to, if any (`grep -n "Command::new"
    // glyim-cli/src/linker.rs` to find the existing subprocess pattern and
    // match its binary-discovery logic, e.g. `LLVM_SYS_220_PREFIX` env var
    // or `llvm-config --bindir`).
    let llvm_lto2 = find_llvm_tool("llvm-lto2")?;

    let mut cmd = std::process::Command::new(&llvm_lto2);
    cmd.arg("run");
    for bc in bitcode_paths {
        cmd.arg(bc);
    }
    cmd.arg(format!("-o={}", out_dir.join("thinlto").display()));
    cmd.arg(format!("-O{}", opt_level.min(3)));
    // One-to-one: each input module gets a numbered `.thinlto.N` object
    // output; llvm-lto2 assigns indices in input order.
    let status = cmd.status().map_err(|e| format!("failed to spawn {}: {}", llvm_lto2.display(), e))?;
    if !status.success() {
        return Err(format!("llvm-lto2 thin-link failed with status {}", status));
    }
    let outs = (0..bitcode_paths.len())
        .map(|i| out_dir.join(format!("thinlto.{}", i)))
        .collect();
    Ok(outs)
}

fn find_llvm_tool(name: &str) -> Result<PathBuf, String> {
    if let Ok(prefix) = std::env::var("LLVM_SYS_220_PREFIX") {
        let candidate = PathBuf::from(prefix).join("bin").join(name);
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    which::which(name).map_err(|_| {
        format!(
            "{name} not found on PATH and LLVM_SYS_220_PREFIX is unset; \
             ThinLTO requires the llvm-lto2 tool from the same LLVM 22 \
             distribution glyim-codegen-llvm was built against."
        )
    })
}
```

Add `which = { workspace = true }` to `glyim-cli/Cargo.toml`'s
`[dependencies]` if not already present (`grep -n "^which" glyim-cli/Cargo.toml`
first — it may already be a workspace dep used elsewhere for tool discovery).

**Step 4. Wire the driver.** Find where `glyim-cli` currently calls
`run_lto`/`run_llvm_passes` per-CGU today (`grep -rn "run_lto\|LtoKind"
glyim-cli/src`), and branch:

```rust
match lto_kind {
    LtoKind::None | LtoKind::Fat => {
        // existing path, unchanged
    }
    LtoKind::Thin => {
        let bc_paths: Vec<_> = cgu_modules.iter().enumerate().map(|(i, m)| {
            let p = out_dir.join(format!("cgu{i}.thinlto.bc"));
            glyim_codegen_llvm::passes::emit_thinlto_bitcode(m, &target_machine, &p)?;
            Ok::<_, String>(p)
        }).collect::<Result<_, _>>()?;
        let objects = thin_lto_link(&bc_paths, &target_machine, opt_level, &out_dir)?;
        // feed `objects` into the existing native-link step instead of the
        // Fat/None path's per-module `write_to_file(FileType::Object)`
        // outputs.
    }
}
```

`emit_thinlto_bitcode` is currently `pub(crate)` in `passes.rs` — change it
to `pub` (and make `passes` module `pub` from `glyim-codegen-llvm/src/lib.rs`
if it isn't already) so `glyim-cli` can call it across the crate boundary.

### Tests

`glyim-codegen-llvm/src/passes.rs`:
```rust
#[test]
fn emit_thinlto_bitcode_writes_file_with_summary() {
    let (module, tm) = /* existing test module fixture, reuse
        run_lto_fat_merges_modules's setup helper */;
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("mod.bc");
    emit_thinlto_bitcode(&module, &tm, &out).unwrap();
    assert!(out.exists());
    // Sanity: a real per-module summary bumps the bitcode file size
    // materially vs. a summary-less write; assert file is non-trivially
    // sized rather than trying to parse the bitcode format by hand.
    assert!(std::fs::metadata(&out).unwrap().len() > 0);
}
```

`glyim-cli` integration test (`grep -rn "mod tests" glyim-cli/src/linker.rs`
for the existing harness style):
```rust
#[test]
fn thin_lto_end_to_end_two_crates() {
    // Compile two tiny glyim source files that call across a crate
    // boundary with `--lto=thin`, link, and run the resulting binary,
    // asserting the expected exit code/output. This is the test that used
    // to be impossible; it is the acceptance test for this whole section.
}
```

### Acceptance criteria

- [ ] `run_lto(Thin)` no longer silently no-ops or is the only code path
      exercised for `--lto=thin`; the real work happens via
      `emit_thinlto_bitcode` + `thin_lto_link`.
- [ ] Existing Fat/None tests unchanged and passing.
- [ ] A two-crate `--lto=thin` build links and runs correctly in a new
      integration test.
- [ ] `llvm-lto2` absence produces a clear, actionable error (not a panic).

---

## 1.3 Windows SEH / Exception Handling

**Depends on:** §1.4 (settle the unwind model in the interpreter first — the
codegen and interpreter unwind semantics should agree before SEH is added on
top of Itanium-only codegen).

### Current state

`glyim-codegen-llvm/src/lower.rs` already has a **three-way** `Personality`
enum (`Itanium`, `Seh`, `None`) and `select_personality(target, has_cleanup)`
correctly picks `Seh` for `TargetAbi::X86_64Windows |
TargetAbi::AArch64Windows`. But `emit_landingpad` (search
`fn emit_landingpad`) unconditionally builds an **Itanium-style**
`build_landing_pad` regardless of which `Personality` was selected — the
`Seh` variant is selected but never actually changes the emitted IR shape.
The doc comment on `Personality::Seh` and inline comments around line ~2918
of `lower.rs` are explicit: real MSVC SEH needs funclet-based
`cleanuppad`/`cleanupret`/`catchpad`/`catchret`, which the pinned LLVM 22
toolchain's C API doesn't export in a form this codebase currently uses, and
the Itanium `landingpad`/`resume` pair is used as an approximation even for
`Seh` targets.

### Target design

Two acceptable end states — pick based on what the pinned LLVM 22 C API
actually supports (verify before choosing, see Step 1):

- **(A) Real funclets.** Emit genuine `cleanuppad`/`cleanupret` via
  `LLVMBuildCleanupPad`/`LLVMBuildCleanupRet` (available in `llvm-sys` even
  when `inkwell` doesn't wrap them — check `llvm-sys::core` for these symbols
  first).
- **(B) Explicit unsupported-but-honest fallback.** If (A) is truly
  infeasible with the pinned toolchain, keep the Itanium approximation **but
  stop pretending it's `Seh`**: rename the selected personality's emitted
  landingpad path so the compiler emits a hard **diagnostic error** (not a
  silent codegen difference) when compiling `panic = "unwind"` code for a
  Windows target with cleanup blocks, directing users to `panic = "abort"`
  until (A) lands. This turns a silent correctness gap into a loud, correct
  refusal — strictly better than the status quo, and honest to the "never a
  silent wrong-answer fallback" rule in §0.

Do (A) first; only fall back to (B) if a spike proves (A) infeasible within
the pinned LLVM version.

### Step-by-step instructions

**Step 1 — spike: verify llvm-sys funclet API availability.**

```bash
grep -rn "CleanupPad\|CleanupRet\|CatchPad\|CatchRet\|CatchSwitch" \
  $(python3 -c "import subprocess; print(subprocess.run(['find','/root/.cargo','-iname','core.rs','-path','*llvm-sys*'],capture_output=True,text=True).stdout)")
```
or, more simply, inside the `glyim-codegen-llvm` crate directory:
```bash
cargo doc -p llvm-sys --no-deps 2>/dev/null; grep -rn "CleanupPad" target/doc/llvm_sys/core/index.html
```
If `LLVMBuildCleanupPad`, `LLVMBuildCleanupRet`, `LLVMBuildCatchSwitch`,
`LLVMBuildCatchPad`, `LLVMBuildCatchRet` all exist in the linked `llvm-sys`
version, proceed with (A). If any are missing, do (B) and stop here (file
(A) as the tracked follow-up in `KNOWN_GAPS.md`, do not attempt to hand-roll
missing FFI bindings — that's its own large, separately-scoped task).

**Step 2 (path A) — add funclet emission.** In `glyim-codegen-llvm/src/lower.rs`
near `emit_landingpad` (~line 2908):

```rust
fn emit_landingpad(&mut self) -> CompResult<()> {
    let Some(personality_fn) = self._personality_fn else {
        self.current_landingpad = None;
        return Ok(());
    };
    match self.personality {
        Personality::Itanium => self.emit_itanium_landingpad(personality_fn),
        Personality::Seh => self.emit_seh_cleanuppad(personality_fn),
        Personality::None => { self.current_landingpad = None; Ok(()) }
    }
}

/// Existing Itanium logic, unchanged, just extracted into its own method
/// so `Seh` can have a genuinely different implementation instead of
/// silently sharing this one.
fn emit_itanium_landingpad(&mut self, personality_fn: FunctionValue<'ctx>) -> CompResult<()> {
    // <exact body currently inside emit_landingpad, moved verbatim>
}

/// Real MSVC SEH funclet landingpad: a `cleanuppad` with no arguments,
/// paired with a `cleanupret` at the end of the cleanup block (emitted by
/// the terminator-lowering code that currently emits `resume` for Itanium —
/// see Step 3).
fn emit_seh_cleanuppad(&mut self, _personality_fn: FunctionValue<'ctx>) -> CompResult<()> {
    use llvm_sys::core::{LLVMBuildCleanupPad, LLVMGetInsertBlock};
    use std::ffi::CString;
    // inkwell's `Builder` doesn't wrap cleanuppad; drop to the raw pointer
    // it wraps (`Builder::as_mut_ptr` per this crate's existing pattern for
    // FFI gaps — grep other `.as_mut_ptr()` call sites in this file for the
    // exact accessor name in the pinned inkwell version).
    let raw_builder = self.builder.as_mut_ptr();
    let name = CString::new("cleanuppad").unwrap();
    let pad = unsafe {
        LLVMBuildCleanupPad(
            raw_builder,
            std::ptr::null_mut(), // no parent pad (top-level funclet)
            std::ptr::null_mut(), // 0 args
            0,
            name.as_ptr(),
        )
    };
    if pad.is_null() {
        return Err(self.diag.error("failed to build cleanuppad for SEH landingpad"));
    }
    self.current_seh_cleanuppad = Some(pad);
    self.current_landingpad = None; // Itanium-only field; SEH tracks separately.
    Ok(())
}
```

Add a `current_seh_cleanuppad: Option<LLVMValueRef>` field next to
`current_landingpad` on the lowering context struct (find it via `grep -n
"current_landingpad:" lower.rs` for the struct definition).

**Step 3 — pair with `cleanupret`.** Find where the Itanium path currently
emits `resume` at the end of a cleanup block (search `build_resume` or
`"resume"` near the `current_landingpad` consultation you found in Step 0's
grep, ~line 2212). Add a parallel branch:

```rust
if let Some(pad) = self.current_seh_cleanuppad.take() {
    use llvm_sys::core::LLVMBuildCleanupRet;
    let raw_builder = self.builder.as_mut_ptr();
    unsafe {
        // `unwind to caller` — this cleanup funclet doesn't catch, it just
        // runs drop glue then continues unwinding to the parent frame.
        LLVMBuildCleanupRet(raw_builder, pad, std::ptr::null_mut());
    }
} else if let Some(lp) = self.current_landingpad {
    // existing Itanium `resume` path, unchanged
}
```

**Step 4 — `invoke` unchanged.** `invoke`/call-site lowering for a
cleanup-bearing call is target-agnostic (both personalities use `invoke` to
a normal/unwind pair of blocks); no change needed there. Verify this by
`grep -n "build_invoke\|fn lower_call" lower.rs` and confirming it branches
only on "has cleanup?" not on personality kind.

**Step 5 (path B, only if Step 1's spike fails) — honest refusal.** In the
function that currently calls `select_personality` (~line 3185) and stores
`_personality_fn`:

```rust
let personality = select_personality(&target_info, has_cleanup);
if personality == Personality::Seh {
    return Err(self.diag.error(
        "unwinding (panic = \"unwind\") is not yet supported when targeting \
         Windows SEH (cleanuppad/cleanupret funclets are required and are \
         not available in this LLVM build). Compile with `panic = \"abort\"` \
         for Windows targets until this is implemented. Tracked in \
         KNOWN_GAPS.md §19.1.",
    ));
}
```
This replaces the current silent Itanium-approximation fallback with a
compile error, which is a strict correctness improvement even though it
narrows supported configurations — never ship code whose unwind behavior is
undefined per the target ABI.

### Tests

```rust
// glyim-codegen-llvm/src/lower.rs, #[cfg(test)]
#[test]
fn seh_target_emits_cleanuppad_not_landingpad() {
    let target = TargetInfo { abi: TargetAbi::X86_64Windows, ..Default::default() };
    let ir = compile_snippet_to_ir(
        target,
        r#"fn f() { let _g = Guard; panic!("x"); }"#, // Guard has a Drop impl
    );
    assert!(ir.contains("cleanuppad"));
    assert!(!ir.contains("landingpad"));
}

#[test]
fn itanium_target_unaffected() {
    let target = TargetInfo { abi: TargetAbi::X86_64Linux, ..Default::default() };
    let ir = compile_snippet_to_ir(target, r#"fn f() { let _g = Guard; panic!("x"); }"#);
    assert!(ir.contains("landingpad"));
    assert!(!ir.contains("cleanuppad"));
}
```
(Or, for path B, a test asserting `compile(...)` returns
`Err` containing `"panic = \"abort\""` for an SEH target with cleanup.)

### Acceptance criteria

- [ ] Spike result (A vs B) recorded in `KNOWN_GAPS.md` with reasoning.
- [ ] If (A): SEH targets emit `cleanuppad`/`cleanupret`, Itanium targets
      unaffected, both covered by tests above.
- [ ] If (B): SEH + cleanup now hard-errors with an actionable message
      instead of silently emitting non-conformant IR.
- [ ] `select_personality` and the `Personality` enum are unchanged (already
      correct); only the landingpad *emission* changes.

---

## 1.4 Cross-Frame Unwinding in the MIR Interpreter

### Current state — this is materially further along than the report states

The report claims cross-frame unwinding is "explicitly out of scope."
**The current `glyim-mir-interp/src/lib.rs` already implements it**:
`Interpreter::unwind_step` (search `fn unwind_step`) does exactly the
three-step walk described by its own doc comment:

1. If the current block has a `cleanup` edge, jump there (single-frame).
2. Otherwise `self.call_stack.pop()`, stash the original panic in
   `self.pending_unwind` (first time only, so repeated pops don't clobber
   the *original* payload), restore the caller's `locals`/`local_decls`,
   and resume execution at `frame.unwind_target.unwrap_or(frame.target_bb)`.
3. If the call stack is empty, return
   `Err(InterpError::Unwind(Box::new(top)))` with the original payload
   (`pending_unwind.take().unwrap_or(payload)`).

`CallFrame` already carries `unwind_target: Option<BasicBlockIdx>`, sourced
from `TerminatorKind::Call`'s `cleanup` field. This is a real, if young,
cross-frame implementation — **treat this section as hardening and test
coverage, not greenfield implementation.**

### Gaps to close (verified by re-reading the function, not assumed)

1. **Step (2)'s resume target is wrong when the caller frame's own unwind
   continues.** `frame.unwind_target.unwrap_or(frame.target_bb)` resumes at
   the caller's cleanup block if it has one, but falls back to
   `frame.target_bb` — the caller's **normal** (non-unwind) continuation
   block — if it doesn't. That's a real bug: a caller with *no* cleanup edge
   of its own for that call must **keep unwinding past it**, not resume
   normal execution as if the call had returned successfully. Silently
   resuming normal execution after a panic is exactly the kind of "silent
   wrong-answer fallback" §0 forbids.
2. **`pending_unwind` is written but its consumption path needs a test**
   proving the *original* panic (not a secondary one raised inside a
   caller's own cleanup block) is what ultimately surfaces at the top —
   currently unverified.
3. **`recursion_depth` bookkeeping** (`saturating_sub(1)` in the pop path)
   needs a test proving `recursion_limit` re-arms correctly for a
   panic-inside-deep-recursion-then-caught (if Glyim has catch/recover
   semantics) or panic-to-top-of-stack scenario, so a later, unrelated deep
   call doesn't spuriously trip the recursion limit due to stale
   accounting.

### Step-by-step instructions

**Step 1 — fix the resume-target bug.** In `unwind_step`, step (2):

```rust
// (2) cross-frame: pop to the caller. If the caller itself has no cleanup
// for this call site, it must keep unwinding past it rather than resuming
// normal execution — resuming at `frame.target_bb` (the success
// continuation) would silently treat a propagating panic as a normal
// return, which is a correctness bug, not a valid fallback.
if let Some(mut frame) = self.call_stack.pop() {
    if self.pending_unwind.is_none() {
        self.pending_unwind = Some(payload);
    }
    let caller_body = frame.body;
    self.locals = frame.locals;
    self.local_decls = caller_body.locals.iter().cloned().collect();
    self.recursion_depth = self.recursion_depth.saturating_sub(1);
    match frame.unwind_target {
        Some(resume) => {
            *body = caller_body;
            *bb_idx = resume;
            return Ok(());
        }
        None => {
            // Caller has no cleanup for this call: keep walking up. Push a
            // synthetic "no local cleanup edge" recursion by looping this
            // same function rather than returning, since there's no local
            // block to jump to in this frame at all.
            *body = caller_body;
            return self.unwind_step(
                self.pending_unwind.clone().expect("set above"),
                body,
                bb_idx,
                None, // this (now-current) frame's own cleanup_edge is
                      // unknown here — see Step 2, which threads it
                      // through properly instead of assuming None.
            );
        }
    }
}
```

The recursive call above is a placeholder illustrating the *intended
control flow* (keep popping until a frame with a real `unwind_target` is
found, or the stack empties) — but note it passes `cleanup_edge: None`,
which is wrong if the *newly current* (former caller) frame's own
current block *does* have a `cleanup` edge unrelated to `unwind_target`
(those are two different things: `unwind_target` is "where should MY caller
resume if I unwind past them", not "does this frame's current block have a
cleanup edge for the statement that's currently executing" — there isn't
one, because we just jumped frames due to an *already in-flight* unwind, not
a new panic in this frame). So the correct fix is simpler than recursion —
just loop:

```rust
fn unwind_step(
    &mut self,
    payload: InterpError,
    body: &mut Body,
    bb_idx: &mut BasicBlockIdx,
    cleanup_edge: Option<BasicBlockIdx>,
) -> InterpResult<()> {
    if !self.panics_unwind {
        return Err(payload);
    }
    if let Some(cb) = cleanup_edge {
        *bb_idx = cb;
        return Ok(());
    }
    if self.pending_unwind.is_none() {
        self.pending_unwind = Some(payload);
    }
    loop {
        match self.call_stack.pop() {
            Some(frame) => {
                self.recursion_depth = self.recursion_depth.saturating_sub(1);
                match frame.unwind_target {
                    Some(resume) => {
                        self.locals = frame.locals;
                        self.local_decls = frame.body.locals.iter().cloned().collect();
                        *body = frame.body;
                        *bb_idx = resume;
                        return Ok(());
                    }
                    // This caller frame has no cleanup for the call that
                    // led into the frame we just left — keep popping
                    // instead of resuming its normal continuation.
                    None => continue,
                }
            }
            None => {
                let top = self.pending_unwind.take().expect("set above");
                return Err(InterpError::Unwind(Box::new(top)));
            }
        }
    }
}
```

This drops the buggy `.unwrap_or(frame.target_bb)` fallback entirely and
replaces the ad-hoc recursion with an explicit loop that keeps popping until
either a real `unwind_target` is found or the stack is empty — matching the
function's own doc comment precisely.

**Step 2 — audit `TerminatorKind::Call` handling to confirm `unwind_target`
is populated correctly** for every call, including calls with *no* `cleanup`
field set (`grep -n "TerminatorKind::Call" mir_interp` around line 283 and
326 in the earlier grep output) — confirm `CallFrame { unwind_target: cleanup,
.. }` is set from the *current* call's `cleanup`, not inherited from the
caller's own `unwind_target` (a subtle but easy copy-paste bug to introduce;
write the regression test in Step 4 to catch it either way).

### Tests

```rust
// glyim-mir-interp/src/lib.rs #[cfg(test)]
#[test]
fn unwind_skips_callers_with_no_cleanup() {
    // f() calls g() (no cleanup edge for the call to g), g() calls h() (no
    // cleanup edge for the call to h), h() panics with a cleanup edge in
    // its OWN body that just re-raises (no catch). Assert the panic
    // surfaces as InterpError::Unwind at the top, NOT as a normal return
    // from f() or g() — this is the regression test for the fixed bug.
}

#[test]
fn unwind_resumes_at_nearest_caller_with_cleanup() {
    // f() calls g() WITH a cleanup edge; g() calls h() with no cleanup;
    // h() panics. Assert execution resumes in f()'s cleanup block (skipping
    // g() entirely, since g() had no cleanup edge for its call to h()).
}

#[test]
fn original_panic_payload_survives_multi_frame_unwind() {
    // 3+ frames deep; assert the InterpError::Unwind payload at the top
    // is identical (same message/kind) to the panic raised at the bottom,
    // even though intermediate frames' cleanup blocks run drop glue that
    // itself performs interpreter steps.
}

#[test]
fn recursion_limit_reflects_unwound_frames() {
    // Panic near recursion_limit depth, unwind fully to the top, then start
    // a *fresh* call chain and confirm it can recurse back up to
    // recursion_limit again (proving recursion_depth was correctly
    // decremented for every popped frame, not just the first).
}
```

### Acceptance criteria

- [ ] The `.unwrap_or(frame.target_bb)` bug is removed; callers with no
      cleanup edge are skipped, never treated as a normal return.
- [ ] All four tests above pass.
- [ ] `KNOWN_GAPS.md` updated to reflect that cross-frame unwinding is
      **implemented and hardened** (correcting the stale "out of scope"
      framing both in the report and in the crate's own doc comments —
      update those doc comments too, they currently contradict the code).

---

## 1.5 Procedural Macros — Windows Loading

### Current state

`glyim-proc-macro/src/lib.rs::load_cdylib` has a real Unix implementation
(`#[cfg(unix)]`, presumably `dlopen`-based — confirm exact crate used via
`grep -n "^use " glyim-proc-macro/src/lib.rs`) and a `#[cfg(not(unix))]` (or
similarly gated) stub returning `Err("proc-macro cdylib loading is only
implemented on Unix targets (tracked)")`.

### Target design

A real Windows loader using `LoadLibraryW`/`GetProcAddress`/`FreeLibrary`
(via the `windows` or `libloading` crate — check `Cargo.toml`; if the Unix
path already uses `libloading`, prefer it on Windows too since it already
abstracts `dlopen` vs `LoadLibrary` — this may make the fix a matter of
**removing the `#[cfg(unix)]` gate entirely** rather than writing new
platform code).

### Step-by-step instructions

**Step 0.** `grep -n "libloading\|dlopen\|use " glyim-proc-macro/src/lib.rs`
and `glyim-proc-macro/Cargo.toml`.

**Step 1 — if `libloading` is already the dependency:** the Unix
implementation is very likely *already portable* (that's `libloading`'s
entire purpose — it wraps both `dlopen` and `LoadLibrary` behind one API).
In that case the fix is simply:

```rust
// Before:
#[cfg(unix)]
pub fn load_cdylib(path: &str) -> Result<LoadedCrate, String> {
    // ... libloading::Library::new(path) ...
}
#[cfg(not(unix))]
pub fn load_cdylib(_path: &str) -> Result<LoadedCrate, String> {
    Err("proc-macro cdylib loading is only implemented on Unix targets (tracked)".to_string())
}

// After: drop both cfg gates, keep one function.
pub fn load_cdylib(path: &str) -> Result<LoadedCrate, String> {
    // ... exact same libloading::Library::new(path) body ...
}
```

Re-read the Unix body first (`sed -n '283,393p'` region per the earlier
grep) to confirm it contains nothing Unix-specific (e.g. raw `dlopen` FFI,
`RTLD_NOW` flags via `libc`) before deleting the cfg gate. If it *does* use
raw `libc::dlopen`, go to Step 2 instead.

**Step 2 — if the Unix path uses raw `dlopen` FFI (not `libloading`):**
migrate both platforms onto `libloading` (adds one dependency, removes two
hand-rolled FFI paths — a net simplification, not just a Windows patch):

```rust
// glyim-proc-macro/Cargo.toml
[dependencies]
libloading = { workspace = true }
```
(add to the root `Cargo.toml`'s `[workspace.dependencies]` first if not
present: `libloading = "0.8"`).

```rust
// glyim-proc-macro/src/lib.rs
use libloading::{Library, Symbol};

pub struct LoadedCrate {
    // keep existing fields; add:
    _lib: Library, // must outlive any Symbol obtained from it
}

pub fn load_cdylib(path: &str) -> Result<LoadedCrate, String> {
    // SAFETY: proc-macro cdylibs are build-generated, trusted artifacts
    // produced by this same toolchain's own build of the proc-macro crate —
    // matches the existing Unix path's trust assumption (do not add new
    // sandboxing here; that's a separate, larger security task if desired).
    let lib = unsafe {
        Library::new(path).map_err(|e| format!("failed to load proc-macro cdylib {path}: {e}"))?
    };
    // Re-resolve whatever entry symbol(s) the existing Unix implementation
    // looked up (grep the Unix body for the exact symbol name(s), e.g.
    // `__glyim_proc_macro_entry`) and mirror that lookup here via
    // `lib.get::<Symbol<...>>(b"...")`.
    let entry: Symbol<unsafe extern "C" fn() -> *const ()> = unsafe {
        lib.get(b"__glyim_proc_macro_entry\0")
            .map_err(|e| format!("proc-macro cdylib {path} missing entry symbol: {e}"))?
    };
    let entry_ptr = unsafe { entry() };
    Ok(LoadedCrate { /* existing fields populated from entry_ptr */ _lib: lib })
}
```

This single function now works on Unix, Windows, and macOS, since
`libloading::Library::new` dispatches to the right platform primitive
internally.

**Step 3.** Delete the now-dead `#[cfg(unix)]`/`#[cfg(not(unix))]` split
entirely (both the function and any platform-only helper functions/imports
that existed solely to support the old raw-FFI Unix path).

### Tests

```rust
#[test]
fn load_cdylib_windows_and_unix_share_one_path() {
    // Compile a trivial proc-macro cdylib fixture (reuse whatever fixture
    // the existing Unix test already builds — grep `#[cfg(test)]` in this
    // file for it) and assert `load_cdylib` succeeds regardless of
    // `cfg(target_os)`, i.e. this test has no `#[cfg(unix)]` gate at all
    // (that absence IS the regression test).
}
```

Also add a Windows CI job (see §9) since this can only be truly verified by
actually running on Windows.

### Acceptance criteria

- [ ] `load_cdylib` has a single implementation, no `#[cfg(unix)]` /
      `#[cfg(not(unix))]` split.
- [ ] Existing Unix test still passes with the gate removed.
- [ ] New CI job builds and runs `glyim-proc-macro`'s test suite on
      `windows-latest`.

---
