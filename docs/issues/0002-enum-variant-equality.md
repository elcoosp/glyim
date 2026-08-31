# ISSUE-0002 — Enum-variant equality: variant values type as `bool` in `==`

**Status:** open (compiler-feature gap)
**Severity:** blocks `Error::kind()`-based control flow in stdlib
**Verified:** 2026-08-31 via `GLYIM_DBG_BINOP` instrumentation on
`stdlib_full_probe` (io.g:449 `e.kind() == ErrorKind::Interrupted`).

## Symptom
2× `mismatched types: Adt63 vs bool` (the `Adt63` is the DiagSink display
renumber of `ErrorKind`; `Error` is AdtId 14, `ErrorKind` is AdtId 17 — both
correctly distinct per `GLYIM_DBG_ADTID`).

Real sites (confirmed via instrumentation):
- `crates/glyim-lang-std/lib/io.g:449` — `Result::Err(ref e) if e.kind() == ErrorKind::Interrupted => continue`
- `crates/glyim-lang-std/lib/io.g` / `net.g` `read`/`copy` match guards using
  `e.kind() == ErrorKind::WouldBlock` / `ErrorKind::Interrupted`.

## Root cause (confirmed this session)
- `e.kind()` correctly resolves to `ErrorKind` (AdtId 17). The RHS
  `ErrorKind::Interrupted` (a **unit** enum variant) resolves to `bool`
  instead of `ErrorKind` when used as a `==` operand, so
  `unify(ErrorKind, bool)` fails.
- A unit enum variant value must carry its *enum* type (`ErrorKind`), not
  `bool`, in the comparison-operand context. The variant-value resolution
  (`unify.rs` `check_path` `VariantRef`/`VariantCtor`, `variant_expr`) returns
  the enum type for the variant, but the `==` operand path collapses it to
  `bool`.

## Proposed fix
In `check_path` / `variant_expr` (`crates/glyim-typeck/src/unify.rs`), when a
unit enum variant is used as a `==`/`!=` operand, type it as its enum
(`ErrorKind`) so `unify(ErrorKind, ErrorKind)` succeeds. The `==` handler at
`check_expr.rs:245` already returns `Ty::BOOL` unconditionally for `Eq`/`Ne`;
only the operand unification must agree.

## Clears
2 errors + likely cascades (any `match`/`if` on `e.kind()` comparison that
currently emits secondary diagnostics).

## Related
- ISSUE-0001 (async — `Pending`/`Ready`/`WouldBlock` enum compares)
- ISSUE-0007 (single enum-variant-as-value-path support)
