#!/usr/bin/env bash
set -euo pipefail

STREAM_ID="${1:?Usage: dispatch.sh SXX}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BRIEF="$SCRIPT_DIR/briefs/${STREAM_ID}.md"

if [ ! -f "$BRIEF" ]; then
    echo "ERROR: Brief not found at $BRIEF" >&2
    echo "Run ./generate-stream.sh $STREAM_ID first" >&2
    exit 1
fi

MASTER_CTX="$SCRIPT_DIR/AGENT_MASTER_CONTEXT.md"
CONTRACTS="$SCRIPT_DIR/CONTRACTS_LOCKED.md"
TEST_INSTR="$SCRIPT_DIR/GLYIM_TEST_INSTRUCTIONS.md"
PROMPT_TEMPLATE="$SCRIPT_DIR/agent-prompt-template.md"
SKILL_DOC="$SCRIPT_DIR/SKILL_PLAN_TO_CAT_SCRIPTS.md"

for f in "$MASTER_CTX" "$CONTRACTS" "$TEST_INSTR" "$PROMPT_TEMPLATE" "$SKILL_DOC"; do
    if [ ! -f "$f" ]; then
        echo "ERROR: Missing $f" >&2
        exit 1
    fi
done

echo "Extracting stream name from brief" >&2
STREAM_NAME=$(sed -n 's/^# Stream [A-Z0-9]*: //p' "$BRIEF" | head -1)

echo "Extracting crate name from brief" >&2
CRATE_NAME=$(sed -n 's/.*cargo test -p \([a-z_\-]*\).*/\1/p' "$BRIEF" | head -1)
if [ -z "$CRATE_NAME" ]; then
    CRATE_NAME=$(sed -n 's/.*cargo clippy -p \([a-z_\-]*\).*/\1/p' "$BRIEF" | head -1)
fi
if [ -z "$CRATE_NAME" ]; then
    CRATE_NAME=$(sed -n 's/.*cargo check -p \([a-z_\-]*\).*/\1/p' "$BRIEF" | head -1)
fi
if [ -z "$CRATE_NAME" ]; then
    CRATE_NAME="unknown"
fi
echo "Detected crate: $CRATE_NAME" >&2

PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CRATE_DIR="$PROJECT_ROOT/crates/$CRATE_NAME"

SOURCE_CONTEXT=""
if [ -d "$CRATE_DIR/src" ]; then
    SOURCE_CONTEXT=$(find "$CRATE_DIR/src" -name '*.rs' -not -path '*/tests/*' | sort | while read -r src_file; do
        rel_path="${src_file#$PROJECT_ROOT/}"
        echo "### $rel_path"
        echo ""
        echo '```rust'
        cat "$src_file"
        echo '```'
        echo ""
    done)
fi

TEST_CTX=""
TEST_LIB="$PROJECT_ROOT/crates/glyim-test/src/lib.rs"
if [ -f "$TEST_LIB" ]; then
    TEST_CTX=$(echo "## glyim-test Public API"; echo ""; echo '```rust'; cat "$TEST_LIB"; echo '```')
fi

echo "# Agent Dispatch: Stream $STREAM_ID - $STREAM_NAME"
echo ""
echo "---"
echo ""
echo "## SYSTEM PROMPT (paste into system/developer message)"
echo ""
cat "$MASTER_CTX"
echo ""
echo "---"
echo ""
cat "$CONTRACTS"
echo ""
echo "---"
echo ""
cat "$TEST_INSTR"
echo ""
echo "---"
echo ""
echo "## Output Skill (MANDATORY)"
echo ""
cat "$SKILL_DOC"
echo ""
echo "---"
echo ""
echo "## USER PROMPT (paste into user message)"
echo ""
sed -e "s|{ID}|$STREAM_ID|g" -e "s|{NAME}|$STREAM_NAME|g" "$PROMPT_TEMPLATE"
echo ""
echo "---"
echo ""
echo "## Your Stream Brief"
echo ""
cat "$BRIEF"
echo ""

if [ -n "$SOURCE_CONTEXT" ]; then
    echo "---"
    echo ""
    echo "## Source Code Context ($CRATE_NAME)"
    echo ""
    echo "$SOURCE_CONTEXT"
fi

if [ -n "$TEST_CTX" ]; then
    echo "---"
    echo ""
    echo "$TEST_CTX"
fi

echo ""
echo "---"
echo ""
echo "## Quick Start"
echo ""
echo "1. Copy the SYSTEM PROMPT section above into the system/developer message"
echo "2. Copy everything from USER PROMPT onward into the user message"
echo "3. The agent will output bash scripts following the plan-to-cat-scripts skill"
echo "4. Save each fenced bash block as .sh and execute in order"
echo "5. If a script exits non-zero, paste the terminal output back for a surgical fix"
echo ""
echo "Stream: $STREAM_ID | Crate: $CRATE_NAME | Brief: $BRIEF"
