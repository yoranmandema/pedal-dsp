// Run: node --experimental-strip-types --test src/graph/eq.test.ts
import { test } from "node:test";
import assert from "node:assert/strict";
import { bandParam, bandFromParam, clampDb, flatWrites, EQ_BANDS } from "./eq.ts";

test("band param ids round-trip", () => {
  for (let i = 0; i < EQ_BANDS; i++) assert.equal(bandFromParam(bandParam(i)), i);
  assert.equal(bandFromParam("gain10"), -1);
  assert.equal(bandFromParam("mix"), -1);
});
test("clampDb limits to +/-12", () => {
  assert.equal(clampDb(30), 12);
  assert.equal(clampDb(-30), -12);
  assert.equal(clampDb(3.5), 3.5);
});
test("flatWrites zeroes all 10 bands", () => {
  const w = flatWrites();
  assert.equal(w.length, 10);
  assert.ok(w.every(([p, v], i) => p === `gain${i}` && v === 0));
});
