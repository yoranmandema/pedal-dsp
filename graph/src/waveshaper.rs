//! `dist.waveshaper` — a data-driven transfer-curve distortion.
//!
//! The curve is an ordered list of control points `(x, y)` in `[-1, 1]²`.
//! It is baked into a fixed 2048-entry lookup table (off the audio path, in
//! `configure`/`prepare` or lazily when the interpolation mode changes); the
//! per-sample path is a gain, a clamp and a linearly interpolated table read.
//! Design: `docs/design/waveshaper.md`.
//!
//! Not implemented (documented follow-up): the optional X2/X4 oversampling.

use crate::prelude::*;
use crate::types::Curve as ParamCurve;
use serde_json::{json, Value};

/// Lookup-table length over the input axis `[-1, 1]`.
pub const TABLE_SIZE: usize = 2048;

/// Interpolation between control points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interpolation {
    Linear,
    /// Monotone cubic Hermite (Fritsch–Carlson): no overshoot between points.
    MonotoneCubic,
}

impl Interpolation {
    fn from_param(v: f32) -> Self {
        if v >= 0.5 {
            Interpolation::MonotoneCubic
        } else {
            Interpolation::Linear
        }
    }
}

/// A validated transfer curve: ≥ 2 points, x strictly increasing, all in [-1, 1].
#[derive(Clone, Debug, PartialEq)]
pub struct TransferCurve {
    points: Vec<[f32; 2]>,
}

impl Default for TransferCurve {
    fn default() -> Self {
        TransferCurve::identity()
    }
}

impl TransferCurve {
    pub fn identity() -> Self {
        TransferCurve { points: vec![[-1.0, -1.0], [1.0, 1.0]] }
    }

    pub fn points(&self) -> &[[f32; 2]] {
        &self.points
    }

    /// Strict constructor: rejects (never repairs) a bad point list.
    pub fn new(points: Vec<[f32; 2]>) -> Result<Self, String> {
        if points.len() < 2 {
            return Err(format!("curve needs at least 2 points, got {}", points.len()));
        }
        for (i, p) in points.iter().enumerate() {
            if !p[0].is_finite() || !p[1].is_finite() {
                return Err(format!("point {i} is not finite"));
            }
            if p[0].abs() > 1.0 || p[1].abs() > 1.0 {
                return Err(format!("point {i} ({}, {}) is outside [-1, 1]", p[0], p[1]));
            }
        }
        for i in 1..points.len() {
            if points[i][0] <= points[i - 1][0] {
                return Err(format!("x values must be strictly increasing (point {i})"));
            }
        }
        Ok(TransferCurve { points })
    }

    /// Lenient constructor: drops non-finite points, clamps to [-1, 1], sorts
    /// by x, collapses duplicate x (last wins) and falls back to the identity
    /// curve when fewer than 2 points survive.
    pub fn sanitized(points: &[[f32; 2]]) -> Self {
        let mut pts: Vec<[f32; 2]> = points
            .iter()
            .filter(|p| p[0].is_finite() && p[1].is_finite())
            .map(|p| [p[0].clamp(-1.0, 1.0), p[1].clamp(-1.0, 1.0)])
            .collect();
        pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
        let mut out: Vec<[f32; 2]> = Vec::with_capacity(pts.len());
        for p in pts {
            match out.last_mut() {
                Some(last) if last[0] == p[0] => *last = p,
                _ => out.push(p),
            }
        }
        if out.len() < 2 {
            return TransferCurve::identity();
        }
        TransferCurve { points: out }
    }

    /// Parse the wire/preset payload `{"points": [[x,y], ...]}` strictly.
    pub fn from_json(v: &Value) -> Result<Self, String> {
        let arr = v
            .get("points")
            .and_then(|p| p.as_array())
            .ok_or("curve must be an object with a \"points\" array")?;
        let mut pts = Vec::with_capacity(arr.len());
        for (i, e) in arr.iter().enumerate() {
            let pair = e.as_array().filter(|a| a.len() == 2);
            let (x, y) = match pair.and_then(|a| Some((a[0].as_f64()?, a[1].as_f64()?))) {
                Some(xy) => xy,
                None => return Err(format!("point {i} must be a [x, y] number pair")),
            };
            pts.push([x as f32, y as f32]);
        }
        TransferCurve::new(pts)
    }

    pub fn to_json(&self) -> Value {
        json!({ "points": self.points.iter().map(|p| json!([p[0], p[1]])).collect::<Vec<_>>() })
    }

    /// Evaluate the curve at `x` (clamped to the outer points' range; flat
    /// extrapolation beyond the first/last point).
    pub fn eval(&self, x: f32, mode: Interpolation) -> f32 {
        let p = &self.points;
        let n = p.len();
        if x <= p[0][0] {
            return p[0][1];
        }
        if x >= p[n - 1][0] {
            return p[n - 1][1];
        }
        // Segment k with p[k].x <= x < p[k+1].x.
        let k = match p.binary_search_by(|q| q[0].total_cmp(&x)) {
            Ok(i) => i.min(n - 2),
            Err(i) => i - 1,
        };
        let (x0, y0, x1, y1) = (p[k][0], p[k][1], p[k + 1][0], p[k + 1][1]);
        let h = x1 - x0;
        let t = (x - x0) / h;
        match mode {
            Interpolation::Linear => y0 + (y1 - y0) * t,
            Interpolation::MonotoneCubic => {
                let m0 = self.tangent(k);
                let m1 = self.tangent(k + 1);
                let t2 = t * t;
                let t3 = t2 * t;
                (2.0 * t3 - 3.0 * t2 + 1.0) * y0
                    + (t3 - 2.0 * t2 + t) * h * m0
                    + (-2.0 * t3 + 3.0 * t2) * y1
                    + (t3 - t2) * h * m1
            }
        }
    }

    /// Fritsch–Carlson monotone tangent at point `i`.
    fn tangent(&self, i: usize) -> f32 {
        let p = &self.points;
        let n = p.len();
        let slope = |a: usize| (p[a + 1][1] - p[a][1]) / (p[a + 1][0] - p[a][0]);
        let mut m = if i == 0 {
            slope(0)
        } else if i == n - 1 {
            slope(n - 2)
        } else {
            let (d0, d1) = (slope(i - 1), slope(i));
            if d0 * d1 <= 0.0 { 0.0 } else { (d0 + d1) * 0.5 }
        };
        // Clamp so the Hermite segment stays monotone (|m| <= 3 * secant).
        for seg in [i.checked_sub(1), (i + 1 < n).then_some(i)].into_iter().flatten() {
            let d = slope(seg);
            if d == 0.0 {
                m = 0.0;
            } else if m / d > 3.0 {
                m = 3.0 * d;
            } else if m / d < 0.0 {
                m = 0.0;
            }
        }
        m
    }

    /// Bake into `table` (no allocation: the caller owns the buffer).
    pub fn bake(&self, mode: Interpolation, table: &mut [f32; TABLE_SIZE]) {
        for (i, slot) in table.iter_mut().enumerate() {
            let u = -1.0 + 2.0 * i as f32 / (TABLE_SIZE - 1) as f32;
            *slot = self.eval(u, mode).clamp(-1.0, 1.0);
        }
    }
}

/// Read the table at `u` ∈ [-1, 1] with linear interpolation. Never panics:
/// non-finite or out-of-range `u` is clamped to the table edges.
#[inline]
pub fn lookup(table: &[f32; TABLE_SIZE], u: f32) -> f32 {
    let u = if u.is_finite() { u.clamp(-1.0, 1.0) } else { 0.0 };
    let pos = (u + 1.0) * 0.5 * (TABLE_SIZE - 1) as f32;
    let i = (pos as usize).min(TABLE_SIZE - 2);
    let frac = pos - i as f32;
    table[i] + (table[i + 1] - table[i]) * frac
}

pub struct WaveShaper {
    curve: TransferCurve,
    table: Box<[f32; TABLE_SIZE]>,
    baked_mode: Interpolation,
}

impl Default for WaveShaper {
    fn default() -> Self {
        let mut w = WaveShaper {
            curve: TransferCurve::identity(),
            table: Box::new([0.0; TABLE_SIZE]),
            baked_mode: Interpolation::Linear,
        };
        w.curve.bake(w.baked_mode, &mut w.table);
        w
    }
}

impl WaveShaper {
    /// Current curve (for snapshots / preset serialisation).
    pub fn curve(&self) -> &TransferCurve {
        &self.curve
    }

    /// Replace the (already validated) curve and rebake the table.
    pub fn set_curve(&mut self, curve: TransferCurve) {
        self.curve = curve;
        self.curve.bake(self.baked_mode, &mut self.table);
    }
}

const INTERP_CHOICES: &[&str] = &["Linear", "Smooth"];

impl Block for WaveShaper {
    fn spec(&self) -> BlockSpec {
        BlockSpec {
            id: "dist.waveshaper",
            name: "Waveshaper",
            category: Category::Distortion,
            inputs: vec![PortSpec::audio("in")],
            outputs: vec![PortSpec::audio("out")],
            params: vec![
                ParamSpec { id: "input_gain", name: "Input", unit: Unit::Linear, min: 0.125, max: 8.0, default: 1.0, curve: ParamCurve::Linear },
                ParamSpec { id: "output_gain", name: "Output", unit: Unit::Linear, min: 0.125, max: 8.0, default: 1.0, curve: ParamCurve::Linear },
                ParamSpec { id: "mix", name: "Mix", unit: Unit::Percent, min: 0.0, max: 100.0, default: 100.0, curve: ParamCurve::Linear },
                ParamSpec { id: "clamp", name: "Clamp", unit: Unit::Linear, min: 0.0, max: 1.0, default: 1.0, curve: ParamCurve::Linear },
                ParamSpec { id: "interpolation", name: "Interpolation", unit: Unit::Choice(INTERP_CHOICES), min: 0.0, max: 1.0, default: 0.0, curve: ParamCurve::Linear },
            ],
            controls_in: vec![],
            controls_out: vec![],
        }
    }

    fn prepare(&mut self, _sr: f32, _max: usize) -> Result<(), BlockError> {
        self.curve.bake(self.baked_mode, &mut self.table);
        Ok(())
    }

    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        let in_gain = ctx.param(0);
        let out_gain = ctx.param(1);
        let mix = (ctx.param(2) / 100.0).clamp(0.0, 1.0);
        let clamp = ctx.param(3) >= 0.5;
        let mode = Interpolation::from_param(ctx.param(4));
        if mode != self.baked_mode {
            // Fixed-size, allocation-free rebuild, once per block at most.
            self.baked_mode = mode;
            self.curve.bake(mode, &mut self.table);
        }
        let n = ctx.n;
        let input = ctx.inputs[0];
        let output = &mut ctx.outputs[0][..];
        for i in 0..n {
            let x = input[i];
            let mut y = lookup(&self.table, x * in_gain) * out_gain;
            if clamp {
                y = y.clamp(-1.0, 1.0);
            }
            let out = x + (y - x) * mix;
            output[i] = if out.is_finite() { out } else { 0.0 };
        }
        Ok(())
    }

    fn reset(&mut self) {}

    /// `configure("curve", <json>)` — strict; on error the curve is unchanged.
    fn configure(&mut self, key: &str, value: &str) -> Result<(), BlockError> {
        if key != "curve" {
            return Ok(());
        }
        let v: Value = serde_json::from_str(value)
            .map_err(|e| BlockError::Other(format!("invalid curve JSON: {e}")))?;
        let curve = TransferCurve::from_json(&v).map_err(BlockError::Other)?;
        self.set_curve(curve);
        Ok(())
    }
}
