// Stroke-drawn glyphs for the block registry, on a 24x24 grid.
//
// Two levels:
//   * `GLYPHS`      — one glyph per category. The fallback, and what rack
//                    headings use.
//   * `BLOCK_GLYPHS` — a per-block override for a block whose sound is more
//                    specific than its category (a cab is not an amp, a pitch
//                    delay is not a clock, a NAM is a network). A block that *is*
//                    the archetype of its category inherits the category glyph.
//
// Everything is inline SVG, `fill="none"`, `currentColor`, round caps, so a
// glyph inherits the category hue from its parent and stays crisp at any size.
// No emoji, no font glyphs: the shapes are drawn, not typed.

import type { JSX } from "preact";
import type { Category } from "../protocol";

const GLYPHS: Record<Category, JSX.Element> = {
  // Amp head over a speaker.
  Modelling: (
    <>
      <rect x="3" y="4" width="18" height="16" rx="2" />
      <circle cx="12" cy="14" r="3.4" />
      <path d="M7 7.5h3M7 10h3" />
    </>
  ),
  // A sine wave clipped flat at the top.
  Distortion: <path d="M3 12h3l2.5-7 3 14 2.5-11 2.5 7H21" />,
  // Three EQ faders.
  Tone: (
    <>
      <path d="M6 4v16M12 4v16M18 4v16" />
      <circle cx="6" cy="9" r="2" />
      <circle cx="12" cy="14" r="2" />
      <circle cx="18" cy="7" r="2" />
    </>
  ),
  // Compressor knee.
  Dynamics: (
    <>
      <path d="M3 19V5" />
      <path d="M3 19h9l4-9h5" />
    </>
  ),
  // Clock face.
  Time: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 7.5V12l3 2" />
    </>
  ),
  // Sine wave with a wobble.
  Modulation: <path d="M2.5 12c2-6.5 4-6.5 6 0s4 6.5 6 0 4-6.5 6 0" />,
  // Rising arrow.
  Pitch: (
    <>
      <path d="M4 18l5-6.5 4 3.5L20 7" />
      <path d="M16.5 7H20v3.5" />
    </>
  ),
  // Gear.
  Utility: (
    <>
      <circle cx="12" cy="12" r="3.2" />
      <path d="M12 3.5v2.6M12 17.9v2.6M3.5 12h2.6M17.9 12h2.6M6 6l1.8 1.8M16.2 16.2L18 18M18 6l-1.8 1.8M7.8 16.2L6 18" />
    </>
  ),
  // In / out arrows.
  "I/O": <path d="M4 8h9l-3-3M20 16H11l3 3" />,
  // One line splitting into two.
  "Split/Merge": (
    <>
      <path d="M12 3v7" />
      <path d="M12 10l-6.5 6.5M12 10l6.5 6.5" />
      <path d="M5.5 16.5V20M18.5 16.5V20" />
    </>
  ),
  // Slider with a handle.
  Control: (
    <>
      <path d="M5 19V5M19 19V5" />
      <path d="M5 12h14" />
      <circle cx="12" cy="12" r="2" />
    </>
  ),
};

/** Per-block overrides, keyed by block type id. */
const BLOCK_GLYPHS: Partial<Record<string, JSX.Element>> = {
  // Level meter: four bars climbing.
  "util.gain": <path d="M4 15v4M9 11v8M14 7v12M19 3v16" />,
  // Master control: a knob with cardinal ticks.
  "util.master": (
    <>
      <circle cx="12" cy="12" r="7" />
      <path d="M12 12l4.2-4.2" />
      <path d="M12 2.8v1.4M12 19.8v1.4M2.8 12h1.4M19.8 12h1.4" />
    </>
  ),
  // Two channels merging into one.
  "util.mix": <path d="M4 8h5l5 4h6M4 16h5l5-4" />,
  // Adder: a sum in a ring.
  "util.sum": (
    <>
      <circle cx="12" cy="12" r="7.5" />
      <path d="M12 8.5v7M8.5 12h7" />
    </>
  ),
  // Three-band stack: sliders with solid handles.
  "tone.stack": (
    <>
      <path d="M6 4v16M12 4v16M18 4v16" />
      <rect x="4" y="8" width="4" height="2.6" rx="1.3" fill="currentColor" stroke="none" />
      <rect x="10" y="13" width="4" height="2.6" rx="1.3" fill="currentColor" stroke="none" />
      <rect x="16" y="6" width="4" height="2.6" rx="1.3" fill="currentColor" stroke="none" />
    </>
  ),
  // Soft clip: the top of the wave flattens.
  "dist.overdrive": <path d="M3 12h2l2.5-7 3 14 2.5-11 2.5 9H21" />,
  // Three echoes, each quieter than the last.
  "time.delay": (
    <>
      <path d="M3 13c1.6-4.5 3.2-4.5 4.8 0s3.2 4.5 4.8 0" />
      <path d="M9.5 13c1.6-4.5 3.2-4.5 4.8 0s3.2 4.5 4.8 0" stroke-opacity="0.55" />
      <path d="M16 13c1.6-4.5 3.2-4.5 4.8 0s3.2 4.5 4.8 0" stroke-opacity="0.28" />
    </>
  ),
  // Diffusion: a source and the reflections spreading out of it.
  "time.reverb": (
    <>
      <circle cx="5" cy="12" r="1.5" fill="currentColor" stroke="none" />
      <path d="M10 8a5 5 0 0 1 0 8" />
      <path d="M14.5 4.5a10 10 0 0 1 0 15" />
    </>
  ),
  // Pitch climbing a staircase.
  "time.pitchdelay": <path d="M4 19h4v-4h4v-4h4V7h4M16.5 7H20v3.5" />,
  // Amp head: the category archetype.
  "amp.basic": (
    <>
      <rect x="3" y="4" width="18" height="16" rx="2" />
      <circle cx="12" cy="14" r="3.4" />
      <path d="M7 7.5h3M7 10h3" />
    </>
  ),
  // Speaker cabinet: one cone in a box.
  "cab.sim": (
    <>
      <rect x="3" y="5" width="18" height="14" rx="2" />
      <circle cx="12" cy="12" r="4.5" />
      <circle cx="12" cy="12" r="1.1" fill="currentColor" stroke="none" />
    </>
  ),
  // A jack: an arrow into a terminal.
  "io.input": <path d="M3 12h9M9 7l5 5-5 5M18 5v14" />,
  // A jack: an arrow out of a terminal.
  "io.output": <path d="M6 5v14M9 12h9M14 7l5 5-5 5" />,
  // A small network: two in, a hidden node, two out.
  "mod.nam": (
    <>
      <circle cx="5.5" cy="7.5" r="1.7" />
      <circle cx="5.5" cy="16.5" r="1.7" />
      <circle cx="12" cy="12" r="1.7" />
      <circle cx="18.5" cy="7.5" r="1.7" />
      <circle cx="18.5" cy="16.5" r="1.7" />
      <path d="M7.2 8.2l3.2 2.6M7.2 15.8l3.2-2.6M13.8 10.9l2.9-2.4M13.8 13.1l2.9 2.4" />
    </>
  ),
};

// Mapping of every block type the engine registry offers to its glyph.
//
// 14 of the 17 registry block types carry an explicit glyph in `BLOCK_GLYPHS`;
// the remaining 3 inherit their category's glyph from `GLYPHS`.
//
//   Block type          | Category      | Glyph
//   -------------------- | ------------- | ----------------------------
//   io.input            | I/O           | jack arrow into a terminal
//   io.output           | I/O           | jack arrow out of a terminal
//   util.gain           | Utility       | level meter, 4 bars climbing
//   util.mix            | Utility       | two streams merging into one
//   util.sum            | Split/Merge   | two streams merging into one
//   util.master         | Utility       | master knob, circle + cardinal ticks
//   dist.overdrive      | Distortion    | soft clip, flattened wave
//   amp.basic           | Modelling     | amp head
//   cab.sim             | Modelling     | speaker cabinet
//   tone.stack          | Tone          | three-band EQ stack
//   time.delay          | Time          | three echoes climbing
//   time.pitchdelay     | Pitch         | pitch climbing a staircase
//   time.reverb         | Time          | diffusion: source + expanding arcs
//   mod.nam             | Modelling     | network: 5 nodes + links
//   dynamics.compressor | Dynamics      | category default: compressor knee
//   mod.tremolo         | Modulation    | category default: modulation LFO
//   pitch.octaver       | Pitch         | category default: rising arrow
//
// All glyphs share one stroke/fill style: 24x24 viewBox, fill="none",
// currentColor, stroke-width 1.7, round caps/joins, no emoji and no text, so the
// set stays coherent at 16 px and 32 px and every glyph inherits the block's
// category hue from the parent.

function Svg({
  size,
  children,
}: {
  size: number;
  children: JSX.Element | JSX.Element[];
}) {
  return (
    <svg
      class="icon"
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width={1.7}
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      {children}
    </svg>
  );
}

/** A category glyph. `size` is the square edge in px. */
export function Icon({ category, size = 16 }: { category: Category; size?: number }) {
  return <Svg size={size}>{GLYPHS[category] ?? GLYPHS.Utility}</Svg>;
}

/**
 * A block glyph: the block's own drawing when it has one, otherwise its
 * category's. `type` is the registry id (`amp.basic`), `category` the fallback.
 */
export function BlockIcon({
  type,
  category,
  size = 16,
}: {
  type: string;
  category: Category;
  size?: number;
}) {
  return <Svg size={size}>{BLOCK_GLYPHS[type] ?? GLYPHS[category] ?? GLYPHS.Utility}</Svg>;
}

/** Power symbol — the bypass switch. */
export function PowerIcon({ size = 11 }: { size?: number }) {
  return (
    <Svg size={size}>
      <path d="M12 3.5V10" />
      <path d="M8.2 6.2a5.5 5.5 0 1 0 7.6 0" />
    </Svg>
  );
}

/** A close/cross. */
export function CloseIcon({ size = 10 }: { size?: number }) {
  return (
    <Svg size={size}>
      <path d="M6.5 6.5l11 11M17.5 6.5l-11 11" />
    </Svg>
  );
}

/** An add/plus. */
export function PlusIcon({ size = 16 }: { size?: number }) {
  return (
    <Svg size={size}>
      <path d="M12 5v14M5 12h14" />
    </Svg>
  );
}

/** A tuning fork: two prongs from y≈4 to y≈13 joined by a crossbar at y≈13,
 *  and a stem from (12,13) down to (12,19). */
export function TunerIcon({ size = 16 }: { size?: number }) {
  return (
    <Svg size={size}>
      <path d="M6 4v9M18 4v9" />
      <path d="M6 13h12" />
      <path d="M12 13v6" />
    </Svg>
  );
}

/** Speaker with a sound wave: the volume indicator. */
export function SpeakerIcon({ size = 11 }: { size?: number }) {
  return (
    <Svg size={size}>
      <path d="M4 9v6h4l5 4V5L8 9H4z" />
      <path d="M15.5 9.5a3.5 3.5 0 0 1 0 5" />
    </Svg>
  );
}

/** Input source: an arrow pointing into a terminal — reuses the `io.input` glyph. */
export function SourceIcon({ size = 11 }: { size?: number }) {
  return <Svg size={size}>{BLOCK_GLYPHS["io.input"] ?? GLYPHS.Utility}</Svg>;
}

/** Jack socket endpoint: a filled dot in a ring. */
export function PortIcon({ size = 11 }: { size?: number }) {
  return (
    <Svg size={size}>
      <circle cx="12" cy="12" r="3.2" fill="currentColor" stroke="none" />
      <circle cx="12" cy="12" r="7" stroke-opacity="0.45" />
    </Svg>
  );
}

