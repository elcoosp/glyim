# Testing work from `better-testing.md` — what was implemented

**HEAD**: `cd3aec4b` · **Matrix**: 4589 passing / 1 failing / 2 skipped

## Doc claims that were STALE (verified against code, not re-done)

| Doc claim | Reality |
|---|---|
| T064 interp stdout never captured | Already fixed (`interpreter_runner.rs:132`) |
| T196 substring stdout oracle | Already fixed (`runner.rs` line-boundary `stdout_matches`) |
| T146 CRLF mapping | Already fixed (`strategy.rs`, `T146-PATCHED`) |
| no-ice corpus all 0 bytes | False — only `empty.g` is 0 by design |
| `check-exit-code` directive missing | Exists as `exit-code:` (`config.rs:177`) |
| proptest / property missing | Both present |
| crate test counts (vm=14, opt=70…) | Wrong — different test organization |

## Implemented

### Harness corpora (`68f6097e`)
- `run-fail/` corpus (was **empty**) — 3 fixtures + `run_fail_corpus.rs`.
- `compile-pass/` corpus (was **empty**) — 5 fixtures + `compile_pass_corpus.rs`.
- 4 stdout-asserting run-pass fixtures (was 3 of 54).
- Each corpus verified to *discriminate* (fails when deliberately broken).

### Bytecode VM ≡ interpreter differential (`ffbec81d`, `fb9031a0`)
The doc's #1 priority. One test file that compiles MIR → runs on both the
interpreter and the emitted bytecode → compares results.

**It found two real emitter bugs the existing tests missed:**

1. **Jump targets never resolved.** Emitter writes block indices; VM reads
   them as byte offsets because `block_offsets` was never built. Every
   control-flow program miscompiled through emitter→VM.
   → added `generate_function_with_blocks`.
2. **`OP_RETURN` had no operand.** MIR returns local 0; VM pops the stack →
   every function failed with `EmptyReturn`.
   → emitter pushes local 0 for value-returning fns; VM treats empty as Unit.

5 differential tests, all passing.

### Optimizer behavior-preservation (`91593901`, `54441a82`)
`glyim-opt/tests/behavior_preservation.rs`: interprets a body before and
after `optimize`, asserts the result is unchanged. Includes a discriminating
case that also asserts the optimizer *actually transformed* the body.

### Solver regressions (`5dd04918`)
`int_var_cycle_does_not_diverge` (SOLVE-1), `ref_to_i32_does_not_unify_ref_str`.
A third candidate (`&T` vs `&mut T`) was dropped — `infer.rs:476` documents
that mutability is deliberately not a unification constraint.

### Borrowck legal corpus (`fceacd74`)
4 accepted programs must produce zero errors, including the MIR-24 shape
(drop-of-already-moved is legal — the fix that false-positived on it was
reverted once; this stops a re-attempt from re-breaking).

### LSP dead-feature fix (`cd3aec4b`)
`navigation::workspace_symbols` was implemented and tested but neither
advertised nor routed. Wired both + a conformance test.

## Not done (needs Linux CI / extra toolchain, not this host)

- T2 LLVM signedness fixtures — would be `Ignored` here
- T3 runtime async — needs runtime execution
- T8 three-backend differential — needs LLVM
- T11 sanitizers — no ASan/UBSan

## Honest limits recorded in the tests themselves

- The LSP conformance test pins the *expected capability set* and the
  specific regression; full "advertised ⟹ routed" needs router
  introspection `async_lsp` doesn't expose.
- The optimizer harness covers 4 bodies; the doc's full property-test
  ambition (random MIR generation) is unimplemented.
