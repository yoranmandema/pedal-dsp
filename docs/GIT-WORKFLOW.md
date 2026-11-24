# Git Workflow for Pedal DSP

This document defines the branch and PR workflow for the pedal-dsp repository.

## Branch Model

```
main (protected, production)
  ↑ PR required + review
develop (integration)
  ↑ auto from worker branches
```

### Rules

1. **Workers NEVER touch `main`** — ever. Main is the protected release branch.
2. **All worker branches originate from `develop`** — created via `scripts/worker-setup.sh <card-id>`.
3. **All changes land via PR to `develop`** — never direct pushes to develop.
4. **Conflicts are auto-resolved** by workers using `git rebase develop` with `--skip` for complex conflicts.
5. **`main` is protected via repository settings** — PR required, 1+ reviewer, no direct pushes.

## Worker Flow

### Starting Work

```bash
# In the card's worktree:
cd ~/.hermes/cache/worktrees/<card-id>
source ../scripts/worker-setup.sh <card-id>
# → Creates/updates branch, rebases onto develop, opens PR
```

### During Work

```bash
# Make changes, commit normally:
git add .
git commit -m "description of changes"
git push
# → PR updates automatically via force-push
```

### Finishing Work

```bash
# From the worktree:
bash scripts/pr-land.sh <card-id>
# → Rebases onto latest develop, runs checks, pushes, creates/updates PR
```

## Land Flow

When a card is marked done:

1. **PR lands to `develop`** — via `scripts/pr-land.sh <card-id>`
2. **Conflicts auto-resolved** — prefer develop's version for infrastructure files
3. **Checks run** — cargo test, pnpm build
4. **PR merges to develop** — by reviewer or orchestrator
5. **develop → main** — periodic batch merge by admin

## Conflict Resolution

Workers must handle conflicts automatically:

1. Run `git rebase develop`
2. If conflicts occur, use `git rebase --skip` to skip rejected patches
3. For merge conflicts in specific files, prefer:
   - **Card code** (new features, app logic) → card's version
   - **Infrastructure** (Cargo.toml, CI configs) → develop's version
   - **Tests** → card's version (tests are part of the feature)
4. If `--skip` doesn't work, abort and report

## Review Flow

1. Worker opens PR to `develop` (via `worker-setup.sh` or `pr-land.sh`)
2. Reviewer (pedal-reviewer) reviews PR
3. If approved → merge to `develop`
4. If changes requested → worker updates PR
5. When `develop` is ready → admin merges to `main`
