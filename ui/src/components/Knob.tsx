import { useRef, useState } from "preact/hooks";

interface KnobProps {
  name: string;
  value: number;
  min: number;
  max: number;
  defaultValue?: number;
  unit?: string;
  format?: (v: number) => string;
  onChange: (v: number) => void;
  /** Discrete step for keyboard arrows (defaults to `1`). The per-band strip
   *  passes the schema's `step` so each control snaps to its granularity. */
  step?: number;
  /** "log" maps the arc logarithmically (equal perceptual steps); "linear"
   *  maps the value directly. Defaults to "linear". */
  curve?: "linear" | "log";
}

/** Geometry of the indicator ring around the knob face. */
const RING_R = 30; // ring centreline radius, px (the face itself is r=27)
const RING_W = 4; // ring thickness
const START = 135; // SVG degrees clockwise from 3 o'clock == 225deg on a clock face

/** Compact readout for the arc's end stops (min / max).
 *
 *  Bare numbers, no unit: the pills sit between the ring and the parameter
 *  name at 6.5px, and a suffix like "cents" would make them wide enough to
 *  reach the next knob. The knob's own value display carries the unit. */
function endLabel(v: number): string {
  const a = Math.abs(v);
  return a >= 100 ? v.toFixed(0) : a >= 10 ? v.toFixed(1) : v.toFixed(2);
}

/** A rotary knob. Drag vertically, double-click to reset. */
export function Knob({
  name,
  value,
  min,
  max,
  defaultValue,
  unit = "",
  format,
  step = 1,
  onChange,
  curve = "linear",
}: KnobProps) {
  const [dragging, setDragging] = useState(false);
  const start = useRef({ y: 0, v: 0 });

  const span = max - min || 1;
  let t = Math.min(1, Math.max(0, (value - min) / span));
  if (curve === "log" && min > 0) {
    // Log spacing: equal steps of the parameter map to equal steps of the arc,
    // which is how pitch and time (ms) read off the dial. Guard against min <= 0
    // (only the linear path is valid there).
    const logMin = Math.log(min);
    const logMax = Math.log(max);
    t = Math.min(1, Math.max(0, (Math.log(value) - logMin) / (logMax - logMin)));
  }
  // 270 degrees of travel, from -135deg to +135deg.
  const angle = -135 + t * 270;
  const display = format ? format(value) : `${value.toFixed(2)}${unit}`;

  const down = (e: PointerEvent) => {
    e.preventDefault();
    (e.target as Element).setPointerCapture?.(e.pointerId);
    start.current = { y: e.clientY, v: value };
    setDragging(true);
  };
  const move = (e: PointerEvent) => {
    if (!dragging) return;
    const dy = start.current.y - e.clientY;
    const next = start.current.v + (dy / 150) * span;
    onChange(Math.min(max, Math.max(min, next)));
  };
  const up = () => setDragging(false);

  const handleKeyDown = (e: KeyboardEvent) => {
    switch (e.key) {
      case "ArrowUp":
      case "ArrowRight":
        e.preventDefault();
        onChange(Math.min(max, Math.max(min, value + step)));
        break;
      case "ArrowDown":
      case "ArrowLeft":
        e.preventDefault();
        onChange(Math.min(max, Math.max(min, value - step)));
        break;
      case "Home":
        e.preventDefault();
        onChange(min);
        break;
      case "End":
        e.preventDefault();
        onChange(max);
        break;
    }
  };

  return (
    <div class="knob-unit flex flex-col items-center w-full gap-1 p-0">
      <div
        class={`knob relative size-[var(--ui-knob)] rounded-full bg-[radial-gradient(circle_at_34%_28%,#454956,#1a1b20_72%)] shadow-knob cursor-ns-resize touch-none ${dragging ? "dragging" : ""}`}
        style={{ "--angle": `${angle}deg` }}
        tabIndex={0}
        role="slider"
        aria-label={name}
        aria-valuemin={min}
        aria-valuemax={max}
        aria-valuenow={value}
        aria-valuetext={format ? format(value) : `${value.toFixed(2)}${unit}`}
        onKeyDown={handleKeyDown}
        onPointerDown={down}
        onPointerMove={move}
        onPointerUp={up}
        onPointerCancel={up}
        onDblClick={() => defaultValue !== undefined && onChange(defaultValue)}
        title={`${name}: ${display} — drag, double-click to reset`}
      >
        {/* The ring is SVG, not a masked conic gradient: exact geometry, and it
            renders identically in every webview. pathLength=100 makes the dash
            numbers percentages of the circumference; 75 = the 270deg window. */}
        {/* Preact preserves SVG prop spelling: presentation attributes must use
            SVG's kebab-case names or the browser ignores the arc geometry. */}
        <svg class="knob-arc absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 pointer-events-none" viewBox="0 0 64 64" width="64" height="64" aria-hidden="true">
          <circle cx="32" cy="32" r={RING_R} fill="none" stroke="#2f323b" stroke-width={RING_W} pathLength={100} stroke-dasharray="75 100" transform={`rotate(${START} 32 32)`} />
          <circle cx="32" cy="32" r={RING_R} fill="none" stroke="var(--amber)" stroke-width={RING_W} stroke-linecap="round" pathLength={100} stroke-dasharray={`${t * 75} 100`} transform={`rotate(${START} 32 32)`} />
        </svg>
        <span class={`knob-indicator absolute left-1/2 top-[8.33%] h-[41.67%] w-[5.56%] origin-[50%_100%] rounded-sm shadow-[0_0_4px_rgba(0,0,0,0.8)] ${dragging ? "bg-amber" : "bg-[#f4f6fa]"}`} style={{ transform: `translateX(-50%) rotate(${angle}deg)` }} />
        {/* End stops, sitting at the two ends of the travel arc. */}
        <span class="knob-end lo absolute right-[calc(50%+var(--space-6))] top-[calc(50%+var(--space-6))] text-pico leading-none text-muted tabular-nums bg-well border border-line-soft rounded-xs pointer-events-none" style={{ padding: 0 }}>{endLabel(min)}</span>
        <span class="knob-end hi absolute left-[calc(50%+var(--space-6))] top-[calc(50%+var(--space-6))] text-pico leading-none text-muted tabular-nums bg-well border border-line-soft rounded-xs pointer-events-none" style={{ padding: 0 }}>{endLabel(max)}</span>
      </div>
      <div class="knob-name text-center text-2xs tracking-[0.8px] text-muted font-semibold uppercase">{name}</div>
      <div class="knob-value text-xs text-text bg-inset border border-line-soft rounded-sm min-w-[56px] text-center tabular-nums px-2">{display}</div>
    </div>
  );
}