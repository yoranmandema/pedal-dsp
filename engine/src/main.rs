//! The engine process: audio + IPC, no UI.
//!
//! The UI can be killed at any moment; audio keeps running.
//!
//! `--headless` runs the same engine on a simulated device (see
//! [`pedal_engine::headless`]) — no audio interface, output discarded — so the
//! pedal can boot on a bench or a Pi with nothing attached. `--bench <seconds>`
//! then measures the CPU cost of the rig and exits.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use pedal_engine::headless;
use pedal_engine::lifecycle::{Engine, EngineArgs};
use pedal_engine::model_index;
use pedal_engine::server::Shared;
use pedal_engine::store::Store;

#[tokio::main]
async fn main() {
    let args = EngineArgs::parse();

    // One-shot: rewrite path-based `model` references to index ids, for models
    // the cached index already has. Nothing is written for a model the index
    // lacks; it is reported and left as a path.
    if args.migrate_presets {
        let store = Store::discover();
        if let Some(e) = &store.error {
            eprintln!("presets: {e}");
        }
        let index = model_index::load_index();
        if index.is_empty() {
            eprintln!(
                "pedal: no model index at {} — refresh the index, then rerun",
                model_index::index_file()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "<unknown>".into())
            );
        }
        let report = model_index::migrate_preset_models(&store.dir, &index);
        for preset in &report.migrated {
            println!("pedal: {preset}: model reference migrated to an index id");
        }
        for (preset, reference) in &report.skipped {
            eprintln!("pedal: {preset}: '{reference}' is not in the model index; left as a path");
        }
        if report.is_empty() {
            println!("pedal: no preset model references needed migrating");
        }
        return;
    }

    // Build the engine. A failure to write the pidfile is fatal here. The IPC
    // listener bind failure surfaces from `run`, so normal quits are graceful.
    let mut engine = match Engine::new(&args).await {
        Ok(e) => e,
        Err(e) => {
            eprintln!("pedal-engine: {e}");
            std::process::exit(1);
        }
    };
    if let Ok(guard) = engine.sh.store.lock() {
        if let Some(e) = guard.error.as_ref() {
            eprintln!("pedal-engine: store error: {e}");
        }
    }

    // Health monitor (~1 Hz): SoC temperature, decoded `vcgencmd get_throttled`
    // flags and ARM clock, one timestamped CSV line per tick, with an alarm on
    // any undervoltage/throttle bit. Fields degrade to `n/a` off the Pi.
    // Blocking loop, so it gets its own detached thread (not a tokio task).
    if args.bench.is_none() {
        let health_log = std::env::var("PEDAL_HEALTH_LOG")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| format!("pedal-health-{}.log", std::process::id()).into());
        let _ = std::thread::Builder::new().name("pedal-health".into()).spawn(move || {
            if let Err(e) = pedal_health::monitor::HealthMonitor::new(health_log, 1.0).run() {
                eprintln!("pedal-health: monitor error: {e}");
            }
        });
    }

    // Bench: measured on the simulated device, then the process exits 0.
    if let Some(secs) = args.bench {
        if let Some(h) = &engine.headless {
            let sh = engine.sh.clone();
            let stats = h.stats.clone();
            tokio::spawn(bench_loop(sh, stats, secs));
        } else {
            eprintln!("pedal-engine: --bench needs --headless (no device to measure)");
        }
    }

    if let Err(e) = engine.run().await {
        eprintln!("pedal-engine: {e}");
        std::process::exit(1);
    }

    // Normal quit: the `Engine` has been dropped, releasing the audio device
    // and the IPC listener. The process exits 0.
    eprintln!("pedal-engine: exited cleanly");
}

/// Report the CPU cost of the rig once a second, then exit.
//
// `cpu` is the fraction of one core spent inside the audio callback, which is
// the number US-5.4 is about: the headroom left on the Pi after the DSP.
async fn bench_loop(sh: Arc<Shared>, stats: Arc<headless::Stats>, seconds: f32) {
    let sample_rate = sh.handle.sample_rate;
    let start = Instant::now();
    let mut total_callbacks = 0u64;
    let mut total_callback_s = 0.0f64;
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let (callbacks, callback_s) = stats.take();
        total_callbacks += callbacks;
        total_callback_s += callback_s;
        let frames = sh.handle.meters.out_frames.load(Ordering::Relaxed);
        let xruns = sh.handle.meters.xruns.load(Ordering::Relaxed);
        let wall = start.elapsed().as_secs_f64().max(0.001);
        let cpu = total_callback_s / wall * 100.0;
        let realtime = frames as f64 / (wall * sample_rate as f64) * 100.0;
        eprintln!(
            "pedal-engine: bench {:.0}s cpu {cpu:.1}% of one core, frames {frames} \
             ({realtime:.1}% real time), xruns {xruns}",
            wall
        );
        if wall >= seconds as f64 {
            eprintln!(
                "pedal-engine: bench done seconds={seconds:.1} callbacks={total_callbacks} \
                 cpu={cpu:.1} frames={frames} realtime={realtime:.1} xruns={xruns}"
            );
            std::process::exit(0);
        }
    }
}
