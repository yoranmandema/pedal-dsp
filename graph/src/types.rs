//! Types shared by the graph, the blocks, and the host simulator.
//!
//! Everything here is platform-independent: no ALSA, no CoreAudio, no I/O.
//! The same types compile to macOS, Linux, and the Pi.

use std::fmt;

/// A stable parameter identifier.
pub type ParamId = &'static str;

/// Units a parameter can be expressed in. Fixed set (decision #5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Decibels,
    Hertz,
    Milliseconds,
    Percent,
    Semitones,
    Cents,
    Ratio,
    Linear,
    Seconds,
    /// A discrete choice. The real value is the option index (`0..len`),
    /// `min = 0`, `max = len - 1`, curve `Linear`. Blocks round it.
    Choice(&'static [&'static str]),
}

impl fmt::Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Unit::Decibels => "dB",
            Unit::Hertz => "Hz",
            Unit::Milliseconds => "ms",
            Unit::Percent => "%",
            Unit::Semitones => "st",
            Unit::Cents => "cents",
            Unit::Ratio => "x",
            Unit::Seconds => "s",
            Unit::Linear | Unit::Choice(_) => "",
        };
        write!(f, "{s}")
    }
}

/// How a parameter's 0..1 normalised value maps to its real value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Curve {
    /// Linear across the range.
    Linear,
    /// Logarithmic — for frequencies and other perceptual quantities.
    Log,
    /// Gain in dB — linear in dB, not in linear gain.
    AmpGain,
}

/// A parameter declaration. The UI, the MIDI mapper, and the serialiser all
/// consume this one description.
#[derive(Clone, Debug)]
pub struct ParamSpec {
    pub id: ParamId,
    pub name: &'static str,
    pub unit: Unit,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub curve: Curve,
}

impl ParamSpec {
    /// The option labels, if this is a choice parameter.
    pub fn choices(&self) -> Option<&'static [&'static str]> {
        match self.unit {
            Unit::Choice(c) => Some(c),
            _ => None,
        }
    }

    /// Map a normalised 0..1 value to the parameter's real value.
    pub fn denormalise(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self.curve {
            Curve::Linear => self.min + (self.max - self.min) * t,
            Curve::Log => {
                let lo = self.min.max(1e-9);
                let hi = self.max.max(lo * 1.000_001);
                lo * (hi / lo).powf(t)
            }
            // AmpGain: the range is already in dB, so it is linear in dB.
            Curve::AmpGain => self.min + (self.max - self.min) * t,
        }
    }

    /// Map a real value back to normalised 0..1.
    pub fn normalise(&self, v: f32) -> f32 {
        match self.curve {
            Curve::Linear | Curve::AmpGain => {
                let span = self.max - self.min;
                if span.abs() < 1e-12 {
                    0.0
                } else {
                    ((v - self.min) / span).clamp(0.0, 1.0)
                }
            }
            Curve::Log => {
                let lo = self.min.max(1e-9);
                let hi = self.max.max(lo * 1.000_001);
                ((v.max(lo) / lo).ln() / (hi / lo).ln()).clamp(0.0, 1.0)
            }
        }
    }
}

/// The port kinds a node can expose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortKind {
    /// PCM audio.
    Audio,
    /// A continuous control value, sampled once per block.
    Control,
    /// A boolean state, once per block.
    Gate,
}

#[derive(Clone, Debug)]
pub struct PortSpec {
    pub name: &'static str,
    pub kind: PortKind,
}

impl PortSpec {
    pub const fn audio(name: &'static str) -> Self {
        Self { name, kind: PortKind::Audio }
    }
    pub const fn control(name: &'static str) -> Self {
        Self { name, kind: PortKind::Control }
    }
    pub const fn gate(name: &'static str) -> Self {
        Self { name, kind: PortKind::Gate }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Io,
    Split,
    Dynamics,
    Tone,
    Distortion,
    Modulation,
    Pitch,
    Time,
    Modelling,
    Utility,
    Control,
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Category::Io => "I/O",
            Category::Split => "Split/Merge",
            Category::Dynamics => "Dynamics",
            Category::Tone => "Tone",
            Category::Distortion => "Distortion",
            Category::Modulation => "Modulation",
            Category::Pitch => "Pitch",
            Category::Time => "Time",
            Category::Modelling => "Modelling",
            Category::Utility => "Utility",
            Category::Control => "Control",
        };
        write!(f, "{s}")
    }
}

/// A block's static description. Built once; never touched in the audio path.
#[derive(Clone, Debug)]
pub struct BlockSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub category: Category,
    pub inputs: Vec<PortSpec>,
    pub outputs: Vec<PortSpec>,
    pub params: Vec<ParamSpec>,
    pub controls_in: Vec<PortSpec>,
    pub controls_out: Vec<PortSpec>,
}

impl BlockSpec {
    /// Look up a parameter's index by id.
    pub fn param_index(&self, id: &str) -> Option<usize> {
        self.params.iter().position(|p| p.id == id)
    }
}

/// Errors a block can raise. Blocks must not panic in `process`.
#[derive(Clone, Debug, PartialEq)]
pub enum BlockError {
    /// A parameter was missing or out of range.
    Param(&'static str),
    /// An audio buffer had the wrong length.
    BufferLength { expected: usize, got: usize },
    /// A resource (IR file, model) could not be loaded.
    Resource(String),
    /// `process` was called before `prepare`. Carries no `String`, so it can
    /// be constructed in the audio path without allocating.
    NotPrepared(&'static str),
    /// Anything else, with a message. Allocates — never build one in
    /// `process` (use `ok_or_else`, or a non-allocating variant).
    Other(String),
}

impl fmt::Display for BlockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BlockError::Param(id) => write!(f, "bad parameter: {id}"),
            BlockError::NotPrepared(id) => write!(f, "{id}: process() called before prepare()"),
            BlockError::BufferLength { expected, got } => {
                write!(f, "buffer length mismatch: expected {expected}, got {got}")
            }
            BlockError::Resource(s) => write!(f, "resource error: {s}"),
            BlockError::Other(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for BlockError {}
