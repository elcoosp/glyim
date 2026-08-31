# ISSUE-0004 — Builtin-method codegen: `Box::into_raw`/`from_raw` raw-pointer intrinsics

**Status:** open (compiler-feature gap, user-named #2)
**Severity:** blocks native `--emit=exec` for `thread.spawn` / future state
**Verified:** 2026-08-31 via `stdlib_full_probe` (assembled stdlib = 47 errors).

## Symptom
- 2× `unresolved name payload_ptr` / `no method payload_ptr`
- 1× `unresolved name slot` / `no method slot`
- 1× `unresolved name slot_ptr` / `no method slot_ptr`

## Real sites (confirmed via source read)
`crates/glyim-lang-std/lib/thread.g:144-176` — `spawn` builds a
`SpawnPayload` via `Box::into_raw(Box::new(...))`, producing local `*mut u8`
vars `payload_ptr` / `slot_ptr` / `slot`. The probe DETAIL misattributes these
to `fs.g:514/516/519/521` (the `canonicalize` fn) because the assembled-file
line remap is off — the snippet text is the `thread.g` spawn code, not
`canonicalize`.

## Root cause (confirmed this session)
- `Box::into_raw` / `Box::from_raw` are registered as `Box` builtin methods
  (added this session in `crates/glyim-type/src/ty_ctx_mut.rs`), but the
  **codegen** for the raw-pointer intrinsic (turning `Box<T>` into `*mut u8`
  and back) is not implemented. So `payload_ptr`/`slot_ptr` (the `*mut u8`
  locals) become unresolved names / no-method because the value they hold is
  never lowered to a raw-pointer type the rest of `spawn` can use.
- The cascade is driven by `Box::into_raw` returning an unresolved/error type
  for the buffer, then every subsequent use of `payload_ptr`/`slot`/`slot_ptr`
  failing.

## Proposed fix
Implement the builtin-method codegen for `Box::into_raw` / `Box::from_raw` in
the `--emit=exec` path (the user-named gap #2): lower `Box<T>` precisely to
`*mut T` (`*mut u8` after erase) and back, so the `SpawnPayload`/`ResultSlot`
raw-pointer locals type-check. This is the native-backend counterpart to the
already-registered `Box` builtin *type* methods.

## Clears
4 errors (2 payload_ptr + 1 slot + 1 slot_ptr) + cascades from the unresolved
`Box::into_raw` return type.

## Related
- ISSUE-0001 (async — future state buffers are the same raw-pointer pattern)
- ISSUE-0003 (trait-dispatch that some accessors delegate to)
