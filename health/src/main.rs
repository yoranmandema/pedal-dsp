//! `pedal-health` — a standalone health monitor for the Pedal DSP Pi.
//!
//! Reads the SoC temperature, the `vcgencmd get_throttled` flags (decoded) and
//! the current ARM clock at ~1 Hz, appends one timestamped CSV line per tick to
//! a log file, and prints a clear alarm when any undervoltage or throttle bit
//! is set.
//!
//! On a non-Pi machine (no `/sys/class/thermal` and no `vcgencmd`) every field
//! degrades to `n/a` and the process keeps running — it never crashes.
//!
//! ```
//! use pedal_health::monitor::HealthMonitor;
//! // Run at ~1 Hz, writing to `pedal-health.log` in the current directory.
//! let mon = HealthMonitor::new(std::path::PathBuf::from("pedal-health.log"), 1.0);
//! mon.run().expect("monitor should never panic");
//! ```

use std::path::PathBuf;

use pedal_health::monitor::HealthMonitor;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut log_path = PathBuf::from("pedal-health.log");
    let mut interval_secs = 1.0;
    for arg in &args[1..] {
        if arg == "--once" {
            // Do a single tick and exit (handy for smoke tests / CI).
            let mon = HealthMonitor::new(
                std::env::var("PEDAL_HEALTH_LOG")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| "pedal-health.log".into()),
                interval_secs,
            );
            let (line, _alarm) = mon.read_tick();
            println!("{line}");
            return;
        } else if arg == "--interval" {
            let next = args
                .iter()
                .position(|a| a == "--interval")
                .and_then(|i| args.get(i + 1))
                .and_then(|v| v.parse::<f64>().ok())
                .filter(|v| *v > 0.0)
                .unwrap_or(1.0);
            interval_secs = next;
        } else if arg == "--log" {
            let next = args
                .iter()
                .position(|a| a == "--log")
                .and_then(|i| args.get(i + 1));
            if let Some(p) = next {
                log_path = PathBuf::from(p);
            }
        }
    }

    println!(
        "pedal-health: monitor starting, log={} interval={}s",
        log_path.display(),
        interval_secs
    );
    let monitor = HealthMonitor::new(log_path, interval_secs);
    if let Err(e) = monitor.run() {
        eprintln!("pedal-health: monitor error: {e}");
        std::process::exit(1);
    }
}
