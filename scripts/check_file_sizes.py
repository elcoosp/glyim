#!/usr/bin/env python3
"""Enforce maximum file size limits for Glyim source files."""
import sys
import os

MAX_LINES = 500
WARN_LINES = 300

def check_file(path):
    with open(path) as f:
        lines = f.readlines()
    count = len(lines)
    if count > MAX_LINES:
        print(f"ERROR: {path}: {count} lines (max {MAX_LINES})", file=sys.stderr)
        return False
    if count > WARN_LINES:
        print(f"WARN: {path}: {count} lines (warning threshold {WARN_LINES})", file=sys.stderr)
    return True

def main():
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    ok = True
    for dirpath, _, filenames in os.walk(root):
        skip = ["target", ".git", "node_modules", "vendor"]
        if any(s in dirpath.split(os.sep) for s in skip):
            continue
        for fname in filenames:
            if fname.endswith(".rs"):
                full = os.path.join(dirpath, fname)
                if not check_file(full):
                    ok = False
    if not ok:
        sys.exit(1)

if __name__ == "__main__":
    main()
