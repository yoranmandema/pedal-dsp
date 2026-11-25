# GitIgnore Audit — pedal-dsp

**Card:** t_389c8275 (audit: files that should be gitignored)
**Assignee:** pedal-architect
**Status:** written (history not rewritten — this is the audit deliverable only)

## 0. Scope & method

Repository: `pedal-dsp` — Rust engine (`engine/`, `dsp/`, `graph/`, `ffi/`), Tauri 2
desktop shell (`shell/`), Preact UI (`ui/`), host tooling (`tone3000/`, `live/`,
`firmware/`), and iPlug2 third-party (`plugin/third_party/iPlug2`).

How the audit was produced:
- `git ls-files` → current tracked file list + sizes (`git cat-file -s`).
- `git rev-list --objects --all` + `git cat-file -s` → all blobs ever committed,
  sorted, to find large/generated history objects.
- `git log --all --diff-filter=A --name-only --pretty=format:` → files ever added.
- `git check-ignore` → which currently-tracked files are already ignored.
- `git log -p --all -S'...'` + `git grep` over all commits → secrets/keys/tokens search.

Numbers:
- Tracked files: **235** (across 259 commits).
- `.git` pack: **228 MB** (`size-pack` 233,971 KB).
- Worktree working tree: **~23 GB** (mostly `target/`, `ui/node_modules/`,
  `firmware/target/`, `live/target/`, `graph/target/`).
- No `.gitmodules` submodules outside `plugin/third_party/iPlug2`.

---

## 1. Currently tracked files that should be untracked

These are tracked now and are fully coverable by ignore patterns (or are
regenerable build outputs). **Untracking** = `git rm --cached -r <path>` then add
to `.gitignore`.

| Path | Size (tracked) | Why it should not be tracked | Action |
|---|---|---|---|
| `audio/fuzz.wav` | 6.14 MB | Generated test/demo audio (commit `d0cd3ce`) | Untrack + ignore |
| `audio/clean.wav` | 6.14 MB | Generated test/demo audio | Untrack + ignore |
| `audio/crunch.wav` | 6.14 MB | Generated test/demo audio | Untrack + ignore |
| `audio/heavy.wav` | 6.14 MB | Generated test/demo audio | Untrack + ignore |
| `audio/lead.wav` | 6.14 MB | Generated test/demo audio | Untrack + ignore |
| `docs/ui-mock-chain.html` | 31 KB | Generated UI mock prototype | Untrack + ignore |
| `docs/ui-mock-chain-states.html` | 20 KB | Generated UI mock prototype | Untrack + ignore |
| `docs/ui-mock-chain.png` | 496 KB | Generated UI mock screenshot | Untrack + ignore |
| `docs/ui-mock-chain-states.png` | 139 KB | Generated UI mock screenshot | Untrack + ignore |
| `docs/ui-mock-neuraldsp.html` | 18 KB | Generated UI mock prototype | Untrack + ignore |
| `shell/gen/schemas/capabilities.json` | 154 B | Generated Tauri 2 schema manifest | Untrack + ignore |
| `shell/gen/schemas/desktop-schema.json` | 117 KB | Generated Tauri 2 schema manifest | Untrack + ignore |
| `shell/gen/schemas/acl-manifests.json` | 67 KB | Generated Tauri 2 schema manifest | Untrack + ignore |
| `shell/gen/schemas/macOS-schema.json` | 117 KB | Generated Tauri 2 schema manifest | Untrack + ignore |
| `Cargo.lock` | 135 KB | **Currently ignored *and* tracked** — conflicting | **Decision needed** (see §1.1) |

**Subtotal of clearly-untrackable generated artifacts: ~38 MB** (36.8 MB audio +
~0.7 MB docs mocks + ~0.3 MB shell schemas).

### 1.1 `Cargo.lock` — conflicting ignore entry (needs a decision)

The root `.gitignore` ends with `Cargo.lock`, so this file is **ignored but
currently tracked**. Two coherent options:

- **Commit it** (recommended for a Cargo workspace). `Cargo.lock` pins the exact
  dependency versions for the engine/shell/workspace builds and is what most
  Rust projects keep in version control. Removing `Cargo.lock` from `.gitignore`
  makes the file tracked again (best practice).
- **Untrack and keep ignored.** If the repo deliberately does not want
  `Cargo.lock` (e.g. an SDK to be consumed, or deps resolved per-device at
  build time), then `Cargo.lock` should be removed from `.gitignore` as a
  mistake, OR the `.gitignore` rule removed and the file untracked.

Either way the `.gitignore` entry and the tracked state disagree; that is the
only inconsistency in the current rule set.

### 1.2 Files that are already untracked now but were tracked historically

`ui/dist/assets/*.js` / `*.css` and `ui/node_modules/**` were committed in
several history commits (see §4) and are **no longer tracked** — the ignore
rules (`ui/dist/`, `ui/node_modules/`) were added later. These are **history-
only** problems (§5), not current tracked files.

---

## 2. Existing root `.gitignore` — coverage and gaps

Current contents of `\.gitignore`:

```
target/
**/*.rs.bk
presets/*-local.json
output.wav
docs/.heartbeat
plugin/build/
plugin/build
index/
.DS_Store
.vscode/
ui/standalone-tmp/
ui/build-standalone.mjs

# Kanban workers: one git worktree per card (see AGENTS.md "Worktrees")
.worktrees/
# Installed and built locally, never committed: committing them made every
# card branch conflict with every other.
ui/node_modules/
ui/dist/
Cargo.lock
```

### 2.1 Already covered (working as intended)

| Pattern | Covers |
|---|---|
| `target/` | All `target/` dirs at any level (`engine/target` is a file here, but `firmware/target/`, `live/target/`, `graph/target/` are covered by the no-slash basename rule) |
| `**/*.rs.bk` | Rust backup files |
| `ui/node_modules/`, `ui/dist/` | React/Preact node_modules and the built UI bundle |
| `plugin/build/`, `plugin/build` | iPlug2 plugin build output |
| `.DS_Store`, `.vscode/` | macOS / VS Code editor files |
| `index/`, `output.wav`, `docs/.heartbeat` | App/root output artifacts |
| `presets/*-local.json` | Per-machine preset overrides |
| `.worktrees/` | Kanban worktree isolation |
| `ui/standalone-tmp/`, `ui/build-standalone.mjs` | Standalone build scratch |
| `Cargo.lock` | (covered; see the conflict in §1.1) |

### 2.2 Gaps — patterns that should be added

| New pattern | Category | Current coverage |
|---|---|---|
| `audio/*.wav` | Test/output audio | `output.wav` only — matches the root output.wav rule |
| `docs/ui-mock*` | Generated UI mock artifacts | none |
| `shell/gen/schemas/` | Generated Tauri 2 schema manifests | none |
| `*.log`, `logs/` | Log files | none at root (nested iPlug2es have their own, irrelevant here) |
| `*.swp`, `*.swo`, `.*.sw[po]`, `.*~` | Editor swap files | none |
| `__pycache__/`, `*.pyc`, `*.pyo` | Python bytecode/cache | none |
| `*.idea/`, `*.iml`, `*.suo`, `*.user`, `*.sdf`, `*.ncb`, `*.opensdf`, `*.vcxproj`, `*.vcproj.*` | IDE files (VS Code root only) | `.vscode/` only; no global IDE coverage |
| `*.dSYM`, `*.dSYM.zip` | macOS debug symbols | none |
| `*.exe`, `*.dll`, `*.dylib`, `*.so`, `*.bin`, `*.o` | Built binaries | `target/` covers Rust objects; these cover other toolchains |
| `coverage/`, `.coverage`, `htmlcov/`, `*.coverage` | Coverage output | none |
| `wasm/pkg/`, `*.wasm`, `*.js.map` | WASM build output / source maps | none (engine has no wasm yet; add proactively) |
| `*.DS_Store?` | macOS `DS_Store` variants | `.DS_Store` only matches the exact name |
| `index/` | (already present) | — |

*Notes on the gaps:* `audio/*.wav` is the **only** currently-present category
that is both present and currently tracked. The others (editor swaps, logs,
coverage, wasm, IDE files) are **recommended additions** because they are
standard and will appear during normal development; none are present in the
current worktree. `shell/gen/schemas/` and `docs/ui-mock*` are the two present
categories that warrant immediate untracking.

---

## 3. Nested `.gitignore` files

All nested `.gitignore` files live under
`plugin/third_party/iPlug2` and belong to the **third-party iPlug2** tree. They
cover Xcode/Xcode-IDE files, build dirs, node_modules, and IDE caches for the
iPlug2 examples and WDL/eel2 — none of them affect the main repo or the pedal
engine. The main repo is governed entirely by the root `.gitignore` (+ the
`target/`, `ui/*` rules which are no-slash patterns and match at any depth).

**Gap:** the iPlug2 nested files are dev-only for that third-party tree and are
otherwise fine. No action required for this audit.

---

## 4. Files ever added in history (large / generated / build artifacts)

### 4.1 Largest blobs in history (top 12 by size)

All of these are `ui/node_modules` build artifacts that were committed during
the Tailwind/Cypress/UI-conversion work. They are **not secrets** (see §5).

| # | Path (abridged) | Size |
|---|---|---|
| 1 | `ui/node_modules/.pnpm/@esbuild+darwin-arm64@0.28.2/.../esbuild` | 10.6 MB |
| 2 | `ui/node_modules/.pnpm/typescript@5.9.3/.../typescript.js` | 9.1 MB |
| 3 | `ui/node_modules/.pnpm/lightningcss-darwin-arm64@1.33.0/.../lightningcss.darwin-arm64.node` | 8.5 MB |
| 4 | `ui/node_modules/.pnpm/typescript@5.9.3/.../_tsc.js` | 6.2 MB |
| 5 | `ui/node_modules/.pnpm/@tailwindcss+oxide-darwin-arm64@4.3.5/.../tailwindcss-oxide.darwin-arm64.node` | 3.0 MB |
| 6 | `ui/node_modules/.pnpm/@rollup+rollup-darwin-arm64@4.68.2/.../rollup.darwin-arm64.node` | 1.9 MB |
| 7 | `ui/node_modules/.pnpm/typescript@5.9.3/.../lib.dom.d.ts` | 1.9 MB |
| 8 | `ui/node_modules/.pnpm/@tailwindcss+oxide-wasm32-wasi@4.3.5/.../tailwindcss-oxide.wasm32-wasi.wasm` | 1.7 MB |
| 9 | `ui/node_modules/.pnpm/jiti@2.7.0/.../jiti/dist/babel.cjs` | 1.5 MB |
| 10 | `ui/node_modules/.pnpm/@babel+parser@7.29.14/.../index.js.map` | 1.5 MB |
| 11 | `ui/node_modules/.pnpm/vite@7.3.8/.../vite/dist/node/chunks/config.js` | 1.3 MB |
| 12 | `ui/node_modules/.pnpm/chrome-remote-interface@0.33.3/.../chrome-remote-interface.js` | 1.3 MB |

- Top 10 node_modules blobs total **~44 MB**; top 40 total **~60 MB**.
- Total `ui/node_modules` history pack ≈ **100–112 MB** (11,801 tracked entries;
  410 is directory/tree entries with small sizes).
- `git rev-list --objects --all` shows the same `ui/dist/assets/*.js`/`.css`
  variants were committed on multiple occasions (the `ui/dist/` files listed in
  §1.2).

### 4.2 Files ever added (`git log --all --diff-filter=A --name-only`)

| File / class | Added by commit | Size/impact |
|---|---|---|
| `audio/*.wav` (6 files) | `d0cd3ce` "Add host reference model (wav_process) + live tweakable pedal app (cpal/egui)" | 36.8 MB |
| `ui/dist/assets/index-*.js` / `*.css` | several build commits | build output, regenerated each build |
| `ui/node_modules/**` | `4fcc81d`, `4255ed6`, `48c1e73`, `a2f485a` (Tailwind/Cypress work) | ~100–112 MB |
| `docs/ui-mock-chain.png` | `8e9916d` "docs: UI verbosity audit/redesign..." and `a227e74` "wip: unreviewed output of parallel kanban workers" | 496 KB |
| `shell/gen/schemas/*.json` | `0606e3b` "shell: Tauri desktop shell hosting the UI and spawning the engine" | ~0.3 MB |
| `app/**` (legacy POC: `app/tests/channel_render.rs`, etc.) | `f882339`, `a227e74`, `ee0b040`, `f9b1111`, `b11c5ac`, `9b6de19` | removed; `app/` dir no longer exists |

### 4.3 Active `target/` vs `node_modules/`

`target/`, `ui/node_modules/`, `ui/dist/`, `plugin/build/` are **all already
gitignored and untracked today** — the earlier "history" blobs in §4 are the
only reason history is fat. `git status` of the current checkout confirms no
`target/`, `ui/node_modules/`, `ui/dist/`, or `plugin/build/` files are tracked.

---

## 5. Secrets, keys, and tokens in history

Searched **every commit** (`git grep -Ei` over `git rev-list --all` for
`api[_-]?key`, `password`, `passwd`, `passphrase`, `secret`, `token`,
`authorization`, `client_secret`, `private key`, `id_rsa`, `id_ed25519`,
`BEGIN RSA/EC`, `Bearer`, `-----BEGIN`, `cookies`, `refresh_token`).

**Result: no secrets, API keys, private keys, or credentials exist in the repo.**
The only matches were documentation prose, e.g. `docs/TONE3000-API-RECON.md`
(discussing token/key concepts semantically, not containing values) and
`AGENTS.md` ("colours or tokens", "states (hover, active...)"). No literal
values, no embedded credentials, no private keys. **No history rewriting is
required for security reasons.**

---

## 6. Is history rewriting necessary?

**Short answer: no.** Untracking is sufficient for the currently-tracked files.

- **Secrets/keys/tokens:** none exist → no security-driven rewrite.
- **Large generated blobs (`ui/node_modules`, historical `ui/dist`):**
  These are pure build artifacts with no runtime value. They add ~100–112 MB to
  the pack and bloat every clone. Removing them permanently requires history
  rewriting (`git filter-repo` or BFG), which rewrites every commit touching
  those paths. That is **optional, not required** — and the task explicitly
  asks for the audit only, not a rewrite.
- **Recommendation:** (a) untrack the listed files and add the ignore patterns;
  (b) leave history alone for now, **or** if the maintainers want a lean
  history, slice it with `filter-repo` (include `--blob-callback "path.endswith('.js') and 'node_modules' in path`" exclusions) as a separate future task.

Untracking today satisfies the acceptance criteria; history rewriting is
deferred as an explicit choice, not an omitted step.

---

## 7. Recommended `.gitignore` additions (grouped by category)

Append to the **root** `.gitignore` (the nested `plugin/third_party/iPlug2`
files govern only that third-party tree):

```
# --- Audio / test / output artifacts ---
audio/*.wav

# --- Generated documentation / mocks ---
docs/ui-mock*

# --- Generated build artifacts ---
shell/gen/schemas/

# --- Logs / coverage ---
*.log
logs/
coverage/
.coverage
htmlcov/
*.coverage

# --- Editor swap files ---
*.swp
*.swo
.*.sw[po]
.*~

# --- IDE / editor files (beyond .vscode/) ---
*.idea/
*.iml
*.suo
*.user
*.sdf
*.ncb
*.opensdf
*.vcxproj
*.vcproj.*
*.natvis

# --- macOS / OS ---
*.DS_Store?
.AppleDouble
.LSOverride
._*

# --- Windows ---
*.toast
*.lnk

# --- Binaries / built objects (supplement target/) ---
*.exe
*.dll
*.dylib
*.so
*.bin
*.o

# --- WASM build output ---
wasm/pkg/
*.wasm
*.js.map

# (already present, re-listed for completeness)
target/
**/*.rs.bk
output.wav
docs/.heartbeat
plugin/build/
plugin/build
index/
.DS_Store
.vscode/
ui/standalone-tmp/
ui/build-standalone.mjs
.worktrees/
ui/node_modules/
ui/dist/
Cargo.lock
```

> Note: `*.DS_Store` already exists; the new `*.DS_Store?` plus `._*` and
> `._*` variants catch `._foo` macOS resource forks.

---

## 8. Recommended actions (for a follow-up card, NOT executed here)

1. **Edit root `.gitignore`** → append the §7 addition list (including
   `audio/*.wav`, `docs/ui-mock*`, `shell/gen/schemas/`, `Cargo.lock` resolution).
2. **Untrack** (in a new branch/commit): the 6 `audio/*.wav`, the 5 `docs/ui-mock*`,
   and the 4 `shell/gen/schemas/*.json` (`git rm --cached -r` per path).
3. **Resolve `Cargo.lock`**: either commit it (remove `Cargo.lock` from
   `.gitignore`) or keep it ignored — the two must not disagree.
4. **History (optional, deferred):** decide on a `filter-repo`/`BFG` pass for
   the ~100 MB of historical `ui/node_modules` blobs. Not required for
   security, purely a size-cleanliness choice.

---

*Audit produced for card t_389c8275. History was inspected but **not rewritten** (that is a
separate decision).*
