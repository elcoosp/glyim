#!/usr/bin/env bash
# M3.1 Round 2 codemod — migrate span-handle construction off the now-private
# `from_raw` escape hatch onto the sanctioned `from_usize` factory.
#
# Background: plan §6 M3.1 ("CT-1 private-constructor IDs"). After
# `glyim-span` tightened `FileId::from_raw` / `ByteIdx::from_raw` to
# `pub(crate)`, every external call site must use the public
# `FileId::from_usize` / `ByteIdx::from_usize` factories instead.
#
# This is a mechanical, order-sensitive rewrite:
#   1. `ByteIdx::from_raw(EXPR as u32)` -> `ByteIdx::from_usize(EXPR)`
#      (the lexer passes `usize` cursors through `as u32`; `from_usize`
#      takes `usize`, so the cast is removed, not just renamed).
#   2. `FileId::from_raw(EXPR as u32)` -> `FileId::from_usize(EXPR)` (none
#      today, but defensive).
#   3. `FileId::from_raw(EXPR)` -> `FileId::from_usize(EXPR)`.
#   4. `ByteIdx::from_raw(EXPR)` -> `ByteIdx::from_usize(EXPR)`.
#
# The `as u32` removals (steps 1-2) MUST run before the generic renames
# (steps 3-4), otherwise the cast would survive inside `from_usize(...)` and
# narrow a `usize` to `u32` only to widen it back. `EXPR` is matched
# greedily up to the first `)`; every current call site has no nested
# parens, so the pattern is exact. `SyntaxContext`/`ExpnId` have no external
# `from_raw` callers (verified workspace-wide) so they need no migration.
set -euo pipefail

for f in "$@"; do
  sed -i \
    -e 's/ByteIdx::from_raw(\([^)]*\) as u32)/ByteIdx::from_usize(\1)/g' \
    -e 's/FileId::from_raw(\([^)]*\) as u32)/FileId::from_usize(\1)/g' \
    -e 's/FileId::from_raw(\([^)]*\))/FileId::from_usize(\1)/g' \
    -e 's/ByteIdx::from_raw(\([^)]*\))/ByteIdx::from_usize(\1)/g' \
    "$f"
done
