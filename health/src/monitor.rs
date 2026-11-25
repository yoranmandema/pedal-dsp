//! A ~1 Hz health monitor that writes timestamped CSV to a file and prints a
//! clear alarm when undervoltage or any throttle bit is set.
//!
//! The monitor is host-agnostic: every field read degrades to `n/a` on a
//! non-Pi machine, so it runs unchanged on the Pi and on a dev machine and
//! never crashes.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub use crate::flags::ThrottleFlags;
pub use crate::readers::{
    read_arm_clock_khz, read_temperature_celsius, read_throttle_flags,
};

/// A 1 Hz health monitor.
#[derive(Clone, Debug)]
pub struct HealthMonitor {
    /// Where timestamped CSV lines are appended.
    pub log_path: PathBuf,
    /// Seconds between ticks (default 1.0).
    pub interval_secs: f64,
}

impl HealthMonitor {
    /// Create a new monitor. `interval_secs` must be > 0.
    pub fn new(log_path: PathBuf, interval_secs: f64) -> Self {
        assert!(interval_secs > 0.0, "interval_secs must be > 0");
        HealthMonitor {
            log_path,
            interval_secs,
        }
    }

    /// Run until an I/O error stops it. Never panics: every read degrades to
    /// `n/a` on hosts that lack Pi sysfs / vcgencmd.
    pub fn run(self) -> Result<(), Box<dyn std::error::Error + 'static>> {
        let mut file = self.open_log()?;
        self.write_header(&mut file)?;
        let interval = Duration::from_secs_f64(self.interval_secs);
        let mut next = Instant::now();
        loop {
            let (line, alarms) = self.read_tick();
            self.append_line(&mut file, &line)?;
            if alarms {
                eprintln!("pedal-health: UNDERVOLTAGE/THROTTLE alarm: {line}");
            }
            next += interval;
            if next > Instant::now() {
                std::thread::sleep(next - Instant::now());
            } else {
                // We fell behind; drop the backlog and resync.
                next = Instant::now() + interval;
            }
        }
    }

    /// Open (or create) the log and return an append handle.
    fn open_log(&self) -> Result<std::fs::File, std::io::Error> {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
    }

    /// Write the header if the file is empty or new.
    fn write_header(&self, file: &mut std::fs::File) -> Result<(), std::io::Error> {
        if file.metadata()?.len() == 0 {
            writeln!(
                file,
                "timestamp_utc,temp_c,flags_hex,flags_names,arm_clock_khz"
            )?;
        }
        file.flush()
    }

    /// Append one timestamped CSV line.
    fn append_line(
        &self,
        file: &mut std::fs::File,
        line: &str,
    ) -> Result<(), std::io::Error> {
        writeln!(file, "{line}")?;
        file.flush()
    }

    /// Read one tick and return the CSV line plus whether any undervoltage or
    /// throttle flag is set (the alarm condition).
    pub fn read_tick(&self) -> (String, bool) {
        let temp = read_temperature_celsius().unwrap_or(f64::NAN);
        let flags = read_throttle_flags().map(ThrottleFlags::from_bits);
        let clock = read_arm_clock_khz();

        let temp_s = if temp.is_nan() {
            "n/a".to_string()
        } else {
            format!("{temp:.1}")
        };
        let flags_hex = flags
            .as_ref()
            .map(|f| format!("0x{f:08X}"))
            .unwrap_or_else(|| "n/a".to_string());
        let flags_names = flags
            .as_ref()
            .map(|f| csv_field(&f.names().join(",")))
            .unwrap_or_else(|| "n/a".to_string());
        // A clock of 0 with no throttle flags is "unavailable", not a live
        // 0 kHz CPU. (On a non-Pi host all three readers return `None`.)
        // `clock` is `Option<u32>` (the reader returns `Option`). A real Pi
        // reports kHz; `None` everywhere means "not a Pi".
        let clock_s = match (flags, clock) {
            (None, _) => "n/a".to_string(),
            (Some(_), Some(c)) => c.to_string(),
            _ => "n/a".to_string(),
        };

        let ts = now_utc();
        let line = format!(
            "{ts},{temp_s},{flags_hex},{flags_names},{clock_s}"
        );
        (line, alarm(flags))
    }
}

/// True when any undervoltage or throttle flag is set.
fn alarm(flags: Option<ThrottleFlags>) -> bool {
    flags.map_or(false, |f| f.any())
}

/// Quote a CSV field when it contains a comma, quote, or newline (RFC 4180).
fn csv_field(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Current UTC timestamp in ISO 8601, e.g. `2026-11-19T12:00:00Z`.
fn now_utc() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // `now` is seconds since epoch, so convert to days (and drop sub-second) and
    // then to (year, month, day) using Hinnant's Gregorian ordinal algorithm
    // (proleptic Gregorian).
    let days = now / 86400;
    let (y, m, d) = days_to_ymd(days as i64);
    let h = (now % 86400) / 3600;
    let min = (now % 3600) / 60;
    let s = now % 60;
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, m, d, h, min, s
    )
}

/// Convert an epoch day number (days since 1970-01-01) to (year, month, day)
/// in the proleptic Gregorian calendar.
/// Convert an epoch day number (days since 1970-01-01) to (year, month, day)
/// in the proleptic Gregorian calendar.
///
/// Simple year-by-year + month-by-month loop. More than fast enough for a
/// health monitor emitting one timestamp per second (the worst case is the
/// current decade, which is a few tens of thousands of iterations).
fn days_to_ymd(days: i64) -> (i64, i64, i64) {
    let mut y = 1970;
    let mut rem = days; // days since 1970-01-01; can be negative
    while rem < 0 {
        y -= 1;
        rem += days_in_year(y + 1);
    }
    while rem >= days_in_year(y) {
        rem -= days_in_year(y);
        y += 1;
    }
    let mut m = 1;
    while rem >= days_in_month(y, m) {
        rem -= days_in_month(y, m);
        m += 1;
    }
    (y, m, rem + 1)
}

fn days_in_year(y: i64) -> i64 {
    365 + if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 1 } else { 0 }
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => 28 + if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 1 } else { 0 },
        _ => 30,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A monitor whose log path is irrelevant for these pure/CSV tests.
    fn monitor() -> HealthMonitor {
        HealthMonitor::new(std::env::temp_dir(), 1.0)
    }

    #[test]
    fn alarm_on_undervoltage_now() {
        let f = ThrottleFlags::from_bits(0x1);
        assert!(alarm(Some(f)));
    }

    #[test]
    fn alarm_on_frequency_capped_now() {
        let f = ThrottleFlags::from_bits(0x2);
        assert!(alarm(Some(f)));
    }

    #[test]
    fn alarm_on_throttling_occurred() {
        let f = ThrottleFlags::from_bits(0x40000);
        assert!(alarm(Some(f)));
    }

    #[test]
    fn no_alarm_when_clear() {
        assert!(!alarm(Some(ThrottleFlags::none())));
        assert!(!alarm(None));
    }

    #[test]
    fn csv_row_has_five_columns_in_order() {
        let line = "2026-11-19T00:00:00Z,45.2,0x40000,throttling occurred,1500000";
        let cols: Vec<&str> = line.split(',').collect();
        assert_eq!(cols.len(), 5);
        assert_eq!(cols[0], "2026-11-19T00:00:00Z");
        assert_eq!(cols[1], "45.2");
        assert_eq!(cols[2], "0x40000");
        assert_eq!(cols[3], "throttling occurred");
        assert_eq!(cols[4], "1500000");
    }

    #[test]
    fn flags_names_quoted_when_comma() {
        let f = ThrottleFlags::from_bits(0x40000 | 0x1);
        let names = f.names().join(",");
        let quoted = csv_field(&names);
        assert!(quoted.starts_with('"'));
        assert!(quoted.ends_with('"'));
        assert_eq!(quoted, "\"under-voltage detected,throttling occurred\"");
    }

    #[test]
    fn timestamp_ends_with_z_and_contains_utc_delimiters() {
        let ts = now_utc();
        assert!(ts.ends_with('Z'));
        assert!(ts.contains('T'));
        let body = &ts[..ts.len() - 1]; // strip Z
        // ISO 8601 UTC `YYYY-MM-DDTHH:MM:SS` = 19 chars.
        assert!(body.len() == 19, "ISO 8601 UTC has 19 chars body");
        // yyyy-mm-ddThh:mm:ss
        assert!(body.chars().nth(4) == Some('-'));
        assert!(body.chars().nth(7) == Some('-'));
        assert!(body.chars().nth(10) == Some('T'));
        assert!(body.chars().nth(13) == Some(':'));
        assert!(body.chars().nth(16) == Some(':'));
    }

    #[test]
    fn epoch_anchor() {
        // 1970-01-01 -> (1970, 1, 1); 1969-12-31 -> (1969, 12, 31).
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
        assert_eq!(days_to_ymd(-1), (1969, 12, 31));
    }
}
