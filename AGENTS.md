# Rules for agents working in pedal-dsp

The user is the **product owner**. They write ideas; agents do all design,
code, git and GitHub work. Never ask the user to run git, edit code, or fix the
board.

## Roles (kanban assignee = your role)

| Assignee | Job |
|---|---|
| `pedal-architect` | Turn a feature idea into a short design (`docs/design/<topic>.md` when non-trivial) and 1–6 small builder cards. Writes little code. |
| `pedal-builder` | Implements one non-UI card: Rust engine, DSP, tooling, docs. |
| `pedal-ui-builder` | Implements one UI card: Preact, TypeScript, CSS in `ui/`. |
| `pedal-reviewer` | Reviews the card's pull request, approves and merges it, or sends it back. |

Stay in your lane. Only the architect creates cards.

## The flow (one card = one branch = one pull request)

1. **Start.** You are in the card's worktree (`$HERMES_KANBAN_WORKSPACE`, under
   `.worktrees/`). Never work in the main checkout. Run `source scripts/agent-env.sh`
   (tools on PATH, shared Rust target dir, `ui/node_modules`, merges the latest
   `origin/main` into your branch).
2. **Build.** Do exactly what the card says. Commit after each verified step to
   your card branch, card id in the message (`ui: clamp knob labels (t_adc96a8c)`).
   Stage by path, never `git add -A`.
3. **Verify** (must pass before you hand off):
   - Rust: `cargo test --workspace --exclude pedal-shell`
   - UI: `cd ui && pnpm run build`
   - UI cards: look at the result (see "Looking at the UI").
4. **Open the PR:** `scripts/card-pr.sh open <card id> "<short title>"`. It pushes
   your branch and prints the PR URL. If it reports a merge conflict with
   `origin/main`, resolve it yourself (keep both sides' intent), re-run the checks,
   commit, and run it again.
5. **Hand off:** `kanban_request_review` with `reviewer: pedal-reviewer`,
   the product-owner summary (below), and `metadata: {"published_pr": "<PR URL>"}`.

### Reviewer
- `scripts/card-pr.sh status <card id>` and `gh-bot review pr diff <PR URL>` to
  read the change. Run the verify commands in the card worktree. Check every
  acceptance criterion. For `ui/` changes, look at it at ~800 px and ~1440 px.
- Trivial problems (typo, missing import, a few lines): fix, commit, push
  (`scripts/card-pr.sh open ...` again), continue.
- Otherwise: `kanban_request_changes` with a numbered list of concrete fixes.
  Before the **second** request on the same card, escalate the model:
  `hermes kanban --board pedal set-model <card id> anthropic/claude-sonnet-5.5 --provider nous`.
  On a third, block with `needs_input`.
- Approve + merge: `scripts/card-pr.sh merge <card id>`. It approves as the review
  bot, arms squash auto-merge and waits for CI.
  - `MERGED` → `kanban_complete` with the builder's product-owner summary (keep
    the "Try it" steps) and `metadata: {"published_pr": "<PR URL>"}`.
  - `CHECKS-FAILED` → read the failing job log (`gh-bot review pr checks <URL>`),
    then `kanban_request_changes` quoting the failure.
- Feature cards (the user's original idea, parent of the build steps): check the
  idea end to end on `main`. Complete it, or block `needs_input` listing gaps.

### Architect
- Read the code the feature touches plus `docs/ARCHITECTURE.md` and
  `docs/IPC-PROTOCOL.md`. Design docs go through a PR like any other change
  (step 4–5, reviewer merges it).
- Each builder card: assignee `pedal-ui-builder` (touches `ui/`) or `pedal-builder`;
  `workspace_kind: worktree`; `completion_contract: yoranmandema/pedal-dsp`;
  roughly 1–4 files; leaves the app working; names exact files, types, functions,
  IPC messages; acceptance criteria as checks; verify commands; "Try it" steps.
  Split mixed engine+UI work into two cards, UI depending on engine (`parents`).
- UI cards: state sizes, spacing, colours/tokens, states (hover/active/disabled/
  focus), behaviour at ~800 px and ~1440 px, and what must not change.
- Pin `model: anthropic/claude-sonnet-5.5, provider: nous` only on genuinely hard
  cards (real-time audio thread, DSP maths, lock-free code, protocol migrations).
- Cards that need the physical Raspberry Pi: create them, then block them
  `needs_input` with reason `hardware: needs the Pi online`.
- List created card ids in `created_cards` when you complete.

## Rules for everyone
- Work only on your own card, in its worktree. Don't touch the main checkout,
  other cards' branches, or `main` directly (it is protected).
- No force-push, no history rewrites. Never commit `target/`, `ui/node_modules/`,
  `ui/dist/`, `ui/standalone-tmp/`, `.DS_Store`, `.vscode/`, `*.wav`.
- Too big for one session: commit and PR a verifiable slice, comment the
  remaining steps on the card, block `needs_input`. Don't create cards.
- Spec wrong or ambiguous: don't redesign. Block `needs_input` with one question,
  2–3 options with trade-offs, and your recommendation.
- `gh` always via `gh-bot dev ...` (builders/architect) or `gh-bot review ...`
  (reviewer). Plain `gh` is not logged in.

## Looking at the UI (UI builders and reviewers)
Each worktree runs its own servers; never use the user's ports 8737/5173.
```sh
"$PEDAL_ENGINE_BIN" --headless --port 18737 &      # no audio device, output discarded
cd ui && VITE_ENGINE_URL=ws://127.0.0.1:18737 pnpm exec vite --port 15173 --strictPort &
```
Open `http://localhost:15173` with the browser tool at ~1440×900 and ~800×600,
screenshot, check with vision, click through the states the card touches.
Kill both afterwards: `lsof -ti tcp:18737,15173 | xargs kill`.

## Product-owner summary (the `summary` field)
It becomes the user's notification.
1. First line: what is different in the app, in plain words. No file names.
2. **Try it:** how to run the app (`docs/RUN-THE-APP.md`) and what to click/hear.
3. **Not done / known limits.**
4. Last line: the PR URL.

## Audio safety
- Never play sound on this machine. Engine always `--headless`; check audio with
  tests or offline renders. Never open speakers, headphones or the Scarlett.
- Before you finish, kill every `pedal-engine`, `pedal-shell` and Vite process
  you started.

## Context
`docs/HANDOFF.md`, `docs/ROADMAP.md` (product), `docs/ARCHITECTURE.md`,
`docs/IPC-PROTOCOL.md` (read before changing engine↔UI messages).
A Tauri `cargo build` without `--features custom-protocol` is a dev build that
loads Vite's dev URL.
