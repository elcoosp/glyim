# ISSUE-0001 — Async desugar: ReadFuture/WriteFuture field/index/cast typing

**Status:** open (compiler-feature gap, NOT stdlib authoring)
**Severity:** blocks native async-TCP run (`--emit=exec`)
**Verified:** 2026-08-31 via `stdlib_full_probe` (assembled stdlib = 47 errors; this gap = ~15 of them)

## Symptom
The stdlib's hand-written futures (`ReadFuture`, `WriteFuture` in
`crates/glyim-lang-std/lib/fs.g` / `net.g`) do not type-check. Clusters:

- 4× `indexing operation requires array or slice type`
- 4× `invalid cast`
- 4× `unresolved path pattern` (the `ReadFuture`/`WriteFuture` variant
  patterns in `match` on the future's state enum)
- 3× `field access on non-ADT, non-tuple type`

## Affected sites (real)
- `crates/glyim-lang-std/lib/fs.g` — `ReadFuture`/`WriteFuture` `poll` impls.
- `crates/glyim-lang-std/lib/net.g` — `TcpStream` async read/write futures.
- DETAIL line numbers in the assembled probe are remapped (concatenation of
  `pub mod X` blocks) — snippet text is real, line prefixes are not; trust the
  snippet, not the `fs.g:Lnnn` attribution.

## Root cause (confirmed this session)
- `desugar_async` (`crates/glyim-hir/src/lower/lower_async.rs`) only rewrites
  `async fn`; the stdlib uses hand-written `impl Future { fn poll(...) }` with
  explicit state enums. Those futures' field/index/cast operations are never
  given correct ADT/state-enum types during typeck, because:
  1. the state-enum variant patterns (`unresolved path pattern` ×4) are not
     resolved as enum variants in `check_path`/`unify.rs` for these generated
     types, and
  2. the `Box<[u8]>`/`slice` buffer fields are not typed as slice/ADT, so
     indexing/casting on them fails.

## Proposed fix
Implement the async future typing path actually used by the stdlib:
1. Resolve `ReadFuture`/`WriteFuture` state-enum variant patterns as enum
   variants (reuse the STEP0 `Enum::Variant` handler in `unify.rs:115` already
   added for `ErrorKind`).
2. Type the future's `Box<[u8]>`/`*mut u8` buffer fields so `index`/`cast` on
   them lower correctly (ties into ISSUE-0004 raw-pointer codegen).
3. Ensure `poll` return type `Poll<T>` projects to the state-enum's `Ready`
   variant (depends on ISSUE-0002 enum-variant equality for the `Pending`/
   `Ready` comparison).

## Clears
~15 errors (4 indexing + 4 invalid cast + 4 unresolved path pattern + 3 field
access).

## Related
- ISSUE-0002 (enum-variant equality — `Pending`/`Ready`/`WouldBlock` compare)
- ISSUE-0004 (raw-pointer/buffer codegen for future state)
- `docs/plans/v0.1.0/KNOWN_GAPS.md` (async-v2 resume-dispatch)
