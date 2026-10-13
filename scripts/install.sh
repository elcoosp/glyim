#!/usr/bin/env bash
# Glyim installer — installs the release archive layout produced by
# .github/workflows/release.yml into ~/.local, and wires up the runtime
# staticlib via GLYIM_RUNTIME_LIB (the compiler resolves it from this
# variable first; see crates/glyim-cli/src/linker.rs).
#
# Usage: ./install.sh [archive.tar.gz]
#   with an archive:  installs exactly that archive
#   without:          installs from ../target/release (developer install)
set -euo pipefail

PREFIX="${GLYIM_INSTALL_PREFIX:-$HOME/.local}"
BIN_DIR="$PREFIX/bin"
LIB_DIR="$PREFIX/lib/glyim"

say() { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

mkdir -p "$BIN_DIR" "$LIB_DIR"

if [[ $# -ge 1 ]]; then
  ARCHIVE="$1"
  [[ -f "$ARCHIVE" ]] || die "archive not found: $ARCHIVE"
  say "Installing from archive $ARCHIVE"
  TMP="$(mktemp -d)"
  tar -xzf "$ARCHIVE" -C "$TMP"
  SRC="$TMP/glyim"
  [[ -f "$SRC/glyim-cli" ]] || die "archive layout unexpected: no glyim-cli"
  install -m 0755 "$SRC/glyim-cli" "$BIN_DIR/glyim-cli"
  install -m 0755 "$SRC/glyip" "$BIN_DIR/glyip"
  [[ -f "$SRC/lib/libglyim_runtime.a" ]] && \
    install -m 0644 "$SRC/lib/libglyim_runtime.a" "$LIB_DIR/libglyim_runtime.a"
  rm -rf "$TMP"
else
  say "Developer install from target/release"
  ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  [[ -f "$ROOT/target/release/glyim-cli" ]] || die "build first: cargo build --release -p glyim-cli -p glyip -p glyim-runtime"
  install -m 0755 "$ROOT/target/release/glyim-cli" "$BIN_DIR/glyim-cli"
  install -m 0755 "$ROOT/target/release/glyip" "$BIN_DIR/glyip"
  [[ -f "$ROOT/target/release/libglyim_runtime.a" ]] && \
    install -m 0644 "$ROOT/target/release/libglyim_runtime.a" "$LIB_DIR/libglyim_runtime.a"
fi

if [[ -f "$LIB_DIR/libglyim_runtime.a" ]]; then
  ENV_SNIPPET="export GLYIM_RUNTIME_LIB=\"$LIB_DIR/libglyim_runtime.a\""
  say "Runtime staticlib installed at $LIB_DIR/libglyim_runtime.a"
  say "Make sure your shell exports: $ENV_SNIPPET"
  if ! grep -q "GLYIM_RUNTIME_LIB" "$HOME/.profile" 2>/dev/null; then
    printf '\n# Glyim runtime staticlib (added by glyim install script)\n%s\n' "$ENV_SNIPPET" >> "$HOME/.profile"
    say "Appended the export to ~/.profile"
  fi
fi

say "Installed:"
say "  $BIN_DIR/glyim-cli"
say "  $BIN_DIR/glyip"
"$BIN_DIR/glyim-cli" --help >/dev/null 2>&1 && say "glyim-cli runs OK" \
  || die "glyim-cli failed to execute"
say "Done. Ensure $BIN_DIR is on your PATH."
