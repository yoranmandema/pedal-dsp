//! Graceful readers for the Pi's health sources.
//!
//! Every function returns `Option`: `Some(v)` when a value was obtained,
//! `None` when the host lacks the Pi's sysfs nodes or the `vcgencmd` tool
//! (i.e. a non-Pi dev machine). None of the readers panics, so a monitor that
//! uses them degrades gracefully to `n/a` rather than crashing.

use std::ffi::OsStr;
use std::fs;

/// SoC temperature in degrees Celsius (one decimal).
///
/// Sources, in priority order:
///   1. `/sys/class/thermal/thermal_zone0/temp` — first line, millidegrees C.
///   2. `vcgencmd measure_temp` — parses `temp=45.2'C`.
pub fn read_temperature_celsius() -> Option<f64> {
    if let Some(v) = read_i64_file("/sys/class/thermal/thermal_zone0/temp") {
        return Some(v as f64 / 1000.0);
    }
    let text = run_vcgencmd(["measure_temp"])?;
    for line in text.lines() {
        if let Some(eq) = line.find('=') {
            let key = &line[..eq];
            if key.trim().eq_ignore_ascii_case("temp") {
                if let Some(degrees) = line.split('\'').next() {
                    if let Ok(v) = degrees.trim().parse::<f64>() {
                        return Some(v);
                    }
                }
            }
        }
    }
    None
}

/// Raw `get_throttled` flags.
///
/// `vcgencmd get_throttled` prints `throttled=00000002` (hex). The flags are
/// decoded by [`ThrottleFlags::from_bits`].
pub fn read_throttle_flags() -> Option<u32> {
    let text = run_vcgencmd(["get_throttled"])?;
    for line in text.lines() {
        if let Some(_eq) = line.find('=') {
            let mut parts = line.splitn(2, '=');
            let _key = parts.next();
            let hex = parts.next()?;
            if let Ok(v) = u32::from_str_radix(hex.trim(), 16) {
                return Some(v);
            }
        }
    }
    None
}

/// Current ARM clock frequency in kHz.
///
/// Sources, in priority order:
///   1. `/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq` — kHz.
///   2. `vcgencmd get_clocks` — `armfreq=1500000` (also kHz).
pub fn read_arm_clock_khz() -> Option<u32> {
    if let Some(v) = read_u32_file(
        "/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq",
    ) {
        return Some(v);
    }
    let text = run_vcgencmd(["get_clocks"])?;
    for line in text.lines() {
        if let Some(_eq) = line.find('=') {
            let mut parts = line.splitn(2, '=');
            let key = parts.next();
            let value = parts.next()?;
            if key.map(|k| k.trim().eq_ignore_ascii_case("armfreq")).unwrap_or(false) {
                if let Ok(v) = value.trim().parse::<u32>() {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// Read a plain integer file (e.g. a sysfs node) and return its value as `T`.
fn read_u32_file(path: &str) -> Option<u32> {
    let s = fs::read_to_string(path).ok()?;
    s.trim().parse::<u32>().ok()
}

fn read_i64_file(path: &str) -> Option<i64> {
    let s = fs::read_to_string(path).ok()?;
    s.trim().parse::<i64>().ok()
}

/// Run `vcgencmd <args>` and return its combined stdout+stderr as text.
///
/// `vcgencmd`'s data goes to stderr while the exit status lands on stdout on
/// some platforms, so we read both. Returns `None` when the binary is missing
/// or the command failed.
fn run_vcgencmd<I, S>(args: I) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr> + Clone,
{
    let output = std::process::Command::new("vcgencmd")
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mut text = String::new();
    if !output.stdout.is_empty() {
        text.push_str(&String::from_utf8_lossy(&output.stdout));
    }
    if !output.stderr.is_empty() {
        text.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    Some(text.trim().to_string())
}
