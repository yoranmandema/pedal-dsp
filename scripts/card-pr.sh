#!/usr/bin/env bash
# Pull-request helper for kanban cards. Run from inside the card's worktree.
#
#   scripts/card-pr.sh open  <card-id> "<title>"   push the branch, open (or reuse) its PR into main
#   scripts/card-pr.sh merge <card-id>             reviewer: approve + squash auto-merge, wait for result
#   scripts/card-pr.sh status <card-id>            print PR url, state, checks
#
# Identities: `open` runs as the dev bot, `merge` as the review bot (GitHub does not
# let an account approve its own PR). Tokens are minted per call by `gh-bot`.
set -euo pipefail
cmd=${1:?usage: card-pr.sh open|merge|status <card-id> [title]}
card=${2:?card id required}
export GH_REPO=yoranmandema/pedal-dsp
branch=$(git branch --show-current)
[ -n "$branch" ] && [ "$branch" != main ] || { echo "card-pr: run this on the card branch, not main/detached" >&2; exit 2; }

pr_url() { gh-bot dev pr list --head "$branch" --state all --json url,state -q '[.[] | select(.state!="CLOSED")][0].url // empty'; }

case "$cmd" in
  open)
    title=${3:-"$card"}
    [ -z "$(git status --porcelain)" ] || { echo "card-pr: commit (or drop) your changes first:" >&2; git status --short >&2; exit 2; }
    git fetch -q origin main
    if ! git merge-base --is-ancestor origin/main HEAD; then
      git merge --no-edit -q origin/main || { git merge --abort; echo "card-pr: merging origin/main conflicts — resolve it on this branch (git merge origin/main), run the checks, commit, then rerun" >&2; exit 3; }
    fi
    PEDAL_GH_ROLE=dev git push -q -u origin "HEAD:$branch" 2>/tmp/card-pr-push.$$ || { cat /tmp/card-pr-push.$$ >&2; rm -f /tmp/card-pr-push.$$; exit 1; }
    rm -f /tmp/card-pr-push.$$
    url=$(pr_url)
    if [ -z "$url" ]; then
      body=$(printf 'Kanban card: `%s`\n\n%s\n' "$card" "$(git log --format='- %s' origin/main..HEAD | head -30)")
      url=$(gh-bot dev pr create --base main --head "$branch" --title "$title ($card)" --body "$body" 2>/dev/null \
            | grep -Eo 'https://github.com/[^ ]+/pull/[0-9]+' | tail -1)
      [ -n "$url" ] || url=$(pr_url)
    fi
    echo "$url" ;;
  merge)
    url=$(pr_url); [ -n "$url" ] || { echo "card-pr: no open PR for $branch" >&2; exit 2; }
    gh-bot review pr review "$url" --approve --body "Reviewed by pedal-reviewer against card $card." >/dev/null 2>&1 || true
    gh-bot review pr merge "$url" --squash --auto --delete-branch >/dev/null
    for _ in $(seq 1 90); do   # up to ~30 min for CI
      state=$(gh-bot review pr view "$url" --json state -q .state 2>/dev/null)
      [ "$state" = MERGED ] && { echo "MERGED $url"; exit 0; }
      failed=$(gh-bot review pr checks "$url" --json name,bucket -q '[.[] | select(.bucket=="fail") | .name] | join(", ")' 2>/dev/null || true)
      [ -n "$failed" ] && { echo "CHECKS-FAILED ($failed) $url"; exit 4; }
      sleep 20
    done
    echo "TIMEOUT $url (auto-merge stays armed; it merges once checks pass)"; exit 5 ;;
  status)
    url=$(pr_url); [ -n "$url" ] || { echo "no PR for $branch"; exit 0; }
    gh-bot dev pr view "$url" --json url,state,mergeStateStatus,reviewDecision -q '"\(.url) \(.state) \(.mergeStateStatus) review=\(.reviewDecision)"' 2>/dev/null
    gh-bot dev pr checks "$url" || true ;;
  *) echo "unknown command $cmd" >&2; exit 2 ;;
esac
