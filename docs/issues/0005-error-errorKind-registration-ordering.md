# ISSUE-0005 — `Error`/`ErrorKind` ADT-registration ordering (resolved-ahead-of-def)

**Status:** open (compiler-feature gap — narrow, high-leverage)
**Severity:** contributes to the Adt63-vs-bool comparison failures
**Verified:** 2026-08-31 via `GLYIM_DBG_ADTID` + `GLYIM_DBG_BINOP` instrumentation.

## Symptom
`e.kind() == ErrorKind::Interrupted` (io.g:449) reports
`mismatched types: Adt63 vs bool`. `Error` = AdtId 14, `ErrorKind` = AdtId 17
(both correctly distinct at registration per `GLYIM_DBG_ADTID`).

## Root cause (confirmed this session)
- There is already a **Pass 1** (typeck lib.rs:280-300) that registers every
  ADT `name → id` up front specifically so forward references (`Error::kind`
  returns `ErrorKind`, defined later in `mod io`) resolve.
- However, `Error::kind`'s declared return type `ErrorKind` is still observed
  resolving to `Error`'s id (14) in the `==` operand (`BINOP_EQ_FAIL lhs=Error
  rhs=Adt(17)`), meaning the method return type is resolved with a stale id
  before `ErrorKind` is fully registered, while `ErrorKind::Interrupted`
  (resolved later via the variant path) gets the real `ErrorKind` id (17).
- Net: `e.kind()` (LHS) and `ErrorKind::Interrupted` (RHS) get different ids
  for the same logical type → the `==` fails.

## Proposed fix
Ensure `Error::kind`'s return type (and all method return types that name a
type defined later in the module) resolve to the *registered* `ErrorKind` id,
not a stale/synthetic one. Two safe options:
1. Guarantee method-signature resolution runs strictly after Pass 1's
   `name → id` map is complete (today `Error::kind` may be checked during Pass
   2 before `ErrorKind`'s full def is registered).
2. Make `return`-type `ErrorKind` resolution go through `adt_id_by_name`
   (which Pass 1 populated) instead of a fresh `next_synthetic_adt_id`.

This overlaps ISSUE-0002 (the RHS types as `bool`); fixing both yields a clean
`e.kind() == ErrorKind::Interrupted`.

## Clears
Contributes to the 2 `Adt63 vs bool` errors (with ISSUE-0002).

## Related
- ISSUE-0002 (enum-variant equality — same comparison site)
- ISSUE-0007 (single variant-as-value-path support)
