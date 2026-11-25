import { useEffect, useRef, useState } from "preact/hooks";
import { Format } from "../params";
import { grDbToPercent, smoothPeak, type MultibandMeter } from "../meter";

/**
 * One per-band gain-reduction meter.
 *
 * Scaling (design doc §7):
 *   * dB scale: 0 dB at the top of the track, -60 dB at the bottom;
 *   * peak-hold: the amber marker pins to the deepest reduction reached;
 *   * ballistics: the fill is smoothed with a fast attack (~10 ms) and slow
 *     release (~120 ms) so a 20 Hz metering stream never jitters the UI;
 *   * accessibility: `role="meter"` with `aria-label`, `aria-valuemin`,
 *     `aria-valuemax`, `aria-valuenow` and `aria-valuetext` (the dB string).
 */
export interface MultibandMeterProps {
  /** Band id, e.g. "band_1". */
  band: string;
  /** Raw peak gain reduction in dB (negative = reduction). */
  grDb: number;
  /** Optional output level in dBFS, shown only when the engine provides it. */
  outDb?: number;
  /** Extra class names. */
  className?: string;
}

export function MultibandMeter({
  band,
  grDb,
  className,
}: MultibandMeterProps) {
  // Raw value, clamped to the meter's dB range.
  const clamped = Math.max(-60, Math.min(0, grDb));

  // `peakRef` is the held peak: the deepest reduction reached so far (a
  // compressor's `gr_db` only moves in one direction, so it acts as a
  // peak-hold — once a band is reduced it stays there until the signal's
  // energy drops below threshold).
  const peakRef = useRef(clamped);
  // `nowRef`/`committed` drive a requestAnimationFrame ballistics loop that
  // turns the held peak into a smooth, jitter-free display value (attack
  // ~10 ms, release ~120 ms). The loop is created once so it never does work
  // on the engine's ~20 Hz metering cadence.
  const nowRef = useRef(clamped);
  const committed = useRef(clamped);
  const [now, setNow] = useState(clamped);
  const rafRef = useRef(0);
  const lastTime = useRef(performance.now());

  // Keep the held peak current on every raw `grDb` change, without
  // re-creating the rAF loop.
  useEffect(() => {
    peakRef.current = Math.min(peakRef.current, clamped);
  }, [clamped]);

  useEffect(() => {
    const step = (time: number) => {
      const dt = Math.min(0.1, (time - lastTime.current) / 1000);
      lastTime.current = time;
      const next = smoothPeak(nowRef.current, peakRef.current, dt);
      nowRef.current = next;
      if (Math.abs(next - committed.current) >= 0.05) {
        committed.current = next;
        setNow(Math.round(next * 10) / 10);
      }
      rafRef.current = requestAnimationFrame(step);
    };
    rafRef.current = requestAnimationFrame(step);
    return () => cancelAnimationFrame(rafRef.current);
  }, []);

  const pct = grDbToPercent(now);
  const pctClamped = Math.max(0, Math.min(100, pct));
  const showsPeak = now < -0.5;

  return (
    <div className={`multiband-meter ${className ?? ""}`}>
      <div
        className="meter-scale"
        role="meter"
        aria-label={`${band} gain reduction`}
        aria-valuemin={-60}
        aria-valuemax={0}
        aria-valuenow={Math.round(now)}
        aria-valuetext={Format.db(now)}
        title={`${band}: ${Format.db(grDb)}`}
      >
        <div
          className="meter-fill"
          style={{ height: `${pctClamped}%` }}
        />
        {showsPeak && (
          <div
            className="meter-peak"
            style={{ top: `${pctClamped}%`, height: 6 }}
          />
        )}
      </div>
      <div className="meter-readout">{Format.db(now)}</div>
    </div>
  );
}
