#!/usr/bin/env bash
set -euo pipefail

# P2-T1 (production-readiness plan §11.1): the previous version set
# FAILED=1 inside a `find | while` pipeline subshell, so the outer check
# always saw 0 — the gate could never fail. `lastpipe` keeps the loop in
# the current shell.
shopt -s lastpipe

echo "Checking for bare tracing::warn fallbacks in critical crates..."
FAILED=0

# List of crates to check (those that should never silently fall back)
CRATES=("glyim-hir" "glyim-lower" "glyim-typeck" "glyim-codegen-llvm" "glyim-mir-interp")

for crate in "${CRATES[@]}"; do
    # Find any line that has tracing::warn! and then a fallback value (return, 0, etc.)
    # We'll just search for tracing::warn and then "return" or "0" on the same line or next line.
    # This is a heuristic, but better than nothing.
    if find ./crates/$crate/src -name "*.rs" -exec grep -l "tracing::warn" {} \; | while read -r file; do
        if grep -A2 "tracing::warn" "$file" | grep -E "(return|=>\s*0|=>\s*None|=>\s*Unit)" > /dev/null; then
            echo "FAIL: $file contains tracing::warn with a fallback"
            FAILED=1
        fi
    done; then
        :
    fi
done

if [ $FAILED -eq 1 ]; then
    echo "ERROR: Found tracing::warn fallbacks. Please use the stub! macro or unreachable! for these cases."
    exit 1
else
    echo "All good: no risky tracing::warn fallbacks found."
    exit 0
fi
