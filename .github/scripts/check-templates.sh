#!/usr/bin/env bash
set -euo pipefail

# M0.5: the process scaffolding is enforced, not aspirational. The PR
# template must exist, must reference the agent operating protocol
# (AGENTS.md), and AGENTS.md itself must exist and carry the protocol's four
# load-bearing sections. CI's `check-templates` job runs this script so a
# deleted template or a re-rooted protocol file fails the PR instead of
# quietly decaying; `just ci-local` runs the same script locally.

cd "$(git rev-parse --show-toplevel)"

FAILED=0

echo "Checking process templates (M0.5)..."

# 1. The PR template exists.
if [ ! -f ".github/PULL_REQUEST_TEMPLATE.md" ]; then
    echo "FAIL: .github/PULL_REQUEST_TEMPLATE.md is missing (M0.5)"
    FAILED=1
fi

# 2. The PR template references AGENTS.md — the protocol is the entry point
#    a reviewer reads first, so the template must point at it.
if [ -f ".github/PULL_REQUEST_TEMPLATE.md" ] && \
   ! grep -q "AGENTS\.md" ".github/PULL_REQUEST_TEMPLATE.md"; then
    echo "FAIL: .github/PULL_REQUEST_TEMPLATE.md does not reference AGENTS.md (M0.5)"
    FAILED=1
fi

# 3. The PR template carries the four M0.5-mandated sections.
if [ -f ".github/PULL_REQUEST_TEMPLATE.md" ]; then
    for section in "Ledger diff" "Perf delta" "Bug-class record" "Breaking-change checklist"; do
        if ! grep -q "$section" ".github/PULL_REQUEST_TEMPLATE.md"; then
            echo "FAIL: PR template is missing the mandatory '$section' section (M0.5 DoD)"
            FAILED=1
        fi
    done
fi

# 4. AGENTS.md exists and carries the §7 protocol sections.
if [ ! -f "AGENTS.md" ]; then
    echo "FAIL: AGENTS.md is missing (M0.5: the §7 agent operating protocol)"
    FAILED=1
else
    for section in "work loop" "Rules" "Review checklist" "Universal definition of done"; do
        if ! grep -qi "$section" "AGENTS.md"; then
            echo "FAIL: AGENTS.md is missing the '$section' section (plan §7)"
            FAILED=1
        fi
    done
fi

# 5. The protocol document must name its source of truth, so drift back to
#    the plan is re-derivable (the plan wins when they disagree).
if [ -f "AGENTS.md" ] && \
   ! grep -q "docs/roadmaps/glyim-master-plan.md" "AGENTS.md"; then
    echo "FAIL: AGENTS.md must name docs/roadmaps/glyim-master-plan.md as its source of truth"
    FAILED=1
fi

if [ $FAILED -ne 0 ]; then
    echo "check-templates FAILED"
    exit 1
fi
echo "check-templates OK"
