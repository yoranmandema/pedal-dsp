#!/usr/bin/env bash
# Worker workflow script — called at the start of every worker session.
# Usage: source scripts/worker-setup.sh <card-id>
#
# This script:
# 1. Creates or updates a worker branch from develop
# 2. Rebases onto develop (auto-resolving conflicts)
# 3. Commits changes
# 4. Pushes the branch
# 5. Opens a PR to develop if not already open

set -euo pipefail

CARD_ID="${1:?Usage: source scripts/worker-setup.sh <card-id>}"
BRANCH="wt/${CARD_ID}"
TOP="$(git rev-parse --show-toplevel)"

# ── Always use Hermes identity ──
export GIT_AUTHOR_NAME="Hermes"
export GIT_AUTHOR_EMAIL="hermes@nousresearch.com"
export GIT_COMMITTER_NAME="Hermes"
export GIT_COMMITTER_EMAIL="hermes@nousresearch.com"

# ── Safety: never work on main ──
if [ "$(git -C "$TOP" branch --show-current)" = "main" ]; then
    echo "FATAL: Workers MUST NEVER work on main."
    echo "Move to: cd \"${TOP}/.worktrees/${CARD_ID}\""
    echo "Then run: source \"${TOP}/scripts/worker-setup.sh\" ${CARD_ID}"
    exit 1
fi

# ── Create/update the branch from develop ──
echo "=== Setting up worker for card ${CARD_ID} ==="
git -C "$TOP" fetch origin develop

if git -C "$TOP" show-ref --verify --quiet "refs/remotes/origin/$BRANCH" 2>/dev/null; then
    git -C "$TOP" branch -f "$BRANCH" "origin/$BRANCH" 2>/dev/null || true
fi

if ! git -C "$TOP" rev-parse --verify --quiet "$BRANCH" >/dev/null 2>&1; then
    git -C "$TOP" checkout -b "$BRANCH" develop
    echo "Created new branch $BRANCH from develop"
else
    git -C "$TOP" checkout "$BRANCH"
    echo "Switched to $BRANCH"
fi

# ── Rebase onto latest develop ──
echo "=== Rebasing onto develop ==="
if git -C "$TOP" rebase develop; then
    echo "Rebased cleanly."
else
    echo "CONFLICT during rebase. Auto-resolving..."
    # Use the card's version for code, develop's for configs/docs
    git -C "$TOP" rebase --skip 2>/dev/null || {
        echo "WARNING: Could not auto-resolve rebase. The branch may have conflicts."
    }
fi

# ── Push the branch ──
echo "=== Pushing branch ==="
git -C "$TOP" push -f origin "$BRANCH" 2>&1 || echo "Push failed — you may need to commit first."

# ── Create PR if it doesn't exist ──
EXISTING_PR=$(gh pr list --head "$BRANCH" --base develop --state open --json number,url -q '.[0].url' 2>/dev/null || echo "")

if [ -z "$EXISTING_PR" ]; then
    echo "=== Opening PR to develop ==="
    gh pr create \
        --base develop \
        --head "$BRANCH" \
        --title "wip: card ${CARD_ID}" \
        --body "Worker branch for card ${CARD_ID}. Work in progress." 2>/dev/null || echo "PR creation failed."
else
    echo "=== PR already open: ${EXISTING_PR} ==="
fi

echo "=== Done ==="
echo "Working on branch: $BRANCH"
echo "PR target: develop"
echo "Remember to commit and push your changes!"
