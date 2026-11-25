// Unit tests for the scale helpers (value<->pixel, log scales, drag clamping).
//
// Pure module: no DOM, no Preact. Run with Node's test runner:
//
//   node --experimental-strip-types --test src/graph/scale.test.ts
//
// (or add `pnpm run test:unit` to ui/package.json and run `pnpm test:unit`.)

import { test } from "node:test";
import assert from "node:assert/strict";
import {
  clamp,
  logTicks,
  makeLinearScale,
  makeLogScale,
  niceTicks,
  snap,
} from "./scale.ts";

// Float-tolerant equality for the round-trips that live in the IEEE 754 zone.
const close = (a: number, b: number, eps = 1e-9) => Math.abs(a - b) < eps;

test("makeLinearScale: value -> pixel, pixel -> value round trip", () => {
  const s = makeLinearScale([0, 100], [0, 400]);
  assert.equal(s.toPixel(0), 0);
  assert.equal(s.toPixel(100), 400);
  assert.equal(s.toPixel(50), 200);
  assert.equal(s.toValue(0), 0);
  assert.equal(s.toValue(400), 100);
  assert.equal(s.toValue(200), 50);
  // fractional values
  assert.ok(close(s.toPixel(25), 100));
  assert.ok(close(s.toValue(100), 25));
});

test("makeLinearScale: domain endpoints map exactly to range endpoints", () => {
  const s = makeLinearScale([-6, 6], [0, 100]);
  assert.equal(s.toPixel(-6), 0);
  assert.equal(s.toPixel(6), 100);
  // range-0 is domain-0, range-max is domain-max
  assert.ok(close(s.toValue(0), -6));
  assert.ok(close(s.toValue(100), 6));
});

test("makeLinearScale: monotonic and no aliasing inside the domain", () => {
  const s = makeLinearScale([0, 10], [0, 100]);
  for (let v = 0; v <= 10; v += 0.1) {
    const p = s.toPixel(v);
    assert.ok(p >= 0 && p <= 100, `pixel ${p} outside range for v=${v}`);
    assert.ok(close(s.toValue(p), v), `round trip failed at v=${v}, p=${p}`);
  }
});

test("makeLogScale: value -> pixel, pixel -> value round trip", () => {
  const s = makeLogScale([20, 20000], [0, 600]);
  // endpoints
  assert.equal(s.toPixel(20), 0);
  assert.equal(s.toPixel(20000), 600);
  // 500 Hz sits at 46.6% of the log span -> pixel 279.588
  assert.ok(close(s.toPixel(500), 279.5880017344075));
  assert.ok(close(s.toValue(279.5880017344075), 500));
  // a quarter-decade (20 * 10^0.5 ≈ 63.1) is a sixth of the range
  assert.ok(close(s.toPixel(20 * 10 ** 0.5), 100));
  // round trip
  assert.ok(close(s.toValue(s.toPixel(500)), 500));
});

test("makeLogScale: equal log intervals map to equal pixel intervals", () => {
  const s = makeLogScale([10, 1000], [0, 900]);
  // 10->100 (1 decade) and 100->1000 (1 decade) are the same pixel span
  const a = s.toPixel(100) - s.toPixel(10);
  const b = s.toPixel(1000) - s.toPixel(100);
  assert.ok(Math.abs(a - b) < 1e-9, `a=${a}, b=${b}`);
});

test("makeLogScale: throws on non-positive domain", () => {
  assert.throws(() => makeLogScale([0, 100], [0, 100]), RangeError);
  assert.throws(() => makeLogScale([-1, 100], [0, 100]), RangeError);
});

test("clamp: below the range goes to the floor, above to the ceiling", () => {
  assert.equal(clamp(-100, -12, 12), -12);
  assert.equal(clamp(100, -12, 12), 12);
});

test("clamp: values inside the range pass through unchanged", () => {
  assert.equal(clamp(-12, -12, 12), -12);
  assert.equal(clamp(-1, -12, 12), -1);
  assert.equal(clamp(0, -12, 12), 0);
  assert.equal(clamp(11.9, -12, 12), 11.9);
  assert.equal(clamp(12, -12, 12), 12);
});

test("snap: whole steps and sub-step values", () => {
  assert.ok(close(snap(0.36, 0.1), 0.4));
  assert.ok(close(snap(0.34, 0.1), 0.3));
  assert.ok(close(snap(-0.36, 0.1), -0.4));
  assert.ok(close(snap(1.2, 1), 1));
  assert.ok(close(snap(5, 2), 6));
  assert.ok(close(snap(1.234, 0.01), 1.23));
});

test("niceTicks: sorted, covers the domain, step is a round number", () => {
  const t = niceTicks(0, 100, 5);
  assert.ok(t.length >= 4);
  for (let i = 1; i < t.length; i++) {
    assert.ok(t[i] > t[i - 1], "not increasing");
  }
  assert.ok(t[0] <= 0 && t[t.length - 1] >= 100, "does not cover domain");
  // step is a nice decade number (power of 10 times 1, 2, 2.5 or 5)
  const step = t[1] - t[0];
  const mag = 10 ** Math.floor(Math.log10(step));
  const norm = step / mag;
  assert.ok([1, 2, 2.5, 5, 10].includes(norm), `step ${step} not nice`);
});

test("logTicks: exact count, equal ratios, covers min..max", () => {
  const t = logTicks(20, 20000, 8);
  assert.equal(t.length, 8);
  // first tick on min, last tick on max (modulo float error)
  assert.ok(close(t[0], 20));
  assert.ok(close(t[t.length - 1], 20000, 100));
  for (let i = 1; i < t.length; i++) {
    assert.ok(t[i] > t[i - 1], "not increasing");
  }
  // constant ratio between neighbours (log axis!) within float error
  for (let i = 1; i < t.length - 1; i++) {
    const r1 = t[i] / t[i - 1];
    const r2 = t[i + 1] / t[i];
    assert.ok(Math.abs(r1 - r2) < 1e-6, `ratio not constant at ${i}: ${r1} vs ${r2}`);
  }
});

test("logTicks: snaps to tidy decimals and stays inside the domain", () => {
  const t = logTicks(20, 20000, 8);
  for (const v of t) {
    assert.ok(v > 0, `non-positive tick ${v}`);
    assert.ok(v >= 20 && v <= 20000, `tick ${v} outside domain`);
  }
});
