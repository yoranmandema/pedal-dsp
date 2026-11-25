//! Live host model: the *same* `pedal_dsp::chain::EffectChain` that runs on
//! the native desktop host, driven in real time from a GUI.
//!
//!   * input  = an audio interface (Scarlett 2i2 by default), mono-summed
//!   * output = CoreAudio (built-in speakers by default)
//!   * a single-producer/single-consumer ring joins the two callbacks
//!   * every parameter is an atomic, so the GUI never blocks the audio path
//!
//! Usage: `cargo run --release`

use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use pedal_dsp::chain::EffectChain;
use pedal_dsp::waveshape::ShaperKind;
use pedal_dsp::PitchMode;

const TAU: f32 = std::f32::consts::TAU;
/// Ring capacity in input samples.
const RING_CAPACITY: usize = 8192;
/// Target buffering delay in samples (~23 ms at 44.1 kHz).  Lower feels
/// more immediate but leaves less slack for a late input callback.
const LATENCY_SAMPLES: usize = 1024;

// ---------------------------------------------------------------------------
// Parameters
// ---------------------------------------------------------------------------

pub struct Params {
    pub drive: AtomicU32,
    pub distortion_mix: AtomicU32,
    pub distortion_level: AtomicU32,
    pub tone: AtomicU32,
    pub level: AtomicU32,
    pub delay_ms: AtomicU32,
    pub delay_mix: AtomicU32,
    pub delay_feedback: AtomicU32,
    pub pitch: AtomicU32,
    pub pitch_glide: AtomicU32,
    pub pitch_rate: AtomicU32,
    pub pitch_mode: AtomicU32,
    pub sweep_gate: AtomicBool,
    pub jitter: AtomicU32,
    pub reverb_mix: AtomicU32,
    pub shaper: AtomicU32,
    pub enabled: AtomicBool,
}

pub const SHAPERS: &[(&str, ShaperKind)] = &[
    ("Tanh", ShaperKind::Tanh),
    ("Soft clip", ShaperKind::SoftClip),
    ("Hard clip", ShaperKind::HardClip),
    ("Tube", ShaperKind::Tube),
    ("Fuzz", ShaperKind::Fuzz),
    ("Half rectify", ShaperKind::RectifyHalf),
    ("Full rectify", ShaperKind::RectifyFull),
];

impl Params {
    fn new() -> Self {
        Self {
            drive: AtomicU32::new(3.0f32.to_bits()),
            distortion_mix: AtomicU32::new(1.0f32.to_bits()),
            distortion_level: AtomicU32::new(0.8f32.to_bits()),
            tone: AtomicU32::new(0.0f32.to_bits()),
            level: AtomicU32::new(0.9f32.to_bits()),
            delay_ms: AtomicU32::new(250.0f32.to_bits()),
            delay_mix: AtomicU32::new(0.0f32.to_bits()),
            delay_feedback: AtomicU32::new(0.35f32.to_bits()),
            pitch: AtomicU32::new(0.0f32.to_bits()),
            pitch_glide: AtomicU32::new(0.0f32.to_bits()),
            pitch_rate: AtomicU32::new(0.0f32.to_bits()),
            pitch_mode: AtomicU32::new(0),
            sweep_gate: AtomicBool::new(true),
            jitter: AtomicU32::new(0.0f32.to_bits()),
            reverb_mix: AtomicU32::new(0.0f32.to_bits()),
            shaper: AtomicU32::new(0),
            enabled: AtomicBool::new(true),
        }
    }
}

#[inline]
fn get_f32(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Ordering::Relaxed))
}
#[inline]
fn set_f32(a: &AtomicU32, v: f32) {
    a.store(v.to_bits(), Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Ring buffer: SPSC. The input callback produces, the output callback consumes.
// ---------------------------------------------------------------------------

struct RingBuffer {
    ptr: std::ptr::NonNull<f32>,
    mask: usize,
    write: AtomicUsize,
    read: AtomicUsize,
}

// SPSC by construction: one input callback writes, one output callback reads.
unsafe impl Sync for RingBuffer {}
unsafe impl Send for RingBuffer {}

impl RingBuffer {
    fn new(capacity: usize) -> Self {
        let cap = capacity.next_power_of_two().max(1024);
        let mut v = vec![0.0f32; cap];
        let ptr = NonNull::from(v.as_mut_slice()).cast::<f32>();
        std::mem::forget(v); // lives for the lifetime of the app
        Self {
            ptr,
            mask: cap - 1,
            write: AtomicUsize::new(0),
            read: AtomicUsize::new(0),
        }
    }

    #[inline]
    fn slot(&self, i: usize) -> *mut f32 {
        // `i & mask` is always inside the allocation.
        unsafe { self.ptr.as_ptr().add(i & self.mask) }
    }

    fn push(&self, samples: &[f32]) {
        let mut w = self.write.load(Ordering::Relaxed);
        for &s in samples {
            unsafe {
                *self.slot(w) = s;
            }
            w = w.wrapping_add(1);
        }
        self.write.store(w, Ordering::Release);
    }

    /// Fill `out` with samples; underrun produces silence.
    fn pop(&self, out: &mut [f32]) {
        let mut r = self.read.load(Ordering::Relaxed);
        let w = self.write.load(Ordering::Acquire);
        for o in out.iter_mut() {
            if r != w {
                *o = unsafe { *self.slot(r) };
                r = r.wrapping_add(1);
            } else {
                *o = 0.0;
            }
        }
        self.read.store(r, Ordering::Release);
    }

    fn available(&self) -> usize {
        self.write
            .load(Ordering::Acquire)
            .wrapping_sub(self.read.load(Ordering::Relaxed))
    }

    /// Trim the buffered delay to `samples` by moving the read pointer.
    /// Used once at startup to discard the backlog that accumulates before
    /// the output stream starts pulling.
    fn set_delay(&self, samples: usize) {
        let w = self.write.load(Ordering::Acquire);
        let r = self.read.load(Ordering::Relaxed);
        let level = w.wrapping_sub(r);
        let target = level.min(samples);
        self.read.store(w.wrapping_sub(target), Ordering::Release);
    }
}

/// Live-path diagnostics: which input channels carry signal, callback
/// sizes, and ring underruns. Printed every couple of seconds.
struct Diag {
    ch_rms: Mutex<[f32; 4]>,
    mono_rms: Mutex<f32>,
    out_rms: Mutex<f32>,
    mute: AtomicBool,
    shaper_resets: AtomicUsize,
    underruns: AtomicUsize,
    in_frames: AtomicUsize,
    out_frames: AtomicUsize,
    in_samples: AtomicUsize,
}

impl Diag {
    fn new() -> Self {
        Self {
            ch_rms: Mutex::new([0.0; 4]),
            mono_rms: Mutex::new(0.0),
            out_rms: Mutex::new(0.0),
            mute: AtomicBool::new(false),
            shaper_resets: AtomicUsize::new(0),
            underruns: AtomicUsize::new(0),
            in_frames: AtomicUsize::new(0),
            out_frames: AtomicUsize::new(0),
            in_samples: AtomicUsize::new(0),
        }
    }
}

// ---------------------------------------------------------------------------
// Audio source (for the non-live modes)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum ToneKind {
    Sine,
    Saw,
    Square,
}

enum Source {
    Silence,
    Tone { kind: ToneKind, freq: f32, phase: f32 },
    Wav { samples: Vec<f32>, pos: usize },
}

impl Source {
    fn sample(&mut self, fs: f32) -> f32 {
        match self {
            Source::Silence => 0.0,
            Source::Tone { kind, freq, phase } => {
                let s = match kind {
                    ToneKind::Sine => (TAU * *phase).sin(),
                    ToneKind::Saw => 2.0 * *phase - 1.0,
                    ToneKind::Square => {
                        if *phase < 0.5 {
                            1.0
                        } else {
                            -1.0
                        }
                    }
                };
                *phase = (*phase + *freq / fs).fract();
                s * 0.35
            }
            Source::Wav { samples, pos } => {
                if samples.is_empty() {
                    return 0.0;
                }
                let s = samples[*pos];
                *pos = (*pos + 1) % samples.len();
                s
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Live,
    Wav,
    Tone,
}

// ---------------------------------------------------------------------------
// Shared audio state
// ---------------------------------------------------------------------------

struct AudioState {
    chain: EffectChain,
    source: Source,
    mode: Mode,
    channels: usize,
    fs: f32,
    peak: Arc<Mutex<f32>>,
    ring_level: Arc<AtomicUsize>,
    diag: Arc<Diag>,
    cb_count: u64,
    last_print: std::time::Instant,
    shaper_idx: u32,
}

impl AudioState {
    fn new(
        fs: f32,
        channels: usize,
        params: &Params,
        peak: Arc<Mutex<f32>>,
        ring_level: Arc<AtomicUsize>,
        diag: Arc<Diag>,
    ) -> Self {
        let mut state = Self {
            chain: EffectChain::new(fs),
            source: Source::Silence,
            mode: Mode::Live,
            channels,
            fs,
            peak,
            ring_level,
            diag,
            cb_count: 0,
            last_print: std::time::Instant::now(),
            shaper_idx: 0,
        };
        state.apply_params(params);
        state
    }

    fn apply_params(&mut self, p: &Params) {
        self.chain.drive = get_f32(&p.drive);
        self.chain.distortion_mix = get_f32(&p.distortion_mix);
        self.chain.distortion_level = get_f32(&p.distortion_level);
        self.chain.tone = get_f32(&p.tone);
        self.chain.level = get_f32(&p.level);
        self.chain.delay_ms = get_f32(&p.delay_ms);
        self.chain.delay_mix = get_f32(&p.delay_mix);
        self.chain.delay_feedback = get_f32(&p.delay_feedback);
        self.chain.pitch_cents = get_f32(&p.pitch);
        self.chain.pitch_glide_ms = get_f32(&p.pitch_glide);
        self.chain.pitch_rate = get_f32(&p.pitch_rate);
        self.chain.pitch_mode = match p.pitch_mode.load(Ordering::Relaxed) {
            1 => PitchMode::Sweep,
            2 => PitchMode::Shepard,
            _ => PitchMode::Target,
        };
        self.chain.sweep_gate = p.sweep_gate.load(Ordering::Relaxed);
        self.chain.jitter = get_f32(&p.jitter);
        self.chain.reverb_mix = get_f32(&p.reverb_mix);
        self.chain.enabled = p.enabled.load(Ordering::Relaxed);
        // Only recreate the shaper when the selection actually changes.
        // Shaper::new() resets the oversampling FIRs, so doing this on every
        // callback would silently disable oversampling and the distortion
        // would alias badly.
        let idx = p.shaper.load(Ordering::Relaxed);
        if idx != self.shaper_idx {
            self.shaper_idx = idx;
            self.chain.set_shaper(SHAPERS[idx as usize % SHAPERS.len()].1);
            self.diag.shaper_resets.fetch_add(1, Ordering::Relaxed);
        }
        self.chain.update_params();
    }

    /// Render `out` (interleaved, `channels` wide) from a mono input buffer.
    fn render(&mut self, p: &Params, in_mono: &[f32], out: &mut [f32]) {
        self.apply_params(p);
        let ch = self.channels.max(1);
        let mut peak = 0.0f32;
        let mut sum_sq = 0.0f32;
        let mut count = 0usize;
        for (i, frame) in out.chunks_mut(ch).enumerate() {
            let s = in_mono.get(i).copied().unwrap_or(0.0);
            let (l, r) = self.chain.process_stereo(s, s);
            peak = peak.max(l.abs()).max(r.abs());
            sum_sq += l * l + r * r;
            count += 2;
            if frame.len() >= 2 {
                frame[0] = l;
                frame[1] = r;
            } else {
                frame[0] = (l + r) * 0.5;
            }
        }
        if let Ok(mut d) = self.diag.out_rms.lock() {
            *d = *d * 0.95 + (sum_sq / count.max(1) as f32).sqrt();
        }
        if let Ok(mut pk) = self.peak.lock() {
            *pk = (*pk * 0.9 + peak * 0.1).max(peak);
        }
        self.cb_count += 1;
        if self.last_print.elapsed().as_secs_f32() > 2.0 {
            let rms = self
                .diag
                .ch_rms
                .lock()
                .map(|r| format!("{:.3} {:.3} {:.3} {:.3}", r[0], r[1], r[2], r[3]))
                .unwrap_or_else(|_| "? ? ? ?".to_string());
            let mono = self.diag.mono_rms.lock().map(|v| *v).unwrap_or(0.0);
            let outr = self.diag.out_rms.lock().map(|v| *v).unwrap_or(0.0);
            // (The loopback auto-test that used to run here has served its
            // purpose: channels 2-3 of the interface are its own output
            // loopback, channel 1 is the real instrument input.)
            eprintln!(
                "audio: {} cb, peak {:.4}, ring {} | shaper resets {} | in {}fr/{}sm out {}fr, underruns {} | ch rms [{}] mono {:.4} out {:.4} {}",
                self.cb_count,
                peak,
                self.ring_level.load(Ordering::Relaxed),
                self.diag.shaper_resets.load(Ordering::Relaxed),
                self.diag.in_frames.load(Ordering::Relaxed),
                self.diag.in_samples.load(Ordering::Relaxed),
                self.diag.out_frames.load(Ordering::Relaxed),
                self.diag.underruns.load(Ordering::Relaxed),
                rms,
                mono,
                outr,
                ""
            );
            self.last_print = std::time::Instant::now();
        }
    }

    fn fill_source(&mut self, out: &mut [f32]) {
        for o in out.iter_mut() {
            *o = self.source.sample(self.fs);
        }
    }
}

// ---------------------------------------------------------------------------
// cpal glue
// ---------------------------------------------------------------------------

trait FromF32: cpal::SizedSample {
    fn from_f32(v: f32) -> Self;
}
impl FromF32 for f32 {
    fn from_f32(v: f32) -> Self {
        v
    }
}
impl FromF32 for i16 {
    fn from_f32(v: f32) -> Self {
        (v.clamp(-1.0, 1.0) * 32767.0) as i16
    }
}
impl FromF32 for i32 {
    fn from_f32(v: f32) -> Self {
        (v.clamp(-1.0, 1.0) * 2_147_483_647.0) as i32
    }
}
impl FromF32 for u16 {
    fn from_f32(v: f32) -> Self {
        ((v.clamp(-1.0, 1.0) + 1.0) * 32767.0) as u16
    }
}

trait ToF32: cpal::SizedSample {
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

fn build_output_stream<T: FromF32 + 'static>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    state: Arc<Mutex<AudioState>>,
    params: Arc<Params>,
    ring: Arc<RingBuffer>,
    channels: usize,
    diag: Arc<Diag>,
) -> Result<cpal::Stream, cpal::Error> {
    device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            let frames = data.len() / channels.max(1);
            diag.out_frames.store(frames, Ordering::Relaxed);
            let mut st = state.lock().unwrap();
            let mut in_mono = vec![0.0f32; frames];
            if st.mode == Mode::Live {
                let before = ring.available();
                ring.pop(&mut in_mono);
                if before < frames {
                    diag.underruns.fetch_add(1, Ordering::Relaxed);
                }
            } else {
                st.fill_source(&mut in_mono);
            }
            let mut out = vec![0.0f32; data.len()];
            st.render(&params, &in_mono, &mut out);
            if diag.mute.load(Ordering::Relaxed) {
                for o in data.iter_mut() {
                    *o = T::from_f32(0.0);
                }
            } else {
                for (o, s) in data.iter_mut().zip(out.iter()) {
                    *o = T::from_f32(*s);
                }
            }
        },
        |err| eprintln!("output callback error: {err}"),
        // A timeout is essential: cpal may call `set_sample_rate` with this
        // value, and `None` can block indefinitely on some CoreAudio devices.
        Some(std::time::Duration::from_millis(500)),
    )
}

fn build_input_stream<T: ToF32 + 'static>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    ring: Arc<RingBuffer>,
    ring_level: Arc<AtomicUsize>,
    diag: Arc<Diag>,
) -> Result<cpal::Stream, cpal::Error> {
    device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            let ch = channels.max(1);
            let frames = data.len() / ch;
            diag.in_frames.store(frames, Ordering::Relaxed);
            diag.in_samples.store(data.len(), Ordering::Relaxed);
            let mut mono = Vec::with_capacity(frames);
            // Per-channel mean square, so we can see which channels are live.
            let mut ms = [0.0f32; 4];
            let mut mono_ms = 0.0f32;
            for frame in data.chunks(ch) {
                // Sum the first two channels at FULL level.  Interfaces often
                // report more channels than they have physical inputs (the
                // Scarlett 2i2 exposes its playback mix as inputs 3-4), and
                // one of the two real inputs may be dead -- averaging would
                // throw away half the guitar signal and push the user to
                // crank the drive into the aliasing region.
                let mut sum = 0.0f32;
                for (i, s) in frame.iter().take(4).enumerate() {
                    let v = s.to_f32();
                    ms[i] += v * v;
                    if i < 2 {
                        sum += v;
                    }
                }
                mono_ms += sum * sum;
                mono.push(sum);
            }
            if let Ok(mut d) = diag.ch_rms.lock() {
                let norm = (frames as f32).max(1.0);
                for i in 0..4 {
                    d[i] = d[i] * 0.95 + (ms[i] / norm).sqrt();
                }
            }
            if let Ok(mut d) = diag.mono_rms.lock() {
                *d = *d * 0.95 + (mono_ms / (frames as f32).max(1.0)).sqrt();
            }
            ring.push(&mono);
            ring_level.store(ring.available().min(1_000_000), Ordering::Relaxed);
        },
        |err| eprintln!("input callback error: {err}"),
        Some(std::time::Duration::from_millis(500)),
    )
}

// ---------------------------------------------------------------------------
// GUI
// ---------------------------------------------------------------------------

struct App {
    params: Arc<Params>,
    state: Arc<Mutex<AudioState>>,
    ring_level: Arc<AtomicUsize>,
    mode: Mode,
    shaper_idx: usize,
    pitch_mode: u32,
    sweep_gate: bool,
    input_name: String,
    output_name: String,
    input_rate: u32,
    output_rate: u32,
}

impl App {
    fn new(
        state: Arc<Mutex<AudioState>>,
        params: Arc<Params>,
        ring_level: Arc<AtomicUsize>,
        input_name: String,
        output_name: String,
        input_rate: u32,
        output_rate: u32,
    ) -> Self {
        Self {
            params,
            state,
            ring_level,
            mode: Mode::Live,
            shaper_idx: 0,
            pitch_mode: 0,
            sweep_gate: true,
            input_name,
            output_name,
            input_rate,
            output_rate,
        }
    }
}

fn slider(ui: &mut egui::Ui, label: &str, value: &AtomicU32, range: std::ops::RangeInclusive<f32>) {
    slider_enabled(ui, true, label, value, range);
}

fn slider_enabled(
    ui: &mut egui::Ui,
    enabled: bool,
    label: &str,
    value: &AtomicU32,
    range: std::ops::RangeInclusive<f32>,
) {
    let mut v = get_f32(value);
    let r = ui.add_enabled(enabled, egui::Slider::new(&mut v, range).text(label));
    if r.changed() {
        set_f32(value, v);
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::containers::Panel::left("controls")
            .resizable(true)
            .show(ui, |ui| {
                ui.heading("Pedal DSP — live");
                ui.label(egui::RichText::new("same code as the native desktop engine").weak());
                ui.separator();

                ui.label(egui::RichText::new("Source").strong());
                if ui.radio_value(&mut self.mode, Mode::Live, "Live input").changed() {
                    if let Ok(mut st) = self.state.lock() {
                        st.mode = Mode::Live;
                    }
                }
                if ui.radio_value(&mut self.mode, Mode::Wav, "Guitar WAV").changed() {
                    if let Ok(mut st) = self.state.lock() {
                        st.mode = Mode::Wav;
                    }
                }
                if ui.radio_value(&mut self.mode, Mode::Tone, "Test tone").changed() {
                    if let Ok(mut st) = self.state.lock() {
                        st.mode = Mode::Tone;
                    }
                }

                ui.add_space(6.0);
                let mut bypass = !self.params.enabled.load(Ordering::Relaxed);
                if ui.checkbox(&mut bypass, "Bypass").changed() {
                    self.params.enabled.store(!bypass, Ordering::Relaxed);
                }

                ui.add_space(6.0);
                ui.label(egui::RichText::new("Distortion").strong());
                slider(ui, "Drive", &self.params.drive, 0.1..=20.0);
                slider(ui, "Mix", &self.params.distortion_mix, 0.0..=1.0);
                slider(ui, "Level", &self.params.distortion_level, 0.0..=2.0);
                slider(ui, "Tone", &self.params.tone, -1.0..=1.0);

                ui.add_space(6.0);
                ui.label(egui::RichText::new("Time").strong());
                slider(ui, "Delay ms", &self.params.delay_ms, 0.0..=1250.0);
                slider(ui, "Delay mix", &self.params.delay_mix, 0.0..=1.0);
                slider(ui, "Delay feedback", &self.params.delay_feedback, 0.0..=0.95);
                slider(ui, "Pitch", &self.params.pitch, -2400.0..=2400.0);
                slider(ui, "Pitch glide ms", &self.params.pitch_glide, 0.0..=2000.0);
                ui.label(egui::RichText::new("Pitch mode").strong());
                let mode_changed = ui
                    .radio_value(&mut self.pitch_mode, 0, "Target (hold the pitch)")
                    .changed()
                    | ui
                        .radio_value(&mut self.pitch_mode, 1, "Sweep (keep climbing)")
                        .changed()
                    | ui
                        .radio_value(&mut self.pitch_mode, 2, "Shepard (endless rise)")
                        .changed();
                if mode_changed {
                    self.params
                        .pitch_mode
                        .store(self.pitch_mode, Ordering::Relaxed);
                }
                slider_enabled(
                    ui,
                    self.pitch_mode > 0,
                    "Pitch rate cents/s",
                    &self.params.pitch_rate,
                    -1200.0..=1200.0,
                );
                if self.pitch_mode > 0 {
                    ui.checkbox(&mut self.sweep_gate, "Sweep running (uncheck to hold the pitch)");
                    self.params
                        .sweep_gate
                        .store(self.sweep_gate, Ordering::Relaxed);
                }
                slider(ui, "Pitch jitter", &self.params.jitter, 0.0..=1.0);
                slider(ui, "Reverb mix", &self.params.reverb_mix, 0.0..=1.0);

                ui.add_space(6.0);
                ui.label(egui::RichText::new("Output").strong());
                slider(ui, "Master", &self.params.level, 0.0..=2.0);

                ui.add_space(10.0);
                ui.label(egui::RichText::new("Shaper").strong());
                egui::ComboBox::from_id_salt("shaper")
                    .selected_text(SHAPERS[self.shaper_idx].0)
                    .show_ui(ui, |ui| {
                        for (i, (name, _)) in SHAPERS.iter().enumerate() {
                            if ui.selectable_value(&mut self.shaper_idx, i, *name).changed() {
                                self.params.shaper.store(i as u32, Ordering::Relaxed);
                            }
                        }
                    });
            });

        egui::containers::CentralPanel::default().show(ui, |ui| {
            ui.heading("Audio");
            let peak = self
                .state
                .lock()
                .map(|s| *s.peak.lock().unwrap())
                .unwrap_or(0.0);
            ui.add(egui::ProgressBar::new(peak.clamp(0.0, 1.0)).text(format!("peak {peak:.2}")));
            let ring = self.ring_level.load(Ordering::Relaxed);
            ui.add(
                egui::ProgressBar::new(ring as f32 / RING_CAPACITY as f32).text(format!("input ring {ring}")),
            );
            ui.separator();
            ui.label(format!("input  : {}  @ {} Hz", self.input_name, self.input_rate));
            ui.label(format!("output : {}  @ {} Hz", self.output_name, self.output_rate));
            ui.separator();
            ui.label("Plug your guitar into the interface, pick Live input, and strum.");
            ui.label(egui::RichText::new("No sound? The interface may be busy, or macOS's CoreAudio daemon may need a restart: sudo killall coreaudiod").weak());
        });
    }
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn device_name(device: &cpal::Device) -> String {
    device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| "unknown device".to_string())
}

fn supports_rate<I>(mut configs: I, rate: u32) -> bool
where
    I: Iterator<Item = cpal::SupportedStreamConfigRange>,
{
    configs.any(|r| r.min_sample_rate() <= rate && rate <= r.max_sample_rate())
}

fn main() -> eframe::Result<()> {
    let host = cpal::default_host();

    // --- pick the audio interface: one device for BOTH input and output ---
    let mut in_devices: Vec<cpal::Device> = Vec::new();
    if let Ok(iter) = host.input_devices() {
        in_devices.extend(iter);
    }
    if in_devices.is_empty() {
        panic!("no audio input devices found");
    }
    eprintln!("input devices:");
    for (i, d) in in_devices.iter().enumerate() {
        eprintln!("  [{i}] {}", device_name(d));
    }
    let iface_idx = in_devices
        .iter()
        .position(|d| device_name(d).contains("Scarlett"))
        .or_else(|| in_devices.iter().position(|d| device_name(d).contains("USB")))
        .unwrap_or(0);
    let iface_name = device_name(&in_devices[iface_idx]);
    eprintln!("audio interface: {iface_name}");

    // --- the interface is also the output device ---
    let mut out_devices: Vec<cpal::Device> = Vec::new();
    if let Ok(iter) = host.output_devices() {
        out_devices.extend(iter);
    }
    if out_devices.is_empty() {
        panic!("no audio output devices found");
    }
    eprintln!("output devices:");
    for (i, d) in out_devices.iter().enumerate() {
        eprintln!("  [{i}] {}", device_name(d));
    }
    // Same device for output when possible; only fall back to the built-in
    // speakers if the interface really has no outputs.
    let out_idx = out_devices
        .iter()
        .position(|d| device_name(d) == iface_name)
        .or_else(|| {
            out_devices.iter().position(|d| {
                let n = device_name(d);
                n.contains("MacBook") || n.contains("Built-in") || n.contains("Speakers")
            })
        })
        .unwrap_or(0);
    let output_device = out_devices[out_idx].clone();
    let output_name = device_name(&output_device);
    let input_device = in_devices[iface_idx].clone();
    let input_name = iface_name.clone();
    eprintln!("using input:  {input_name}");
    eprintln!("using output: {output_name}");

    // --- defaults + a common sample rate ---
    let out_default = output_device
        .default_output_config()
        .expect("no default output configuration");
    let in_default = input_device
        .default_input_config()
        .expect("no default input configuration");
    let out_rate = out_default.sample_rate();
    let in_rate = in_default.sample_rate();

    let target_rate = if out_rate == in_rate {
        out_rate
    } else {
        let out_ok = output_device
            .supported_output_configs()
            .map(|c| supports_rate(c, in_rate))
            .unwrap_or(false);
        if out_ok {
            in_rate
        } else {
            let in_ok = input_device
                .supported_input_configs()
                .map(|c| supports_rate(c, out_rate))
                .unwrap_or(false);
            if in_ok {
                out_rate
            } else {
                out_rate
            }
        }
    };
    eprintln!("sample rate: out default {out_rate}, in default {in_rate}, using {target_rate}");

    let mut out_config: cpal::StreamConfig = out_default.into();
    out_config.sample_rate = target_rate;
    let mut in_config: cpal::StreamConfig = in_default.into();
    in_config.sample_rate = target_rate;
    let out_channels = out_config.channels as usize;
    let in_channels = in_config.channels.max(1) as usize;
    eprintln!("channels: out {out_channels}, in {in_channels}");

    // --- shared state ---
    let params = Arc::new(Params::new());
    let peak = Arc::new(Mutex::new(0.0f32));
    let ring_level = Arc::new(AtomicUsize::new(0));
    let diag = Arc::new(Diag::new());
    let ring = Arc::new(RingBuffer::new(RING_CAPACITY));
    let state = Arc::new(Mutex::new(AudioState::new(
        target_rate as f32,
        out_channels,
        &params,
        Arc::clone(&peak),
        Arc::clone(&ring_level),
        Arc::clone(&diag),
    )));

    // Pick a source for the non-live modes.
    let wav_path = std::path::Path::new("/Users/yoran/Downloads/3-Audio 0010 [2026-11-03 215835].wav");
    {
        let source = if wav_path.exists() {
            match hound::WavReader::open(wav_path) {
                Ok(mut r) => {
                    let spec = r.spec();
                    let samples: Vec<f32> = match (spec.sample_format, spec.bits_per_sample) {
                        (hound::SampleFormat::Int, 16) => r
                            .samples::<i16>()
                            .map(|s| s.unwrap() as f32 / 32768.0)
                            .collect(),
                        (hound::SampleFormat::Int, 24) => r
                            .samples::<i32>()
                            .map(|s| s.unwrap() as f32 / 8_388_608.0)
                            .collect(),
                        (hound::SampleFormat::Float, 32) => {
                            r.samples::<f32>().map(|s| s.unwrap()).collect()
                        }
                        _ => Vec::new(),
                    };
                    Source::Wav { samples, pos: 0 }
                }
                Err(e) => {
                    eprintln!("could not read WAV: {e}");
                    Source::Tone { kind: ToneKind::Saw, freq: 110.0, phase: 0.0 }
                }
            }
        } else {
            Source::Tone { kind: ToneKind::Saw, freq: 110.0, phase: 0.0 }
        };
        let mut st = state.lock().unwrap();
        st.source = source;
    }

    // --- input stream ---
    let in_format = in_default.sample_format();
    let in_stream = match in_format {
        cpal::SampleFormat::F32 => build_input_stream::<f32>(
            &input_device,
            in_config,
            in_channels,
            Arc::clone(&ring),
            Arc::clone(&ring_level),
            Arc::clone(&diag),
        ),
        cpal::SampleFormat::I16 => build_input_stream::<i16>(
            &input_device,
            in_config,
            in_channels,
            Arc::clone(&ring),
            Arc::clone(&ring_level),
            Arc::clone(&diag),
        ),
        cpal::SampleFormat::I32 => build_input_stream::<i32>(
            &input_device,
            in_config,
            in_channels,
            Arc::clone(&ring),
            Arc::clone(&ring_level),
            Arc::clone(&diag),
        ),
        cpal::SampleFormat::U16 => build_input_stream::<u16>(
            &input_device,
            in_config,
            in_channels,
            Arc::clone(&ring),
            Arc::clone(&ring_level),
            Arc::clone(&diag),
        ),
        other => panic!("unsupported input sample format: {other:?}"),
    }
    .expect("failed to build input stream");
    in_stream.play().expect("failed to start input");
    eprintln!("input stream playing");
    std::mem::forget(in_stream);

    // --- output stream ---
    let out_format = out_default.sample_format();
    let out_stream = match out_format {
        cpal::SampleFormat::F32 => build_output_stream::<f32>(
            &output_device,
            out_config,
            Arc::clone(&state),
            Arc::clone(&params),
            Arc::clone(&ring),
            out_channels,
            Arc::clone(&diag),
        ),
        cpal::SampleFormat::I16 => build_output_stream::<i16>(
            &output_device,
            out_config,
            Arc::clone(&state),
            Arc::clone(&params),
            Arc::clone(&ring),
            out_channels,
            Arc::clone(&diag),
        ),
        cpal::SampleFormat::I32 => build_output_stream::<i32>(
            &output_device,
            out_config,
            Arc::clone(&state),
            Arc::clone(&params),
            Arc::clone(&ring),
            out_channels,
            Arc::clone(&diag),
        ),
        cpal::SampleFormat::U16 => build_output_stream::<u16>(
            &output_device,
            out_config,
            Arc::clone(&state),
            Arc::clone(&params),
            Arc::clone(&ring),
            out_channels,
            Arc::clone(&diag),
        ),
        other => panic!("unsupported output sample format: {other:?}"),
    }
    .expect("failed to build output stream");
    out_stream.play().expect("failed to start output");
    eprintln!("output stream playing");
    std::mem::forget(out_stream);

    // Discard the startup backlog so the pedal feels immediate.
    ring.set_delay(LATENCY_SAMPLES);
    eprintln!("latency trimmed to {LATENCY_SAMPLES} samples");

    // --- GUI ---
    eprintln!("starting UI...");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([980.0, 560.0])
            .with_title("Pedal DSP — live"),
        ..Default::default()
    };
    eframe::run_native(
        "Pedal DSP — live",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(App::new(
                state,
                params,
                ring_level,
                input_name,
                output_name,
                target_rate,
                target_rate,
            )))
        }),
    )
}
