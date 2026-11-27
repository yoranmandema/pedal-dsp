import { useRef, useState } from "preact/hooks";

import { openGainStaging } from "../state";

interface HeaderKnobProps {
  label: string;
  value: number;
  min: number;
  max: number;
  defaultValue?: number;
  onChange: (v: number) => void;
  /** When true, render a compact "GS" trigger button beside the knob. */
  gsTrigger?: boolean;
}

/**
 * A compact rotary control for the 58 px top bar.
 *
 * Vertical drag maps 150 px to the full value span; double-click resets to
 * `defaultValue`. It is a full `role="slider"` with arrow-key handling, unlike
 * the editor's `Knob` which lives in the rack. `ui/src/components/Knob.tsx` is
 * deliberately left untouched.
 */
export function HeaderKnob({
  label,
  value,
  min,
  max,
  defaultValue,
  onChange,
  gsTrigger,
}: HeaderKnobProps) {
  const [dragging, setDragging] = useState(false);
  const start = useRef({ y: 0, v: 0 });

  // Geometry: t is the normalised position, angle sweeps -135 .. +135 deg.
  const t = (value - min) / (max - min);
  const angle = -135 + t * 270;

  const down = (e: PointerEvent) => {
    e.preventDefault();
    (e.target as Element).setPointerCapture?.(e.pointerId);
    start.current = { y: e.clientY, v: value };
    setDragging(true);
  };

  const move = (e: PointerEvent) => {
    if (!dragging) return;
    const next = start.current.v + ((start.current.y - e.clientY) / 150) * (max - min);
    onChange(Math.min(max, Math.max(min, next)));
  };

  const up = () => setDragging(false);

  const adjust = (delta: number) => {
    onChange(Math.min(max, Math.max(min, value + delta)));
  };

  return (
    <div
      class="header-knob flex flex-none items-center gap-2 compact:gap-0"
      tabIndex={0}
      role="slider"
      aria-label={label}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={value}
      title={`${label}: ${value.toFixed(1)} dB — drag, double-click to reset`}
      onKeyDown={(e) => {
        switch (e.key) {
          case "ArrowUp":
          case "ArrowRight":
            adjust(0.5);
            e.preventDefault();
            break;
          case "ArrowDown":
          case "ArrowLeft":
            adjust(-0.5);
            e.preventDefault();
            break;
          case "Home":
            onChange(min);
            e.preventDefault();
            break;
          case "End":
            onChange(max);
            e.preventDefault();
            break;
        }
      }}
    >
      {/* GS trigger — dark, subtle, rounded; 1.05× scale on hover. */}
      {gsTrigger && (
        <button
          class="flex items-center justify-center rounded-md bg-[#2f323b] px-1.5 py-1 text-[9px] font-bold tracking-widest text-muted transition-transform hover:scale-[1.05]"
          onClick={() => openGainStaging()}
          title="Gain Staging"
        >
          GS
        </button>
      )}
      <div
        class="knob relative size-[var(--ui-touch)] rounded-full bg-[radial-gradient(circle_at_34%_28%,#454956,#1a1b20_72%)] shadow-knob cursor-ns-resize touch-none"
        style={{ "--angle": `${angle}deg` }}
        onPointerDown={down}
        onPointerMove={move}
        onPointerUp={up}
        onPointerCancel={up}
        onDblClick={() => defaultValue !== undefined && onChange(defaultValue)}
      >
        {/* The arc sits centred on the face; pathLength=100 so the dash numbers
            are percentages of the circumference, and 75 = the 270deg window. */}
        <svg
          class="pointer-events-none absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2"
          viewBox="0 0 64 64"
          width={56}
          height={56}
          aria-hidden="true"
        >
          <circle
            cx="32"
            cy="32"
            r={22}
            fill="none"
            stroke="#2f323b"
            stroke-width={5}
            pathLength={100}
            stroke-dasharray="75 100"
            transform="rotate(135 32 32)"
          />
          <circle
            cx="32"
            cy="32"
            r={22}
            fill="none"
            stroke="var(--amber)"
            stroke-width={5}
            stroke-linecap="round"
            pathLength={100}
            stroke-dasharray={`${t * 75} 100`}
            transform="rotate(135 32 32)"
          />
        </svg>
        {/* Same indicator geometry as `Knob.tsx`, so it rotates about the face
            centre; it is inside the face, and the svg keeps it out of the
            pointer path. */}
        <span
          class={`knob-indicator absolute left-1/2 top-[9.26%] h-[33.33%] w-[5.56%] -translate-x-1/2 origin-[50%_122.22%] rounded-sm shadow-[0_0_4px_rgba(0,0,0,0.8)] ${dragging ? "bg-amber" : "bg-[#f4f6fa]"}`}
          style={{ transform: `translateX(-50%) rotate(${angle}deg)` }}
        />
        {/* Label and readback sit beside the face; they collapse out below
            820 px (the `compact` variant) so only the face remains. */}
        <span class="flex flex-col gap-0.5 leading-none compact:hidden pointer-events-none">
          <span class="text-[9px] tracking-[0.6px] text-muted font-semibold uppercase">
            {label}
          </span>
          <span class="text-[11px] tabular-nums text-text">
            {value.toFixed(1)} dB
          </span>
        </span>
      </div>
    </div>
  );
}
