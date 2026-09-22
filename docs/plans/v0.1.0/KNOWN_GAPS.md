# Known Gaps

Tracked gaps for the Glyim v0.1.0 de-stubbing effort. Each entry: what is
missing, why, and the remediation tracking.

## async-v2 — async multi-await resume-dispatch (MIR transform)

- **What**: `async fn` with more than one sequential top-level `.await` currently
  does NOT suspend/resume correctly. Loop-await and multi-await are rejected at
  `desugar_async` with a clear diagnostic (errors 60/61) — NOT a silent
  miscompile.
- **Empirical state (verified 2026-08-23 via `PipelineCompiler`)**: even the
  **single-await** path does NOT compile end-to-end. Probing a real
  `async fn one_step(a) { let x = ready(a).await; x + 1 }` through the full
  pipeline yields 9 diagnostics: "enum-variant value paths are not yet
  supported", "no method `poll` found for type", "trait `Future` is not
  implemented", "unresolved name `panic`", etc. The existing `desugar_one_async_fn`
  emits a poll `Match` + `Pending => panic` skeleton, but the surrounding async
  machinery (enum-variant value paths, `poll` method resolution, real future
  types, `panic` builtin) is not yet wired, so the body does not type-check.
  The `desugar_async_fn_compiles` test only passes because its `add_one` has
  **no actual `.await`** — it exercises the `async fn` declaration form, not the
  await desugar. So the async feature is broadly non-functional end-to-end even
  for the "supported" single-await shape.
- **Prerequisite blocker triage (2026-08-23, after driving the real compiler)**:
  Isolated which of the 9 diagnostics are general vs async-specific by probing
  each construct through the pipeline in isolation:
  - **`panic` unresolved** — GENERAL bug (no `panic` builtin/intrinsic). The
    desugar emitted `Pending => panic!()` as a bare function call, but `panic`
    is a *macro* (`panic.g` → `panic_any(...) -> !`), not a resolvable `FnDef`.
    **FIXED (stopgap)**: the desugar now emits `Pending => loop {}` (a diverging
    expression that unifies with `Poll<_>`). This removes the `panic`` symptom
    for the single-await shape; a genuine `Pending` should suspend/resume (M4).
  - **`let`-bound locals unresolved inside impl-method bodies** — GENERAL
    typechecker name-resolution bug, independent of async. `check_pattern`
    bound the `let` name under the *remapped* (TyCtx-interner) `Name`, while
    later references resolve via the original HIR `Name`; when those two
    interners differ (impl-method bodies), the binding is invisible →
    "unresolved name `x`". Verified: `let x = self.f0; x + 1` in a trait-impl
    `poll` body failed before, passes after. **FIXED**: `check_pattern` now also
    binds the original HIR `Name` (sharing one `LocalVarId`), so lookups via
    either interner succeed. Full workspace stays green.
  - **`self` typed as `<error>` in the nested-async case** — RESOLVED
    (2026-08-24). The `&mut <error> vs Adt...` / "no method `poll` found"
    cascade was NOT a HIR-desugar structural defect (the desugared poll
    bodies are well-formed). It was two **typechecker ordering/side-effect**
    bugs exposed only when a desugared `poll` body references a top-level fn:
    1. The main body-checking loop type-checks `impl Future::poll` bodies
       *before* `check_fn_items_in_module` registers top-level fn signatures
       under the def-map's `LocalDefId`. A poll body that calls a desugared
       `async fn` wrapper (`ready(self.f0)`) therefore resolved that callee to
       an unregistered value → spurious "enum-variant value paths are not yet
       supported" → `<error>` receiver → cascade. **FIXED**: pre-register every
       top-level fn signature (under the def-map `LocalDefId`) before the main
       loop.
    2. `resolve_method_call` unified the receiver against *every* impl's `Self`
       to find candidates, but `unify` is side-effecting: a non-matching
       candidate (e.g. a `readyFuture` receiver against `impl Future for
       one_stepFuture`) emitted a spurious "mismatched types: Adt… vs Adt…"
       diagnostic. **FIXED**: snapshot the inference table + diagnostics buffer
       around the per-candidate `unify` and roll back on failure, so only the
       selected method may emit diagnostics.
    With both fixed, `async fn one_step(a) { let x = ready(a).await; x + 1 }`
    compiles end-to-end with **zero** diagnostics through `PipelineCompiler`
    (regression test `nested_async_single_await_compiles`). The single-await
    shape is now fully unblocked.
- **Status**: M0–M3 done + green; single-await fully unblocked (panic +
  let-in-impl + nested-cross-future resolution all fixed; regression test
  `nested_async_single_await_compiles` green). M4 (multi-await state-machine
  codegen) and M5 (host-run `two_step` proof) remain; M5 is host-infeasible for
  runtime validation on this macOS host (Linux-gated executor).
