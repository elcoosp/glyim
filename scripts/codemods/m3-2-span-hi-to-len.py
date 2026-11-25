#!/usr/bin/env python3
"""M3.2 span codemod: `Span { file, lo, hi }` -> `Span { file, lo, len }`.

The M3.2 plan item (glyim-master-plan.md §6 M3.2, §5.2 CT-3) reshapes
`glyim_span::Span` to the `(lo, len)` representation with private fields:

- every `Span::new(file, lo, hi, ctx)` call site becomes
  `Span::range(file, lo, hi, ctx)` (the checked `(lo, hi)` bridge);
- direct field reads `x.file` / `x.lo` / `x.hi` / `x.ctx` become the
  accessor calls `x.file()` / `x.lo()` / `x.hi()` / `x.ctx()`, because the
  struct fields are now private.

The rewrite is mechanical; `cargo check` is the oracle afterwards (any
accessor that landed on a non-Span receiver shows up as a compile error
and is fixed by hand). Run from the repository root:

    python3 scripts/codemods/m3-2-span-hi-to-len.py

Idempotent: files that already use the new API are skipped (the accessor
rewrite does not touch `.hi()` forms, and `Span::range` no longer matches
`Span::new`).
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]

# The crate that owns the new API, and the new-API test module: both already
# use the post-reshape spelling and must not be rewritten.
SKIP = {
    REPO / "crates/glyim-span/src/lib.rs",
    REPO / "crates/glyim-span/src/tests/checked_span_tests.rs",
}

FIELD_RE = re.compile(
    r"""
    \.(?P<field>file|lo|hi|ctx)      # the private Span fields
    (?!\()                           # not already an accessor call
    (?![A-Za-z0-9_])                 # not a longer identifier (.hi_end)
    """,
    re.VERBOSE,
)

# Whitespace/line-spanned `Span::new(` -> `Span::range(` (call sites pass the
# same four arguments; only the constructor name changes).
CTOR_RE = re.compile(r"\bSpan::new\s*\(")


def rewrite(text: str) -> tuple[str, int]:
    """Apply both rewrites; return (new_text, replacement_count)."""
    count = 0

    def field_sub(m: re.Match[str]) -> str:
        nonlocal count
        count += 1
        return f".{m.group('field')}()"

    text, ctor_count = CTOR_RE.subn("Span::range(", text)
    count += ctor_count
    text = FIELD_RE.sub(field_sub, text)
    return text, count


def main() -> int:
    roots = [REPO / "crates", REPO / "tools", REPO / "examples"]
    total = 0
    touched: list[Path] = []
    for root in roots:
        if not root.is_dir():
            continue
        for path in root.rglob("*.rs"):
            resolved = path.resolve()
            if resolved in SKIP:
                continue
            # Codemod sources themselves and cargo artifacts are not call sites.
            if "/target/" in str(path) or "/codemods/" in str(path):
                continue
            text = path.read_text(encoding="utf-8")
            new_text, count = rewrite(text)
            if count:
                path.write_text(new_text, encoding="utf-8")
                touched.append(path)
                total += count
    for path in sorted(touched):
        print(f"rewrote {path.relative_to(REPO)}")
    print(f"total replacements: {total}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
