#!/usr/bin/env bash
set -euo pipefail

STREAM_ID="${1:?Usage: generate-stream.sh SXX}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
DATA_FILE="$SCRIPT_DIR/streams.json"
TEMPLATE="$SCRIPT_DIR/stream-template.md"
OUTPUT_DIR="$SCRIPT_DIR/briefs"
mkdir -p "$OUTPUT_DIR"

if [ ! -f "$DATA_FILE" ]; then
    echo "streams.json not found"
    exit 1
fi

if [ ! -f "$TEMPLATE" ]; then
    echo "stream-template.md not found"
    exit 1
fi

PY_TMP=$(mktemp) || { echo "ERROR: cannot create temp file"; exit 1; }
cat > "$PY_TMP" << 'PYEOF'
import sys
import json

stream_id = sys.argv[1]
data_path = sys.argv[2]
template_path = sys.argv[3]
output_path = sys.argv[4]

with open(data_path, 'r') as f:
    streams = json.load(f)

stream = next((s for s in streams if s['id'] == stream_id), None)
if stream is None:
    print(f"Stream {stream_id} not found in {data_path}")
    sys.exit(1)

owned = "\n".join([f"- {c}" for c in stream.get('owned_crates', [])])
locked = "\n".join([f"- {i}" for i in stream.get('locked_interfaces', [])])
tests = "\n".join([f"- {k}: {v}" for k, v in stream.get('tests', {}).items()])
upstream = "\n".join([f"- {u}" for u in stream.get('upstream', [])])
downstream = "\n".join([f"- {d}" for d in stream.get('downstream', [])])

replacements = {
    "{ID}": stream_id,
    "{NAME}": stream.get('name', ''),
    "{SCOPE_SUMMARY}": stream.get('scope_summary', ''),
    "{OWNED_CRATES_AND_MODULES}": owned,
    "{LOCKED_INTERFACE_LIST}": locked,
    "{TEST_CASES}": tests,
    "{MOCKING_INSTRUCTIONS}": stream.get('mocking', ''),
    "{CRATE_NAME}": stream.get('crate', ''),
    "{UPSTREAM_DEPENDENCIES}": upstream,
    "{WHO_USES_YOUR_OUTPUT}": downstream,
}

with open(template_path, 'r') as f:
    content = f.read()

for key, val in replacements.items():
    content = content.replace(key, val)

with open(output_path, 'w') as f:
    f.write(content)

print(f"Generated: {output_path}")
PYEOF

if python3 "$PY_TMP" "$STREAM_ID" "$DATA_FILE" "$TEMPLATE" "$OUTPUT_DIR/${STREAM_ID}.md"; then
  rm "$PY_TMP"
else
  echo "ERROR: Python template render failed for ${STREAM_ID}"
  rm -f "$PY_TMP"
  exit 1
fi
