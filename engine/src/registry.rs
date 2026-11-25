//! Host-side registry of live engine processes, keyed by instance id.
//!
//! The host (shell / UI server) opens engines on demand with [`Registry::open`].
//! Each engine gets its own loopback port, control pipe, pidfile-free log and
//! reader thread. The control pipe is the only host<->engine channel; engine
//! events (`Announced`, `Closed`) reach the host through one `mpsc` channel.
//!
//! Shutdown: `close(id)` sends `engine.wait`, then escalates to kill. Dropping
//! the registry closes every engine, and a host that dies without dropping
//! closes the pipes, which the engine sees as EOF and shuts down on.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::net::TcpListener;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Events flowing engine -> host over the control pipe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    /// The engine announced its instance id (`instance <id>`).
    Announced(String),
    /// The control pipe closed (engine exited or crashed).
    Closed(String),
}

struct Entry {
    child: Child,
    stream: UnixStream,
    port: u16,
    reader: Option<JoinHandle<()>>,
    sock_path: PathBuf,
}

/// Live engines keyed by instance id.
pub struct Registry {
    bin: PathBuf,
    headless: bool,
    engines: HashMap<String, Entry>,
    tx: Sender<HostEvent>,
    rx: Receiver<HostEvent>,
}

impl Registry {
    /// `bin` is the `pedal-engine` binary; `headless` runs engines on the
    /// simulated device (no audio output).
    pub fn new(bin: PathBuf, headless: bool) -> Self {
        let (tx, rx) = channel();
        Registry { bin, headless, engines: HashMap::new(), tx, rx }
    }

    /// Open an engine for `instance`; returns its loopback port. Errors if the
    /// id is already live.
    pub fn open(&mut self, instance: &str, rig: &str) -> std::io::Result<u16> {
        self.reap();
        if self.engines.contains_key(instance) {
            return Err(std::io::Error::new(ErrorKind::AlreadyExists, format!("instance {instance} already open")));
        }
        let port = {
            let l = TcpListener::bind("127.0.0.1:0")?;
            l.local_addr()?.port()
        };
        let sock_path = std::env::temp_dir().join(format!("pedal-reg-{}-{}.sock", std::process::id(), instance));
        let _ = std::fs::remove_file(&sock_path);
        let listener = UnixListener::bind(&sock_path)?;
        listener.set_nonblocking(true)?;
        let mut cmd = Command::new(&self.bin);
        cmd.arg("--rig").arg(rig)
            .arg("--port").arg(port.to_string())
            .arg("--instance").arg(instance)
            .arg("--control-pipe").arg(&sock_path)
            .arg("--pidfile").arg(std::env::temp_dir().join(format!("pedal-reg-{}-{}.pid", std::process::id(), instance)))
            .arg("--log").arg(std::env::temp_dir().join(format!("pedal-reg-{}-{}.log", std::process::id(), instance)));
        if self.headless {
            cmd.arg("--headless");
        }
        let mut child = cmd.stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
        let deadline = Instant::now() + Duration::from_secs(15);
        let stream = loop {
            match listener.accept() {
                Ok((s, _)) => break s,
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    if child.try_wait()?.is_some() || Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        let _ = std::fs::remove_file(&sock_path);
                        return Err(std::io::Error::new(ErrorKind::TimedOut, "engine did not connect"));
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(e);
                }
            }
        };
        stream.set_nonblocking(false)?;
        let _ = std::fs::remove_file(&sock_path);
        let rd = stream.try_clone()?;
        let tx = self.tx.clone();
        let id = instance.to_string();
        let reader = thread::spawn(move || {
            for line in BufReader::new(rd).lines().map_while(Result::ok) {
                if let Some(rest) = line.strip_prefix("instance ") {
                    let _ = tx.send(HostEvent::Announced(rest.trim().to_string()));
                }
            }
            let _ = tx.send(HostEvent::Closed(id));
        });
        self.engines.insert(instance.to_string(), Entry { child, stream, port, reader: Some(reader), sock_path });
        Ok(port)
    }

    /// Drop entries whose process already exited.
    fn reap(&mut self) {
        let dead: Vec<String> = self.engines.iter_mut()
            .filter_map(|(k, e)| matches!(e.child.try_wait(), Ok(Some(_))).then(|| k.clone()))
            .collect();
        for k in dead {
            if let Some(mut e) = self.engines.remove(&k) {
                let _ = e.child.wait();
                if let Some(r) = e.reader.take() { let _ = r.join(); }
            }
        }
    }

    pub fn ids(&mut self) -> Vec<String> {
        self.reap();
        let mut v: Vec<String> = self.engines.keys().cloned().collect();
        v.sort();
        v
    }

    pub fn port(&self, instance: &str) -> Option<u16> {
        self.engines.get(instance).map(|e| e.port)
    }

    pub fn pid(&self, instance: &str) -> Option<u32> {
        self.engines.get(instance).map(|e| e.child.id())
    }

    pub fn is_running(&mut self, instance: &str) -> bool {
        match self.engines.get_mut(instance) {
            Some(e) => matches!(e.child.try_wait(), Ok(None)),
            None => false,
        }
    }

    /// Events from every engine (non-blocking drain).
    pub fn events(&self) -> Vec<HostEvent> {
        self.rx.try_iter().collect()
    }

    /// Block up to `timeout` for the next event.
    pub fn next_event(&self, timeout: Duration) -> Option<HostEvent> {
        self.rx.recv_timeout(timeout).ok()
    }

    /// Shut one engine down (graceful, then kill) and remove it. The other
    /// engines are untouched. Closing an unknown id is a no-op.
    pub fn close(&mut self, instance: &str, timeout: Duration) -> std::io::Result<Option<ExitStatus>> {
        let Some(mut e) = self.engines.remove(instance) else { return Ok(None) };
        let _ = e.stream.write_all(b"engine.wait\n");
        let deadline = Instant::now() + timeout;
        let status = loop {
            if let Some(s) = e.child.try_wait()? { break s; }
            if Instant::now() >= deadline {
                let _ = e.child.kill();
                break e.child.wait()?;
            }
            thread::sleep(Duration::from_millis(20));
        };
        let _ = e.stream.shutdown(std::net::Shutdown::Both);
        if let Some(r) = e.reader.take() { let _ = r.join(); }
        let _ = std::fs::remove_file(&e.sock_path);
        Ok(Some(status))
    }

    pub fn close_all(&mut self) {
        let ids: Vec<String> = self.engines.keys().cloned().collect();
        for id in ids {
            let _ = self.close(&id, Duration::from_secs(5));
        }
    }
}

impl Drop for Registry {
    fn drop(&mut self) {
        self.close_all();
    }
}
