import { useState, useEffect, useRef } from "preact/hooks";
import { smoothPeak } from "../meter";

/**
 * Horizontal peak reduction meter for the smart noise gate.
 *
 * Displays a gradient bar (green at 0 dB → yellow at -30 dB → red at -60 dB)
 * with a white vertical indicator tracking smoothed reduction and a text
 * readout to the right of the bar.
 *
 * The engine sends `reduction_db` as a negative number (0 = no reduction,
 * -60 = full reduction).
 */
export function ReductionMeter({
  reductionDb,
}: {
  reductionDb: number;
}) {
  const [smoothed, setSmoothed] = useState(0);
  const timeRef = useRef(0);

  const target = Math.min(0, Math.max(-60, reductionDb));

  useEffect(() => {
    const now = performance.now();
    const dt = timeRef.current
      ? (now - timeRef.current) / 1000
      : 0.016;
    timeRef.current = now;
    const next = smoothPeak(smoothed, target, dt, 10, 120);
    setSmoothed(next);
  }, [reductionDb, target]);

  const pct = (Math.abs(smoothed) / 60) * 100;
  const color =
    Math.abs(smoothed) <= 30
      ? interpolateColor("#4ade80", "#fbbf24", Math.abs(smoothed) / 30)
      : interpolateColor("#fbbf24", "#ef4444", (Math.abs(smoothed) - 30) / 30);

  return (
    <div class="reduction-meter flex items-center gap-2 w-full">
      <span class="text-[10px] tracking-[0.8px] text-muted uppercase">
        REDUCTION
      </span>
      <div class="flex-1 relative h-[4px] rounded-full overflow-visible">
        {/* Static gradient background */}
        <div
          class="absolute inset-0 h-full rounded-full"
          style={{
            background:
              "linear-gradient(to right, #4ade80, #fbbf24 50%, #ef4444)",
          }}
        />
        {/* Vertical indicator */}
        <div
          class="absolute top-0 h-full w-[2px] bg-white rounded-sm pointer-events-none"
          style={{ left: `${pct}%`, transform: "translateX(-50%)" }}
        />
        {/* Readout beside bar */}
        <span
          class="absolute top-[2px] text-[9px] font-mono font-semibold tabular-nums pointer-events-none"
          style={{
            left: `${pct}%`,
            transform: "translateX(calc(-50% + 6px))",
            color,
          }}
        >
          {Math.abs(smoothed) > 0.1
            ? `${Math.round(smoothed)} dB`
            : "0 dB"}
        </span>
      </div>
    </div>
  );
}

/* ── helpers ──────────────────────────────────────────────────────────── */

function interpolateColor(
  a: string,
  b: string,
  t: number,
): string {
  const cA = hexToRgb(a);
  const cB = hexToRgb(b);
  const r = Math.round(cA.r + (cB.r - cA.r) * t);
  const g = Math.round(cA.g + (cB.g - cA.g) * t);
  const bl = Math.round(cA.b + (cB.b - cA.b) * t);
  return `rgb(${r}, ${g}, ${bl})`;
}

function hexToRgb(hex: string): { r: number; g: number; b: number } {
  const val = parseInt(hex.replace("#", ""), 16);
  return { r: (val >> 16) & 255, g: (val >> 8) & 255, b: val & 255 };
}
