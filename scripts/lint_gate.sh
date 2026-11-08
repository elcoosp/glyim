#!/usr/bin/env bash
# M0/M1.1 enforced clippy gate, exactly as documented in the root manifest:
#   cargo clippy <crates> --all-targets --all-features -- \
#     -D warnings -W clippy::pedantic -A clippy::<each manifest allow>
# The `-A` re-asserts are DERIVED from [workspace.lints.clippy] in the root
# Cargo.toml so the manifest stays the single source of truth (CLI -W would
# otherwise override the manifest's rationale-annotated allows on
# dependency crates too).
set -euo pipefail

cd "$(dirname "$0")/.." || exit 1

# Derive the allow list from the root manifest (sed-free, POSIX awk).
ALLOWS=$(awk '
    /^\[workspace\.lints\.clippy\]/ { insec = 1; next }
    /^\[/ { insec = 0 }
    insec && /= "allow"/ {
        line = $0
        sub(/=.*$/, "", line)
        gsub(/[[:space:]]/, "", line)
        if (line != "") print "-A", "clippy::" line
    }
' Cargo.toml)

echo "gate: -D warnings -W clippy::pedantic + $(printf '%s\n' "$ALLOWS" | grep -c clippy) manifest-derived re-asserts"
exec cargo clippy "$@" --all-targets --all-features -- -D warnings -W clippy::pedantic $ALLOWS
