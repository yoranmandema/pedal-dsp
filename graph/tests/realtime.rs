//! Real-time guarantees of the executor.
//!
//! This is its own test binary because it installs a counting global
//! allocator. Counting is per-thread, so parallel tests don't interfere.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use dsp_graph::blocks::{Amp, Cab, Delay, Gain, IoInput, IoOutput, Master, MixSum, Overdrive, PitchDelay, Reverb, ShimmerReverb, ToneStack};
use dsp_graph::prelude::*;
use dsp_graph::{CompileError, Executor, Graph, MAX_PORTS};

struct Counting;

thread_local! {
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
        System.realloc(p, l, n)
    }
}

#[global_allocator]
static A: Counting = Counting;

fn allocs() -> usize {
    ALLOCS.with(|c| c.get())
}

const SR: f32 = 48_000.0;
const BLOCK: usize = 64;

/// Pure delay that declares its latency, so the graph gets compensation
/// delay lines and those are covered too.
struct Latent(Vec<f32>, usize);
impl Block for Latent {
    fn spec(&self) -> BlockSpec {
        BlockSpec {
            id: "test.latent",
            name: "Latent",
            category: Category::Utility,
            inputs: vec![PortSpec::audio("in")],
            outputs: vec![PortSpec::audio("out")],
            params: vec![],
            controls_in: vec![],
            controls_out: vec![],
        }
    }
    fn prepare(&mut self, _: f32, _: usize) -> Result<(), BlockError> {
        Ok(())
    }
    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        let (inp, out) = (ctx.inputs[0], &mut ctx.outputs[0][..]);
        for i in 0..ctx.n {
            out[i] = self.0[self.1];
            self.0[self.1] = inp[i];
            self.1 = (self.1 + 1) % self.0.len();
        }
        Ok(())
    }
    fn reset(&mut self) {
        self.0.fill(0.0);
    }
    fn latency_samples(&self) -> u32 {
        self.0.len() as u32
    }
}

/// Every library block, with a latent branch in parallel:
/// in -> gain -> tone -> drive -> latent -> delay -> mix.a
///                          \--> reverb ---------> mix.b -> out
fn full_rig() -> Executor {
    let mut g = Graph::new();
    g.add("in", Box::new(IoInput)).unwrap();
    g.add("gain", Box::new(Gain::default())).unwrap();
    g.add("tone", Box::new(ToneStack::default())).unwrap();
    g.add("drive", Box::new(Overdrive::default())).unwrap();
    g.add("lat", Box::new(Latent(vec![0.0; 100], 0))).unwrap();
    g.add("delay", Box::new(Delay::default())).unwrap();
    g.add("verb", Box::new(Reverb::default())).unwrap();
    g.add("mix", Box::new(MixSum::default())).unwrap();
    g.add("out", Box::new(IoOutput)).unwrap();
    for (a, b) in [("in", "gain"), ("gain", "tone"), ("tone", "drive"), ("drive", "lat"), ("lat", "delay"), ("drive", "verb")] {
        g.connect(a, "out", b, "in").unwrap();
    }
    g.connect("delay", "out", "mix", "in_a").unwrap();
    g.connect("verb", "out", "mix", "in_b").unwrap();
    g.connect("mix", "out", "out", "in").unwrap();
    Executor::compile_owned(&mut g, SR, BLOCK).unwrap()
}

/// The audio callback must never allocate.
#[test]
fn run_does_not_allocate() {
    let mut ex = full_rig();
    assert!(ex.delay_count() > 0, "rig should exercise compensation delays");
    let input: Vec<f32> = (0..BLOCK).map(|i| (i as f32 * 0.05).sin()).collect();
    ex.run(&input, BLOCK).unwrap(); // warm-up, in case a block lazily inits
    let before = allocs();
    for _ in 0..1000 {
        let out = ex.run(&input, BLOCK).unwrap();
        std::hint::black_box(out);
    }
    let n = allocs() - before;
    assert_eq!(n, 0, "run() allocated {n} times over 1000 blocks");
}

/// The pool is not zeroed between blocks, so every block must write all `n`
/// frames of every output. Pre-fill outputs with NaN and check none survive.
#[test]
fn every_library_block_writes_all_frames_without_allocating() {
    let blocks: Vec<Box<dyn Block>> = vec![
        Box::new(Gain::default()),
        Box::new(ToneStack::default()),
        Box::new(Overdrive::default()),
        Box::new(Reverb::default()),
        Box::new(Delay::default()),
        Box::new(MixSum::default()),
        Box::new(Amp::default()),
        Box::new(Cab::default()),
        Box::new(PitchDelay::default()),
        Box::new(ShimmerReverb::default()),
        Box::new(Master),
    ];
    for mut b in blocks {
        let spec = b.spec();
        b.prepare(SR, BLOCK).unwrap();
        let params: Vec<f32> = spec.params.iter().map(|p| p.default).collect();
        let input: Vec<f32> = (0..BLOCK).map(|i| (i as f32 * 0.1).sin() * 0.5).collect();
        let ins: Vec<&[f32]> = spec.inputs.iter().map(|_| &input[..]).collect();
        let mut bufs: Vec<Vec<f32>> = spec.outputs.iter().map(|_| vec![f32::NAN; BLOCK]).collect();
        // Twice: the second block catches outputs written only on first run.
        for _ in 0..2 {
            for buf in &mut bufs {
                buf.fill(f32::NAN);
            }
            let mut outs: Vec<&mut [f32]> = bufs.iter_mut().map(|v| &mut v[..]).collect();
            let mut ctx = BlockCtx {
                inputs: &ins,
                outputs: &mut outs,
                params: &params,
                controls_in: &[],
                controls_out: &mut [],
                n: BLOCK,
                sample_rate: SR,
                block_index: 0,
            };
            let before = allocs();
            b.process(&mut ctx).unwrap();
            let n = allocs() - before;
            assert_eq!(n, 0, "{}: process() allocated {n} times", spec.id);
            for (k, buf) in bufs.iter().enumerate() {
                if let Some(i) = buf.iter().position(|v| v.is_nan()) {
                    panic!("{}: output {k} frame {i} left unwritten", spec.id);
                }
            }
        }
    }
}

/// Short final blocks (n < max_block) must work and must not read past n.
#[test]
fn short_blocks_are_supported() {
    let mut ex = full_rig();
    let input = vec![0.1f32; BLOCK];
    for n in [1, 7, 33, BLOCK] {
        let out = ex.run(&input[..n], n).unwrap();
        assert_eq!(out.len(), n);
        assert!(out.iter().all(|v| v.is_finite()));
    }
    // Asking for more frames than given input is clamped, not a panic.
    assert_eq!(ex.run(&input[..10], BLOCK).unwrap().len(), 10);
}

/// The executor is built on the UI thread and moved to the audio thread.
#[test]
fn executor_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<Executor>();
}

/// A node wider than the stack port tables is rejected at compile time.
#[test]
fn too_many_ports_is_a_compile_error() {
    struct Wide;
    impl Block for Wide {
        fn spec(&self) -> BlockSpec {
            const NAMES: [&str; 9] = ["o0", "o1", "o2", "o3", "o4", "o5", "o6", "o7", "o8"];
            BlockSpec {
                id: "test.wide",
                name: "Wide",
                category: Category::Utility,
                inputs: vec![PortSpec::audio("in")],
                outputs: NAMES.iter().map(|n| PortSpec::audio(n)).collect(),
                params: vec![],
                controls_in: vec![],
                controls_out: vec![],
            }
        }
        fn prepare(&mut self, _: f32, _: usize) -> Result<(), BlockError> {
            Ok(())
        }
        fn process(&mut self, _: &mut BlockCtx<'_>) -> Result<(), BlockError> {
            Ok(())
        }
        fn reset(&mut self) {}
    }
    assert_eq!(MAX_PORTS, 8);
    let mut g = Graph::new();
    g.add("in", Box::new(IoInput)).unwrap();
    g.add("wide", Box::new(Wide)).unwrap();
    g.add("out", Box::new(IoOutput)).unwrap();
    g.connect("in", "out", "wide", "in").unwrap();
    g.connect("wide", "o0", "out", "in").unwrap();
    match Executor::compile_owned(&mut g, SR, BLOCK) {
        Err(CompileError::TooManyPorts { node, ports }) => {
            assert_eq!(node, "wide");
            assert_eq!(ports, 9);
        }
        other => panic!("expected TooManyPorts, got {other:?}"),
    }
}
