#!/usr/bin/env bash
# Land a card's work on main: the only way kanban work reaches the user's checkout.
#
#     scripts/kanban-land.sh <card id>      # run from inside .worktrees/<card id>
#
# Run it at the very end, right before kanban_complete:
# - the reviewer, when approving a builder card;
# - the architect, for its own design notes;
# - anyone who committed fixes on a feature card.
#
# Steps: check the worktree is clean, merge the latest main into the card
# branch, run the checks for what the card changed, then fast-forward the
# user's main checkout to it. Fast-forward only: if anything is off, it stops,
# prints why, and changes nothing on main.
set -euo pipefail

REPO=/Users/yoran/Documents/Repos/pedal-dsp
die() { echo "kanban-land: $*" >&2; exit 1; }

# --verify <card id>: is this card's work actually on main? Run before every
# kanban_complete. Exit 0 only when done honestly means landed:
#   LANDED           branch is an ancestor of main
#   LANDED-EMPTY     branch contributes nothing (content already on main)
#   NOT-LANDED       content missing from main — do NOT complete; list files
if [ "${1:-}" = "--verify" ]; then
  vid="${2:?usage: scripts/kanban-land.sh --verify <card id>}"
  vbr="wt/$vid"
  cd "$REPO" || exit 1
  git rev-parse --verify -q "$vbr" >/dev/null || {
    if git log -1 --format='%h' --grep "$vid" main 2>/dev/null | grep -q .; then
      echo "LANDED-BY-RECORD: branch gone, but main carries its commits at $(git log -1 --format='%h %s' --grep "$vid" main | cut -c1-60)"; exit 0
    fi
    echo "NOT-LANDED: branch $vbr does not exist and no commit on main names $vid"; exit 1; }
  if git merge-base --is-ancestor "$vbr" main; then
    echo "LANDED: $vbr is on main ($(git rev-parse --short main))"; exit 0
  fi
  if git diff --quiet "main...$vbr"; then
    echo "LANDED-EMPTY: $vbr contributes nothing not already on main"; exit 0
  fi
  echo "NOT-LANDED: $vbr has content missing from main:"
  git diff --name-only "main...$vbr" | sed 's/^/  /'
  exit 1
fi

id="${1:?usage: scripts/kanban-land.sh <card id>}"
br="wt/$id"
wt="$(git rev-parse --show-toplevel)"
cd "$wt"

[ "$(git branch --show-current)" = "$br" ] || die "run this inside the card's worktree, on branch $br (you are on '$(git branch --show-current)' in $wt)"
git rev-parse -q --verify MERGE_HEAD >/dev/null && die "there is an unfinished merge in this worktree (unresolved conflicts). Resolve and commit it, or run 'git merge --abort', then rerun. A half-merged worktree silently blocks every later land."
[ -z "$(git status --porcelain)" ] || die "the worktree has uncommitted changes. Commit them (or stash what doesn't belong), then rerun"
[ "$(git -C "$REPO" branch --show-current)" = "main" ] || die "the user's checkout ($REPO) is not on main. Block with needs_input; don't switch it yourself"
[ -z "$(git -C "$REPO" status --porcelain)" ] || die "the user's main checkout has uncommitted changes; leave them untouched and retry after they are committed or safely stashed"

if ! git merge-base --is-ancestor main HEAD; then
  git merge --no-edit -q main || die "merging main into $br conflicts. Resolve it here, commit, rerun"
fi

changed="$(git diff --name-only main...HEAD)"
if [ -z "$changed" ]; then
  echo "kanban-land: $br has nothing that isn't already on main. Nothing to land"
  exit 0
fi

# shellcheck source=/dev/null
source "$wt/scripts/agent-env.sh" >/dev/null
if grep -q -E '(\.rs|Cargo\.(toml|lock))$' <<<"$changed"; then
  echo "kanban-land: Rust changed: cargo test"
  cargo test --workspace --exclude pedal-shell -q >"$TMPDIR/kanban-land-$id.log" 2>&1 \
    || { tail -30 "$TMPDIR/kanban-land-$id.log"; die "cargo test failed (full log: $TMPDIR/kanban-land-$id.log)"; }
fi
if grep -q '^ui/' <<<"$changed"; then
  echo "kanban-land: UI changed: typecheck + build"
  ( cd ui && pnpm run build ) >"$TMPDIR/kanban-land-$id.log" 2>&1 \
    || { tail -30 "$TMPDIR/kanban-land-$id.log"; die "UI build failed (full log: $TMPDIR/kanban-land-$id.log)"; }
fi

git -C "$REPO" merge --ff-only -q "$br" \
  || die "could not fast-forward main. The user probably has local edits in the same files. Block with needs_input"
echo "kanban-land: landed $br on main ($(git rev-parse --short HEAD)):"
git --no-pager log --oneline "main@{1}..main" 2>/dev/null | sed 's/^/  /' || true

# The card's work is now on main. Return a blocked card to ready so the next
# dispatch pass picks it up automatically — no manual `kanban_unblock` needed.
HERMES_BIN="${HERMES_HERMES_BIN:-/Users/yoran/.hermes/hermes-agent/.hermes/bin/hermes}"
if "$HERMES_BIN" kanban unblock "$id" --reason "Landed on main via kanban-land.sh ($br)" >/dev/null 2>&1; then
  echo "kanban-land: auto-unblocked $id"
else
  echo "kanban-land: NOTE: no unblock performed for $id (not blocked/scheduled — e.g. already done; or gateway unreachable). If it is genuinely blocked, run: hermes kanban --board pedal unblock $id" >&2
fi
