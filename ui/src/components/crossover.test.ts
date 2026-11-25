// Unit tests for the crossover resolution logic (pure, no DOM/Preact).
//
// Run with: node --experimental-strip-types --test src/components/crossover.test.ts

import { test } from "node:test";
import assert from "node:assert/strict";
import {
  CROSSOVER_PARAMS,
  activeCrossoverParamIds,
  resolveCrossoverValues,
  formatCrossover,
  formatTickValue,
  validateCrossoverSet,
} from "./crossover.ts";

test("activeCrossoverParamIds: derives the active count from bands", () => {
  assert.equal(activeCrossoverParamIds(2).length, 1);
  assert.deepEqual(activeCrossoverParamIds(2), ["crossover_1"]);
  assert.deepEqual(activeCrossoverParamIds(3), ["crossover_1", "crossover_2"]);
  assert.deepEqual(activeCrossoverParamIds(4), ["crossover_1", "crossover_2", "crossover_3"]);
  // floors and clamps
  assert.deepEqual(activeCrossoverParamIds(3.9), ["crossover_1", "crossover_2"]);
  assert.deepEqual(activeCrossoverParamIds(1), ["crossover_1"]);
});

test("resolveCrossoverValues: identity default set passes through", () => {
  const v = resolveCrossoverValues([200, 4000, 8000], CROSSOVER_PARAMS);
  assert.deepEqual(v, [200, 4000, 8000]);
});

test("resolveCrossoverValues: clamps to each parameter range", () => {
  // crossover_1 is clamped to [20, 2000]
  const v = resolveCrossoverValues([-5, 4000, 8000], CROSSOVER_PARAMS);
  assert.equal(v[0], 20);
  // crossover_2 clamped to [200, 20000]
  const v2 = resolveCrossoverValues([200, 50000, 8000], CROSSOVER_PARAMS);
  assert.equal(v2[1], 20000);
  // crossover_3 clamped to [200, 20000]
  const v3 = resolveCrossoverValues([200, 4000, 50000], CROSSOVER_PARAMS);
  assert.equal(v3[2], 20000);
});

test("resolveCrossoverValues: keeps the defaults ordered and spaced", () => {
  const v = resolveCrossoverValues([200, 4000, 8000], CROSSOVER_PARAMS);
  assert.ok(v[0] < v[1] && v[1] < v[2]);
  assert.ok(v[1] >= v[0] * 2);
  assert.ok(v[2] >= v[1] * 2);
});

test("resolveCrossoverValues: restores ordering when raw values are scrambled", () => {
  // crossover_1 dragged to the top of its range, crossover_2 dragged low,
  // crossover_3 dragged to the bottom.
  const v = resolveCrossoverValues([2000, 3000, 2500], CROSSOVER_PARAMS);
  assert.equal(v[0], 2000);
  assert.ok(v[1] >= v[0] * 2, `expected >= ${v[0] * 2}, got ${v[1]}`);
  assert.ok(v[2] >= v[1] * 2, `expected >= ${v[1] * 2}, got ${v[2]}`);
  // The engine's param ranges force a ripple: with crossover_1 at 2000, the
  // next edge must be >= 4000 and the third >= 8000.
  assert.equal(v[1], 4000);
  assert.equal(v[2], 8000);
});

test("resolveCrossoverValues: a mid marker dragged past its right neighbour is raised", () => {
  // crossover_1 stays 200; crossover_2 dragged to 300 (below 200*2=400) pushes
  // itself up to 400.
  const v = resolveCrossoverValues([200, 300, 8000], CROSSOVER_PARAMS);
  assert.equal(v[0], 200);
  assert.equal(v[1], 400);
  assert.ok(v[2] >= v[1] * 2);
});

test("resolveCrossoverValues: a right neighbour dragged low clamps the left", () => {
  // crossover_3 dragged to 3600 (below 4000*2=8000) forces crossover_2 up to
  // 2000 (>= 4000/2) if needed.
  const v = resolveCrossoverValues([200, 4000, 3600], CROSSOVER_PARAMS);
  assert.ok(v[1] * 2 <= v[2] + 1e-6, "crossover_2 must be at least half of crossover_3");
  assert.ok(v[1] <= 4000);
});

test("resolveCrossoverValues: returns at most `params.length` entries", () => {
  const v = resolveCrossoverValues([200, 4000, 8000, 16000], CROSSOVER_PARAMS);
  assert.equal(v.length, 3);
});

test("formatCrossover: Hz below 1 kHz, kHz above", () => {
  assert.equal(formatCrossover(20), "20 Hz");
  assert.equal(formatCrossover(200), "200 Hz");
  assert.equal(formatCrossover(1500), "1.5 kHz");
  assert.equal(formatCrossover(4000), "4 kHz");
  assert.equal(formatCrossover(8000), "8 kHz");
  assert.equal(formatCrossover(12500), "12.5 kHz");
  assert.equal(formatCrossover(20000), "20 kHz");
});

test("formatTickValue: compact log ticks", () => {
  assert.equal(formatTickValue(20), "20");
  assert.equal(formatTickValue(100), "100");
  assert.equal(formatTickValue(999), "999");
  assert.equal(formatTickValue(1000), "1k");
  assert.equal(formatTickValue(1250), "1.3k");
  assert.equal(formatTickValue(20000), "20k");
});

test("validateCrossoverSet: accepts a valid set", () => {
  assert.equal(validateCrossoverSet([200, 4000, 8000], CROSSOVER_PARAMS), null);
});

test("validateCrossoverSet: rejects out-of-range, out-of-order and too-close values", () => {
  assert.ok(validateCrossoverSet([200, 4000, 80000], CROSSOVER_PARAMS) !== null);
  assert.ok(validateCrossoverSet([8000, 4000, 200], CROSSOVER_PARAMS) !== null);
  // too close (3000 < 200*2), but in range
  assert.ok(validateCrossoverSet([200, 300, 8000], CROSSOVER_PARAMS) !== null);
});
