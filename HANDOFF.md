# HANDOFF — session 34 (interp half of §3.9: still open, but diagnosis advanced)

> **SUPERSEDED (session 35).** Everything this handoff left open is now
> closed: the §3.9 interp half was fixed and verified in session 34's
> aftermath, and session 35 closed the remaining array→slice unsize
> coercion gaps (call ABI, aggregate/tuple fields, `Place::ty`
> ref-peel, builder auto-deref, interpreter `operand_ty` + value
> boundaries) — see `FIXES.md` §8 and the session-35 resolution block
> in `KNOWN_GAPS.md` §3.9. The notes below are kept verbatim as the
> historical record of how the interp bug was narrowed.

## 0. State at handoff

**HEAD:** `aad6585a` — in sync with `origin/main`
**Tree:** **LIKELY DIRTY** — my last action applied temporary instrumentation to `crates/glyim-mir-interp/src/lib.rs` (added a shape-discriminator fix to `slice_length_from_value` plus a `[DBG-LEN]` eprintln) and restored `crates/glyim-test/tests/run-pass/behavior/slice_iter_next.g`. The differential test showed `rc=100` (still failing) and the `[DBG-LEN]` lines were **empty** (never fired). The instrumentation was NOT reverted before this handoff.

**First action for next agent:**
```bash
cd /Users/adm/Documents/Repos/glyim-v2
git status --short
# Expect: M crates/glyim-mir-interp/src/lib.rs, ?? crates/glyim-test/tests/run-pass/behavior/slice_iter_next.g
rm -f .git/index.lock  # if present from prior runs
git checkout HEAD -- crates/
rm -f crates/glyim-test/tests/run-pass/behavior/slice_iter_next.g
rm -f crates/glyim-test/tests/run-pass/behavior/vec_iter_next.g
git status --short  # must be clean
```

---

## 1. What session 34 accomplished

### Landed / committed

**`aad6585a`** — docs(gaps): §3.9 — shape-discriminator fix was insufficient; needs live interp debug
**`370653ba`** — docs(gaps): §3.9 interp root cause — slice_length_from_value treats arrays as (ptr,len)

Both are documentation corrections after I attempted a fix that didn't work.

### Not committed but live on disk (must be reverted)

- A **shape discriminator** in `slice_length_from_value`: if `fields[0]` is `Ref|String|StaticPtr` and `fields[1]` is `Int|Uint`, treat as fat-pointer; else treat as raw array. This IS a real bugfix (returning `fields[1]` for `[10,20,30]` = 20 not 3 is wrong), but **it is NOT the interp panic's trigger** — the differential still fails with rc=100 when this fix is present.
- A `[DBG-LEN]` eprintln at the exit of `slice_length_from_value` which **never fires** during the failing differential — suggesting `Rvalue::Len` may not be the code path being executed.

---

## 2. §3.9 — accurate status

### LLVM half: **CLOSED** (commit `e8588f0d`)

`MethodDispatch::Static`'s autoref predicate was `!recv_is_ref` — a wrong test. Corrected to `pass_through = (f == recv_ty) || (&mut X -> &X coercion)`. Now `s.iter().next()` under LLVM returns the correct value (was SIGBUS/138). No fixtures restored yet because they break the differential (see below).

### Interp half: **OPEN**

Symptom: `s.iter().next()` compiles and runs under LLVM (exit 10) but panics under interp (exit 101). The captured error is `Panic("program aborted")`.

### Projection normalization: **OPEN**

`while let Some(_) = it.next() {}` on `VecIntoIter` still ICEs with `TyKind::Projection reached LLVM codegen`. Independent of both above.

### `&[T].into_iter()`: **OPEN**

`&[i32].into_iter()` fails with `[T0001] cannot dereference non-pointer type`. Points to the same projection-normalization gap.

---

## 3. What we learned this session about the interp panic (the useful part)

### The panic is `unreachable!()`, not `panic!(...)`

Instrumented the interp's extern-call dispatch:
- `"glyim_panic" | "glyim_process_abort_with_msg" =>` — DID NOT fire
- `"glyim_process_abort" =>` — FIRED, zero-arg version

`glyim_process_abort` is what `unreachable!()` and `process::abort()` call. **The panic is an `unreachable!()` reach, not a normal `panic!` with a message.**

### Which `unreachable!` it is

From `crates/glyim-lang-core/lib/slice.g`:
```rust
fn split_first(&self) -> Option<(&T, &[T])> {
    if self.is_empty() {
        Option::None
    } else {
        Option::Some((&self[0], &self[1..]))
    }
}
```
`split_first` has no `unreachable!()`.

From `crates/glyim-lang-core/lib/option.g`:
```rust
fn unwrap(self) -> T {
    match self {
        Option::Some(val) => val,
        Option::None => panic!("called `Option::unwrap()` on a `None` value"),
    }
}
```
`unwrap` uses `panic!` (which goes through `glyim_panic` with a message) — NOT what we observed.

**Where `unreachable!()` shows up**: `option.g:173` and `option.g:185` inside `insert` / `get_or_insert` — neither is on the iterator's code path.

**Most likely culprit**: a `bool`-based `Assert { cleanup: None }` in `Iter::next`'s MIR (bounds check at bb6: `Assert { cond: false, msg: BoundsCheck }`). Firing an `Assert` with cleanup=None under interp goes through the abort path without a message.

### MIR is correct post-typeck-fix

For `s.iter().next()`:
- Caller: `$8: &mut &[i32]`, `Assign(local 8, Ref(local 6, Mut))`, `Call(FnDefId(428), args: [Move(local 8)])`
- Callee `$1: &mut &[T]`, matches. No ABI mismatch.

### `slice_length_from_value` bug is real but not the trigger

Original code:
```rust
InterpValue::Aggregate(fields) => {
    if fields.len() >= 2 {
        match &fields[1] { InterpValue::Int(i) => Ok(*i as usize), ... }
    }
}
```
For `[10,20,30]` this returns `20` (the second element, not the length 3). That's a real bug — but session-34 attempt to fix it (shape discriminator) **did not stop the panic** — the differential still showed interp=101, llvm=10. The `[DBG-LEN]` line never fired in the failing run, which suggests `Rvalue::Len` isn't reaching `slice_length_from_value` on the failing path (or the instrumentation was misplaced).

---

## 4. Five wrong hypotheses accumulated (do NOT re-attempt)

1. **Fat-vs-thin ABI downstream of typeck** — this WAS the LLVM bug. Correct for LLVM. Wrong for interp (MIR matches).
2. **`eval_unary_op` Deref not applying projection** — hypothesis, never verified. The failing case has empty projection on the receiver.
3. **`split_first` returning `None` for non-empty slice** — `split_first` body has no abort path.
4. **`Len` returning 0** — original hypothesis. Real bug in `slice_length_from_value` for arrays, but fixing it doesn't stop the panic.
5. **Array-vs-fat-pointer ambiguity** — the fix I tried; insufficient (differential still fails).

**Lesson:** Static MIR analysis and hypothesis-hopping have failed five times. The next probe MUST be a live interp debug — a breakpoint on `Interpreter::run_body` via `lldb`, or a `tracing::debug!` trace enabled by an env var, or `eprintln!` at every basic block entry (with a step limit to keep it manageable).

---

## 5. Concrete next steps for the next agent

### First (mandatory): revert instrumentation

See §0 above. `git status --short` must be empty before any new work.

### Then: live interp debug

**Option A — step-limit + basic-block logging.**
Add to `Interpreter::run_body` (or the basic-block loop, near `crates/glyim-mir-interp/src/lib.rs:1865` where `stmt_idx` is tracked):
```rust
if std::env::var("GLYIM_INTERP_TRACE").is_ok() {
    eprintln!("[TRACE] owner={:?} bb={:?} depth={}",
        self.current_body_owner, bb_idx, self.frame_depth);
}
```
Then run:
```bash
GLYIM_INTERP_TRACE=1 RUSTC_WRAPPER="" \
    cargo run --quiet -p glyim-cli -- --backend=bytecode --with-stdlib \
    --emit=obj /tmp/iter_probe.g -o /tmp/iter_probe.obj 2>&1 | head -200
```
where `/tmp/iter_probe.g` is:
```g
fn main() -> i32 {
    let s: &[i32] = &[10, 20, 30];
    let mut it = s.iter();
    match it.next() { Some(x) => *x, None => 0 }
}
```
(Note: `--backend=bytecode --emit=obj` is the interp path; `--emit=exec` requires LLVM.)

Read the last 30 trace lines before the abort — the `bb_idx` in the last line tells you which basic block aborted. If it's bb6 (the bounds check `Assert { false }`), then `Len(deref(local_3))` really is returning <1 — either `slice_length_from_value` is broken (my fix should help, but didn't fire), or `read_place(deref(local_3))` isn't reading the right value.

**Option B — breakpoint.**
`lldb -- target/debug/glyim-cli --backend=bytecode --with-stdlib --emit=obj /tmp/iter_probe.g -o /tmp/iter_probe.obj`, `b Interpreter::run_body` (or line 1696 in `lib.rs`), run, `bt` when it fires. This gets the interp's own call stack, which is what all my probes have been unable to obtain.

**Option C — print every Assign / Call.**
Instrument `execute_statement` and the `TerminatorKind::Call` handler to `eprintln!` their ops + places. Combined with `GLYIM_INTERP_TRACE`, this gives the full trace. Heavy but definitive.

### Then, once the abort site is known

- If it's the bounds `Assert` at bb6 → `Len(deref(local_3))` is returning <1 → the fix is in how the interp reads `Len` on a `&[T]` that came from a `UnaryOp(Deref)` of a `&mut &[T]`. My session-34 shape-discriminator attempt was in the right place but possibly the wrong layer (`read_place`'s `Deref` may be stripping the `&[T]` incorrectly, so `slice_length_from_value` never sees the array — it sees the underlying local, whatever it is).
- If it's some other `Assert` → follow the value flow from that branch.
- If it's a real `unreachable!()` (not Assert) → the abort site is at one of `option.g:173,185` — but neither is on the iterator path, so the interp must be mis-executing an early-return or match dispatch.

---

## 6. Session-34 commit history (for reference)

```
aad6585a docs(gaps): §3.9 — shape-discriminator fix was insufficient; needs live interp debug
370653ba docs(gaps): §3.9 interp root cause — slice_length_from_value treats arrays as (ptr,len)
185708ba docs(gaps): §3.9 — interp bug narrowed to Len(cur) returning 0 for non-empty &[T]
98c3e0f4 docs(gaps): §3.9 — interp bug narrowed to Len(cur) returning 0 for non-empty &[T]
43dac5ac docs(gaps): §3.9 — interp bug narrowed to Len(cur) returning 0 for non-empty &[T]
3721e7d1 docs(gaps): §3.9 — interp panic narrowed to eval_unary_op Deref not applying projection
dc1db6cf docs(gaps): §3.9 — interp bug isolated to interp-side, MIR is correct
17886348 docs(gaps): §3.9 — document interpreter-side bug (LLVM fixed, interp still panics)
df2096a0 docs(gaps): §3.9 — LLVM SIGBUS fixed (e8588f0d); interpreter + projection remain
e8588f0d fix(typeck): autoref wraps when pointee mismatches — fixes §3.9 SIGBUS
98bcc7d4 docs(gaps): §3.9 — typeck autoref fires correctly; bug is downstream (Ref lowering or local ABI)
edf31796 docs(gaps): dedupe §3.10 — keep the CLOSED entry, drop the stale OPEN duplicate
b989ca98 docs(gaps): §3.10 CLOSED — rollback_to var-vector length fix
d3836570 fix(solve): rollback_to preserves var-vector length — closes §3.10 ICE
```

---

## 7. Traps carried forward

All previous traps plus:

- **`rm -f .git/index.lock`** — has recurred four times this session. The scripts that read `git status` sometimes find a lock left by an interrupted prior operation. Always `rm -f .git/index.lock` at the top of any git-touching script.
- **`git checkout HEAD -- crates/`** is the safe reset. The `git checkout` failure earlier was due to the lock, not the checkout itself.
- **`set -e` + `head` in pipelines** kills the script when the command exits non-zero. Use `set +e` / `set -e` around probe commands that may return non-zero, and don't `set -e` a script that will intentionally run failing probes.
- **Nextest filter `-E 'test(...)'`** is required — `-p glyim-test differential` matches zero tests (the test name isn't "differential", it's "llvm_and_interp_agree_on_run_pass_corpus").
- **`--backend=bytecode --emit=exec` is refused** — the bytecode backend can only `--emit=obj`. Interp-only probes need `--emit=obj` (which is what the differential harness does internally).
- **Rustup toolchain update kills mid-run commands** (session 34 — SIGKILL'd cargo at 22 GB free). After a rustup update, `cargo build` may need a moment.
- **`include_str!` embeds at compile time** — after editing `.g` sources, `cargo build -p glyim-lang-std -p glyim-lang-test` (or a rebuild of the test binary) is required for the change to take effect.

---

## 8. Two failures to keep separate

The session-32 handoff notes one bug (§3.9, projection ICE) but there are **two independent memory-safety bugs**:

1. **§3.9 LLVM SIGBUS** — CLOSED (e8588f0d).
2. **§3.10 trait-impl `&mut self` ICE** — CLOSED (d3836570, rollback_to fix).

The **interp panic** (this session) is a third, still-open issue. Its relationship to #1 and #2: it shares the surface (`impl Iterator for &[T]`) with #1, but the LLVM-vs-interp divergence proves it's an interp-only bug — probably in `read_place`'s handling of `UnaryOp(Deref)` returning a Ref whose pointee is itself a fat pointer.

---

## 9. Recommended scope for the next session

**Do not attempt a fix blind.** Spend the first 30-45 minutes on Option A (trace logging) or Option B (lldb). The next 30 minutes will be obvious once you have the abort bb. Budget an hour for the fix + fixtures + gate.

If after 90 minutes the trace doesn't reveal the site, **stop** and update §3.9 with the trace output. A printed trace is worth more than another hypothesis in the doc.

**Do NOT re-attempt** the shape discriminator (insufficient — see §3), `eval_unary_op` projection changes (untested hypothesis), or any of the five listed wrong hypotheses (§4). Each one has already consumed session time.
