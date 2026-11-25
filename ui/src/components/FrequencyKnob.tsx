import { useRef, useState } from "preact/hooks";
import { clamp } from "../blocks/parametric-eq";

interface FrequencyKnobProps {
  name: string;
  value: number;
  min: number;
  max: number;
  defaultValue?: number;
  unit?: string;
  format?: (v: number) => string;
  onChange: (v: number) => void;
}

/** A frequency knob with a logarithmic travel: equal pointer travel maps to
 *  equal ratios, so the face reads like a real freq display (100→300→1k…).
 *  The geometry, end stops and drag behaviour are the same as `Knob`; only the
 *  value→angle mapping is logarithmic. */
export function FrequencyKnob({
  name,
  value,
  min,
  max,
  defaultValue,
  unit = "",
  format,
  onChange,
}: FrequencyKnobProps) {
  const [dragging, setDragging] = useState(false);
  const start = useRef({ y: 0, v: value });

  const span = Math.log(max) - Math.log(min);
  const t = clamp((Math.log(value) - Math.log(min)) / span, 0, 1);
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
    const span = max - min || 1;
    const dt = (dy / 150) / span;
    const nextT = clamp(t + dt, 0, 1);
    onChange(Math.exp(Math.log(min) + nextT * (Math.log(max) - Math.log(min))));
  };
  const up = () => setDragging(false);

  return (
    <div class="knob-unit flex flex-col items-center w-full gap-1 p-0">
      <div
        class={`knob relative size-[var(--ui-knob)] rounded-full bg-[radial-gradient(circle_at_34%_28%,#454956,#1a1b20_72%)] shadow-knob cursor-ns-resize touch-none ${dragging ? "dragging" : ""}`}
        style={{ "--angle": `${angle}deg` }}
        onPointerDown={down}
        onPointerMove={move}
        onPointerUp={up}
        onPointerCancel={up}
        onDblClick={() => defaultValue !== undefined && onChange(defaultValue)}
        // Keyboard: arrow up/down (and left/right) step the frequency by a
        // decade-fraction so a 20 Hz→2000 Hz knob is reachable both ways.
        onKeyDown={(e) => {
          if (e.key === "ArrowUp" || e.key === "ArrowRight") {
            e.preventDefault();
            const nextT = clamp(t + 0.04, 0, 1);
            onChange(Math.exp(Math.log(min) + nextT * (Math.log(max) - Math.log(min))));
          } else if (e.key === "ArrowDown" || e.key === "ArrowLeft") {
            e.preventDefault();
            const nextT = clamp(t - 0.04, 0, 1);
            onChange(Math.exp(Math.log(min) + nextT * (Math.log(max) - Math.log(min))));
          } else if (e.key === "Home") {
            e.preventDefault();
            onChange(min);
          } else if (e.key === "End") {
            e.preventDefault();
            onChange(max);
          } else if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onChange(defaultValue ?? value);
          }
        }}
        tabIndex={0}
        title={`${name}: ${display} — drag, arrows, Home/End, double-click to reset`}
        aria-label={`${name}: ${display}`}
      >
        <svg
          class="knob-arc absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 pointer-events-none"
          viewBox="0 0 64 64"
          width="64"
          height="64"
          aria-hidden="true"
        >
          <circle
            cx="32"
            cy="32"
            r={30}
            fill="none"
            stroke="#2f323b"
            stroke-width={4}
            pathLength={100}
            stroke-dasharray="75 100"
            transform={`rotate(135 32 32)`}
          />
          <circle
            cx="32"
            cy="32"
            r={30}
            fill="none"
            stroke="var(--amber)"
            stroke-width={4}
            stroke-linecap="round"
            pathLength={100}
            stroke-dasharray={`${t * 75} 100`}
            transform={`rotate(135 32 32)`}
          />
        </svg>
        <span
          class={`knob-indicator absolute left-1/2 top-[8.33%] h-[41.67%] w-[5.56%] origin-[50%_100%] rounded-sm shadow-[0_0_4px_rgba(0,0,0,0.8)] ${dragging ? "bg-amber" : "bg-[#f4f6fa]"}`}
          style={{ transform: `translateX(-50%) rotate(${angle}deg)` }}
        />
        <span
          class="knob-end lo absolute right-[calc(50%+var(--space-6))] top-[calc(50%+var(--space-6))] text-pico leading-none text-muted tabular-nums bg-well border border-line-soft rounded-xs pointer-events-none"
          style={{ padding: 0 }}
        >
          {endLabel(min)}
        </span>
        <span
          class="knob-end hi absolute left-[calc(50%+var(--space-6))] top-[calc(50%+var(--space-6))] text-pico leading-none text-muted tabular-nums bg-well border border-line-soft rounded-xs pointer-events-none"
          style={{ padding: 0 }}
        >
          {endLabel(max)}
        </span>
      </div>
      <div class="knob-name text-center text-2xs tracking-[0.8px] text-muted font-semibold uppercase">{name}</div>
      <div class="knob-value text-xs text-text bg-inset border border-line-soft rounded-sm min-w-[56px] text-center tabular-nums px-2">{display}</div>
    </div>
  );
}

/** Compact readout for the arc's end stops (min / max) — same convention as
 *  the flat Knob. */
function endLabel(v: number): string {
  const a = Math.abs(v);
  return a >= 100 ? v.toFixed(0) : a >= 10 ? v.toFixed(1) : v.toFixed(2);
}
