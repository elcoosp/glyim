# ISSUE-0007 — Enum-variant value paths not yet supported (single-segment variant as value)

**Status:** open (compiler-feature gap)
**Severity:** low (1 genuine error; blocks single-segment variant-value syntax)
**Verified:** 2026-08-31 via `stdlib_full_probe` (1× `enum-variant value paths
are not yet supported`).

## Symptom
1× `enum-variant value paths are not yet supported` — the `resolved.values`
fallthrough in `crates/glyim-typeck/src/unify.rs:294-301` when a value-namespace
path resolves to a LocalDefId that is neither an `FnDef` nor a `Const` (i.e. a
single-segment enum variant used as a value, e.g. `Ok(x)` / `None` /
`Interrupted` without the `Enum::` prefix).

## Real site
The probe DETAIL attributes it to `fs.g:273` (a `canonicalize` comment) — this
is a **misattribution** from the assembled-file line remap. The real trigger is
a single-segment enum-variant value used somewhere in the assembled stdlib
(e.g. a `match` scrutinee or `let` binding that references a variant without its
enum prefix). Trust the category, not the `fs.g:L273` line.

## Root cause (confirmed this session)
- `check_path` (`unify.rs`) has a STEP0 `Enum::Variant` handler
  (two-segment `Enum::Variant`, added this session) and a `resolved.values`
  branch for variant ctors/refs, but the **single-segment** variant-as-value
  case falls through to the "not yet supported" error instead of being treated
  as a `VariantRef`/`VariantCtor`.
- Two-segment `ErrorKind::Interrupted` works (modulo ISSUE-0002 typing); the
  single-segment form is what the fallthrough rejects.

## Proposed fix
In `unify.rs:294-301` (`resolved.values` branch), when the resolved value is a
variant LocalDefId, build a `VariantRef` (unit) or `VariantCtor` (data) instead
of emitting "enum-variant value paths are not yet supported". Reuse the same
construction `variant_expr` already does for the two-segment case.

## Clears
1 error.

## Related
- ISSUE-0002 (enum-variant equality — same variant-resolution machinery)
- ISSUE-0005 (ErrorKind id-splitting at the comparison site)
