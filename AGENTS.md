# Rules for agents working in pedal-dsp

These rules apply to every agent session in this repo, including kanban workers.
Kanban runs one worker at a time. Every card has its own Git worktree under
`.worktrees/<card id>`, so the user's main checkout and live app never see
uncommitted worker edits. Spend the budget on code that ends up committed.

## Roles (the kanban board)
Your card's assignee says which role you are. Each role runs on a model sized
for its job, so stay in your lane:

| Assignee | Model | Job |
|---|---|---|
| `pedal-architect` | Opus 5.5 (expensive; #1 on frontend leaderboards) | Design and plan, including UI and visual design. Turns a feature into small, precise builder cards. Writes little code. |
| `pedal-ui-builder` | Haiku 5.5 (cheap) | Implements one UI card: Preact, TypeScript and CSS in `ui/`. |
| `pedal-builder` | Haiku 5.5 (cheap) | Implements one non-UI card: the Rust engine, DSP, tooling, docs. |
| `pedal-reviewer` | Sonnet 5.5 | Checks a builder card against its acceptance criteria, then approves or sends it back. |

### pedal-architect
- Read the code the feature touches, plus `docs/ARCHITECTURE.md` and
  `docs/IPC-PROTOCOL.md`. If the design is non-trivial (a data model, a protocol
  change, threading, or DSP), write it down in `docs/design/<topic>.md` and commit.
- Split the work into **builder cards** with `kanban_create`. Each card must be:
  - assigned to `pedal-ui-builder` if it touches `ui/` (components, styling,
    layout, interaction), or to `pedal-builder` otherwise. Split a mixed change
    into an engine card and a UI card, with the UI card depending on the engine
    one. Use `workspace_kind: worktree`,
    `workspace_path: /Users/yoran/Documents/Repos/pedal-dsp`, and the same
    `priority` as your own card;
  - one session of work: roughly 1–4 files, and it must leave the app working;
  - concrete: the exact files, types and functions to touch, the approach, and
    the IPC messages if any. Also give acceptance criteria phrased as checks,
    the verify commands, and the "Try it" steps for the product owner. A
    builder that reads only the card and this file must not need to make design
    decisions.
- Order dependent cards with `parents`. Use at most ~6 cards per feature.
- Builder cards for the feature may already exist from an earlier automatic
  split; they're often vague because that split never read the code. Don't
  duplicate them. Add your precise spec as a comment on each existing card
  (`hermes kanban --board pedal comment <id> "..."`), and only create cards for
  missing work.
- Only for a card that is genuinely hard (real-time audio-thread code, DSP maths,
  lock-free or concurrency work, a subtle protocol migration, or a UI change
  spanning many components), pin it with
  `model: anthropic/claude-sonnet-5.5`, `provider: nous`. Everything else stays
  on the builders' default cheap model. The daily model allowance is small, so
  make this rare.
- For UI cards, state the visual intent precisely: sizes, spacing scale,
  colours or tokens, states (hover, active, disabled, focus), behaviour at
  ~800 px and ~1440 px wide, and what must not move or change. The UI builder
  is a cheap model: the more exact the card, the better the result.
- If a product decision is needed, block your own card with `needs_input` (see
  below) before creating cards.
- Finish with `scripts/kanban-land.sh <your-card-id>` to verify the work and
  fast-forward it into `main`, then `scripts/kanban-land.sh --verify
  <your-card-id>` (must say LANDED) before `kanban_complete`, not review. Only
  after the card lands should its dependent children run; they start from the
  updated `main`. The first line of the summary says what will change for the
  user, e.g. "Planned cross-channel routing: 5 build steps queued". List child
  card IDs in `created_cards`.

### pedal-builder and pedal-ui-builder
- Do what the card says, and only that. If the spec turns out to be wrong or
  ambiguous, don't redesign. Block with `needs_input` and ask one precise
  question.
- UI builders: look at your result before handing off (see "Looking at the UI"
  below), at ~800 px and ~1440 px wide.
- If the card is too big for one session, finish one verifiable slice, commit it,
  record the remaining steps as a comment on the card, then block with
  `needs_input`. Don't create cards.
- When done, hand off with `kanban_request_review` and `reviewer:
  pedal-reviewer`, using the product-owner summary format below. Do not land
  the branch yourself; it must pass its review first.

### pedal-reviewer
- Find the card's commits with `git log --oneline --grep <card id>`, read the
  diff, run the verify commands, and check each acceptance criterion.
- For anything that changes `ui/`, also look at it (see "Looking at the UI"):
  screenshot it at ~800 px and ~1440 px wide and check it with the vision tool
  against the card's visual intent. Check alignment, overflow, clipping and
  states. A UI card that builds but looks wrong is not done.
- Fix trivial issues yourself (typos, a missing import, a few lines) and commit
  them. That costs less than another round trip.
- Approve with `scripts/kanban-land.sh <card-id>` to run the checks and fast-forward
  the approved commits to `main`, confirm with `scripts/kanban-land.sh
  --verify <card-id>`, then `kanban_complete`. Keep the builder's
  product-owner summary and its "Try it" steps.
- On `kanban_request_changes`, do not land; the same card worktree returns to its builder.
- Otherwise call `kanban_request_changes` with a numbered list of specific
  fixes. If you are about to request changes on this card for the **second**
  time, first escalate it to a stronger model (either builder):
  `hermes kanban --board pedal set-model <card id> anthropic/claude-sonnet-5.5 --provider nous`.
  For a third time, block with `needs_input` instead.
- **Feature cards** are the user's original request. Their parents are the build
  steps, so they run only after all of those are done. Check the original ask
  end to end against the code. If it is met, complete the card with a
  product-owner summary of the whole feature. If something is missing, block
  with `needs_input` and list the gaps so the user can send them back through
  Triage.
- If you make code changes during review, commit and run `scripts/kanban-land.sh`
  before approving. Never fast-forward code that has not passed its checks.

### Everyone
- Work only on your own card.
- Never call `kanban_complete` unless `scripts/kanban-land.sh --verify
  <card id>` exits 0. Landing is part of done, not a follow-up.
- Never run `hermes kanban specify`/`decompose` or reassign cards. Only the
  architect creates cards.

## Worktrees and landing (read before editing)

Every kanban card runs in `.worktrees/<card-id>` on branch `wt/<card-id>`;
never edit from the main checkout. At session start, run:

```sh
source scripts/agent-env.sh
```

This adds Hermes' Node/pnpm to PATH, gives Rust builds a separate shared target
directory, installs this worktree's `ui/node_modules`, and merges the latest
`main` into a clean card branch. Each worktree has its own `node_modules` and
ignored `ui/dist`; nothing generated is committed. Rust builds share
`~/.cache/pedal-dsp/target` to avoid rebuilding for every card. This cache and
all worktrees are separate from `target/` and source files in the user's live
checkout.

Commit verified work to **your card branch** with the card ID in each message,
for example `ui: clamp knob labels (t_adc96a8c)`. Stage by path. These commits
are not visible in the user's app until review passes. Do not switch branches or
run `git checkout` in the main checkout.

**Do not merge or push by hand.** To land approved changes, run
`scripts/kanban-land.sh <card-id>` from that card's worktree. It checks the
worktree is clean, merges the latest main, runs Rust/UI checks for changed
files, and fast-forwards the user's main checkout. It stops without changing
main if any check fails, main has user edits, or there is a conflict. Fix the
issue in your worktree and rerun. A failed land is a reason to block with
`needs_input`, not to force-push or clean the user's tree.

A `request_changes` handoff keeps the same card branch for the builder to fix.
After a reviewed card is landed, dependent cards start from the updated main.

## Landing is part of done (non-negotiable)

`done` means the work is on `main`. Landing is not an optional follow-up
anyone can skip. Before **any** `kanban_complete`, from the main checkout run:

    scripts/kanban-land.sh --verify <card id>

- `LANDED` / `LANDED-EMPTY` (exit 0) → you may complete. The summary's last
  line must state `on main: <short hash>` (the `main` tip it printed).
- `NOT-LANDED` (exit 1) → do **not** call `kanban_complete`. Land the card
  (`scripts/kanban-land.sh <card id>` from its worktree). If the land fails,
  fix and rerun, or block with `needs_input`/`transient` quoting the failure
  line. A card marked done whose work is not on main is a defect of the
  board: the next card that builds on "main" silently misses it, and
  half-landed features (UI without engine) look like broken controls.
- If a card's content is reapplied onto `main` by hand (graft, cherry-pick,
  redesign that supersedes it), the same session must either land the branch
  as an ancestor (`git merge -s ours wt/<card id> -m "record: <card>
  content already on main"`) or comment the decision on the card. Never
  leave a done card pointing at work that only exists in a stale branch.
  If the branch was already deleted, satisfy the gate with an empty commit:
  `git commit --allow-empty -m "record: <card id> verified on main at
  <hash> — <why>"` (`--verify` greps main for the card id as its fallback).

A land that dies is loud by design: the scripts stop with a message, never
half-merge. `agent-env.sh` aborts a conflicting `main` merge instead of
leaving the worktree half-merged, and `kanban-land.sh` refuses to run over
an unfinished merge.

## Commit discipline (non-negotiable)
- Commit to your card's `wt/<card-id>` branch after every verified step.
  Stage only your paths; never `git add -A`. Include the card ID in each message.
- Never commit `.DS_Store`, `.vscode/`, `ui/standalone-tmp/`, `target/`,
  `ui/node_modules/`, `ui/dist/`, or `*.wav`.
- Do not checkout, reset, clean, or otherwise modify the user's main checkout.
- If you hit a rate limit or need to block, leave verified commits in your
  worktree; note the branch/commit in a Kanban comment. Uncommitted work stays
  isolated in this card's worktree and can be resumed there.
- Don't rewrite history or force-push.

## Verify before you commit
- Rust: from the worktree root run `source scripts/agent-env.sh` first, then
  `cargo test --workspace --exclude pedal-shell`. It must stay green.
- UI: from the worktree root run `source scripts/agent-env.sh` first, then
  `cd ui && pnpm run build`. This typechecks and builds. `ui/dist/` is ignored;
  never commit generated build output.
- If you touch the Tauri shell, note that a plain `cargo build` produces a dev
  build that loads Vite's dev URL. A self-contained binary needs
  `--features custom-protocol`.

## Looking at the UI (UI builders and reviewers)
Do not commit `ui/dist/` or `ui/node_modules/`; they're locally generated and
ignored. The user's dev server reads the main checkout live, but card branches
are isolated, so its source changes become visible only after a reviewer runs
`scripts/kanban-land.sh <card-id>`.

Each UI worktree has its own dev server. Use separate ports (for example
engine 18737 and Vite 15173); do not stop or reuse the user's ports 8737/5173.
Source `scripts/agent-env.sh` first, then:

```sh
"$PEDAL_ENGINE_BIN" --headless --port 18737                    # background; no audio device, output discarded
cd ui && VITE_ENGINE_URL=ws://127.0.0.1:18737 pnpm exec vite --port 15173 --strictPort   # background
```

Open `http://localhost:15173` with the browser tool, set the viewport to
~1440×900 and then ~800×600, take screenshots, and check them with vision.
Click through the states the card touches (select a block, open the modal,
drag). Afterwards, kill both processes (`lsof -ti tcp:18737,15173 | xargs kill`).
If `pnpm` isn't on PATH, use `~/.hermes/tools/node-*/bin` and
`~/.hermes/tools/pnpm-*`, like `shell/run.sh` does.

## Handoffs to the product owner (the user)
The user is the product owner, not a code reviewer. The `summary` you pass to
`kanban_complete` or `kanban_request_review` is what pops up as their desktop
notification. Write it for someone who will open the app and try the change:

1. First line (shown in the notification): what is different in the app, in
   plain words. Example: "Knob min/max labels no longer overlap; they sit just
   outside the ring in smaller text." Don't put file names or test counts here.
2. **Try it:** the exact command to run the app (see `docs/RUN-THE-APP.md`) and
   what to click, drag or listen for.
3. **Not done / known limits:** anything left out on purpose.
4. Last: commit hash(es) and the required `on main: <short hash>` line (see
   "Landing is part of done"). Put test results in `metadata`, not in the summary.

Reviewers who complete a card after review must keep this format and carry the
"Try it" steps forward. Don't replace the summary with "approved".

## When you need a decision
If the card needs a product decision (look, feel, sound, scope) or something
only the user can do (hardware, accounts, listening tests), don't guess. Call
`kanban_block` with kind `needs_input`, and make the reason:
- one concrete question in one sentence,
- 2–3 labelled options with a one-line trade-off each,
- your recommendation.

Commit or stash your work first. The user answers with a comment on the card
and unblocks it. Read their comment before you continue.

## Audio safety
- Clean up after yourself. Before you complete, block, or stop, kill every
  `pedal-engine`, `pedal-shell` and Vite (`pnpm dev`) process you started.
  A leftover engine holds port 8737, and the user's app then silently talks to
  your stale engine instead of its own.
- Never play sound on this machine. Run the engine with `--headless` (simulated
  device, output discarded) and check audio behaviour with tests or offline
  renders. Never open the speakers, headphones, or the Scarlett output.

## Context
- Read `docs/HANDOFF.md` and `docs/ROADMAP.md` for product context.
- Read `docs/IPC-PROTOCOL.md` before changing anything between the engine and
  the UI.
- Unreviewed work from earlier parallel runs is on the branch
  `wip/parallel-workers-2026-11-15`. If your card's work already exists there,
  port it. Don't rewrite it from scratch.
