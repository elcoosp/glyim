#!/usr/bin/env python3
"""M3.1 codemod: migrate `glyim-lsp` off the removed `from_raw` escape hatch.

The M3.1 round made the span handles' `from_raw` constructors
`#[cfg(test)]`-only (`crates/glyim-span/src/lib.rs`), but the LSP crate
kept calling them, so the crate (and the workspace) no longer compiled.
The sanctioned production path is `from_usize` (documented on every
handle); this codemod rewrites the 14 call sites mechanically:

- `X::from_raw(<int literal>)` -> `X::from_usize(<literal>)`
- `ByteIdx::from_raw(u32::from(<rowan TextSize>))`
   -> `ByteIdx::from_usize(u32::from(<expr>) as usize)`
- `ByteIdx::from_raw(<usize> as u32)` -> `ByteIdx::from_usize(<usize>)`
- `FileId::from_raw(<u32>)` -> `FileId::from_usize(<u32> as usize)`

Run from the repository root: `python3 scripts/codemods/m3_1_from_usize_lsp.py`.
"""

from __future__ import annotations

import re
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]

FILES = [
    "crates/glyim-lsp/src/database.rs",
    "crates/glyim-lsp/src/diagnostics.rs",
    "crates/glyim-lsp/src/goto_declaration.rs",
    "crates/glyim-lsp/src/moniker.rs",
    "crates/glyim-lsp/src/prepare_rename.rs",
    "crates/glyim-lsp/src/selection_range.rs",
]


def rewrite(text: str) -> tuple[str, int]:
    count = 0

    # ByteIdx::from_raw(u32::from(range.start())) — rowan TextSize -> u32
    # -> usize (both conversions are real, so no useless_conversion).
    def textsize(match: re.Match[str]) -> str:
        nonlocal count
        count += 1
        inner = match.group(1)
        return f"ByteIdx::from_usize(u32::from({inner}) as usize)"

    text = re.sub(
        r"ByteIdx::from_raw\(u32::from\(([^()]+(?:\([^()]*\))?[^()]*)\)\)",
        textsize,
        text,
    )

    # ByteIdx::from_raw(start as u32) — the operand is already a usize.
    def usize_operand(match: re.Match[str]) -> str:
        nonlocal count
        count += 1
        return f"ByteIdx::from_usize({match.group(1).strip()})"

    text = re.sub(
        r"ByteIdx::from_raw\(([A-Za-z_][A-Za-z0-9_]*)\s*as\s*u32\)",
        usize_operand,
        text,
    )

    # FileId::from_raw(self.next_id) — u32 operand, widened losslessly.
    def u32_operand(match: re.Match[str]) -> str:
        nonlocal count
        count += 1
        return f"FileId::from_usize({match.group(1)} as usize)"

    text = re.sub(r"FileId::from_raw\((self\.[A-Za-z0-9_]+)\)", u32_operand, text)

    # X::from_raw(<int literal>) — literals infer as usize.
    def literal(match: re.Match[str]) -> str:
        nonlocal count
        count += 1
        return f"{match.group(1)}::from_usize({match.group(2)})"

    text = re.sub(r"\b(FileId|ByteIdx)::from_raw\((\d+)\)", literal, text)
    return text, count


def main() -> None:
    total = 0
    for rel in FILES:
        path = REPO / rel
        text = path.read_text()
        rewritten, count = rewrite(text)
        if count:
            path.write_text(rewritten)
        total += count
        print(f"{rel}: {count} site(s)")
    print(f"total: {total} site(s) migrated")


if __name__ == "__main__":
    main()
