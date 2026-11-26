#!/usr/bin/env python3
"""M3.1 Round 3 codemod — finish the CT-1 private-constructor ratchet by
migrating every remaining external `from_raw(` call site onto the sanctioned
`from_usize` factory, and `IndexVec::from_raw` onto `IndexVec::from_vec`.

Background: plan §6 M3.1 ("CT-1 private-constructor IDs", DoD "`from_raw`
outside arena modules = 0 (ledger metric)"). Round 2 (R2, commit fac33c8c)
did this for `glyim-span`'s `FileId`/`ByteIdx`. This round covers the rest of
the workspace after `glyim-core`'s `define_idx!`/`define_def_id!` families
and `Idx<T>` gained public `from_usize(usize)` factories and `#[cfg(test)]`
`from_raw` escape hatches (mirroring the R2 precedent), and the `IdxLike`
raw-wrap machinery was renamed `from_u32` (it is `IndexVec` internals, not a
public constructor), and `IndexVec::from_raw(Vec<T>)` was renamed
`from_vec` (it wraps a `Vec<T>`, not a raw handle — the shared name made the
CT-1 metric count a legitimate non-ID constructor).

Mechanical, order-sensitive rewrite per call site (balanced-paren aware,
comment lines untouched):

  1. `T::from_raw(EXPR as u32)` -> `T::from_usize(EXPR)`  (the cast
     narrowed a `usize` slot to `u32`; `from_usize` takes `usize`, so the
     cast is removed, not renamed).
  2. `T::from_raw(EXPR as usize)` -> `T::from_usize(EXPR)`.
  3. `T::from_raw(EXPR.to_raw())` -> `T::from_usize(EXPR.index()`
     (`.index()` is the same value as `.to_raw()` widened to `usize`, which
     is exactly what `from_usize` expects).
  4. `IndexVec::from_raw(VEC)` -> `IndexVec::from_vec(VEC)` (any receiver
     path ending in `IndexVec`).
  5. `T::from_raw(EXPR)` -> `T::from_usize(EXPR)`.

Rules 1-3 must run before rule 5 so no cast survives inside `from_usize(...)`.
The remaining sites whose argument is a `u32`-typed expression (e.g.
`builtin.adt_id()`, `u32::MAX - 1`, `u32` counter fields) do NOT compile
under rule 5 — they are fixed type-aware in the same PR, either by widening
at the call site (`usize::from(...)`) or by changing the producer to hand
out `usize` slots.

Usage: `python3 scripts/codemods/m3-1-r3-from-raw-to-from-usize.py <files...>`
"""
from __future__ import annotations
import re
import sys

# glyim-core modules own the raw escape hatches and the (renamed) trait
# machinery; their `from_raw` sites are definitions under `#[cfg(test)]` and
# are exempt from the metric and from this codemod.
EXEMPT = ("crates/glyim-core/src/arena.rs", "crates/glyim-core/src/def_id.rs")

FROM_RAW = re.compile(r"(?<![A-Za-z0-9__.])((?:[A-Za-z_][A-Za-z0-9_]*::)*[A-Za-z_][A-Za-z0-9_]*)::from_raw\(")


def balanced_arg(text: str, open_idx: int) -> tuple[int, str]:
    """Return (index after the matching close paren, argument text) for the
    paren opened at `open_idx`."""
    depth = 0
    in_str = False
    i = open_idx
    while i < len(text):
        c = text[i]
        if in_str:
            if c == "\\":
                i += 1
            elif c == '"':
                in_str = False
        elif c == '"':
            in_str = True
        elif c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
            if depth == 0:
                return i + 1, text[open_idx + 1:i]
        i += 1
    raise ValueError("unbalanced parens")


def rewrite_arg(arg: str) -> str:
    a = arg.strip()
    m = re.fullmatch(r"(.+?)\s+as\s+u32", a, re.S)
    if m and " as u32" not in m.group(1):
        return m.group(1).strip()
    m = re.fullmatch(r"(.+?)\s+as\s+usize", a, re.S)
    if m:
        return m.group(1).strip()
    m = re.fullmatch(r"([A-Za-z_][A-Za-z0-9_.:\s()\[\]]*?)\.to_raw\(\)", a)
    if m:
        return f"{m.group(1)}.index()"
    return a


def mask_comments(src: str) -> str:
    """Same-length copy of `src` with the contents of `//...` line comments
    and `///`/`//!` doc comments blanked, so pattern matches on the mask
    align exactly with positions in the original text. String literals are
    preserved verbatim so comment-looking text inside strings is kept."""
    out = list(src)
    i = 0
    n = len(src)
    in_str = False
    while i < n:
        c = src[i]
        if in_str:
            if c == "\\":
                i += 1
            elif c == '"':
                in_str = False
            i += 1
            continue
        if c == '"':
            in_str = True
            i += 1
            continue
        if c == "/" and i + 1 < n and src[i + 1] == "/":
            j = src.find("\n", i)
            if j == -1:
                j = n
            for k in range(i, j):
                out[k] = " "
            i = j
            continue
        i += 1
    return "".join(out)


def process(path: str) -> int:
    src = open(path, encoding="utf-8").read()
    masked = mask_comments(src)
    out = []
    pos = 0
    changed = 0
    while True:
        m = FROM_RAW.search(masked, pos)
        if not m:
            out.append(src[pos:])
            break
        name = m.group(1)
        start = m.start()
        open_idx = m.end() - 1
        end, arg = balanced_arg(masked, open_idx)
        if name.endswith("IndexVec"):
            new_arg = arg
            new_call = f"{name}::from_vec({arg})"
        else:
            new_arg = rewrite_arg(arg)
            new_call = f"{name}::from_usize({new_arg})"
        if arg != new_arg or "from_raw" in new_call:
            changed += 1
        # untouched segments come from the ORIGINAL text (the mask only
        # blanks comments; positions align 1:1)
        out.append(src[pos:start])
        out.append(new_call)
        pos = end
    new_src = "".join(out)
    if new_src != src:
        open(path, "w", encoding="utf-8").write(new_src)
    return changed


def main() -> None:
    total = 0
    files = sys.argv[1:]
    if not files:
        import os
        for base in ("crates", "tools", "fuzz"):
            if not os.path.isdir(base):
                continue
            for dirpath, dirnames, filenames in os.walk(base):
                dirnames[:] = [d for d in dirnames if d not in ("target", ".git")]
                for fn in filenames:
                    if fn.endswith(".rs"):
                        rel = os.path.relpath(os.path.join(dirpath, fn))
                        if rel in EXEMPT:
                            continue
                        files.append(rel)
    for f in files:
        total += process(f)
    print(f"rewrote {total} site(s) across {len(files)} file(s)")


if __name__ == "__main__":
    main()
