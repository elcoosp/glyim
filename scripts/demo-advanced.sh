#!/usr/bin/env bash
set -e
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT
cat > "$TMPDIR/advanced.g" << 'ADV_EOF'
struct Point { x, y }

fn id(x) { x }

fn main() -> i64 {
    let p = Point { x: 10, y: 20 }
    let s = id(p.x + p.y)
    let opt = Some(s)
    match opt {
        Some(val) => val,
        None => 0,
    }
}
ADV_EOF
echo "── Advanced Source ───────────────────"
cat "$TMPDIR/advanced.g"
echo "── Compile & Run ─────────────────────"
./target/release/glyim run "$TMPDIR/advanced.g"
EXIT_CODE=$?
echo "── Exit Code ─────────────────────────"
echo "$EXIT_CODE"
