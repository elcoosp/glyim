# Closure support — design

**Status:** design. No implementation yet.
**Blocking:** the closure-passing builtins (`Option::map`,
`Result::{map,map_err,map_or}`), and any user code that uses closures.
**Evidence base:** all findings below were verified against the tree at
commit `be038762`, with the exact file:line shown.

---

## 1. Verified findings

Closures are **broken end-to-end in the LLVM backend**. The interpreter
handles them; the LLVM backend does not.

| Program | Interpreter | LLVM backend |
|---|---|---|
| `let f = |x: i32| -> i32 { x + 1 }; f(5)` | works | `Layout error: UnknownType(Ty(22677))` |
| `let f = |x: i32| x + 1; f(5)` | works | same |
| `apply(|x| x + 1, 5)` where `fn apply(f: fn(i32)->i32, ..)` | — | compiles + links, then **SIGBUS** |

**Why it was never noticed:** the test tree has **zero closure coverage**.
No fixture in `run-pass/`, `ui/`, or `no-ice/` uses closure syntax. The
whole subsystem is untested on the LLVM side.

### Root cause: the closure signature is `fn(Infer) -> Infer`

A codegen probe on the failing call printed:

```
CLOSURE SIG: output=Ty(22677) kind=Infer(Ty(TyVar(11368)))
  in[0]=Ty(22677) kind=Infer(Ty(TyVar(11368)))
```

Every parameter and the return type are unzonked inference variables —
**even when the source annotates them**. `fn_abi_of` cannot lay out
`Infer`, so it fails. That single fact explains all four defects below.

---

## 2. The four defects

### D1 — The annotation is parsed, then dropped at HIR

The parser *does* parse it:

- `crates/glyim-frontend/src/parser/expr.rs:722` — closure param `: type`
- `crates/glyim-frontend/src/parser/expr.rs:737` — closure `-> type`

HIR then discards it:

- `crates/glyim-hir/src/lower/lower_expr.rs:430` —
  `let (_, pat_id) = lower_param(&param_node, ..);`
  The first tuple element is a `Param`, which **carries `.ty`**. It is
  bound to `_` and thrown away.
- `crates/glyim-hir/src/lib.rs:721` — `Pat::Binding` has fields
  `{ name, mutability, subpattern }`. **No type field.**
- `Expr::Closure { params, body, is_move }` (`lower_expr.rs:453`) has
  **no return-type field** either.

So `|x: i32| -> i32` and `|x|` lower to *identical* HIR.

### D2 — Typeck never reads an annotation

`crates/glyim-typeck/src/check_expr.rs:2280`:

```rust
let ty = match expected_param_tys.get(param_idx).copied() {
    Some(t) if t != Ty::ERROR => t,
    _ => self.fresh_infer_ty(),
};
```

The only source of a parameter type is a *pending `fn(..)` expectation*
(set when a closure is passed directly to a `fn`-typed formal). Absent
that, every param is a fresh inference variable. There is no path that
reads the annotation — because (D1) it isn't there to read.

### D3 — `closure_sigs` is never zonked

`crates/glyim-type/src/ty_ctx_mut.rs:1241` registers the sig. The zonk
pass (`crates/glyim-typeck/src/zonk.rs`) walks `thir::Body`s and their
types, but has **no reference to `closure_sigs`** (grep for
`closure_sig` in `zonk.rs` returns nothing). So even a param type that
inference *did* resolve (e.g. from body usage) is left as `Infer` in the
registered signature.

Note the ordering: the sig is registered *during* typeck, zonk runs
*after*. The sig table is only useful if something re-zonks it, or if the
consumer (codegen) zonks on read.

### D4 — No closure to `fn`-pointer coercion

`TyKind::Closure(id, substs)` lays out as a struct of captures (one field
per capture). A `fn(i32) -> i32` formal expects a **bare code pointer**.

There is no coercion path between them (grep for `coerce`/`shim` around
closures finds nothing). So `apply(|x| x + 1, 5)` passes a *struct* where
a *pointer* is expected — the callee calls through capture bytes — SIGBUS.

The direct-call path (`let f = |x| x+1; f(5)`) *is* handled — see
`crates/glyim-codegen-llvm/src/lower.rs:6208`, which extracts captures
from the struct and passes them as leading args. It is only the
`fn`-pointer position that has no story.

---

## 3. Design

Four layers, each independently shippable and testable. Do them in order.

### Layer 1 — Preserve the annotation (HIR)

**Goal:** `|x: i32| -> i32 { .. }` and `|x| { .. }` must be
distinguishable in HIR.

Add an optional annotation to the closure param and return type. Two
options:

- **(a) Widen `Pat::Binding`** with `ty: Option<TypeRef>`. Mirrors Rust
  (`Pat` can carry a type ascription). Touches every `Pat::Binding`
  construction site.
- **(b) Localize to closures**: change
  `Expr::Closure { params: Vec<PatId>, .. }` to
  `params: Vec<(PatId, Option<TypeRef>)>` and add `ret_ty: Option<TypeRef>`.

**Recommendation: (a).** Rationale: `let x: i32 = 5;` has the same shape
and will want the same treatment; doing (b) now means doing (a) later.
But (b) is acceptable as a first step if (a) proves too churny.

`lower_closure_expr` must stop discarding `lower_param`'s first element
and store its `.ty` (as a `TypeRef`) on the param.

**Gate:** a HIR unit test asserting `|x: i32|` and `|x|` produce
different `Pat`/`Expr` structures.

### Layer 2 — Typeck reads the annotation

**Goal:** an annotated closure param has its declared type, not `Infer`.

In the closure arm (`check_expr.rs:~2280`), the fallback chain becomes:

1. pending `fn(..)` expectation (existing behaviour — keep it, it is how
   inference flows in),
2. **the param's own annotation**, resolved via
   `tyconv::resolve_type_ref`,
3. `fresh_infer_ty()` (last resort).

Same for the return type: if `ret_ty` is annotated, use it as the
closure's output instead of a fresh var.

Also: `closure_sigs` must be built from these resolved types.

**Gate:** `let f = |x: i32| -> i32 { x + 1 }; f(5)` compiles and runs on
the LLVM backend. This is the single highest-value checkpoint — it makes
the common case work and exercises Layers 1-3 together if Layer 3 is done
at the same time.

### Layer 3 — Zonk the closure signature

**Goal:** no `Infer` survives into `closure_sigs` for a reachable closure.

Two options:

- **(a) Zonk the table in the zonk pass.** `zonk_bodies` gains access to
  the closure-sig table (requires `&mut` — either a `TyCtxMut`, or a
  "produce a new frozen table" signature). Cleanest end state.
- **(b) Zonk on read in codegen.** When `lower_call` fetches
  `closure_sig(id)`, substitute any remaining `Infer` using the enclosing
  body's already-resolved types. Cheaper, but papers over the table being
  stale.

**Recommendation: (a).** A table that is correct only if the reader
patches it is a trap for the next person. Zonking it once, at the same
point bodies are zonked, keeps the invariant "frozen `TyCtx` contains no
`Infer`" true.

**Gate:** the codegen probe (`fn_abi_of` on a closure sig) no longer
prints `Infer`.

### Layer 4 — Decide the `fn`-pointer story

**Goal:** stop miscompiling. Two acceptable outcomes, in order of cost:

- **(a) Reject with a diagnostic (smallest).** When a `TyKind::Closure`
  flows into a `fn(..)`-typed position, emit
  "closures cannot yet be coerced to fn pointers; use a direct call".
  This turns a SIGBUS into a clear error. **Do this first.**
- **(b) Synthesize a shim (large).** A real `__glyim_shim_N` function
  that takes the captures (via a global, thread-local, or boxed
  environment) and forwards to the closure body, plus the coercion in
  typeck. This is the Rust `Fn`-trait machinery in miniature. Out of
  scope for this design; revisit if `fn`-pointer positions prove common.

**Gate:** `apply(|x| x + 1, 5)` produces a diagnostic, not a crash.

### Layer 5 — Unblock the builtin arms (after 1-4)

Once a closure value can be *called*, `Option::map` etc. become ordinary:

- Factor the capture-extraction + call sequence out of `lower_call`
  (`lower.rs:6208-6330`) into a helper
  `emit_closure_call(closure_val, explicit_args, dest)`.
- The `Option::map` arm: extract the payload from `Some`, call the
  closure with it, wrap the result in `Some` (`None` passes through).
- Interp already has working closure support; the arms are mechanical.

---

## 4. Incremental plan

| Step | Deliverable | Gate |
|---|---|---|
| 0 | **Closure fixtures** (this is first, not last) | a fixture exists per row of section 1's table |
| 1 | HIR preserves annotations | HIR unit test |
| 2 | Typeck reads them | `cl_ann` runs on LLVM |
| 3 | Zonk `closure_sigs` | probe shows no `Infer` |
| 4 | Reject closure-to-fn-pointer coercion | `cl_fnptr` is a diagnostic, not SIGBUS |
| 5 | `Option::map` / `Result::map` arms | differential includes them |

**Step 0 is not optional and is not busywork.** The reason this bug
survived to production-readiness review is that *nothing tested
closures*. Adding the fixtures first means every subsequent step has a
failing test to turn green, and any regression is caught immediately.

---

## 5. Fixtures to add (step 0)

In `crates/glyim-test/tests/run-pass/behavior/`:

| Fixture | Exit | Covers |
|---|---|---|
| `closure_bare.g` | 6 | `let f = |x: i32| -> i32 { x + 1 }; f(5)` |
| `closure_inferred.g` | 6 | `let f = |x: i32| x + 1; f(5)` |
| `closure_capture.g` | 6 | captures a local (`n + x`) |
| `closure_no_args.g` | 7 | `|| 7` |
| `closure_nested.g` | 3 | a closure inside a closure |

And in `crates/glyim-test/tests/ui/` (expected-fail):

| Fixture | Expects |
|---|---|
| `closure_to_fn_ptr.g` | a clear diagnostic (until Layer 4b) |

---

## 6. Out of scope

- `Fn`/`FnMut`/`FnOnce` traits as bounds (`impl Fn(i32) -> i32`). Closures
  are a concrete `TyKind::Closure`; there is no trait abstraction yet.
- `move` semantics beyond what `is_move` already threads through.
- Returning a closure from a function (needs the same coercion/shim
  machinery as Layer 4b).
- Closures in const contexts.

---

## 7. Why this ordering

Layers 1-3 make the *common* case (a directly-called closure) work. That
is ~90% of closure usage and unblocks the builtins. Layer 4 stops the
remaining case from miscompiling — turning a silent SIGBUS into a loud
error is a correctness win even before the shim exists. Layer 5 is
mechanical once the value can be called.

Doing Layer 5 first (as an "`Option::map` arm") would build on a
signature that is still `fn(Infer) -> Infer` — it would fail the same way
and teach nothing.
