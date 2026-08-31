# ISSUE-0003 — Trait-method dispatch: `From`/`Into`/`AsRef`/`Display` + missing `impl` blocks

**Status:** open (compiler-feature gap)
**Severity:** blocks `Result::Err("...".into())` and `Error`/`Path`/`File` accessors
**Verified:** 2026-08-31 via `stdlib_full_probe` (assembled stdlib = 47 errors).

## Symptom
- 2× `no method into found for type` — `Result::Err("failed to spawn thread".into())`
  (crates/glyim-lang-std/lib/thread.g:170) and a `net.g` site.
- 2× `metadata` — `Metadata` accessor (`fs.g` `Metadata` impl).
- 2× `None` — `Option::None`/unwrap-style resolution.
- 2× `file` — `File` accessor.
- 1× each: `len`, `is_file`, `is_dir`, `take`, `expect`, `as_ptr`, `to_string`,
  `kind`, `digit`, `result`, `src`, `dst`, `s`, `cannot dereference`.

## Root cause (confirmed this session)
- The compiler has **no `From`/`Into`/`AsRef`/`Display` trait dispatch**. The
  `into` method call resolves via normal method resolution (`collect_for` in
  `check_expr.rs:1708` scans all `impl` blocks), but `Into<Error> for &str`
  (and its blanket `impl<T, U: From<T>> Into<U> for T`) is not registered, and
  `From<&str> for Error` has no `impl` block in the stdlib.
- The accessor methods (`metadata`, `len`, `is_file`, `is_dir`, `take`,
  `expect`, `as_ptr`, `kind`) are declared on `impl` blocks but the compiler
  does not resolve trait/impl method calls for them (trait-dispatch gap).

## Proposed fix
1. Add builtin-default `From`/`Into` dispatch (or register the needed `impl`
   blocks in the stdlib + make the compiler resolve them through `collect_for`
   / trait solver). Highest-leverage: `From<&str> for Error` and
   `From<&str> for String`.
2. Ensure `AsRef`/`Display` method dispatch resolves for `Path`/`File`/`Error`.
3. Verify the existing `collect_for` (`check_expr.rs:1738`) actually finds
   these `impl` blocks when given a resolvable trait — the earlier
   inherent-impl scan was confirmed to be a no-op, so trait-method resolution
   itself is the missing piece, not the impl-block discovery.

## Clears
~14 errors (2 into + 2 metadata + 2 None + 2 file + 1 len + 1 is_file +
1 is_dir + 1 take + 1 expect + 1 as_ptr + 1 to_string + 1 kind + 1 digit +
1 result + 1 src + 1 dst + 1 s + 1 cannot dereference).

## Related
- ISSUE-0001 (async — needs `Into`/conversions for future state)
- ISSUE-0004 (raw-pointer builtins that some accessors delegate to)
