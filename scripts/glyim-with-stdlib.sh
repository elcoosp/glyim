#!/usr/bin/env bash
# Compile a .g file together with the assembled stdlib and a generated
# prelude so user programs can use `println`, `Vec`, `Option`, `Result`,
# `String`, `Box`, etc. without explicit `use` statements.
#
# Usage:
#   scripts/glyim-with-stdlib.sh INPUT.g [glyim-cli args...]
#
# Depends on /tmp/glyim_assembled_stdlib.g and
# /tmp/glyim_assembled_modules.txt (produced by
# `cargo run -q -p glyim-lang-std --example dump_assembled`).

set -uo pipefail

WORKSPACE_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GLYIM="$WORKSPACE_ROOT/target/debug/glyim-cli"

if [ ! -x "$GLYIM" ]; then
  echo "glyim-cli not built; run: cargo build -p glyim-cli" >&2
  exit 2
fi
if [ $# -lt 1 ]; then
  echo "usage: $0 INPUT.g [glyim-cli args...]" >&2
  exit 2
fi

INPUT="$1"
shift

STDLIB=/tmp/glyim_assembled_stdlib.g
MODS=/tmp/glyim_assembled_modules.txt
if [ ! -s "$STDLIB" ] || [ ! -s "$MODS" ]; then
  cargo run -q --manifest-path "$WORKSPACE_ROOT/Cargo.toml" \
    -p glyim-lang-std --example dump_assembled -- "$STDLIB" "$MODS" >&2
fi

PRELUDE=/tmp/glyim_prelude.g
: > "$PRELUDE"
while IFS= read -r m; do
  [ -z "$m" ] && continue
  printf 'use %s::*;\n' "$m" >> "$PRELUDE"
done < "$MODS"

COMBINED="/tmp/glyim_combined_$$.g"
{
  cat "$STDLIB"
  printf '\n'
  cat "$PRELUDE"
  printf '\n'
  cat "$INPUT"
} > "$COMBINED"

"$GLYIM" "$COMBINED" "$@"
EC=$?
rm -f "$COMBINED"
exit $EC
