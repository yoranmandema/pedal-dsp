# Engine instance lifecycle: ownership, isolation, and multi-instance support

**Status:** design (not implemented). Owner: `pedal-architect`. Target: `t_88e1c8d1`.
**Scope:** how the shell owns the engine (and guarantees its teardown), and how
port/singletons/state are isolated so several engine+shell pairs can run at once.
Implications for VST3/CLAP plugin hosting are included.

---

## 1. What exists today

### 1.1 The shell (`shell/src/main.rs`)

- One `static ENGINE: Mutex<Option<std::process::Child>>` — a **process-global singleton** holding the engine child.
- `spawn_engine()` runs `pedal-engine --rig 04-rainbow`, stdout null, stderr appended to the **shared** file `/tmp/pedal-engine.log`.
- The Tauri window runs, then `stop_engine()` runs on the way out: `child.kill()` + `child.wait()`.
- No cleanup on abnormal exit: if the window closes by force (SIGKILL, OS kill, crash), the engine is orphaned and keeps the audio device and its port.
- `engine_status()` is exposed to the UI as an invoke command, but is read-only status.

### 1.2 The engine (`engine/src/main.rs`, `engine/src/server.rs`)

- `main()` parses `--port` (default `PORT = 8737`, `engine/src/server.rs:24`), `--rig`, `--headless`, `--bench`, `--migrate-presets`.
- The IPC listener binds `127.0.0.1:<port>`; `serve()`/`serve_port()`/`serve_listener()` are the only bind sites. `serve_listener` is the test seam.
- One `Shared` per process, owned by `main`; the UI talks to it over loopback WebSockets (`docs/IPC-PROTOCOL.md`).
- `AudioThread` owns the executor, the sample rings, the `running: AtomicBool`, and the audio callback (`engine/src/audio.rs`).
- `new_engine()` returns a `Handle` (all UI-thread-only objects, `Arc`-ed) plus the command/trash queues and the `running` flag.
- Streams: `host::start()` builds cpal input/output streams and **`std::mem::forget`t them** so the process stays alive; the callback keeps running until the process exits.
- Shutdown hooks: none besides `serve_port` failing → `std::process::exit(1)`. No `SIGTERM` handler, no "parent died" detection, no heartbeat.

### 1.3 Hardcoded/unisolated surface today

| Surface | Where | Problem |
|---|---|---|
| Loopback port | `engine/src/server.rs:24` `PORT = 8737` (only overridable via `--port`) | Two instances collide; `run.sh` must kill a stale engine first |
| Engine stdout/stderr | `shell/src/main.rs` → `/tmp/pedal-engine.log` (append) | One shared log between instances |
| Preset dir | `engine/src/store.rs::preset_dir()` → `PEDAL_PRESETS` env, else walk-up from exe | `PEDAL_PRESETS` is process-global env; a second instance can point elsewhere, but nothing is per-instance by default |
| Model/home dirs | `PEDAL_HOME`, `PEDAL_MODELS`, `PEDAL_PRESETS` env (inherited) | Same inherited values for all instances; no per-instance model set |
| UI bridge | `ui/src/bridge.ts:165`, `ui/vite.config.ts:7` → `ws://127.0.0.1:8737` | The UI is wired to one hardcoded port |
| Any other singleton | workspace-wide `grep static/once_cell/lazy_static` → none besides the above | No `once_cell`/`lazy_static` globals in `dsp`/`graph`/`engine` |

### 1.4 Relationship diagram (current)

```text
Pedal (shell)             pedal-engine              UI (Preact webview)
   │  spawn --rig 04-rainbow ──▶                       │
   │                            │ binds 127.0.0.1:8737  │
   │                            ◀── "ipc listening" ────│
   │  (window closed)  kill/wait ──▶                    │
   │                                  │  WS req/evs  ───┘
```

Documented symptom: force-kill the shell → engine **survives**, holds 8737 and the Scarlett,
and the next launch says `Address already in use` (`docs/RUN-THE-APP.md` §9).

---

## 2. Ownership model: one shell, exactly one engine

**Rule.** Each shell instance creates, owns, and tears down exactly one engine instance.
The shell is the only thing that can start or stop the engine; the engine has no
autonomous lifecycle. The guarantee is: **when the shell is gone, the engine is gone.**

### 2.1 Shell side

- Replace `static ENGINE: Mutex<Option<Child>>` with an **RAII handle** owned by `main`:
  `shell::EngineHandle { child: Child, log: File, pid: u32 }`.
  - `EngineHandle::spawn(argv) -> io::Result<Self>` — launches `pedal-engine` with the
    instance's port, instance id, log path and per-instance env; returns the handle.
  - `stop(timeout) -> Result<ExitStatus, Error>` — sends the shutdown request first
    (`Wait` control message + `SIGTERM`), waits up to `timeout` (e.g. 5 s), then escalates
    to `SIGKILL`, then reaps. Returns an error if the engine did not exit.
  - `Drop` — best-effort non-panicking `kill()` + `wait()`; never unwinds into the app.
- `main()`:
  1. `let mut engine = EngineHandle::spawn(...)`;
  2. starts the Tauri app (window);
  3. on `run()` returning (window closed / quit) → `engine.stop()` (normal quit path);
  4. on **any** unwinding/crash path before `run()` completes → the handle is dropped,
     `stop()` runs its best-effort reap.
- The shell currently passes `--rig 04-rainbow` as a fixed default; the new contract keeps
  that default for the original single-instance app (see §3.6) and only adds instance flags
  when requested.

### 2.2 Engine side

- **Graceful shutdown sequence** (installed in `engine/src/main.rs`, driven by
  `SIGTERM` + a control/pipe liveness channel):
  1. `running.store(false)` (the audio callback already gates on this; the callback returns,
     no more blocks are processed).
  2. Stop the audio I/O: the input/output cpal streams are **no longer `mem::forget`t**
    — they are owned by `main` behind an `Arc<Mutex<Option<...>>>` and the callback closes
    the stream when `running` goes false. `AudioThread` drops its `cmd_rx`/`trash_tx`.
  3. Stop the IPC server: drop the listener (`serve_listener` drains on accept, in-flight
    clients are closed by `handle_client`); `tuner_loop`/`meters_loop` exit on empty client
    list; `Shared`/`Meters`/`Ring`s are dropped.
  4. `return` from `main` → exit status 0. **No** `std::process::exit` anywhere except the
    documented one-shot flags (`--bench`, `--migrate-presets`, unrecoverable bind failure).
- **Parent-death detection** (Unix): the shell keeps one end of a control pipe open and
  passes the other end to the engine as `--control-pipe` (alternative: the engine opens the
  peer's PID and waits via `waitid(P_PID, ...)` — but the pipe is simpler and works for
  `kill -9`). When the read end hits EOF, the engine runs the same shutdown sequence and
  exits 0.
- **Readiness.** The engine writes its pid into `--pidfile` (default
  `/tmp/pedal-engine-<instance>.pid`) and prints `pedal-engine: ready pid=<n> port=<p>`
  before the listener binds. The shell waits for the ready line (or the port) before it
  reports "engine up" to the UI.
- **Watchdog from the shell:** after `stop()` starts the graceful sequence, the shell polls
  `/proc/<pid>`/`kill(0)` and the log; if the engine does not exit within the timeout it
  `SIGKILL`s and records the incident in the instance log. This is what actually guarantees
  the audio device and port are released, since Rust `Drop` cannot run on a hard kill.

### 2.3 Teardown order (engine)

```text
SIGTERM (or control message / EOF on control pipe)
  → running=false
  → stop audio streams (drop cpal handles; callbacks return)
  → stop IPC server (close listener + clients)
  → drop Shared / Meters / rings / executor
  → main returns → exit 0
shell: SIGTERM → wait ≤5s → SIGKILL → reap
```

**Never** `std::process::exit` in the audio callback or the server task. Never call
`abort()` / `panic!` that crosses an ABI boundary into the host of a plugin instance.

### 2.4 Acceptance checks for the ownership model

- After the app quits normally: `pgrep -f pedal-engine` prints nothing; `lsof -iTCP:<port>`
  is empty; the audio device is released.
- `engine_status()` returns `stopped` from the UI after quit.
- Sent `SIGTERM` to the engine: exit status 0, `pedal-engine: ready` present in its log
  **before** any `ipc listening` line.
- Force-killing the shell (SIGKILL) leaves no engine process and the port is released
  within ~1 s (control-pipe EOF path).
- The engine never exits non-zero from a normal quit, and never leaves a zombie child.

---

## 3. Isolation: no shared ports, singletons, or files

### 3.1 Port allocation

- The **default port** for the original single-instance app stays `8737`
  (`engine/src/server.rs:24`), so the shipped app keeps its documented address.
- For any instance that needs it, the shell **allocates the port itself** by binding
  `127.0.0.1:0` (or using `TcpListener::bind` on port 0), reads the assigned port, and
  passes `--port <p>` to the engine. Both sides agree on the port before any network traffic,
  so N instances cannot collide.
- The engine also accepts `--port 0` → ephemeral, for standalone use (tests, `run.sh`,
  scripts). `serve_port`/`serve_listener` already exist as the seam (`engine/src/server.rs`),
  so no other bind site is introduced.
- The UI/bridge connects to `ws://127.0.0.1:<port>`; the port is **not** a compile-time
  constant in the UI. At dev time the shell already injects `VITE_ENGINE_URL` into the
  Vite dev server; at ship time the embedded bundle is told the port by the shell at startup
  (a build-time constant generated from the port the shell will use), falling back to the
  documented default 8737. See card 3 (UI bridge wiring).

### 3.2 Instance identity

- Each engine/shell pair carries a unique `--instance <id>` (default: `<pid>-<random16>`).
- The id is used in:
  - the pidfile name and the log path (`/tmp/pedal-engine-<id>.log`, or `$PEDAL_ENGINE_LOG`),
  - the `pedal-engine: ready pid=<n> port=<p>` readiness line,
  - any per-instance state the product needs later (state dir, latest snapshot cache).
- The id is **not** sent over the wire as protocol data: connections are identified by
  their TCP socket, per `docs/IPC-PROTOCOL.md` "loopback only".

### 3.3 Per-instance logs and state

- Log file: `PEDAL_ENGINE_LOG` (default `/tmp/pedal-engine-<id>.log`), append mode, one file
  per instance. The old shared `/tmp/pedal-engine.log` is removed from the engineering
  path; a combined "journal" view is a separate feature, not part of isolation.
- No shared writes: the engine owns exactly one `Store` (presets), one `Shared`, one set of
  rings, one model index. Preset `save` keeps its existing clobber-avoidance (`-k` suffix for
  duplicate names; `write_exact` refuses overwrite) — two instances writing the same name
  must never clobber each other.
- Limit: two instances using the same preset dir is supported (each reads the same files,
  writes are still safe); two instances with different model sets need different
  `PEDAL_MODELS`/`PEDAL_HOME` (see card 4).

### 3.4 Command/response contract (shell → engine)

New engine lifecycle messages (added to the closed command set in
`engine/src/server.rs::apply`, prefix `engine.`):
`engine.wait` (no args, answers `res` with `ok: true` and the snapshot) — the shell sends
this when quitting, so the engine starts its graceful sequence from the same command path
the UI uses; `engine.status` answers `"running"`/`"stopped"`/`"dead"`.

### 3.5 Backward compatibility

- The original single-instance app is unchanged in behaviour: no flags by default,
  port 8737, logs to the documented location. The new flags are additive and the old
  command surface is preserved.
- `docs/IPC-PROTOCOL.md` already says a breaking protocol change gets a **new port** and a
  new `protocol` crate; per-instance ports satisfy this without changing the wire format.

### 3.6 Acceptance checks for isolation

- Two shell instances launched concurrently: each engine binds a distinct port; the UI of
  each talks to its own engine only (`client connected` count per engine log).
- `--port 0` instance gets an ephemeral port and no collision with 8737.
- Two instances' logs are separate files and never interleave.
- No instance writes to another instance's pidfile.
- A stale instance from a previous session cannot hold the port of a newly launched one
  unless it is the same instance id (the port is allocated by the new shell, so this is
  inherently safe).

---

## 4. Plugin hosting (VST3/CLAP)

### 4.1 Why this matters

A host loads **multiple plugin instances**. Each instance must be an independent engine:
own audio device/port/state, own teardown, and not depend on the UI. The design in §2–§3 is
readily reused; the question is process vs. in-process.

### 4.2 Recommended: out-of-process per instance

- The host spawns one `pedal-engine` per instance with `--instance <uuid> --port <p>` (the
  port allocated by the host, like the shell) and connects its UI to that port.
- **Pros:** memory isolation (an engine crash cannot take down the host), crash containment,
  no realtime-thread cross-ABI issues, the engine's own `Drop` guarantees teardown.
- **Cons:** extra startup latency; the host's "plugin idle" latency budget must account for
  the spawn.
- The host owns the instance's lifetime: `pluginActivate` → spawn + wait for readiness;
  `pluginDeactivated` → `engine.stop()` (SIGTERM, 5 s, then SIGKILL); `hostStop` → the same
  teardown, then the host reaps the child. The engine must also be able to run detached if
  the host wants "tape out" (engine continues, no UI) — the control-pipe design supports
  this with the host holding no write end.
- The engine must not call `std::process::exit` on plugin stop, and must not touch the host
  process or any process-global state.

### 4.3 In-process fallback: `NativeBridge`

- If the host requires low latency, the engine is compiled as a library behind a
  `PluginEngine` trait: per instance `Shared`, `Handle`, `AudioThread`, `Ring`s, and one
  cpal device. The engine is a plain library: **no global state** (remove `static ENGINE`,
  no `once_cell`/`lazy_static`, no process-global env).
- Risks: an engine panic/abort kills the host; realtime-thread safety is the host's
  responsibility; the audio callback and UI must stay on their separate threads (already
  the case for the desktop shell).
- Out-of-process is still recommended for the product; the in-process path is a
  well-defined alternative for latency-critical hosts, and the port/id isolation code is
  identical in theory but must be verified (no `process::id()`-based uniqueness — use
  host-provided instance ids).

### 4.4 Plugin-specific teardown

- The engine's shutdown sequence (§2.2) is host-agnostic: `SIGTERM` + control pipe
  + `running=false` works whether the parent is a shell, a VST3 host, or a CLI.
- The plugin host must **not** rely on the UI to stop the engine: `pluginDeactivated` alone
  triggers `engine.stop()`; the UI window is orthogonal.
- The engine must release the audio device in `stop()` (streams dropped) — otherwise the
  host's subsequent instance (or a later app launch) inherits the device.

### 4.5 Acceptance checks for plugin support

- A host that loads two instances: both engines bind distinct ports, both serve different
  rigs, and when instance 2 is deactivated its engine exits 0 and its port is released.
- An engine's `Drop`/shutdown does not call `exit()` and leaves the process in a clean
  state for a fresh `pedal-engine` binary to start.
- No per-instance state collides between two instances in the same host process (ports,
  pidfiles, logs, model dirs).

---

## 5. Changes to the UI bridge

- `ui/src/bridge.ts:165` and `ui/vite.config.ts:7` currently hardcode `ws://127.0.0.1:8737`.
- The port is sourced from the shell (dev: `VITE_ENGINE_URL` env; ship: build-time constant
  from the allocated port), with the documented default 8737 as the fallback for the
  original single-instance app.
- The bridge must handle a **different port per instance** (no shared global), reconnect on
  `client connected` loss, and survive the engine restarting (new `rev` counter — the IPC
  protocol already requires ignoring a stale snapshot; a restarted engine's counter begins
  again, which the protocol says to accept).

---

## 6. Decisions recorded

1. Keep port 8737 as the default for the shipped single-instance app; new flags are
   additive.
2. The shell allocates the port (binds 127.0.0.1:0), passes `--port` to the engine;
   `--port 0` means ephemeral elsewhere.
3. `std::mem::forget` on cpal streams is removed; the streams are owned by `main` and the
   callback closes them when `running` goes false.
4. Graceful shutdown = `running=false` → drop audio streams → drop listener → drop
   `Shared` → exit 0. No `std::process::exit` in normal paths.
5. Parent-death detection uses a control pipe (EOF), not `prctl(PR_SET_PDEATHSIG)`: it is
   the portable Unix answer and needs no platform-specific syscall.
6. Each instance owns its log file (`/tmp/pedal-engine-<id>.log`) and pidfile; no shared
   append log, no shared pidfile.
7. Plugin instances are out-of-process by default; in-process `NativeBridge` is the
   documented fallback, requiring the no-global-state rule (veto: `static ENGINE`,
   `once_cell`/`lazy_static`).
8. The UI receives the instance port from the shell at startup (dev env var / ship-time
   constant); it never hardcodes a port in the product path.

---

## 7. Builder cards

| # | Card | Files | Approach | Acceptance criteria | Verify | Try it |
|---|---|---|---|---|---|---|
| 1a | RAII engine ownership handle + guaranteed teardown (engine) | `engine/src/main.rs`, `engine/src/audio.rs`, new `engine/src/lifecycle.rs` | Replace `static ENGINE` in the shell with `shell::EngineHandle` (RAII over `Child`); engine `Drop` runs teardown, `running=false` stops the callback, streams are dropped instead of forgotten, listener dropped, `Shared`/`Meters`/`Ring`s freed, exit 0 | after quit: no `pedal-engine` process, port released, device released; `engine_status()` = `stopped`; SIGTERM → exit 0; `--bench`/migrations still exit 0; force-kill of shell leaves no engine | `cargo test -p pedal-engine` (lifecycle tests); run app, quit, `pgrep -f pedal-engine` empty | Open the app, close the window, confirm the engine is really gone and the Scarlett is released |
| 1b | Shell child ownership, readiness, and status | `shell/src/main.rs`, `shell/capabilities` | Replace the global with `EngineHandle`; spawn once, pass `--instance --port --control-pipe --pidfile --log`; wait for the `ready` line; `engine_status()`/`engine.wait` commands; `stop()` = graceful then SIGKILL | exactly one engine child per shell; `ready` precedes `ipc listening`; `stop()` returns the exit status; no zombie children | `cargo build -p pedal-shell`; run `./shell/run.sh dev` and quit; `ps -ef | grep pedal-engine` empty | Start an app instance, open its UI, quit; the UI's `engine_status` shows `stopped` |
| 2 | Loopback port allocation per instance | `engine/src/server.rs`, `shell/src/main.rs`, `ui/src/bridge.ts`, `ui/vite.config.ts` | shell binds 127.0.0.1:0 → `--port`; engine `--port 0` → ephemeral; UI port from `VITE_ENGINE_URL`/build-time constant, default 8737 | two instances bind distinct ports; `--port 0` works; no instance talks to another's port | `lsof -iTCP:<a>,<b>` distinct; two shells concurrently: `client connected` per engine log | Run two app instances, each UI controls its own rig |
| 3 | Instance identity, logs, env isolation | `engine/src/main.rs`, `engine/src/store.rs`, `shell/src/main.rs`, `docs/` | `--instance <id>` (default `<pid>-<rand16>`); log `PEDAL_ENGINE_LOG` or `/tmp/pedal-engine-<id>.log`; pidfile; `PEDAL_PRESETS`/`PEDAL_MODELS` honours per instance | two instances have separate logs and pids; preset store lists only its own dir; no cross-instance port/preset leakage | `cat` both logs; `PEDAL_PRESETS=<dir2> ./shell/run.sh dev` shows instance 2's presets | Run two instances in different preset dirs; each shows its own presets in the UI |
| 4 | Plugin hosting: out-of-process engine, teardown contract | `engine/src/main.rs`, `engine/src/server.rs`, `engine/src/audio.rs`, `plugin/` | host spawns one engine per instance with its own port/instance/env; `pluginDeactivated` → `engine.stop()`; engine never `exit()`s; audio device released on stop | two host instances both run, one deactivates, its port free and engine exited 0; in-process `NativeBridge` variant passes the no-global-state rule | smoke test: host loads 2 instances, deactivates 1, `pgrep pedal-engine` shows the survivor only | In a test host, load two pedals, kill one, see the other keep working |
| 5 | Shared-state audit and removal | workspace-wide (`dsp/`, `graph/`, `engine/`, `shell/`), docs | remove `static ENGINE`, any `once_cell`/`lazy_static`, shared default paths; port/code derived per instance; `PORT` becomes a documented default, not a global contract | grep shows no process-global singleton (shell `static ENGINE` replaced by the RAII handle; no `lazy_static` in `dsp`/`graph`/`engine`); no file the engine writes is shared | `git grep -nE '\b(static|once_cell|lazy_static)\b'` over the workspace; `ls /tmp/pedal-engine.log` absent in the dev path | `cargo build --workspace`; `cargo test --workspace --exclude pedal-shell` green |
| 6 | Verification tests and stale-engine resolver | `engine/tests/`, `scripts/`, docs | lifecycle test (spawn, quit, assert no child/port), two-instance concurrent test, stale-engine test (kill shell → assert engine exits), plugin teardown smoke | `cargo test -p pedal-engine` green; two instances coexist; stale engine cleans up | `cargo test --workspace --exclude pedal-shell`; `./shell/run.sh dev` twice on different ports | CI-like script: `cargo test -p pedal-engine --test lifecycle` prints "OK: no stray engine" |

**Card order.** 1a/1b (ownership) before 2/3 (isolation); 2 before 3 (port needs the id);
5 (audit) before 4 (plugin, which reuses the isolation); 6 (verification) last. Cards 1a and
1b can be built together by `pedal-builder` in one session.

---

## 8. Open questions for the product owner

1. Ship mode: should each instance's UI receive its port via a build-time constant generated
   by `shell/build-bundle.sh`/`tauri.conf.json`, or via a startup query (`APP_QUERY` or a
   Tauri command)? — I recommend the build-time constant for the shipped app, since the
   shell knows the port before the window is shown.
2. Should the product ship a per-instance log viewer / journal for multiple concurrent
   instances? Out of scope now: per-instance logs are the isolation primitive; a journal is
   a UI feature on top of it.
3. VST3 vs CLAP: which host is on the critical path? Keep the design host-agnostic (out of
   process either way); only the licence/SDK choice and the `NativeBridge` surface are
   host-specific.
