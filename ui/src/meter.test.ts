// Run: node --experimental-strip-types --test src/meter.test.ts
import { test } from "node:test";
import assert from "node:assert/strict";
import { smoothPeak } from "./meter.ts";

// Two-sample sanity check: attack and release time constants are in the same
// ballpark as a real peak meter (fast attack, slow release).

test("attack: target equal to current never moves", () => {
  assert.equal(smoothPeak(5, 5, 0.01), 5);
});

test("attack (deeper peak): moves the display quickly", () => {
  // target is deeper (more negative) and dt = 10 ms, attack = 10 ms => ~63%
  // toward target. current -10 -> target -20 gives ~ -16.4.
  const t = smoothPeak(-10, -20, 0.01, 10, 120);
  assert.ok(t >= -18 && t < -15);
});

test("release (shallower peak): moves slowly", () => {
  // target is shallower and dt = 10 ms, release = 120 ms => ~8% toward target.
  const t = smoothPeak(-10, -5, 0.01, 10, 120);
  assert.ok(t >= -10 && t < -9.5);
});

test("small dt yields a proportionally small step", () => {
  // release with dt = 1 ms => k ≈ 1 - exp(-0.001/0.12) ≈ 0.0083; -10 + 0.0415.
  const t = smoothPeak(-10, -5, 0.001, 10, 120);
  assert.ok(t >= -10 && t < -9.9);
});

test("non-finite inputs fall back to zero instead of NaN", () => {
  assert.ok(!Number.isNaN(smoothPeak(NaN, -20, 0.01)));
  assert.ok(!Number.isNaN(smoothPeak(-10, NaN, 0.01)));
});
