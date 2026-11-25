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
<<<<<<< HEAD
    let args: Vec<String> = std::env::args().collect();
    let flag = |name: &str| args.iter().any(|a| a == name);
    let value = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let headless_mode = flag("--headless");
    let port = value("--port")
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(PORT);
    let bench = value("--bench")
        .and_then(|v| v.parse::<f32>().ok())
        .filter(|s| *s > 0.0);
    let preset = value("--rig").unwrap_or_else(|| "04-rainbow".into());
=======
    let args = EngineArgs::parse();
>>>>>>> origin/develop

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

<<<<<<< HEAD
    let store = Store::discover();
    if let Some(e) = &store.error {
        eprintln!("presets: {e}");
    }

    // Pick the devices first: the executor and the audio thread must agree on
    // the sample rate. `--headless` skips this entirely — there is no device.
    let devices = if headless_mode {
        None
    } else {
        Some(host::pick())
    };
    let setup = match &devices {
        Some(d) => Setup {
            sample_rate: d.rate as f32,
            out_channels: d.out_channels,
            block: BLOCK,
            channel_mode: pedal_engine::audio::ChannelMode::First(2),
            latency_frames: LATENCY_FRAMES,
        },
        None => Setup {
            sample_rate: 48_000.0,
            out_channels: 2,
            block: BLOCK,
            channel_mode: pedal_engine::audio::ChannelMode::First(2),
            latency_frames: LATENCY_FRAMES,
        },
    };
    let source = Arc::new(AtomicU8::new(2)); // silence by default; the riff is opt-in
    let (mut handle, cmd_rx, trash_tx, running) = new_engine(setup, source.clone());
    handle.preset_dir = store.dir.clone();
    let meters = handle.meters.clone();
    let registry = handle.registry.clone();

    // Compile and install the initial rig before the streams start.
    let doc = store
        .presets
        .iter()
        .find(|(n, _)| *n == preset)
        .map(|(_, d)| d.clone())
        .or_else(|| store.presets.first().map(|(_, d)| d.clone()))
        .unwrap_or_else(|| Rig::new("Untitled").to_doc());
    let rig = Rig::from_doc(&doc);
    {
        let resolver = pedal_engine::model_index::resolver();
        let mut g = rig
            .to_doc()
            .build_in_with(&handle.registry, &handle.preset_dir, &resolver)
            .expect("initial rig builds");
        rig.apply_routings(&mut g, &handle.registry)
            .expect("initial rig routings apply");
        let ex = dsp_graph::Executor::compile_owned(&mut g, setup.sample_rate, BLOCK)
            .expect("initial rig compiles");
        *handle.node_ids.lock().unwrap() = ex.node_ids().to_vec();
        handle
            .push(Cmd::Swap(ex, Vec::new()))
            .expect("initial swap");
    }
    handle.set_source(2); // silence

    // The streams own the command queue from here on. `_headless` stays alive
    // until the process exits; dropping it stops the simulated device.
    let mut _headless: Option<headless::Headless> = None;
    let mut headless_stats = None;
    let tuner_ring = match devices {
        Some(d) => {
            let (_ring, tuner_ring) = host::start(
                &d,
                cmd_rx,
                trash_tx,
                running,
                meters.clone(),
                source.clone(),
            );
            // The audio ring handle is dropped here; the input and output
            // streams are `std::mem::forget`t, so they keep running until the
            // process exits.
            tuner_ring
        }
        None => {
            // The riff by default: with no interface there is no live input, and
            // a silent DSP proves nothing about CPU cost.
            let src = match value("--source").as_deref() {
                Some("live") => 1,
                Some("silence") => 2,
                _ => 0,
            };
            handle.set_source(src);
            let name = match src {
                1 => "live",
                2 => "silence",
                _ => "riff",
            };
            // The headless thread reads this source. Pass handle.source so the
            // "live" tone actually reaches the tuner ring: new_engine's
            // handle.source was constructed from source.clone() at engine
            // start, so set_source writes to a different AtomicU8 than the one
            // passed to the headless thread. Without this, the tuner ring
            // stays empty and no tuner event reaches the UI.
            let h = headless::start(
                setup,
                cmd_rx,
                trash_tx,
                running,
                meters.clone(),
                handle.source.clone(),
            );
            headless_stats = Some(h.stats.clone());
            eprintln!("pedal-engine: headless (no audio device), source {name}");
            let tuner_ring = h.tuner_ring.clone();
            _headless = Some(h);
            tuner_ring
        }
    };

    pedal_engine::tuner::spawn(tuner_ring, setup.sample_rate, meters.clone());

    // --- IPC ---------------------------------------------------------------
    let sh = Arc::new(Shared::new(handle, registry, source, store));
    {
        let mut r = sh.rig.lock().unwrap();
        *r = rig;
        drop(r);
        let mut p = sh.preset.lock().unwrap();
        *p = preset.clone();
=======
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
>>>>>>> origin/develop
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
