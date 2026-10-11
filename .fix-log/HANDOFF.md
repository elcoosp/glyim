# HANDOFF — Glyim compiler (current)

**Updated**: end of Session 9
**HEAD**: `995ac244`
**Matrix**: **4599 passing / 0 failing / 2 skipped** — fully green.

> Session 5's handoff (`.fix-log/HANDOFF.md` before this) claimed
> `17aacb46`, 4570/1. Superseded.

---

## 1. Session 7 summary

Started from the `better-testing.md` doc, ended up fixing five real
compiler defects plus building a four-class regression suite.

### Bugs fixed (6 commits)

| Commit | Bug |
|---|---|
| `7df95f81` | typeck returned sentinel `FnDefId(u32::MAX-2)` for **any** `.to_string()`; lowering replaced it with a poison `Error` constant → every `to_string` returned 0 |
| `9950a459` | builtin methods never registered in `fn_sigs` (codegen panicked "no signature registered"); `str::to_string` had no LLVM arm |
| `59ed4b2c` | `String`/`Vec`/`Box` are `TyKind::Adt(1050/1020/1040)`, not `TyKind::String` — the builtin-before-impl guard skipped them, so `s.len()` resolved to an empty `str::len` stub (uninitialized → 0) |
| `414e19a9` | `format!("{}", x)` interpolation end-to-end (emitter + LLVM `as_str`/`new`/`push_str` arms) |
| `cf68f584` | interpreter now hard-fails on sentinel ids instead of returning poison |

`probe_string_push_str.g` — the failure that motivated this — passes.

### Regression suite (4 classes, all committed)

| Class | Where | Guards |
|---|---|---|
| 1. Registry invariants | `glyim-type/src/tests/builtin_registry.rs` | builtin id present in method table but missing `fn_sigs`; sentinel slots; duplicate/out-of-range ids |
| 2. Poison hard-fail | `glyim-mir-interp/src/tests/poison.rs` | a compiler sentinel reaching a live path must panic |
| 3. Behavioral matrix | `glyim-test/tests/run-pass/behavior/*.g` (9 fixtures) | method resolving to the wrong/empty body — value assertions through the real pipeline |
| 4. Backend completeness | `glyim-cli/tests/emit_modes.rs::object_has_no_unresolved_glyim_fn_symbols` | a builtin with no lowering arm → undefined `__glyim_fn_*` symbol |

---

## 2. Original plan status (`docs/roadmaps/glyim-fix-plan.md`)

202 tasks (T001–T202), 199 findings, 4 waves.

- **Waves 1–4**: done in sessions 1–4 (114 commits).
- Status markers in tree: **`-PATCHED` 111 files**, `-REVIEWED` 2, `-REVISED` 1, **`-DEFERRED` 1** (`str.g`).

---

## 3. T140 — status after session 9

### Implemented (tested)

| Item | Fixture |
|---|---|
| `str::len`, `contains`, `trim` | `tests/run-pass/str/` (15) |
| `[T]::len` | `tests/run-pass/slice/` (4) |
| `MaybeUninit::{uninit,new,assume_init}` | `tests/run-pass/core/maybe_uninit_new.g` |
| `mem::size_of` / `align_of` | `tests/run-pass/mem/` (6) |
| `ptr::read` / `write` (direct + through fn) | `tests/run-pass/ptr/` (3) |
| raw deref `*p` read/write | same |
| `UnsafeCell::new` | `tests/run-pass/core/unsafecell_new.g` |
| extern abort hooks stop execution | `tests/run-pass/core/abort_stops.g` |
| `hint::spin_loop` | no-op is correct |

### Compiler bugs fixed across sessions 8-9

1. DCE ignored projection locals (`062cc9db`).
2. DCE ignored `UnaryOp` operands (`0a3cdf56`).
3. Block-like statements absorbed trailing operators (`02be5cf5`).
4. `str` dynamic range slicing + fat-ptr `str::len` (`daa01ead`).
5. extern abort hooks returned 0 instead of stopping (`1853419d`).
6. `size_of`/`align_of`/`needs_drop` had no lowering (`aa409971`,
   `const_fns` registry + layout fold).
7. `*p = v` lowered as an rvalue, not a place (`702ea7db`).
8. Block-tail statements (`unsafe { *p = 7 }`) were discarded (`702ea7db`).
9. DCE ignored the LHS *place* of a projected assignment (`e7c13984`).
10. const-prop did not kill aliased constants on a `Deref` write
    (`e7c13984`) — both analysis and rewrite loops.

### Still stubs

| Item | Blocker |
|---|---|
| `ptr::copy` / `copy_nonoverlapping` | need a byte-memcpy intrinsic (per backend) |
| `ptr::drop_in_place` | needs drop-glue lowering |
| `Chars::next` / `Lines::next` | unicode decoding + `&str` iteration state |
| `panic_any` | `msg.to_string()` on an `impl Display` *type param* hits the generic-display gap (Error sentinel → undefined symbol); `glyim_process_abort` works, so a message-free abort is possible |
| `UnsafeCell::get` runtime | `&self.value` records a `Ref` into the callee frame, dead on return — needs `Ref` to carry a projection |

### Verification

    cargo check --workspace --tests
    cargo nextest run --workspace --no-fail-fast   # 4599 passed / 2 skipped

## 4. Verification (corrected)

```bash
cargo check --workspace --tests   # --tests is REQUIRED (skips test targets otherwise)
cargo nextest run --workspace --no-fail-fast
```

Expected: **4598 passed / 2 skipped**.

---

## 5. Session 9 addendum — `panic_any` blocker (do not just enable it)

Enabling `panic_any` (route to the runtime `glyim_process_abort`) **breaks
every `println` fixture**. Root cause, confirmed from MIR:

- hello world's stdlib calls `panic_any` (`FnDefId(307)`) on the error
  branches of `write_all` / `unwrap` / `flush` (`grep FnDefId(307)` over the
  hello MIR shows ~5 call sites).
- Those branches are supposed to be runtime-dead. They are **not** — a live
  path reaches `panic_any`, which today silently no-ops, so `println`
  "works". Wiring a real abort exposes it.

So `panic_any` is *not* an isolated stub. Making it abort requires first
fixing whatever in the write path takes the error branch. Until then,
`panic_any` must stay a no-op (or message-free abort only where the write
path is verified clean).

`glyim_process_abort` itself is fine — `tests/run-pass/core/abort_stops.g`
calls it directly and correctly exits 101.

---

## 5. Session 9 addendum — `panic_any` blocker (do not just enable it)

Enabling `panic_any` (route to the runtime `glyim_process_abort`) **breaks
every `println` fixture**. Root cause, confirmed from MIR:

- hello world's stdlib calls `panic_any` (`FnDefId(307)`) on the error
  branches of `write_all` / `unwrap` / `flush` (`grep FnDefId(307)` over the
  hello MIR shows ~5 call sites).
- Those branches are supposed to be runtime-dead. They are **not** — a live
  path reaches `panic_any`, which today silently no-ops, so `println`
  "works". Wiring a real abort exposes it.

So `panic_any` is *not* an isolated stub. Making it abort requires first
fixing whatever in the write path takes the error branch. Until then,
`panic_any` must stay a no-op (or message-free abort only where the write
path is verified clean).

`glyim_process_abort` itself is fine — `tests/run-pass/core/abort_stops.g`
calls it directly and correctly exits 101.

---

## 6. ROOT CAUSE: enum matches ignore field patterns (found session 9)

`lower_match` + `collect_switch_values` (`crates/glyim-lower/src/lower_rvalue.rs`)
compile an enum `match` to a `SwitchInt` on the **discriminant only**. The
`PatternKind::Struct` arm of `collect_switch_values` (line ~1985) pushes
`(variant_idx, arm_bb)` and **ignores the pattern's fields**.

Consequence — a silent miscompile:

```glyim
match r {
    Result::Ok(0) => 1,     // pushed (0, bb_a)
    Result::Ok(n) => n,     // pushed (0, bb_b)  <-- same key
    Result::Err(_) => 9,
}

---

## 6. ROOT CAUSE: enum matches ignore field patterns (found session 9)

`lower_match` + `collect_switch_values` (`crates/glyim-lower/src/lower_rvalue.rs`)
compile an enum `match` to a `SwitchInt` on the **discriminant only**. The
`PatternKind::Struct` arm of `collect_switch_values` (line ~1985) pushes
`(variant_idx, arm_bb)` and **ignores the pattern's fields**.

Consequence — a silent miscompile:

```glyim
match r {
    Result::Ok(0) => 1,     // pushed (0, bb_a)
    Result::Ok(n) => n,     // pushed (0, bb_b)  <-- same key
    Result::Err(_) => 9,
}

---

## 7. Session 9 addendum — remaining T140 stubs are features, not patches

After fixing 6 bugs and closing ~15 stubs, the remaining T140 items each
need a real feature. None is a one-line fix.

### `ptr::add` / `ptr::sub` / `ptr::addr` / `ptr::is_null`

`check_expr.rs` (~line 2871) recognizes these on a `RawPtr` receiver and
returns dispatch `None`. The `None` arm of the `match dispatch` in
`Expr::MethodCall` then emits an **indirect call** on the receiver value, so
`p.add(2)` panics with "indirect call through non-function value". There is
no pointer-arithmetic MIR.

**Design:** add a `MethodDispatch::Builtin`-style path (or a new
`MethodDispatch::PtrArith { op }`) for raw-pointer receivers, lowered to a
`Rvalue::BinaryOp(Add/Sub, ptr, byte_offset)` scaled by `size_of::<T>()`
(now available — see `aa409971`), plus interpreter handling for
pointer +/- integer. ~half a day.

### `ptr::copy` / `copy_nonoverlapping`

Need a byte-memcpy intrinsic. With `ptr::add` working (above), a pure-Glyim
byte loop becomes possible; but for large copies an intrinsic is wanted.
Either way it depends on `ptr::add`.

### `ptr::drop_in_place`

Needs drop-glue lowering (call the pointed-to type's `Drop::drop`), which the
backend already generates for scope drops — routing it through a raw pointer
is the work.

### `Chars::next` / `Lines::next`

Need unicode scalar decoding (`char::from_u32` + UTF-8 byte walking) and
`&str` iteration state. `Chars::next` currently fails with "invalid cast".

### `UnsafeCell::get` runtime use

`&self.value` records `InterpValue::Ref { frame, local }` into the **callee**
frame, dead on return ("deref of Ref from dead frame"). The interpreter's
`Ref` carries no projection, so a reference to a *field* cannot outlive the
frame. Needs `InterpValue::Ref` to carry a projection path — a core
representation change touching every `Ref` producer/consumer.

### `panic_any`

Blocked on the enum-match field-pattern fix (§6). Once `Ok(0)` vs `Ok(n)` is
distinguished, `write_all` stops returning `Err`, `unwrap` stops panicking,
and a real `panic_any` can be enabled.

### Enum match field patterns (§6)

The fix was implemented this session (sequential arm testing) and produced
**correct MIR** for `Ok(0)`/`Ok(n)`/`Some(0)` — three minimal probes passed.
But it regressed all 7 `println` fixtures with an abort traced to `main`'s
frame that could not be explained from the MIR. It was **reverted** rather
than left in. The specific open question: *why does sequential match lowering
abort during `println` when the identical shape works for a standalone match?*
The implementation is in the session's git history if someone wants to
resume; the design is in §6.

---

**Session 9 end state: 4599 passing / 0 failing / 2 skipped.** Six compiler
bugs fixed (extern abort fallthrough, `size_of`/`align_of` folding, deref-as-
place, block-tail statement, DCE aliasing, const-prop aliasing) plus the
enum-match root cause found and documented.

---

## 7. Session 9 addendum — remaining T140 stubs are features, not patches

After fixing 6 bugs and closing ~15 stubs, the remaining T140 items each
need a real feature. None is a one-line fix.

### `ptr::add` / `ptr::sub` / `ptr::addr` / `ptr::is_null`

`check_expr.rs` (~line 2871) recognizes these on a `RawPtr` receiver and
returns dispatch `None`. The `None` arm of the `match dispatch` in
`Expr::MethodCall` then emits an **indirect call** on the receiver value, so
`p.add(2)` panics with "indirect call through non-function value". There is
no pointer-arithmetic MIR.

**Design:** add a `MethodDispatch::Builtin`-style path (or a new
`MethodDispatch::PtrArith { op }`) for raw-pointer receivers, lowered to a
`Rvalue::BinaryOp(Add/Sub, ptr, byte_offset)` scaled by `size_of::<T>()`
(now available — see `aa409971`), plus interpreter handling for
pointer +/- integer. ~half a day.

### `ptr::copy` / `copy_nonoverlapping`

Need a byte-memcpy intrinsic. With `ptr::add` working (above), a pure-Glyim
byte loop becomes possible; but for large copies an intrinsic is wanted.
Either way it depends on `ptr::add`.

### `ptr::drop_in_place`

Needs drop-glue lowering (call the pointed-to type's `Drop::drop`), which the
backend already generates for scope drops — routing it through a raw pointer
is the work.

### `Chars::next` / `Lines::next`

Need unicode scalar decoding (`char::from_u32` + UTF-8 byte walking) and
`&str` iteration state. `Chars::next` currently fails with "invalid cast".

### `UnsafeCell::get` runtime use

`&self.value` records `InterpValue::Ref { frame, local }` into the **callee**
frame, dead on return ("deref of Ref from dead frame"). The interpreter's
`Ref` carries no projection, so a reference to a *field* cannot outlive the
frame. Needs `InterpValue::Ref` to carry a projection path — a core
representation change touching every `Ref` producer/consumer.

### `panic_any`

Blocked on the enum-match field-pattern fix (§6). Once `Ok(0)` vs `Ok(n)` is
distinguished, `write_all` stops returning `Err`, `unwrap` stops panicking,
and a real `panic_any` can be enabled.

### Enum match field patterns (§6)

The fix was implemented this session (sequential arm testing) and produced
**correct MIR** for `Ok(0)`/`Ok(n)`/`Some(0)` — three minimal probes passed.
But it regressed all 7 `println` fixtures with an abort traced to `main`'s
frame that could not be explained from the MIR. It was **reverted** rather
than left in. The specific open question: *why does sequential match lowering
abort during `println` when the identical shape works for a standalone match?*
The implementation is in the session's git history if someone wants to
resume; the design is in §6.

---

**Session 9 end state: 4599 passing / 0 failing / 2 skipped.** Six compiler
bugs fixed (extern abort fallthrough, `size_of`/`align_of` folding, deref-as-
place, block-tail statement, DCE aliasing, const-prop aliasing) plus the
enum-match root cause found and documented.
