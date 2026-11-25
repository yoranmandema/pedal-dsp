//! The Pedal desktop shell.
//!
//! A Tauri window hosting the shared Preact UI, plus the engine process that
//! the UI talks to. The shell owns the engine's lifecycle: exactly one engine
//! per shell, started on launch and stopped on exit, so quitting the app never
//! leaves a stray process holding the audio device. There is no global engine
//! handle: the shell owns an `EngineHandle` (RAII), and the UI talks to the
//! engine over the engine's own loopback WebSocket (see
//! `docs/IPC-PROTOCOL.md`), never to the shell.
//!
//! Per-instance plumbing: each engine gets its own loopback port, its own
//! control-pipe listener (for `engine.wait` commands), its own log and pidfile.
//! Closing this shell terminates its engine with no orphan processes.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::net::TcpListener;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio, ExitStatus};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// The engine handle (RAII)
// ---------------------------------------------------------------------------

/// Lifecycle state reported by the shell to the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineStatus {
    Running,
    Exited,
    NotStarted,
}

impl EngineStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            EngineStatus::Running => "running",
            EngineStatus::Exited => "exited",
            EngineStatus::NotStarted => "not started",
        }
    }
}

/// One engine owned by the shell.
///
/// The handle owns the child process, the instance's loopback port, the
/// control-pipe listener (for `engine.wait` commands), the instance log and
/// pidfile. When the handle is dropped (normal or abnormal exit), the
/// child is reaped and the control-pipe write end is closed, so a surviving
/// engine detects EOF and tears itself down (see
/// `pedal_engine::lifecycle`).
pub struct EngineHandle {
    child: Child,
    control_listener: UnixListener,
    control_stream: UnixStream,
    pidfile: PathBuf,
    log: File,
    instance: String,
    port: u16,
}

impl EngineHandle {
    /// Resolve the engine binary.
    ///
    /// Priority (most-specific first):
    ///   1. `PEDAL_ENGINE` (explicit override).
    ///   2. `PEDAL_ENGINE_BIN` (agent-env.sh shared build cache).
    ///   3. Worktree-local `target/` debug/release.
    ///   4. Self-contained bundle (engine next to the app binary).
    ///   5. `pedal-engine` on PATH.
    fn engine_path() -> PathBuf {
        if let Ok(p) = std::env::var("PEDAL_ENGINE") {
            if !p.is_empty() {
                return PathBuf::from(p);
            }
        }
        if let Ok(p) = std::env::var("PEDAL_ENGINE_BIN") {
            if !p.is_empty() && PathBuf::from(&p).exists() {
                return PathBuf::from(p);
            }
        }
        let manifest = env!("CARGO_MANIFEST_DIR");
        for profile in ["debug", "release"] {
            let local = PathBuf::from(manifest)
                .parent()
                .unwrap()
                .join("target")
                .join(profile)
                .join("pedal-engine");
            if local.exists() {
                return local;
            }
        }
        // Self-contained bundle ships the engine beside the app binary.
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                let bundled = dir.join("pedal-engine");
                if bundled.exists() {
                    return bundled;
                }
            }
        }
        PathBuf::from("pedal-engine")
    }

    /// Spawn an engine with per-instance identity.
    ///
    /// The shell:
    ///   1. allocates a loopback port (bind `127.0.0.1:0`, read the assigned
    ///      port, close its copy; the engine binds it itself);
    ///   2. binds a unix-domain listener for the control pipe and accepts one
    ///      connection (the engine's);
    ///   3. creates the instance log and pidfile;
    ///   4. spawns `pedal-engine --rig --port --instance --control-pipe
    ///      --pidfile --log`;
    ///   5. waits for the engine's `ready` line (or the port to open).
    pub fn spawn(rig: &str, instance: &str) -> std::io::Result<Self> {
        // 1. Ephemeral loopback port.
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        drop(listener);

        // 2. Control pipe path: the shell binds the listener *before* the
        //    engine starts (the engine connects to it), but accepts only
        //    after the spawn; see step 4b.
        let control_path = std::env::temp_dir().join(format!(
            "pedal-engine-{}-{}.sock",
            std::process::id(),
            instance
        ));
        let _ = std::fs::remove_file(&control_path);
        let control_listener = UnixListener::bind(&control_path)?;
        control_listener.set_nonblocking(true)?;

        // 3. Instance log + pidfile.
        let pidfile = std::env::temp_dir().join(format!("pedal-engine-{instance}.pid"));
        let log_path = std::env::temp_dir().join(format!("pedal-engine-{instance}.log"));
        let log = File::create(&log_path)?;

        // 4. Spawn the engine with per-instance flags.
        let mut cmd = Command::new(Self::engine_path());
        cmd.arg("--rig")
            .arg(rig)
            .arg("--port")
            .arg(port.to_string())
            .arg("--instance")
            .arg(instance)
            .arg("--control-pipe")
            .arg(&control_path)
            .arg("--pidfile")
            .arg(&pidfile)
            .arg("--log")
            .arg(&log_path);
        // Headless (simulated device) is opt-in for tests/CI. The dev flow
        // runs the engine on the host audio device; `Engine::new` fails
        // cleanly if no device is attached.
        if std::env::var("PEDAL_SHELL_HEADLESS").is_ok() {
            cmd.arg("--headless");
        }
        cmd.stdout(Stdio::null())
            .stderr(log.try_clone()?);
        let mut child = cmd.spawn()?;

        // 4b. Accept the engine's control connection (it connects once the
        //     engine is built). Never block forever: bail out if the engine
        //     dies or the deadline passes, and reap the child either way.
        let control_stream = match accept_control(&control_listener, &mut child, Duration::from_secs(15)) {
            Ok(s) => s,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_file(&control_path);
                return Err(e);
            }
        };
        let _ = std::fs::remove_file(&control_path);

        // 5. Wait for readiness (the `ready` line or the port open).
        if let Err(e) = wait_for_ready(&log_path, port, Duration::from_secs(15)) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }

        Ok(EngineHandle {
            child,
            control_listener,
            control_stream,
            pidfile,
            log,
            instance: instance.to_string(),
            port,
        })
    }

    /// The loopback WebSocket URL of this shell's own engine.
    pub fn url(&self) -> String {
        format!("ws://127.0.0.1:{}", self.port)
    }

    /// Report the engine's current status to the UI.
    pub fn status(&mut self) -> String {
        match self.child.try_wait() {
            Ok(None) => "running".to_string(),
            Ok(Some(status)) => format!("exited: {status}"),
            Err(_) => "unknown".to_string(),
        }
    }

    /// Graceful shutdown, with SIGTERM/SIGKILL escalation.
    ///
    /// 1. Send `engine.wait` (the engine's own shutdown command) over the
    ///    control pipe and wait for the child to exit.
    /// 2. If it does not exit within `timeout`, send `SIGTERM`, wait, then
    ///    `SIGKILL` if needed.
    pub fn stop(&mut self, timeout: Duration) -> std::io::Result<ExitStatus> {
        // Ask the engine to shut down gracefully.
        if let Some(s) = self.child.try_wait()? {
            return Ok(s); // already gone: idempotent
        }
        let _ = self.control_stream.write_all(b"engine.wait\n");
        match self.wait_timeout(timeout)? {
            Some(s) => return Ok(s),
            None => {}
        }
        // Escalate: SIGTERM, then SIGKILL.
        self.child.kill().ok();
        match self.wait_timeout(Duration::from_secs(3))? {
            Some(s) => Ok(s),
            None => Err(std::io::Error::new(
                ErrorKind::Other,
                "engine did not exit within the shutdown timeout",
            )),
        }
    }

    /// Poll the child until it exits (or the deadline passes).
    fn wait_timeout(&mut self, timeout: Duration) -> std::io::Result<Option<ExitStatus>> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.child.try_wait()? {
                Some(s) => return Ok(Some(s)),
                None => {
                    if Instant::now() >= deadline {
                        return Ok(None);
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        }
    }
}

impl Drop for EngineHandle {
    fn drop(&mut self) {
        // Best-effort reap + close the control-pipe write end. A surviving
        // engine detects EOF on the control pipe and tears itself down.
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.pidfile);
    }
}

// ---------------------------------------------------------------------------
// Readiness wait + engine-side helper
// ---------------------------------------------------------------------------

/// Poll the engine's instance log (and the loopback port) until the engine is
/// ready, reporting the last log tail on failure.
fn wait_for_ready(log_path: &PathBuf, port: u16, deadline: Duration) -> std::io::Result<()> {
    let start = Instant::now();
    let mut tail = String::new();
    loop {
        // The engine's stderr was redirected to `log_path`; read the tail to
        // see the `ready` line (the engine prints it before binding the port).
        let mut f = std::fs::OpenOptions::new().read(true).open(log_path)?;
        let mut content = String::new();
        f.read_to_string(&mut content)?;
        if content.contains("pedal-engine: ready") || content.contains("ipc listening") {
            return Ok(());
        }
        // The port is the stronger signal: the engine bound its listener.
        if port_is_open("127.0.0.1", port) {
            return Ok(());
        }
        if start.elapsed() > deadline {
            tail = content;
            return Err(std::io::Error::new(
                ErrorKind::Other,
                format!(
                    "engine did not become ready within {} s; last log:\n--\n{}--",
                    deadline.as_secs(),
                    tail
                ),
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn accept_control(l: &UnixListener, child: &mut Child, deadline: Duration) -> std::io::Result<UnixStream> {
    let start = Instant::now();
    loop {
        match l.accept() {
            Ok((st, _)) => {
                st.set_nonblocking(false)?;
                return Ok(st);
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {}
            Err(e) => return Err(e),
        }
        if let Some(status) = child.try_wait()? {
            return Err(std::io::Error::new(ErrorKind::Other, format!("engine exited during startup: {status}")));
        }
        if start.elapsed() > deadline {
            return Err(std::io::Error::new(ErrorKind::TimedOut, "engine never connected its control pipe"));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn port_is_open(addr: &str, port: u16) -> bool {
    std::net::TcpStream::connect((addr, port)).is_ok()
}

// ---------------------------------------------------------------------------
// Tauri invoke: engine status
// ---------------------------------------------------------------------------

/// The shell's engine, held in Tauri managed state (one per shell process).
struct ShellEngine(std::sync::Mutex<EngineHandle>);

#[tauri::command]
fn engine_status(state: tauri::State<ShellEngine>) -> String {
    match state.0.lock() {
        Ok(mut h) => h.status(),
        Err(_) => "unknown".to_string(),
    }
}

#[tauri::command]
fn engine_url(state: tauri::State<ShellEngine>) -> String {
    state.0.lock().map(|h| h.url()).unwrap_or_default()
}

fn stop_engine(app: &tauri::AppHandle) {
    use tauri::Manager;
    if let Some(state) = app.try_state::<ShellEngine>() {
        if let Ok(mut h) = state.0.lock() {
            let _ = h.stop(Duration::from_secs(5));
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    // Unique instance id per shell (the pid): logs, pidfiles, ports and
    // control pipes are isolated, so several shells can run at once.
    let instance = format!("shell-{}", std::process::id());
    let engine = EngineHandle::spawn("04-rainbow", &instance).expect("failed to spawn the engine");
    let url = engine.url();
    println!("pedal-shell: instance {instance} engine {url}");

    // The UI connects to *this* shell's engine: the URL is injected before any
    // page script runs (see ui/src/bridge.ts, `__PEDAL_ENGINE_URL__`).
    let init = format!("window.__PEDAL_ENGINE_URL__ = {};", serde_json::to_string(&url).unwrap());

    let app = tauri::Builder::default()
        .manage(ShellEngine(std::sync::Mutex::new(engine)))
        .invoke_handler(tauri::generate_handler![engine_status, engine_url])
        .append_invoke_initialization_script(init)
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    // Window close, Cmd-Q and exit requests all end in `RunEvent::Exit`: ask
    // the engine to shut down, wait (SIGKILL after the timeout), then exit.
    // The window-close and quit-request events shut the engine down *before*
    // the window goes away (bounded by the 5 s timeout inside `stop`);
    // `Exit` is the backstop. `stop` is idempotent. Page reload/beforeunload
    // deliberately does not stop the engine: the shell owns its lifetime.
    app.run(|app, event| match event {
        tauri::RunEvent::WindowEvent {
            event: tauri::WindowEvent::CloseRequested { .. },
            ..
        }
        | tauri::RunEvent::ExitRequested { .. }
        | tauri::RunEvent::Exit => stop_engine(app),
        _ => {}
    });
}
