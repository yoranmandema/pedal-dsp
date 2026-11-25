//! Engine-owned lifecycle: every engine instance is a single owner of its
//! audio streams, IPC listener, shared state and executor. No global
//! singletons; each instance is independent and is torn down deterministically
//! by `Engine::stop()` (explicit, idempotent) and `Drop` (best-effort).
//!
//! Ownership contract
//! -----------------
//! * `Engine::new` builds the instance and returns it. The caller (the shell,
//!   via `engine/src/main.rs`) owns the `Engine` and drops it on exit.
//! * `Engine::stop()` is the explicit shutdown API: it sets `running=false` and
//!   signals the main loop. It is idempotent (a second call returns immediately).
//! * `Drop` runs the same teardown best-effort: drops the streams (stops the
//!   audio device), drops the listener (stops accepting), drops `Shared` (frees
//!   state, executor, rings), then the tuner thread handle.
//! * Parent death is detected with a control pipe: the shell keeps one end open
//!   and passes the other to the engine; on EOF the engine runs the same teardown
//!   and (via `main`) exits 0.
//! * `engine.wait` (a control message over the IPC socket) is the explicit,
//!   idempotent API for a clean shutdown: it sets `running=false`, wakes the main
//!   loop, and the process exits 0. `Engine::stop()` is idempotent.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::thread;

use tokio::net::TcpListener;
use tokio::sync::Notify;

use crate::audio::{new_engine, Handle, Setup};
use crate::headless;
use crate::server::{serve_listener, Shared};
use crate::store::Store;

/// Loopback-only default port for the shipped single-instance app (`docs/IPC-PROTOCOL.md`).
pub const PORT: u16 = 8737;

/// Default pidfile path for an instance: `/tmp/pedal-engine-<instance>.pid`.
pub fn default_pidfile(instance: &str) -> PathBuf {
    PathBuf::from("/tmp").join(format!("pedal-engine-{instance}.pid"))
}

/// Default log path for an instance: `$PEDAL_ENGINE_LOG`, else
/// `/tmp/pedal-engine-<instance>.log`.
pub fn default_log_path(instance: &str) -> PathBuf {
    if let Ok(p) = std::env::var("PEDAL_ENGINE_LOG") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    PathBuf::from("/tmp").join(format!("pedal-engine-{instance}.log"))
}

/// The engine's lifecycle state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EngineStatus {
    /// The engine is running and serving clients.
    Running,
    /// The engine is stopped (graceful or via `engine.wait`).
    Stopped,
}

impl EngineStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            EngineStatus::Running => "running",
            EngineStatus::Stopped => "stopped",
        }
    }
}

/// Audio streams owned by the engine. They are kept alive for the lifetime of
/// the process and dropped on shutdown so the device is released.
///
/// The device-backed case owns the cpal input + output streams. The headless
/// case owns nothing here (its device is a simulated thread owned by the
/// engine process and released when the process exits).
pub enum AudioStreams {
    Device {
        input: cpal::Stream,
        output: cpal::Stream,
    },
}

impl AudioStreams {
    /// The device-backed streams.
    pub fn device(input: cpal::Stream, output: cpal::Stream) -> Self {
        Self::Device { input, output }
    }
}

/// Arguments the engine process accepts at startup. All new per-instance
/// flags are additive to the existing surface (the shipped app keeps the old
/// defaults: port 8737, no flags).
#[derive(Clone, Debug)]
pub struct EngineArgs {
    pub instance: String,
    pub port: u16,
    pub control_pipe: Option<PathBuf>,
    pub pidfile: Option<PathBuf>,
    pub log: Option<PathBuf>,
    pub bench: Option<f32>,
    pub headless: bool,
    /// Headless input source: 0 riff (default), 1 live tone, 2 silence.
    pub source: u8,
    pub rig: String,
    pub migrate_presets: bool,
}

impl EngineArgs {
    /// Parse the command line into per-instance values.
    pub fn parse() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let flag = |name: &str| args.iter().any(|a| a == name);
        let value = |name: &str| {
            args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
        };
        Self {
            instance: value("--instance").unwrap_or_else(Self::default_instance_id),
            port: value("--port")
                .and_then(|v| v.parse::<u16>().ok())
                .unwrap_or(PORT),
            control_pipe: value("--control-pipe").map(PathBuf::from),
            pidfile: value("--pidfile").map(PathBuf::from),
            log: value("--log").map(PathBuf::from),
            bench: value("--bench")
                .and_then(|v| v.parse::<f32>().ok())
                .filter(|s| *s > 0.0),
            headless: flag("--headless"),
            source: match value("--source").as_deref() {
                Some("live") => 1,
                Some("silence") => 2,
                _ => 0,
            },
            rig: value("--rig").unwrap_or_else(|| "04-rainbow".into()),
            migrate_presets: flag("--migrate-presets"),
        }
    }

    fn default_instance_id() -> String {
        // `<pid>-<random16>`, hex. The 16 bytes are random-looking but
        // deterministic (same pid → same id), so two restarts of the same
        // process get the same id: a stale pidfile from a killed instance is
        // then owned by the new one, which is exactly the stale-engine
        // resolver's contract (docs/IPC-PROTOCOL.md §3.3).
        let pid = std::process::id() as u64;
        let mut state = pid.wrapping_mul(0x9E3779B97F4A7C15);
        state ^= state >> 33;
        state = state.wrapping_mul(0xC870C39B3C2325B1);
        let mut out = [0u8; 16];
        for b in out.iter_mut() {
            *b = (state & 0xFF) as u8;
            state = state.wrapping_mul(0x9E3779B97F4A7C15);
        }
        // Hex-encode instead of UTF-8-lossily-decoding raw bytes: the PRNG
        // output is not guaranteed to be valid UTF-8.
        const HEX: &[u8] = b"0123456789abcdef";
        let mut hex = String::with_capacity(32);
        for &b in &out {
            hex.push(HEX[(b >> 4) as usize] as char);
            hex.push(HEX[(b & 0x0F) as usize] as char);
        }
        format!("{}\n", hex)
    }

    /// Where the engine writes its pidfile and prints the `ready` line. The
    /// shell coordinates the `ready` line with port allocation.
    pub fn pidfile(&self) -> PathBuf {
        self.pidfile.clone().unwrap_or_else(|| default_pidfile(&self.instance))
    }

    pub fn log_path(&self) -> PathBuf {
        self.log.clone().unwrap_or_else(|| default_log_path(&self.instance))
    }
}

/// An engine process instance: owns the audio streams, the listener and the
/// shared state, and tears everything down in the right order.
///
/// The field order is the teardown order: `streams` and `listener` are dropped
/// before `sh` (the shared state), which is dropped before `tuner`. This keeps
/// the audio device and the IPC socket released before the executor /
/// registry / meters are freed.
pub struct Engine {
    /// Per-instance identity and endpoints.
    pub instance: String,
    pub port: u16,
    /// Audio streams backed by the device. `None` for headless. Dropped before
    /// the listener and the shared state.
    pub streams: Option<AudioStreams>,
    /// The IPC listener. Dropped before the shared state.
    pub listener: Option<TcpListener>,
    /// The audio thread's running flag. `false` starts the shutdown sequence.
    pub running: Arc<AtomicBool>,
    /// The idempotent stop flag.
    pub stopped: AtomicBool,
    /// The simulated headless device (only used when `--headless`).
    pub headless: Option<headless::Headless>,
    /// Everything the protocol can touch. Dropped last so the audio device and
    /// the listener are released first, and the executor, registry and meters
    /// are freed last (no live callbacks reference them).
    pub sh: Arc<Shared>,
    /// The detached tuner thread. Dropped last, as the process exits when
    /// `main` returns and the OS reclaims all threads.
    pub tuner: Option<thread::JoinHandle<()>>,
    /// Tells the tuner thread to exit.
    tuner_stop: Arc<AtomicBool>,
    /// The control-pipe watcher thread (if any).
    watcher: Option<thread::JoinHandle<()>>,
}

impl Engine {
    /// Construct a new engine instance from parsed arguments.
    ///
    /// This does the per-instance setup: instance identity, pidfile, the
    /// `ready` log line, the initial rig, the audio streams, the IPC listener,
    /// the tuner thread and the control-pipe watcher. It does not block on the
    /// accept loop.
    ///
    /// # Errors
    ///
    /// Returns an error if the pidfile cannot be written. A failure to bind
    /// the listener is reported as an error from `run`, not from `new`.
    pub async fn new(args: &EngineArgs) -> std::io::Result<Self> {
        let instance = args.instance.clone();
        let pid = std::process::id() as u32;
        let port = args.port;

        // 1. Per-instance pidfile + `ready` line (before the listener binds).
        let pidfile = args.pidfile();
        if !pidfile.as_os_str().is_empty() {
            write_pidfile(&pidfile, pid)?;
        }
        eprintln!("pedal-engine: ready pid={pid} port={port} instance={instance}");

        // 2. Initial rig (compiled before the streams start).
        let config = crate::host::Config::low_latency();
        let devices = if args.headless {
            None
        } else {
            Some(crate::host::pick(Some(&config)))
        };
        let setup = Setup {
            sample_rate: devices
                .as_ref()
                .map(|d| d.rate as f32)
                .unwrap_or(48_000.0),
            out_channels: devices
                .as_ref()
                .map(|d| d.out_channels)
                .unwrap_or(2),
            block: crate::host::BLOCK,
            channel_mode: crate::audio::ChannelMode::First(2),
            latency_frames: crate::host::LATENCY_FRAMES,
        };
        let source = Arc::new(AtomicU8::new(2)); // silence by default

        let (mut handle, cmd_rx, trash_tx, running) = new_engine(setup, source.clone());
        let store = Store::discover();
        handle.preset_dir = store.dir.clone();
        let registry = handle.registry.clone();

        // Compile and install the initial rig.
        let doc = store
            .presets
            .iter()
            .find(|(n, _)| *n == args.rig)
            .map(|(_, d)| d.clone())
            .or_else(|| store.presets.first().map(|(_, d)| d.clone()))
            .unwrap_or_else(|| crate::channels::Rig::new("Untitled").to_doc());
        let rig = crate::channels::Rig::from_doc(&doc);
        {
            let resolver = crate::model_index::resolver();
            let mut g = doc.build_in_with(&handle.registry, &handle.preset_dir, &resolver)
                .unwrap_or_else(|e| {
                    eprintln!("pedal-engine: could not build initial rig: {e}");
                    std::process::exit(1);
                });
            rig.apply_routings(&mut g, &handle.registry)
                .unwrap_or_else(|e| {
                    eprintln!("pedal-engine: rig routings: {e}");
                    std::process::exit(1);
                });
            let ex = dsp_graph::Executor::compile_owned(&mut g, setup.sample_rate, crate::host::BLOCK)
                .unwrap_or_else(|e| {
                    eprintln!("pedal-engine: initial rig compile failed: {e}");
                    std::process::exit(1);
                });
            *handle.node_ids.lock().unwrap() = ex.node_ids().to_vec();
            handle
                .push(crate::audio::Cmd::Swap(ex, Vec::new()))
                .unwrap_or_else(|e| eprintln!("pedal-engine: initial swap failed: {e}"));
        }
        handle.set_source(2); // silence

        // 3. Per-instance log path (the shell also sets PEDAL_ENGINE_LOG).
        let _log = args.log_path();

        // 4. Audio streams (owned by Engine for headless; device-backed for live).
        let (streams, tuner_ring);
        let mut headless_device;
        if args.headless {
            handle.set_source(args.source);
            let headless = crate::headless::start(
                setup,
                cmd_rx,
                trash_tx,
                running.clone(),
                handle.meters.clone(),
                handle.source.clone(),
            );
            tuner_ring = headless.tuner_ring.clone();
            eprintln!("pedal-engine: headless (no audio device)");
            streams = None;
            // The headless device is owned by `Engine` and dropped with it.
            headless_device = Some(headless);
        } else {
            match crate::host::start(
                &devices.as_ref().expect("audio device"),
                &config,
                cmd_rx,
                trash_tx,
                running.clone(),
                handle.meters.clone(),
                source.clone(),
            ) {
                Ok((s, tr)) => {
                    streams = Some(AudioStreams::device(s.input, s.output));
                    tuner_ring = tr;
                }
                Err(e) => {
                    eprintln!("pedal-engine: failed to start audio streams: {e}");
                    std::process::exit(1);
                }
            }
            headless_device = None;
        }

        // 5. IPC shared state.
        let running_flag = running.clone();
        let sh = Arc::new(Shared::new(handle, registry, source, store, running_flag, Arc::new(crate::models::Models::new())));
        // Record the rig name so the UI's snapshot shows it.
        *sh.rig.lock().unwrap() = rig;
        *sh.preset.lock().unwrap() = args.rig.clone();
        let sh_watcher = sh.clone();
        // `self.running` keeps the flag; `sh.running` mirrors it so the server
        // (`apply`) can observe it.
        // 6. Shutdown signal: the main loop wakes on `sh.shutdown`, woken by
        //    `engine.wait` or parent death (control-pipe EOF).
        // 7. Parent-death detection + graceful shutdown command: read the
        // control pipe until EOF or a `engine.wait` command. Either path
        // clears `running` and wakes the main loop so the streams, listener
        // and shared state tear down in order and the process exits 0.
        let watcher = args.control_pipe.clone().map(|cp| {
            let running = running.clone();
            let instance = instance.clone();
            thread::spawn(move || {
                eprintln!("engine: watcher connecting to {:?}", cp);
                let r = read_control_pipe(&cp, &instance, &running, &(*sh_watcher).shutdown);
                if r.is_ok() {
                    eprintln!("engine: control pipe closed (parent died); starting teardown");
                }
                eprintln!("engine: watcher read_control_pipe returned {:?}", r);
            })
        });

        // 8. IPC listener + tuner thread.
        let listener = TcpListener::bind(("127.0.0.1", port)).await?;
        // Port 0 asks the OS for a free port; report the real one.
        let port = listener.local_addr()?.port();
        eprintln!("pedal-engine: ipc listening on ws://127.0.0.1:{port}");
        let tuner_stop = Arc::new(AtomicBool::new(false));
        let tuner = crate::tuner::spawn(
            tuner_ring,
            setup.sample_rate,
            sh.handle.meters.clone(),
            tuner_stop.clone(),
        );

        Ok(Self {
            instance,
            port,
            streams,
            listener: Some(listener),
            running,
            stopped: AtomicBool::new(false),
            headless: headless_device,
            sh,
            tuner: Some(tuner),
            tuner_stop,
            watcher,
        })
    }

    /// Graceful shutdown: idempotent. Starts the sequence; the actual teardown
    /// happens in `run` when the shutdown signal fires the main loop.
    pub fn stop(&self) -> Result<ExitStatus, String> {
        if self.stopped.swap(true, Ordering::Relaxed) {
            return Ok(ExitStatus::default());
        }
        self.running.store(false, Ordering::Relaxed);
        // Wake the main loop so the streams/listener are dropped.
        self.sh.shutdown.notify_one();
        Ok(ExitStatus::default())
    }

    /// Explicit, idempotent full teardown: stops the audio callback, joins the
    /// tuner thread and the simulated device thread, releases the audio device
    /// and the IPC listener (the port is free on return). A second call is a
    /// no-op. `Drop` calls this too.
    pub fn shutdown(&mut self) -> Result<(), String> {
        self.stopped.store(true, Ordering::Relaxed);
        self.running.store(false, Ordering::Relaxed);
        self.tuner_stop.store(true, Ordering::Relaxed);
        self.sh.shutdown.notify_one();
        self.streams = None;
        self.listener = None;
        if let Some(mut h) = self.headless.take() {
            h.stop();
        }
        if let Some(t) = self.tuner.take() {
            t.join().map_err(|_| "tuner thread panicked".to_string())?;
        }
        // The watcher blocks on the control pipe; it exits on EOF and is left
        // detached while the pipe is still open.
        if let Some(w) = self.watcher.take() {
            if w.is_finished() {
                let _ = w.join();
            }
        }
        Ok(())
    }

    /// Current lifecycle state.
    pub fn status(&self) -> EngineStatus {
        if self.stopped.load(Ordering::Relaxed) {
            EngineStatus::Stopped
        } else if self.running.load(Ordering::Relaxed) {
            EngineStatus::Running
        } else {
            EngineStatus::Stopped
        }
    }

    /// The main loop: serve clients until the listener is dropped or the
    /// shutdown signal fires, then tear everything down and return.
    pub async fn run(&mut self) -> Result<(), String> {
        let listener = self.listener.as_ref().expect("listener set in new()");
        // `serve_listener` runs the accept loop. When the shutdown signal fires
        // we drop the listener, which makes `accept()` return and the loop
        // unwind, releasing the port and clients.
        tokio::select! {
            res = serve_listener(self.sh.clone(), listener) => {
                if let Err(e) = res {
                    eprintln!("pedal-engine: ipc: {e}");
                }
                Ok(())
            }
            _ = self.sh.shutdown.notified() => {
                // Teardown order: streams first (stops the device and the
                // callback), then the listener (stops accepting clients),
                // then Shared (frees state, executor, rings). `self` itself
                // is dropped by the caller, releasing the tuner thread.
                self.streams = None;
                self.listener = None;
                Ok(())
            }
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // Best-effort teardown: never panics, never calls `exit()`.
        // Order: streams -> listener -> Shared (the struct fields are
        // dropped in declaration order, which is the order we want)
        // -> tuner (dropped last as it is the last field).
        let _ = self.shutdown();
    }
}

/// Write a pidfile containing `pid`, creating parent directories as needed.
pub fn write_pidfile(path: &Path, pid: u32) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, format!("{pid}\n"))?;
    Ok(())
}

/// Read commands over the control pipe until EOF or a command.
///
/// The control pipe is a unix-domain stream socket: the shell holds one end
/// (a [`UnixListener`] it bound) and the engine holds the other (the connected
/// [`UnixStream`] the engine established). The shell writes one command per
/// line. The only command today is `engine.wait`, which asks the engine to
/// start its graceful shutdown: `running` is cleared and the main loop is
/// woken, so the audio callback returns, the streams and listener are dropped,
/// and `main` returns 0.
///
/// EOF (`Ok(0)`) means the shell closed its write end: a normal quit sent
/// `engine.wait` and then closed the write end, or the shell died. Either way
/// the engine tears itself down.
pub fn read_control_pipe(path: &Path, instance: &str, running: &AtomicBool, shutdown: &Notify) -> std::io::Result<()> {
    let mut stream = UnixStream::connect(path)?;
    // Announce this instance's id to the host on boot: `instance <id>\n`.
    stream.write_all(format!("instance {}\n", instance.trim()).as_bytes())?;
    let mut buf = [0u8; 4096];
    let mut line_buf = Vec::new();
    loop {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            // EOF: the shell closed its write end. Clear `running` and wake the
            // main loop so the teardown proceeds to exit 0.
            running.store(false, Ordering::Relaxed);
            shutdown.notify_one();
            return Ok(());
        }
        line_buf.extend_from_slice(&buf[..n]);
        while let Some(pos) = line_buf.iter().position(|&b| b == b'\n') {
            let line = std::str::from_utf8(&line_buf[..pos]).unwrap_or_default();
            let cmd = line.trim();
            if !cmd.is_empty() && cmd.eq_ignore_ascii_case("engine.wait") {
                running.store(false, Ordering::Relaxed);
                shutdown.notify_one();
            }
            line_buf.drain(..pos + 1);
        }
    }
}
