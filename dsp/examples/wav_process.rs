//! Host reference model: run the pedal's effect chain over a real WAV file.
//!
//! This uses the *same* `EffectChain` code as the desktop/shell engine, so what
//! you hear here is what the pedal will do (minus codec/analogue colour).
//!
//! ```bash
//! cargo run --release --example wav_process -- input.wav output.wav crunch
//! ```
//!
//! Presets: clean, crunch, heavy, lead, fuzz, clean-delay.

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use pedal_dsp::chain::EffectChain;

fn apply_preset(c: &mut EffectChain, name: &str) {
    // Reset to a known baseline.
    c.enabled = true;
    c.drive = 3.0;
    c.distortion_level = 0.8;
    c.distortion_mix = 1.0;
    c.tone = 0.0;
    c.level = 0.9;
    c.delay_ms = 250.0;
    c.delay_mix = 0.0;
    c.delay_feedback = 0.35;
    c.reverb_mix = 0.0;

    match name {
        "clean" => {
            c.enabled = false;
        }
        "crunch" => {
            c.drive = 2.2;
            c.tone = 0.15;
            c.level = 0.95;
        }
        "heavy" => {
            c.drive = 9.0;
            c.tone = -0.35;
            c.level = 0.72;
        }
        "lead" => {
            c.drive = 5.0;
            c.tone = 0.35;
            c.level = 0.85;
            c.delay_ms = 285.0;
            c.delay_mix = 0.32;
            c.delay_feedback = 0.42;
            c.reverb_mix = 0.22;
        }
        "fuzz" => {
            c.drive = 14.0;
            c.tone = 0.1;
            c.level = 0.6;
        }
        "clean-delay" => {
            // Dry path: distortion mix at 0 passes the input straight through.
            c.distortion_mix = 0.0;
            c.distortion_level = 1.0;
            c.delay_ms = 320.0;
            c.delay_mix = 0.35;
            c.delay_feedback = 0.45;
            c.reverb_mix = 0.15;
        }
        other => {
            eprintln!("unknown preset '{other}', using defaults");
        }
    }
    c.update_params();
}

fn read_samples<R: std::io::Read + std::io::Seek>(
    spec: &WavSpec,
    reader: &mut WavReader<R>,
) -> Vec<f32> {
    match (spec.sample_format, spec.bits_per_sample) {
        (SampleFormat::Int, 16) => reader
            .samples::<i16>()
            .map(|s| s.expect("bad 16-bit sample") as f32 / 32768.0)
            .collect(),
        (SampleFormat::Int, 24) => reader
            .samples::<i32>()
            .map(|s| s.expect("bad 24-bit sample") as f32 / 8_388_608.0)
            .collect(),
        (SampleFormat::Int, 32) => reader
            .samples::<i32>()
            .map(|s| s.expect("bad 32-bit sample") as f32 / 2_147_483_648.0)
            .collect(),
        (SampleFormat::Float, 32) => reader
            .samples::<f32>()
            .map(|s| s.expect("bad f32 sample"))
            .collect(),
        other => panic!("unsupported sample format: {other:?}"),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let input = args.get(1).cloned().unwrap_or_else(|| "input.wav".into());
    let output = args.get(2).cloned().unwrap_or_else(|| "output.wav".into());
    let preset = args.get(3).cloned().unwrap_or_else(|| "crunch".into());

    let mut reader = WavReader::open(&input).expect("could not open input WAV");
    let spec = reader.spec();
    eprintln!(
        "input : {}  ({} Hz, {} ch, {:?} {}-bit)",
        input, spec.sample_rate, spec.channels, spec.sample_format, spec.bits_per_sample
    );
    if spec.sample_rate != 48_000 {
        eprintln!(
            "warning: the chain is tuned for 48 kHz; {} Hz will be mis-tuned",
            spec.sample_rate
        );
    }

    let samples = read_samples(&spec, &mut reader);
    let mut chain = EffectChain::new(spec.sample_rate as f32);
    apply_preset(&mut chain, &preset);

    let mut out: Vec<f32> = Vec::with_capacity(samples.len());
    let mut peak = 0.0f32;
    let mut clip = 0usize;
    let mut sum_sq = 0.0f64;

    for &s in &samples {
        // Mono source: feed both channels, take the left.
        let (l, _r) = chain.process_stereo(s, s);
        if l.abs() > 1.0 {
            clip += 1;
        }
        peak = peak.max(l.abs());
        sum_sq += (l as f64) * (l as f64);
        out.push(l);
    }

    let out_spec = WavSpec {
        channels: 1,
        sample_rate: spec.sample_rate,
        bits_per_sample: 24,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(&output, out_spec).expect("could not create output WAV");
    for &y in &out {
        let v = (y.clamp(-1.0, 1.0) * 8_388_607.0) as i32;
        writer.write_sample(v).expect("write sample");
    }
    writer.finalize().expect("finalize WAV");

    let rms = (sum_sq / out.len() as f64).sqrt();
    eprintln!(
        "output: {}  preset={}  peak={:.3}  rms={:.3}  clipped_samples={}  duration={:.1}s",
        output,
        preset,
        peak,
        rms,
        clip,
        out.len() as f32 / spec.sample_rate as f32
    );
    if clip > 0 {
        eprintln!("note: {clip} samples exceeded full scale (soft-limited, so they were clamped)");
    }
}
