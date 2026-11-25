// Pure helpers for the Graphic EQ editor (no DOM, unit-testable under node).

/** 10-band ISO 1/1-octave centres (Hz), band 0 = 31.5 Hz. */
export const EQ_CENTRES_HZ = [31.5, 63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000] as const;
export const EQ_BANDS = EQ_CENTRES_HZ.length;
export const EQ_MIN_DB = -12;
export const EQ_MAX_DB = 12;

/** Engine param id for a band (matches `tone.graphic_eq` gain0..gain9). */
export const bandParam = (band: number) => `gain${band}`;

/** Parse an engine param id back to a band index, or -1. */
export function bandFromParam(param: string): number {
  const m = /^gain(\d+)$/.exec(param);
  const n = m ? parseInt(m[1], 10) : -1;
  return n >= 0 && n < EQ_BANDS ? n : -1;
}

export const clampDb = (v: number) => Math.min(EQ_MAX_DB, Math.max(EQ_MIN_DB, v));

/** The (param, value) writes needed to flatten every band. */
export function flatWrites(): Array<[string, number]> {
  return Array.from({ length: EQ_BANDS }, (_, i) => [bandParam(i), 0] as [string, number]);
}

/** Keyboard step for a slider: 0.5 dB, Shift = 3 dB. */
export const keyStep = (shift: boolean) => (shift ? 3 : 0.5);
