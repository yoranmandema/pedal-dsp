//! Decoding of the `vcgencmd get_throttled` bit pattern into readable names.
//!
//! These are the bits documented for the Raspberry Pi's `vcgencmd get_throttled`
//! command (Raspberry Pi OS `vcgencmd` reference):
//!
//! | Bit | Value | Meaning (now) | Meaning (since last reboot) |
//! |-----|-------|---------------|-----------------------------|
//! | 0   | 0x1   | Under-voltage detected        | —                           |
//! | 1   | 0x2   | Arm frequency capped          | —                           |
//! | 2   | 0x4   | Currently throttled           | —                           |
//! | 3   | 0x8   | Soft temperature limit active | —                           |
//! | 16  | 0x10000 | —                           | Under-voltage has occurred  |
//! | 17  | 0x20000 | —                           | Arm frequency capping occurred |
//! | 18  | 0x40000 | —                           | Throttling has occurred     |
//! | 19  | 0x80000 | —                           | Soft temperature limit occurred |
//!
//! A zero value means no flag is set.
//!
//! The engine and the monitor do not hard-depend on this crate, so we keep the
//! decoding dependency-free: a plain struct with the named fields and a
//! `from_bits` constructor.

/// The decoded `vcgencmd get_throttled` bit pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThrottleFlags {
    /// Raw bit pattern (kept for `UpperHex` / `Display` and round-trips).
    bits: u32,
    /// bit 0 (0x1) — undervoltage detected right now.
    pub under_voltage: bool,
    /// bit 1 (0x2) — arm frequency capped right now ("frequency capped").
    pub arm_frequency_capped: bool,
    /// bit 2 (0x4) — currently throttled right now.
    pub throttled: bool,
    /// bit 3 (0x8) — soft temperature limit active right now.
    pub temp_limit_active: bool,
    /// bit 16 (0x10000) — undervoltage has occurred since last reboot.
    pub under_voltage_occurred: bool,
    /// bit 17 (0x20000) — arm frequency capping has occurred.
    pub arm_frequency_cap_occurred: bool,
    /// bit 18 (0x40000) — throttling has occurred.
    pub throttling_occurred: bool,
    /// bit 19 (0x80000) — soft temperature limit has occurred.
    pub temp_limit_occurred: bool,
}

impl ThrottleFlags {
    /// All flags clear.
    pub const fn none() -> Self {
        ThrottleFlags {
            bits: 0,
            under_voltage: false,
            arm_frequency_capped: false,
            throttled: false,
            temp_limit_active: false,
            under_voltage_occurred: false,
            arm_frequency_cap_occurred: false,
            throttling_occurred: false,
            temp_limit_occurred: false,
        }
    }

    /// Decode a raw `get_throttled` value into named flags.
    pub const fn from_bits(bits: u32) -> Self {
        ThrottleFlags {
            bits,
            under_voltage: bits & 0x1 != 0,
            arm_frequency_capped: bits & 0x2 != 0,
            throttled: bits & 0x4 != 0,
            temp_limit_active: bits & 0x8 != 0,
            under_voltage_occurred: bits & 0x10000 != 0,
            arm_frequency_cap_occurred: bits & 0x20000 != 0,
            throttling_occurred: bits & 0x40000 != 0,
            temp_limit_occurred: bits & 0x80000 != 0,
        }
    }

    /// Decode from a hex string, e.g. `"00000002"` (from `vcgencmd get_throttled`).
    pub fn from_hex(s: &str) -> Option<Self> {
        let cleaned = s.trim().trim_start_matches("0x").trim_start_matches("0X");
        if cleaned.is_empty() || cleaned == "0" {
            return Some(Self::none());
        }
        let v = u32::from_str_radix(cleaned, 16).ok()?;
        Some(Self::from_bits(v))
    }

    /// True when any "now" or "occurred" flag is set.
    pub fn any(&self) -> bool {
        self.under_voltage
            || self.arm_frequency_capped
            || self.throttled
            || self.temp_limit_active
            || self.under_voltage_occurred
            || self.arm_frequency_cap_occurred
            || self.throttling_occurred
            || self.temp_limit_occurred
    }

    /// True when no flag is set at all.
    pub fn is_clear(&self) -> bool {
        !self.any()
    }

    /// The readable names of every set flag, in bit order.
    pub fn names(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if self.under_voltage {
            names.push("under-voltage detected");
        }
        if self.arm_frequency_capped {
            names.push("arm frequency capped");
        }
        if self.throttled {
            names.push("throttled");
        }
        if self.temp_limit_active {
            names.push("soft temperature limit active");
        }
        if self.under_voltage_occurred {
            names.push("under-voltage occurred");
        }
        if self.arm_frequency_cap_occurred {
            names.push("arm frequency capping occurred");
        }
        if self.throttling_occurred {
            names.push("throttling occurred");
        }
        if self.temp_limit_occurred {
            names.push("soft temperature limit occurred");
        }
        names
    }
}

impl std::fmt::UpperHex for ThrottleFlags {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:08X}", self.bits)
    }
}

#[cfg(test)]
mod tests {
    use super::ThrottleFlags;

    #[test]
    fn decodes_no_flags() {
        let f = ThrottleFlags::from_bits(0);
        assert!(f.is_clear());
        assert!(f.names().is_empty());
    }

    #[test]
    fn decodes_only_undervoltage_now() {
        // bit 0 = 0x1: under-voltage detected
        let f = ThrottleFlags::from_bits(0x1);
        assert!(f.under_voltage);
        assert!(!f.throttled);
        assert!(!f.under_voltage_occurred);
        assert!(f.names() == vec!["under-voltage detected"]);
    }

    #[test]
    fn decodes_only_frequency_capped_now() {
        // bit 1 = 0x2: arm frequency capped
        let f = ThrottleFlags::from_bits(0x2);
        assert!(f.arm_frequency_capped);
        assert!(!f.throttled);
        assert!(f.names() == vec!["arm frequency capped"]);
    }

    #[test]
    fn decodes_only_throttled_now() {
        // bit 2 = 0x4
        let f = ThrottleFlags::from_bits(0x4);
        assert!(f.throttled);
        assert!(f.names() == vec!["throttled"]);
    }

    #[test]
    fn decodes_only_undervoltage_occurred_since_boot() {
        // bit 16 = 0x10000
        let f = ThrottleFlags::from_bits(0x10000);
        assert!(f.under_voltage_occurred);
        assert!(!f.under_voltage);
        assert!(f.names() == vec!["under-voltage occurred"]);
    }

    #[test]
    fn decodes_only_throttling_occurred_since_boot() {
        // bit 18 = 0x40000
        let f = ThrottleFlags::from_bits(0x40000);
        assert!(f.throttling_occurred);
        assert!(!f.throttled);
        assert!(f.names() == vec!["throttling occurred"]);
    }

    #[test]
    fn decodes_full_pattern() {
        // every documented bit: 0x1 | 0x2 | 0x4 | 0x8 | 0x10000 | 0x20000 | 0x40000 | 0x80000
        let f = ThrottleFlags::from_bits(0x1 | 0x2 | 0x4 | 0x8 | 0x10000 | 0x20000 | 0x40000 | 0x80000);
        assert!(f.under_voltage);
        assert!(f.arm_frequency_capped);
        assert!(f.throttled);
        assert!(f.temp_limit_active);
        assert!(f.under_voltage_occurred);
        assert!(f.arm_frequency_cap_occurred);
        assert!(f.throttling_occurred);
        assert!(f.temp_limit_occurred);
        assert!(!f.is_clear());
        let names = f.names();
        assert!(names.contains(&"under-voltage detected"));
        assert!(names.contains(&"arm frequency capped"));
        assert!(names.contains(&"throttled"));
        assert!(names.contains(&"soft temperature limit active"));
        assert!(names.contains(&"under-voltage occurred"));
        assert!(names.contains(&"arm frequency capping occurred"));
        assert!(names.contains(&"throttling occurred"));
        assert!(names.contains(&"soft temperature limit occurred"));
        assert_eq!(names.len(), 8);
    }

    #[test]
    fn decodes_from_hex_strings() {
        assert!(ThrottleFlags::from_hex("00000000").unwrap().is_clear());
        assert!(ThrottleFlags::from_hex("1").unwrap().under_voltage);
        assert!(ThrottleFlags::from_hex("0x2").unwrap().arm_frequency_capped);
        assert!(ThrottleFlags::from_hex("0x40000").unwrap().throttling_occurred);
    }

    #[test]
    fn from_hex_rejects_invalid() {
        assert!(ThrottleFlags::from_hex("zz").is_none());
        assert!(ThrottleFlags::from_hex("100000000").is_none()); // overflows u32
    }
}
