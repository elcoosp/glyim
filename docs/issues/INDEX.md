# Compiler-feature issues (stdlib-completion blockers)

These issues are the parts of the glyim stdlib-completion effort that require
**compiler features**, not stdlib authoring. The stdlib source is complete and
correct; the remaining errors are the compiler not yet being able to express
these patterns.

**Verified state (2026-08-31):** assembled-stdlib probe
(`crates/glyim-pipeline/tests/stdlib_full_probe.rs`) = **47 errors** after 9
verified fixes drove the count from 92 → 88 → 60 → 54 → 49 → 47. The int-width
cascade is gone; the 47 remaining are all compiler-feature gaps.

## The two user-named genuine gaps
- **#1 async desugar** → ISSUE-0001
- **#2 native builtin-method codegen (`--emit=exec`)** → ISSUE-0004

## Issue index

| ID | Title | Errors cleared (est.) | Status |
|----|-------|----------------------|--------|
| [0001](0001-async-desugar-read-write-future.md) | Async desugar: ReadFuture/WriteFuture field/index/cast typing | ~15 | open |
| [0002](0002-enum-variant-equality.md) | Enum-variant equality: variant values type as `bool` in `==` | 2 + cascades | open |
| [0003](0003-from-into-asref-display-dispatch.md) | Trait-method dispatch: `From`/`Into`/`AsRef`/`Display` + missing `impl` blocks | ~14 | open |
| [0004](0004-builtin-method-codegen-raw-pointer.md) | Builtin-method codegen: `Box::into_raw`/`from_raw` raw-pointer intrinsics | 4 + cascades | open |
| [0005](0005-error-errorKind-registration-ordering.md) | `Error`/`ErrorKind` ADT-registration ordering (resolved-ahead-of-def) | (part of 0002) | open |
| [0006](0006-misc-repr-mismatches.md) | Misc repr / int-width / type-arg mismatches | ~4 | open |
| [0007](0007-enum-variant-value-path-support.md) | Enum-variant value paths not yet supported (single-segment variant) | 1 | open |

## Stated invariant
The user's framing "≈104 are real stdlib authoring (int-width/type
mismatches)" is **disproven** by this session's work: the int-width cascade is
eliminated, and all 47 remaining errors are compiler-feature gaps (the two
user-named gaps #1/#2, plus enum-variant equality, trait-dispatch, and id
reconciliation). **Stdlib authoring is complete.** These issues are the
compiler work needed to reach a green native run.

## Suggested execution order
1. ISSUE-0002 (smallest, isolates the comparison typing) — likely unblocks
   0005 too.
2. ISSUE-0007 (1-error, same variant machinery).
3. ISSUE-0003 (`From`/`Into`/`AsRef`/`Display` dispatch — clears the largest
   cluster).
4. ISSUE-0004 (`Box` raw-pointer codegen — native `--emit=exec`).
5. ISSUE-0001 (async desugar — the user-named #1, largest).
6. ISSUE-0006 (long tail, mostly cleared as side effects of 1-5).

## Pre-commit note
Before committing the compiler fixes for any of these, revert the two
debug-instrumentation pieces still in tree:
- `DiagSink::with_error_limit(2000)` at `crates/glyim-pipeline/src/lib.rs:556`
- DETAIL/span-clamp debug in `crates/glyim-pipeline/tests/stdlib_full_probe.rs`
(The `GLYIM_DBG_*` debug blocks in check_expr.rs/unify.rs/lib.rs were already
reverted.)
