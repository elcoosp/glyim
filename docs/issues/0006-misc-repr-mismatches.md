# ISSUE-0006 — Misc repr / int-width / type-arg mismatches

**Status:** open (compiler-feature gap — small, varied)
**Severity:** trailing errors after the big gaps close
**Verified:** 2026-08-31 via `stdlib_full_probe` (assembled stdlib = 47 errors).

## Symptom (1× each, confirmed in probe categories)
- `mismatched type argument counts` — a generic ADT instantiated with the
  wrong number of type args (likely a `Result<T, E>`/`Poll<T>` site).
- `mismatched types: Adt97 vs Adt91` — two ADTs that should be the same logical
  type but got split ids (same class as ISSUE-0005 id-splitting).
- `mismatched types: Adt1050<u8> vs str` — `String` (Adt1050) compared/assigned
  against `str` (Adt1061) without the `&str → String` conversion (ties into
  ISSUE-0003 `From`).
- `cannot dereference non-pointer type` — a `*T` deref on a non-pointer
  (raw-pointer codegen, ISSUE-0004).
- `enum-variant value paths are not yet supported` — 1 genuine site (the
  `resolved.values` fallthrough in `unify.rs:294-301`); see ISSUE-0007.

## Root cause
These are the long tail of (a) id-splitting (Adt97 vs Adt91), (b) missing
`From`/`AsRef` conversions (Adt1050 vs str), and (c) raw-pointer deref
(`cannot dereference`) that the four primary issues (0001-0004) will largely
resolve. They are NOT stdlib authoring errors — the stdlib source is correct;
the compiler cannot express these patterns yet.

## Proposed fix
Most clear as a side effect of ISSUE-0001 through ISSUE-0004. Track the two
genuinely distinct ones:
- `Adt97 vs Adt91` / `mismatched type argument counts` → confirm after
  ISSUE-0005 (id-splitting) lands; if they persist, they are a separate
  cross-module ADT-id reconciliation case (the `adt_id_for_item` all-modules
  search added this session handles most, but a specific site may still split).
- `enum-variant value path` → ISSUE-0007.

## Clears
~4 errors (type-arg-count + Adt97-vs-Adt91 + Adt1050-vs-str + cannot
dereference), minus any already cleared by 0003/0004.

## Related
- ISSUE-0001, ISSUE-0003, ISSUE-0004, ISSUE-0005, ISSUE-0007
