# Audit status tracker

Companion to `glyim-bug-and-performance-audit.md`. Records which findings
have been **verified fixed** or **verified stale** (not reproducible) against
the current HEAD, so future sessions do not re-investigate them.

> **Canonical status is in `KNOWN_GAPS.md`.** This file tracks audit findings
> against their original IDs. When the two disagree, `KNOWN_GAPS.md` wins.
> The audit-status file is a per-finding checklist; `KNOWN_GAPS.md` is the
> authoritative gap register.

**Snapshot:** HEAD `40226c1d`, rebuilt 2026-11-02.
**Test count:** 4642 / 4642 (2 skipped) under
`GLYIM_LLVM=1 GLYIM_TEST_REAL_LLVM=1 --features glyim-test/real-llvm`.

**Rebuild before re-verifying:**
```
cargo build -p glyim-runtime -p glyim-cli
cargo nextest run --workspace     # full green gate
```

**Verification method:** `glyim-cli --emit=mir` / `--emit=llvm-ir` + a
manual probe of the exact source the audit described.

---

## Criticals — verified FIXED (behavior-confirmed)

Each entry was checked by building the compiler, running the audit's exact
repro, and observing correct behavior.

| ID | Evidence |
|----|----------|
| HIR-1 | `$()*` empty repetition terminates (matcher `new_i == i` guard). |
| HIR-2 | depth-aware bindings; `stmts!(1+2, 3)` expands to 2, not 6. |
| HIR-3 | nested repetition `$( $( $x ),* ),*` expands. |
| HIR-4 | multi-metavar patterns match (`consume_fragment`). |
| HIR-10 | async tail rewrites the outermost block. |
| HIR-11 | statements between/after awaits run (interp → 103). |
| HIR-29 | `match f()` calls `f` once (MIR has 1 `Call`). |
| HIR-30 | `loop { break 42; }` writes `break_place` (MIR has `Int(42)`). |
| HIR-31 | `&mut` capture aliases (interp `c == 2`). |
| FE-6 | deep unary chain / array nesting → `P0001` (no hang). |
| SOLVE-1 | int-var cycle repro no longer hangs. |
| SOLVE-7 | `can_coerce` checks pointees with correct precedence. |
| SOLVE-8 | blanket `impl<T: Foo> Foo for T {}` compiles clean. |
| MIR-1 | const-prop uses a worklist; no stale block-exit substitution. |
| MIR-6 | DCE counts `Drop` terminators as uses. |
| MIR-10 | drop-elaboration emits `new_blocks` without clobbering CFG. |
| MIR-13 | interpreter truncates to the dest width (`trunc_to_ty`). |
| MIR-14 | `IntToInt` casts truncate/sign-extend correctly. |
| MIR-17 | enum field reads/writes both account for the tag. |
| MIR-21 | `places_conflict` handles `Index` projections. |
| RT-3 | VM `Drop` wire format matches the emitter. |
| RT-4 | `OP_LEN` operand format consistent. |
| RT-11 | bytecode bool switch branches not inverted (e2e exit 0). |
| RT-12 | `block_offsets` table produced. |
| RT-31 | proc-macro tempdir persisted past dlopen. |
| LL-1 | signed/unsigned compare picks `SLT`/`ULT` by operand type. |
| LL-2 | widening uses `build_int_s_extend` (sext), not zext. |
| LL-7 | all allocas emitted in `entry`, not per-loop. |
| LL-9 | ZST args (`fn f(u: (), x: i32)`) compile. |
| LL-10 | `&&`/`||` short-circuit (no eager div-by-zero). |
| INF-11 | LSP source map uses UTF-16 columns. |
| INF-12 | identifier extraction uses `char_indices`. |
| INF-13 | rename edits only the identifier. |
| INF-16 | LSP document sync (`didOpen`/`didChange`) wired. |
| INF-23 | array/slice drop glue emits the per-element loop. |

## Criticals — verified STALE (not reproducible)

| ID | Why |
|----|-----|
| LL-6 | struct literal operands already in declaration order. |
| LL-11 | `resolve_trait_method_fn` probe rollback present. |
| SOLVE-2 | scoped fix via SOLVE-1; blanket chain-follow intentionally absent. |
| MIR-11 | MAY-init consumed safely (tested `if c { s = make(); }`). |
| MIR-24 | audit's move-analysis fix produces false positives; reverted. |
| RT-13 | interpreter `--backend=bytecode` array index works (stride ok). |
| HIR-4 | (also listed fixed) multi-metavar works. |

---

## Front-end (FE) findings

### Verified FIXED

| ID | Evidence |
|----|----------|
| FE-3 | chained casts `x as u8 as u32` — `if`→`while` in `parse_cast_expr`. |
| FE-4 | (stale — already correct pre-audit) |
| FE-5 | (stale — already correct pre-audit) |
| FE-6 | see Criticals above. |
| FE-7 | `:literal` accepts `true`/`false` (they lex as `KwTrue`/`KwFalse`). |
| FE-8 | (stale — already correct pre-audit) |
| FE-10 | labeled `break 'a;` / `continue 'a;` in statement position. |
| FE-12 | `pub(in path)` visibility accepted. |
| FE-13 | `extern crate name;` consumes the `;`. |
| FE-14 | `const _: T = ..;` / `static _: T = ..;` no spurious error. |
| FE-15 | `fn f(&x: &i32)` no longer ICEs. |
| FE-16 | (stale — already errors pre-audit) |
| FE-17 | `1e_` rejected (not parsed as float). |
| FE-18 | unterminated block comment now reported. |

### REAL but NOT fixed (need more care)

| ID | Why deferred |
|----|--------------|
| FE-9 | `x @ subpat` — naive `parse_pat` edit changes CST for unrelated programs (`current_kind()` flushes trivia into the open `PatIdent`, absorbing trailing whitespace). Needs a trivia-preserving approach. |
| FE-11 | leading `|` in a match arm — same `parse_pat` hazard. |

Both are the only parser-side items still open. Everything else in the
FE series is either fixed or was stale at audit time.

---

## Mediums / Highs — status

The audit's ~120 High/Medium findings in MIR / RT / INF / HIR / LL have **not**
all been individually re-verified against the current HEAD. Treat their
severity labels as unconfirmed until reproduced.

The following were confirmed STALE by re-running the audit's repros against
a prior HEAD. They are **not regressions and need no action**:

FE-4, FE-5, FE-8, FE-16, HIR-9, HIR-13, HIR-14, HIR-21, HIR-22, HIR-23,
HIR-24, HIR-27, HIR-28, MIR-2, MIR-3, MIR-4, MIR-5, MIR-7, MIR-15, MIR-20,
MIR-23, SOLVE-4, SOLVE-5, SOLVE-10, SOLVE-12, SOLVE-15, SOLVE-16, SOLVE-17,
RT-9, RT-14, RT-15, RT-18, RT-20, RT-29, LL-2 (sext present), LL-5 (index
sext), LL-9 (ZST), plus the whole Criticals set listed above.

Everything else is **unverified**. If picking up an audit finding, re-run
its repro against HEAD first; do not fix based on the audit's description.

---

## Recent probe results (idiomatic Rust snippets)

Run against HEAD `40226c1d` with `--with-stdlib`:

```
  p1: FAIL — [X0000] add: mismatched types @0..0 
  p2: FAIL — [T0001] unresolved struct `Some` @120243..120278 [T0001] unresolved name `x` @120264..120266 
  p3: PASS
  p4: PASS
  p5: FAIL — [T0001] no method `iter` found for type @120252..120260 [T0001] no method `sum` found for type @120252..120266 
  p6: PASS
  p7: FAIL — [T0001] unresolved struct `Some` @120269..120322 
  p8: PASS
  p9: PASS
  p10: FAIL — [T0001] unresolved struct `Some` @120230..120265 [T0050] non-exhaustive match: missing variants `Some` @120230..120265 
  p11: PASS
  p12: PASS```

**Interpretation:** pass = the snippet compiles and runs; fail = a real
language-surface gap in an idiomatic Rust pattern. Failures are candidates
for the next bounded fix (write the failing snippet as a
`compile-pass/` fixture, then fix the underlying gap).

---

## Stale claims removed from this file

The prior revision of this file claimed:
- `4530/4530 tests pass` — actual is 4642 (this was true at some earlier
  HEAD; the entry drifted as tests were added).
- "**Medium + Performance recon (verified against a fresh CLI)** — fixed
  this session" — those fixes were landed in earlier sessions; the file
  is a snapshot, not a rolling log. Removed the "this session" framing.
- `docs/roadmaps/glyim-fix-plan.md` was accidentally staged at commit
  `6bab26ed` — it is NOT part of this repo's history. Preserved here as
  a note so it isn't confused with repo state.

---

## How to re-verify any single item

```
cargo build -p glyim-runtime -p glyim-cli
cat > /tmp/probe.g <<'EOF'
// the audit's repro, verbatim
EOF
target/debug/glyim-cli --with-stdlib /tmp/probe.g --emit=exec -o /tmp/probe.bin
/tmp/probe.bin; echo "exit=$?"
```

If the observed behavior differs from the audit's claim, the finding is
either (a) fixed since the audit, or (b) stale (never existed as described).
Either way, note it in this file with the current HEAD hash.
