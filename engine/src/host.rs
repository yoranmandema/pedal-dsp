//! Audio device selection and stream construction, shared by the engine
//! process and the egui development app.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, ChannelCount};
use rtrb::{Consumer, Producer};

use crate::audio::{run_callback, AudioThread, ChannelMode, Cmd, FromF32, Meters, Ring, Setup};

/// Mono samples buffered between the input and output callbacks.
///
/// The input callback pushes one mono sample per frame and the output
/// callback drains this ring on every block, so in steady state the level is
/// ~0 and the capacity is only ever a *backlog guard* (if the output thread
/// lags the input, the oldest samples are dropped rather than allowed to
/// pile up). It is therefore capped at ~2 blocks: the previous 8192 frames
/// (~170 ms at 48 kHz) added no audible latency but was a large, needless
/// inter-thread queue that could overflow on a slow output, so it is gone.
/// Capacity is deliberately small so a falling output can exhaust the ring
/// and starve (silence) instead of buffering forever.
pub const RING_CAPACITY: usize = 128;
/// Initial buffer level: samples discarded from the front of the ring at
/// stream start so the very first output blocks are not silence. A few
/// blocks of lead is enough — the ~1024 (21 ms) default was far larger than
/// any device needed, added audible boot lag, and made startup latency
/// predictable. 128 frames (~2.7 ms at 48 kHz) is kept here so the UI ring
/// level reflects reality.
pub const LATENCY_FRAMES: usize = 128;
/// The tuner ring is a separate buffer (input -> tuner_thread) and is NOT on
/// the input->output audio signal path, so it is not part of the 1-2 block
/// budget. It must be at least one YIN window (the tuner only drains every
/// 20 ms) to avoid overwriting the window while it drains.
pub const TUNER_RING_CAPACITY: usize = 2048;
/// Block size the executor runs in.
pub const BLOCK: usize = 64;

/// The I/O buffer size we request when latency matters: 128 frames at the
/// default sample rate gives ~2.7 ms one-way and ~5.3 ms round-trip on a 48
/// kHz device. Small enough to sit well under the 20 ms target and large
/// enough to stay stable.
pub const DEFAULT_IO_BUFFER_FRAMES: u32 = 128;

/// Owned I/O streams. The caller owns the streams (no [`std::mem::forget`]);
/// dropping them stops the cpal device.
pub struct Streams {
    pub input: cpal::Stream,
    pub output: cpal::Stream,
}
/// Fallback ladder for the I/O buffer, smallest first. If the device rejects
/// the requested size it reports `UnsupportedConfig`, so we try each larger
/// size below before giving up.
pub const IOBUFFER_FALLBACK: [u32; 3] = [128, 256, 512];

/// Engine I/O configuration: the sample rate and I/O buffer size the device is
/// asked to use, with a low-latency default.
#[derive(Clone, Debug)]
pub struct Config {
    /// Requested I/O buffer size in frames. `DEFAULT_IO_BUFFER_FRAMES` by
    /// default.
    pub buffer_frames: u32,
    /// Requested sample rate in Hz; `None` keeps the device default (48 kHz
    /// when the device reports no preference and no other rate is requested).
    pub sample_rate: Option<u32>,
}

impl Config {
    /// The low-latency default: 128 frames at the device's native rate.
    pub fn low_latency() -> Self {
        Self {
            buffer_frames: DEFAULT_IO_BUFFER_FRAMES,
            sample_rate: None,
        }
    }
}

pub fn device_name(d: &cpal::Device) -> String {
    d.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "unknown".into())
}

pub fn pick_input(host: &cpal::Host) -> (cpal::Device, String) {
    let devices: Vec<_> = host.input_devices().expect("no input devices").collect();
    if devices.is_empty() {
        panic!("no audio input devices");
    }
    for (i, d) in devices.iter().enumerate() {
        eprintln!("  in [{i}] {}", device_name(d));
    }
    let pick = devices
        .iter()
        .position(|d| device_name(d).contains("Scarlett"))
        .or_else(|| devices.iter().position(|d| device_name(d).to_lowercase().contains("usb")))
        .unwrap_or(0);
    let name = device_name(&devices[pick]);
    eprintln!("using input: {name}");
    (devices[pick].clone(), name)
}

pub fn pick_output(host: &cpal::Host, prefer: &str) -> (cpal::Device, String) {
    let devices: Vec<_> = host.output_devices().expect("no output devices").collect();
    if devices.is_empty() {
        panic!("no audio output devices");
    }
    for (i, d) in devices.iter().enumerate() {
        eprintln!("  out [{i}] {}", device_name(d));
    }
    let pick = devices
        .iter()
        .position(|d| device_name(d) == prefer)
        .or_else(|| {
            devices
                .iter()
                .position(|d| device_name(d).contains("MacBook") || device_name(d).contains("Built-in"))
        })
        .unwrap_or(0);
    let name = device_name(&devices[pick]);
    eprintln!("using output: {name}");
    (devices[pick].clone(), name)
}

/// Prefer 48 kHz, then 44.1 kHz, but only if both devices support it.
pub fn pick_rate(in_dev: &cpal::Device, out_dev: &cpal::Device, in_rate: u32, out_rate: u32) -> u32 {
    for r in [48_000, 44_100] {
        let i_ok = in_dev
            .supported_input_configs()
            .map(|mut c| c.any(|c| c.min_sample_rate() <= r && r <= c.max_sample_rate()))
            .unwrap_or(false);
        let o_ok = out_dev
            .supported_output_configs()
            .map(|mut c| c.any(|c| c.min_sample_rate() <= r && r <= c.max_sample_rate()))
            .unwrap_or(false);
        if i_ok && o_ok {
            return r;
        }
    }
    if in_rate == out_rate {
        return in_rate;
    }
    in_rate
}

/// The `cpal` sample conversions, as a trait so the stream builders can be
/// generic over the device's sample format.
pub trait ToF32: cpal::SizedSample {
    fn to_f32(&self) -> f32;
}

impl ToF32 for f32 {
    fn to_f32(&self) -> f32 {
        *self
    }
}
impl ToF32 for i16 {
    fn to_f32(&self) -> f32 {
        *self as f32 / 32768.0
    }
}
impl ToF32 for i32 {
    fn to_f32(&self) -> f32 {
        *self as f32 / 2_147_483_648.0
    }
}
impl ToF32 for u16 {
    fn to_f32(&self) -> f32 {
        (*self as f32 / 32768.0) - 1.0
    }
}

pub fn build_input<T: ToF32 + 'static>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    ring: Arc<Ring>,
    tuner: Arc<Ring>,
    meters: Arc<Meters>,
) -> Result<cpal::Stream, cpal::Error> {
    device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            let ch = channels.max(1);
            let mut peak = 0.0f32;
            for frame in data.chunks(ch) {
                let mut s = 0.0f32;
                for v in frame.iter().take(2.min(ch)) {
                    s += v.to_f32();
                }
                peak = peak.max(s.abs());
                ring.push(s);
                tuner.push(s);
            }
            meters.in_peak.fetch_max(peak.to_bits(), Ordering::Relaxed);
        },
        |_err| {{
        }},
        Some(Duration::from_millis(500)),
    )
}

#[allow(clippy::too_many_arguments)]
pub fn build_output<T: FromF32 + cpal::SizedSample + 'static>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    ring: Arc<Ring>,
    cmd_rx: Consumer<Cmd>,
    trash_tx: Producer<dsp_graph::Executor>,
    running: Arc<std::sync::atomic::AtomicBool>,
    meters: Arc<Meters>,
    source: Arc<AtomicU8>,
) -> Result<cpal::Stream, cpal::Error> {
    let setup = Setup {
        sample_rate: config.sample_rate as f32,
        out_channels: channels,
        block: BLOCK,
        channel_mode: ChannelMode::First(2),
        latency_frames: LATENCY_FRAMES,
    };
    let mut engine = AudioThread::new(setup, meters.clone(), cmd_rx, trash_tx, running, source);
    engine.set_block(BLOCK);
    device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            run_callback(&mut engine, &ring, &meters, data, channels);
        },
        |_err| {{
        }},
        Some(Duration::from_millis(500)),
    )
}

/// Everything the stream builders need, picked once so the executor and the
/// audio thread agree on the sample rate.
pub struct Devices {
    pub in_dev: cpal::Device,
    pub out_dev: cpal::Device,
    pub in_cfg: cpal::StreamConfig,
    pub out_cfg: cpal::StreamConfig,
    pub in_format: cpal::SampleFormat,
    pub out_format: cpal::SampleFormat,
    pub rate: u32,
    pub in_channels: usize,
    pub out_channels: usize,
    pub in_name: String,
    pub out_name: String,
}

/// Pick the input and output devices and a sample rate both support. Buffer
/// sizes come from `config` when it is `Some`, otherwise the device defaults
/// are used.
pub fn pick(config: Option<&Config>) -> Devices {
    let host = cpal::default_host();
    let (in_dev, in_name) = pick_input(&host);
    let (out_dev, out_name) = pick_output(&host, &in_name);
    let in_default = in_dev.default_input_config().expect("no input config");
    let out_default = out_dev.default_output_config().expect("no output config");
    let rate = pick_rate(&in_dev, &out_dev, in_default.sample_rate(), out_default.sample_rate());
    let out_channels = out_default.channels().max(1) as usize;
    let in_channels = in_default.channels().max(1) as usize;
    // The sample format lives on the supported config, not on StreamConfig.
    let in_format = in_default.sample_format();
    let out_format = out_default.sample_format();

    let mut in_cfg: cpal::StreamConfig = in_default.into();
    if let Some(c) = config {
        in_cfg.sample_rate = c.sample_rate.unwrap_or(rate);
        in_cfg.buffer_size = BufferSize::Fixed(c.buffer_frames);
    } else {
        in_cfg.sample_rate = rate;
    }
    let mut out_cfg: cpal::StreamConfig = out_default.into();
    if let Some(c) = config {
        out_cfg.sample_rate = c.sample_rate.unwrap_or(rate);
        out_cfg.buffer_size = BufferSize::Fixed(c.buffer_frames);
    } else {
        out_cfg.sample_rate = rate;
    }

    Devices {
        in_dev,
        out_dev,
        in_cfg,
        out_cfg,
        in_format,
        out_format,
        rate,
        in_channels,
        out_channels,
        in_name,
        out_name,
    }
}

/// Resolve which I/O buffer size to use.
///
/// Tries each size in `ladder` in order with `build`. On the first `Ok`, the
/// size (in frames) together with the built value is returned. On the last
/// size, if `build` failed with `UnsupportedConfig`, the whole thing is
/// returned as an error.
fn resolve_buffer_size<R, F>(ladder: &[u32], mut build: F) -> Result<(R, u32), cpal::Error>
where
    F: FnMut(u32) -> Result<R, cpal::Error>,
{
    for &frames in ladder {
        match build(frames) {
            Ok(r) => return Ok((r, frames)),
            Err(e) => {
                if e.kind() == cpal::ErrorKind::UnsupportedConfig {
                    continue;
                }
                return Err(e);
            }
        }
    }
    Err(cpal::Error::new(cpal::ErrorKind::UnsupportedConfig))
}

/// Probe whether the device accepts a stream configuration, using a no-op data
/// callback. The no-op closure captures nothing, so a rejected size frees
/// nothing and the caller's shared channels (`cmd_rx`/`trash_tx`) are left
/// intact for the next attempt.
/// Probe whether the device accepts a stream configuration (a given buffer
/// size at a given sample rate and format), using a no-op data callback. The
/// no-op closure captures nothing, so a rejected size frees nothing and the
/// caller's shared channels (`cmd_rx`/`trash_tx`) are left intact for the next
/// attempt. Returns `true` when the config is accepted.
fn probe_output(device: &cpal::Device, rate: u32, frames: u32, format: cpal::SampleFormat) -> bool {
    let config = cpal::StreamConfig {
        channels: 1,
        sample_rate: rate,
        buffer_size: BufferSize::Fixed(frames),
    };
    match format {
        cpal::SampleFormat::F32 => device
            .build_output_stream(
                config,
                move |_: &mut [f32], _: &cpal::OutputCallbackInfo| {},
                |e| eprintln!("pedal-engine: output probe rejected ({e})"),
                None,
            )
            .is_ok(),
        cpal::SampleFormat::I16 => device
            .build_output_stream(
                config,
                move |_: &mut [i16], _: &cpal::OutputCallbackInfo| {},
                |e| eprintln!("pedal-engine: output probe rejected ({e})"),
                None,
            )
            .is_ok(),
        cpal::SampleFormat::I32 => device
            .build_output_stream(
                config,
                move |_: &mut [i32], _: &cpal::OutputCallbackInfo| {},
                |e| eprintln!("pedal-engine: output probe rejected ({e})"),
                None,
            )
            .is_ok(),
        cpal::SampleFormat::U16 => device
            .build_output_stream(
                config,
                move |_: &mut [u16], _: &cpal::OutputCallbackInfo| {},
                |e| eprintln!("pedal-engine: output probe rejected ({e})"),
                None,
            )
            .is_ok(),
        _ => false,
    }
}
pub fn one_way_latency_ms(frames: u32, sample_rate: u32) -> f32 {
    frames as f32 / sample_rate as f32 * 1000.0
}

/// Round-trip I/O latency in milliseconds: one input buffer plus one output
/// buffer of `frames` at `sample_rate` Hz.
pub fn round_trip_latency_ms(frames: u32, sample_rate: u32) -> f32 {
    2.0 * one_way_latency_ms(frames, sample_rate)
}

/// Start both streams with low-latency buffer sizes.
///
/// The requested I/O buffer size comes from `config`. If the device rejects
/// the requested size it falls back to each larger size in the ladder and
/// reports the result. Once both streams are live, the effective buffer size
/// and the computed one-way and round-trip I/O latency are logged.
#[allow(clippy::too_many_arguments)]
pub fn start(
    d: &Devices,
    config: &Config,
    cmd_rx: Consumer<Cmd>,
    trash_tx: Producer<dsp_graph::Executor>,
    running: Arc<std::sync::atomic::AtomicBool>,
    meters: Arc<Meters>,
    source: Arc<AtomicU8>,
) -> Result<(Streams, Arc<Ring>), cpal::Error> {
    let in_channels = d.in_channels;
    let out_channels = d.out_channels;
    let ring = Arc::new(Ring::new(RING_CAPACITY));
    let ring_in = ring.clone();
    // The tuner reads its own copy of the input stream (see tuner.rs); the
    // audio ring above is consumed by the output callback. Returns the tuner
    // ring so main can spawn the detached tuner thread.
    let tuner_ring = Arc::new(Ring::new(TUNER_RING_CAPACITY));
    let tuner_in = tuner_ring.clone();

    // The requested I/O buffer size comes from the engine config (the low-latency
    // default is 128 frames). The fallback ladder then tries progressively larger
    // sizes, so the device's rejection of the requested size is logged and the
    // next larger one is used instead.
    let requested_frames = config.buffer_frames;
    let ladder = [requested_frames, requested_frames * 2, requested_frames * 4];

    // Input stream: smallest buffer first, larger fallback on rejection. The
    // input callback captures only a fresh `ring`/`meters` clone and `d.in_dev`
    // (a reference), so a rejected size frees nothing the next attempt needs.
    let (in_stream, in_frames) = resolve_buffer_size(&ladder, |frames| {
        let cfg = cpal::StreamConfig {
            channels: in_channels as ChannelCount,
            sample_rate: d.rate,
            buffer_size: BufferSize::Fixed(frames),
        };
        build_input::<f32>(&d.in_dev, cfg, in_channels, ring_in.clone(), tuner_in.clone(), meters.clone())
    })?;
    in_stream.play().expect("failed to start input");
    // The caller owns the streams; dropping them stops the cpal device. No
    // `std::mem::forget` here.

    let ring_out = ring.clone();

    // Output stream: the real build captures `cmd_rx`/`trash_tx` (via the audio
    // thread) the moment it is constructed, so a rejected size frees the shared
    // channels and the next attempt would no longer see them. We therefore
    // validate each size with a no-op probe closure (which captures nothing) and
    // build the real stream once with the accepted config.
    let out_cfg = resolve_output_config(requested_frames, &d.out_dev, d.rate, out_channels, d.out_format)?;
    let out_stream = match d.out_format {
        cpal::SampleFormat::F32 => build_output_with_config::<f32>(
            &d.out_dev, out_cfg, out_channels, ring_out.clone(), cmd_rx, trash_tx, running.clone(), meters.clone(), source.clone(),
        ),
        cpal::SampleFormat::I16 => build_output_with_config::<i16>(
            &d.out_dev, out_cfg, out_channels, ring_out.clone(), cmd_rx, trash_tx, running.clone(), meters.clone(), source.clone(),
        ),
        cpal::SampleFormat::I32 => build_output_with_config::<i32>(
            &d.out_dev, out_cfg, out_channels, ring_out.clone(), cmd_rx, trash_tx, running.clone(), meters.clone(), source.clone(),
        ),
        cpal::SampleFormat::U16 => build_output_with_config::<u16>(
            &d.out_dev, out_cfg, out_channels, ring_out.clone(), cmd_rx, trash_tx, running.clone(), meters.clone(), source.clone(),
        ),
        other => panic!("unsupported output format {other:?}"),
    }?;
    out_stream.play().expect("failed to start output");
    // The caller owns the streams; dropping them stops the cpal device. No
    // `std::mem::forget` here.
    ring.trim_to(LATENCY_FRAMES);

    // Effective I/O latency: one buffer one-way, both directions round trip.
    let one_way_ms = one_way_latency_ms(in_frames, d.rate);
    let round_trip_ms = round_trip_latency_ms(in_frames, d.rate);

    eprintln!("input : {} @ {} Hz, {in_channels} ch, I/O buffer {in_frames} frames", d.in_name, d.rate);
    eprintln!("output: {} @ {} Hz, {out_channels} ch, I/O buffer {in_frames} frames", d.out_name, d.rate);
    eprintln!(
        "pedal-engine: I/O latency one-way = {one_way_ms:.2} ms, round-trip = {round_trip_ms:.2} ms @ {} Hz",
        d.rate
    );
    Ok((Streams { input: in_stream, output: out_stream }, tuner_ring))
}

/// Validate the output device accepts each low-latency buffer size, using a
/// no-op probe closure, then pick the smallest accepted one. The ladder starts
/// at `requested_frames` and doubles.
fn resolve_output_config(
    requested_frames: u32,
    device: &cpal::Device,
    rate: u32,
    channels: usize,
    format: cpal::SampleFormat,
) -> Result<cpal::StreamConfig, cpal::Error> {
    let ladder = [requested_frames, requested_frames * 2, requested_frames * 4];
    for &frames in &ladder {
        if probe_output(device, rate, frames, format) {
            return Ok(cpal::StreamConfig {
                channels: channels as ChannelCount,
                sample_rate: rate,
                buffer_size: BufferSize::Fixed(frames),
            });
        }
    }
    // The device cannot meet any of our low-latency sizes. Fall back to the
    // device's default output configuration (smallest supported buffer) so the
    // stream still opens, and log that we fell back.
    let default = device.default_output_config().map_err(|e| {
        eprintln!("pedal-engine: could not read output config ({e}); keeping {requested_frames} frames");
        e
    })?;
    let (buf, min_frames) = match *default.buffer_size() {
        cpal::SupportedBufferSize::Range { min, .. } => (BufferSize::Fixed(min), min),
        cpal::SupportedBufferSize::Unknown => (BufferSize::Default, 0),
    };
    let fallback_msg = if min_frames > 0 {
        format!(
            "pedal-engine: I/O buffer {requested_frames} frames unsupported on output; falling back to device default ({min_frames} frames @ {rate} Hz)"
        )
    } else {
        format!(
            "pedal-engine: I/O buffer {requested_frames} frames unsupported on output; falling back to device default unknown size @ {rate} Hz"
        )
    };
    eprintln!("{}", fallback_msg);
    Ok(cpal::StreamConfig {
        channels: channels as ChannelCount,
        sample_rate: rate,
        buffer_size: buf,
    })
}

/// Build the output stream with a pre-validated configuration (produced by
/// `resolve_output_config` or `probe_output`). Because `resolve_output_config`
/// has already proven the config, the audio thread is created exactly once
/// and the caller's `cmd_rx`/`trash_tx` are not consumed by a failed probe.
fn build_output_with_config<T: cpal::SizedSample + 'static + FromF32>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    ring: Arc<Ring>,
    cmd_rx: Consumer<Cmd>,
    trash_tx: Producer<dsp_graph::Executor>,
    running: Arc<std::sync::atomic::AtomicBool>,
    meters: Arc<Meters>,
    source: Arc<AtomicU8>,
) -> Result<cpal::Stream, cpal::Error> {
    let setup = Setup {
        sample_rate: config.sample_rate as f32,
        out_channels: channels,
        block: BLOCK,
        channel_mode: ChannelMode::First(2),
        latency_frames: LATENCY_FRAMES,
    };
    let mut engine = AudioThread::new(setup, meters.clone(), cmd_rx, trash_tx, running, source);
    engine.set_block(BLOCK);
    device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            run_callback(&mut engine, &ring, &meters, data, channels);
        },
        |e| eprintln!("pedal-engine: output callback error: {e}"),
        Some(Duration::from_millis(500)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_low_latency() {
        let c = Config::low_latency();
        assert_eq!(c.buffer_frames, DEFAULT_IO_BUFFER_FRAMES);
        assert_eq!(c.sample_rate, None);
    }

    #[test]
    fn latency_one_way_and_round_trip() {
        // 128 frames @ 48 kHz -> 2.667 ms one-way, 5.333 ms round trip.
        assert!((one_way_latency_ms(128, 48_000) - 2.666_6667).abs() < 1e-4);
        assert!((round_trip_latency_ms(128, 48_000) - 5.333_3333).abs() < 1e-4);
        // 64 frames @ 48 kHz -> 1.333 ms one-way.
        assert!((one_way_latency_ms(64, 48_000) - 1.333_3333).abs() < 1e-4);
    }

    /// A stub builder that accepts only the size it is asked for, otherwise
    /// reports the config as unsupported (the shape cpal returns for a device
    /// that cannot meet the requested buffer).
    fn stub_builder(accept: u32) -> impl FnMut(u32) -> Result<(), cpal::Error> {
        move |frames| -> Result<(), cpal::Error> {
            if frames == accept {
                Ok(())
            } else {
                Err(cpal::Error::new(cpal::ErrorKind::UnsupportedConfig))
            }
        }
    }

    #[test]
    fn fallback_uses_requested_size_when_accepted() {
        let ((), frames) = resolve_buffer_size(&IOBUFFER_FALLBACK, stub_builder(128)).unwrap();
        assert_eq!(frames, 128);
    }

    #[test]
    fn fallback_tries_next_larger_when_requested_is_rejected() {
        // The ladder starts at the requested 64 and doubles (64, 128, 256). The
        // stub accepts only the next step (128), so the requested 64 is
        // rejected and the next larger size is used.
        let ((), frames) = resolve_buffer_size(&[64, 128, 256], stub_builder(128)).unwrap();
        assert_eq!(frames, 128);
    }

    #[test]
    fn fallback_tries_last_larger_when_requested_and_next_are_rejected() {
        // The ladder starts at the requested 64 (64, 128, 256). The stub
        // accepts only the final step (256), so both the requested and the
        // next-larger sizes are rejected.
        let ((), frames) = resolve_buffer_size(&[64, 128, 256], stub_builder(256)).unwrap();
        assert_eq!(frames, 256);
    }

    #[test]
    fn fallback_returns_error_when_every_size_is_rejected() {
        let err = resolve_buffer_size(
            &IOBUFFER_FALLBACK,
            |_| -> Result<(), cpal::Error> {
                Err(cpal::Error::new(cpal::ErrorKind::UnsupportedConfig))
            },
        )
        .unwrap_err();
        assert_eq!(err.kind(), cpal::ErrorKind::UnsupportedConfig);
    }
}
