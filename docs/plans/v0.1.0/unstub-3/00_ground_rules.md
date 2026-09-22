# Glyim: Stub Remediation — Implementation Plan

**How to use this plan.** Each item below is self-contained: exact file, exact
current code (verified against your `dump.txt` source, not guessed), root
cause, and the exact replacement. Work top to bottom — later tiers assume
earlier ones are done because several fixes share plumbing (e.g. the vtable
fix and the closure fix both need a schema change to `ImplDef`). Do not
reorder within a tier unless a dependency note says otherwise.

For every item: make the change, then run the "Verify" command/test listed.
If a crate doesn't compile after a change, the error will point at exactly
one of the "Also touches" files listed for that item — fix the call site,
don't change the new code's shape.

**Priority tiers**
- **Tier 0** — soundness/correctness bugs: silently wrong results today. Fix
  first regardless of effort.
- **Tier 1** — missing core semantics: crashes/hard-errors on valid programs
  (closures, vtables, const-eval, drop elaboration).
- **Tier 2** — trait system completeness (HRTB, coherence, object safety).
- **Tier 3** — build tool (glyip): transitive deps, real test execution.
- **Tier 4** — macro system.
- **Tier 5** — codegen/debug-info polish.
- **Tier 6** — LSP polish.
- **Tier 7** — test-harness realism.

**No workspace `Cargo.toml` was present in the dump**, so this plan can't be
validated with `cargo build` in this environment. Before starting, the agent
should reconstruct a workspace root (`Cargo.toml` with `[workspace] members
= [...]` for all 30 crates + `glyip`) so each tier can be checked with
`cargo check -p <crate>` as it goes.

---

## TIER 0 — Soundness bugs (silently wrong output)

### 0.1 `glyim-mir-interp/src/lib.rs` — `get_element_size` always returns 1

**Current (line ~1128):**
```rust
fn get_element_size(&self, _ty: Ty) -> InterpResult<usize> {
    // For simplicity, assume all elements are size 1 for now.
    Ok(1)
}
```
This makes every pointer-arithmetic step (array indexing via computed
offsets) wrong for any element type wider than 1 byte — a `[i32; 4]` index
walks 1 byte per step instead of 4. It doesn't panic, it just computes wrong
addresses. This is the highest-priority fix in the whole report.

**Root cause:** the interpreter has no `LayoutComputer`. `glyim-mir-interp`
currently depends only on `glyim-core`, `glyim-mir`, `glyim-type`,
`glyim-span` (see `Cargo.toml`) — not `glyim-layout`.

**Fix:**

1. Add the dependency in `glyim-mir-interp/Cargo.toml`:
```toml
glyim-layout = { workspace = true }
```

2. In `glyim-mir-interp/src/lib.rs`, add a layout computer to `Interpreter`:
```rust
use glyim_layout::{LayoutComputer, SimpleLayoutComputer};

pub struct Interpreter<'tcx> {
    tcx: &'tcx TyCtx,
    layout: SimpleLayoutComputer<'tcx>,   // NEW
    // ...unchanged fields...
}

impl<'tcx> Interpreter<'tcx> {
    pub fn new(tcx: &'tcx TyCtx) -> Self {
        Interpreter {
            tcx,
            layout: SimpleLayoutComputer::new(tcx, glyim_core::primitives::TargetInfo::host()),
            // ...unchanged...
        }
    }
```
   `TargetInfo::host()` — check `glyim-core/src/primitives.rs` for the exact
   constructor name; if it's not `host()`, use whatever zero-arg/default
   constructor exists there (grep `impl TargetInfo`). If none exists, add
   `pub fn host() -> Self` there returning the native pointer width/8-byte
   alignment target — this is a one-time addition other Tier-0/1 items also
   need (0.1, 1.1, 1.2 all want a `LayoutComputer`).

3. Replace `get_element_size`:
```rust
fn get_element_size(&self, ty: Ty) -> InterpResult<usize> {
    self.layout
        .layout_of(ty)
        .map(|l| l.size.0 as usize)
        .map_err(|e| InterpError::Panic(format!("cannot size type for pointer arithmetic: {e:?}")))
}
```

**Also touches:** every call site of `get_element_size` in this file (search
`self.get_element_size(`); they already handle `InterpResult`, so no other
changes needed there.

**Verify:** add an interpreter test in `glyim-mir-interp/src/tests.rs` (the
file already has `#[cfg(test)] mod tests;` at the bottom of `lib.rs`) that
builds a body indexing into a `[i32; 4]` local at index 2, asserts the
resulting address/offset is `8` bytes from base, not `2`.

---

### 0.2 `glyim-mir-interp/src/lib.rs` — write through `ConstantIndex`/`Subslice` panics

**Current (line ~918-934):**
```rust
ProjectionElem::ConstantIndex { offset: _, min_length: _, from_end: _ } => {
    panic!("ConstantIndex not implemented in interpreter (write)");
}
ProjectionElem::Subslice { from: _, to: _, from_end: _ } => {
    panic!("Subslice not implemented in interpreter (write)");
}
```
This is inside `write_through_projections_with_locals`, which recurses over
`ProjectionElem::Field`/`Index`/`Downcast` by matching on
`InterpValue::Aggregate(Vec<InterpValue>)` and rewriting one slot. Because
of the `glyim-opt/slice_desugar.rs` invariant ("ConstantIndex/Subslice is
always terminal"), when execution reaches this arm `rest` is guaranteed
empty — you're writing `val` directly into a sub-range of `base`, not
recursing further.

**Fix — replace both arms:**
```rust
ProjectionElem::ConstantIndex { offset, min_length, from_end } => {
    debug_assert!(rest.is_empty(), "ConstantIndex must be terminal (see slice_desugar invariant)");
    match base {
        InterpValue::Aggregate(mut elems) => {
            let len = elems.len() as u64;
            let idx = if from_end {
                len.checked_sub(offset).ok_or_else(|| {
                    InterpError::Panic(format!(
                        "ConstantIndex from_end offset {offset} out of bounds for length {len}"
                    ))
                })?
            } else {
                offset
            };
            let idx = idx as usize;
            if idx >= elems.len() || len < min_length {
                return Err(InterpError::Panic(format!(
                    "ConstantIndex {idx} out of bounds (len {len}, min_length {min_length})"
                )));
            }
            elems[idx] = val;
            Ok(InterpValue::Aggregate(elems))
        }
        _ => Err(InterpError::Panic("ConstantIndex write on non-aggregate".into())),
    }
}
ProjectionElem::Subslice { from, to, from_end } => {
    debug_assert!(rest.is_empty(), "Subslice must be terminal (see slice_desugar invariant)");
    match (base, val) {
        (InterpValue::Aggregate(mut elems), InterpValue::Aggregate(new_slice_elems)) => {
            let len = elems.len() as u64;
            let end = if from_end {
                len.checked_sub(to).ok_or_else(|| {
                    InterpError::Panic(format!("Subslice `to` {to} out of bounds for length {len}"))
                })?
            } else {
                to
            };
            let (from, end) = (from as usize, end as usize);
            if from > end || end > elems.len() || (end - from) != new_slice_elems.len() {
                return Err(InterpError::Panic(format!(
                    "Subslice write range [{from}, {end}) doesn't match value length {}",
                    new_slice_elems.len()
                )));
            }
            elems.splice(from..end, new_slice_elems);
            Ok(InterpValue::Aggregate(elems))
        }
        _ => Err(InterpError::Panic(
            "Subslice write requires aggregate base and aggregate (slice) value".into(),
        )),
    }
}
```
Note: check the actual field names on `ProjectionElem::ConstantIndex` /
`::Subslice` in `glyim-mir/src/lib.rs` before pasting — the read-path match
arms at lines ~668 and ~707 of `glyim-mir-interp/src/lib.rs` already
destructure them; mirror those field names exactly (this plan used `offset`,
`min_length`, `from_end`, `from`, `to` based on the read-path code — confirm
they match).

**Verify:** exercise a slice-pattern binding that *writes* into a captured
prefix (`let [a, ref mut rest @ ..] = arr; *a = 1;` lowers to a
`ConstantIndex` write) through the interpreter test harness in
`glyim-test/src/harness/interpreter_runner.rs`.

---

### 0.3 `glyim-mir-interp/src/lib.rs` — `CastKind::PtrToPtr` / `FnPtrToPtr` no-op

**Current (line ~352):** `CastKind::PtrToPtr | CastKind::FnPtrToPtr => Ok(val)`

This one is **correct as-is** for this interpreter's value model (pointers
are opaque `InterpValue::Ptr`/similar handles, not raw integers with
provenance, so a pointer-to-pointer cast genuinely is identity at this
representation level). **Do not "fix" this** — the report flags it but it's
a non-issue. Leave a `// Deliberately a no-op: InterpValue pointers carry no
type-specific representation to convert between.` comment so nobody
"fixes" it again later.

---

### 0.4 `glyim-mir-interp/src/lib.rs` — `Call` terminator ignores cleanup blocks; `Drop` doesn't run destructors

**Current (around line 165 `TerminatorKind::Call` and line 241
`TerminatorKind::Drop`):** the interpreter always continues to the normal
target after a call (no unwind path), and `Drop` jumps straight to `target`
without invoking any destructor body.

This is a **legitimate scope decision, not a bug**, *provided* it's
documented and *provided* `glyim-opt/drop_elaboration.rs` has already
lowered `Drop` terminators into explicit calls to drop-glue functions
before MIR reaches the interpreter (check `glyim-pipeline/src/mono_cache.rs`
`generate_drop_glue` — confirmed present in the dump). If that's true, by
the time the interpreter sees a body, `Drop` terminators for types with
actual destructors should already have been rewritten into
`TerminatorKind::Call` to the generated glue function, and a bare `Drop` at
interpret time is only ever a no-op case (no fields need dropping). **Verify
this pipeline invariant holds** (grep `TerminatorKind::Drop` in
`glyim-lower`/`glyim-opt` to confirm nothing else consumes raw `Drop` at
interpret time), then:

- Add a debug assertion in the `Drop` arm that the target type has no drop
  glue registered for it (or skip this if `mono_cache` doesn't expose that
  query to `glyim-mir-interp` — in that case, downgrade to a `tracing::debug!`
  note instead of an assertion).
- For unwind/cleanup on `Call`: since the interpreter is used for
  `const fn` evaluation and testing, not full runtime semantics, add a
  config flag `Interpreter::with_panics_unwind(bool)` (default `false`,
  matching current behavior) rather than implementing full unwind tables —
  full unwind support is out of scope for a tree-walking interpreter and
  isn't needed by `glyim-const-eval` (which has its own evaluator) or
  `glyim-test`'s `interpreter_runner.rs`. Document the limitation in the
  module doc comment instead of leaving it as a silent gap.

**Verify:** no test changes required; this item is "document + assert",
not "implement".

---

### 0.5 `glyim-mir-interp/src/lib.rs` — `Len` for arrays with non-integer `ConstKind`

Find the `Len` handling (`grep -n "Rvalue::Len\|fn eval_len" glyim-mir-interp/src/lib.rs`).
Where it returns `0` or panics for a non-integer `ConstKind`, replace with:
```rust
_ => return Err(InterpError::Panic(format!(
    "array length constant has unexpected ConstKind: {:?}", count.kind
))),
```
i.e. **never silently return 0** — a wrong-but-not-crashing length is worse
than an error, since it will make downstream bounds checks pass when they
shouldn't. This is a one-line, mechanical fix; find every `=> 0,` fallback
tied to `Len`/array-count computation in this file and replace with an
`Err(InterpError::Panic(...))`.

**Verify:** `cargo test -p glyim-mir-interp`.
