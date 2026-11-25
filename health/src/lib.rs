//! `pedal-health`: health monitoring for the Pedal DSP Pi deployment.
//!
//! - [`flags::ThrottleFlags`] — decode the `vcgencmd get_throttled` bit pattern
//!   into named flags (unit tested).
//! - [`readers`] — gracefully read the SoC temperature, throttle flags and ARM
//!   clock from sysfs / `vcgencmd`, returning `Option` so nothing crashes on a
//!   non-Pi machine.
//! - [`monitor::HealthMonitor`] — a ~1 Hz loop that appends timestamped CSV
//!   lines and prints a clear alarm on undervoltage/throttle.

pub mod flags;
pub mod monitor;
pub mod readers;
