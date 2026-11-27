// Block theme registry — types, palettes, lookup and fallback.

import type { Category } from "./protocol";

// --- Types -------------------------------------------------------------------

export type Material = "metal" | "wood" | "plastic" | "glass" | "paper";

export interface BlockPalette {
  base: string;
  surface: string;
  bg: string;
  line: string;
  lineSoft: string;
  lineHi: string;
  accent: string;
  accentSoft: string;
  text: string;
  textMuted: string;
  control: string;
  controlOff: string;
  material: Material;
}

export type BlockAnchorPos = "centre" | "bottom-right" | "top-left" | "bottom-left" | "top-right";

export interface BlockAnchor {
  x: number;
  y: number;
  anchor?: BlockAnchorPos;
}

export interface BlockTheme {
  palette: BlockPalette;
  artwork?: string;
  artworkState?: Record<"on" | "off", string>;
  anchors?: Record<string, BlockAnchor>;
  vars?: Partial<Record<BlockVar, string>>;
}

export const enum BlockVar {
  hue = "--block-h",
  material = "--block-material",
  state = "--block-state",
  palette = "--block-palette",
}

export interface BlockThemeRegistry {
  get(type: string): BlockTheme | undefined;
  byCategory(category: Category): BlockTheme;
  fallback(type: string, category: Category): BlockTheme;
}

// --- Constants ---------------------------------------------------------------

const CATEGORY_HUE: Record<Category, number> = {
  Modelling: 268,
  Distortion: 10,
  Tone: 42,
  Dynamics: 152,
  Time: 206,
  Modulation: 306,
  Pitch: 186,
  Utility: 222,
  "I/O": 0,
  "Split/Merge": 96,
  Control: 24,
};

// --- Helpers -----------------------------------------------------------------

export function categoryHue(category: Category): number {
  return CATEGORY_HUE[category] ?? 220;
}

/** Build a dark coherent palette from a single hue. */
export function buildPalette(hue: number): BlockPalette {
  return {
    base: `hsl(${hue}, 35%, 12%)`,
    surface: `hsl(${hue}, 40%, 8%)`,
    bg: `hsl(${hue}, 45%, 6%)`,
    line: `hsl(${hue}, 60%, 70%)`,
    lineSoft: `hsl(${hue}, 50%, 50%)`,
    lineHi: `hsl(${hue}, 65%, 80%)`,
    accent: `hsl(${hue}, 80%, 55%)`,
    accentSoft: `hsl(${hue}, 60%, 35%)`,
    text: `hsl(${hue}, 20%, 8%)`,
    textMuted: `hsl(${hue}, 50%, 70%)`,
    control: `hsl(${hue}, 65%, 70%)`,
    controlOff: `hsl(${hue}, 40%, 35%)`,
    material: "metal",
  };
}

// Hex ↔ HSL helpers for paletteFromHex

function hexToRgb(hex: string): [number, number, number] {
  const m = /^#?([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex);
  if (!m) throw new Error(`Invalid hex: ${hex}`);
  return [parseInt(m[1], 16), parseInt(m[2], 16), parseInt(m[3], 16)];
}

function rgbToHsl(r: number, g: number, b: number): [number, number, number] {
  r /= 255; g /= 255; b /= 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b);
  let h = 0, s = 0;
  const l = (max + min) / 2;
  if (max !== min) {
    const d = max - min;
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    switch (max) {
      case r: h = ((g - b) / d + (g < b ? 6 : 0)) * 60; break;
      case g: h = ((b - r) / d + 2) * 60; break;
      case b: h = ((r - g) / d + 4) * 60; break;
    }
  }
  return [Math.round(h), Math.round(s * 100), Math.round(l * 100)];
}

function hslToCss(h: number, s: number, l: number): string {
  return `hsl(${h}, ${s}%, ${l}%)`;
}

/** Build a palette from a base hex colour using fixed HSL offsets. */
export function paletteFromHex(baseHex: string, _material: Material): BlockPalette {
  const [r, g, b] = hexToRgb(baseHex);
  const [h, s, l] = rgbToHsl(r, g, b);

  const lineSh = Math.min(s + 20, 100);
  const lineSl = Math.min(l + 58, 100);
  const lineSoftSl = Math.min(l + 38, 100);
  const lineHiSl = Math.min(l + 78, 100);
  const accentS = Math.min(s + 40, 100);
  const accentL = Math.min(l + 30, 100);
  const accentSoftL = Math.max(l - 20, 0);
  const textMutedL = Math.min(l + 40, 100);
  const controlL = Math.min(l + 78, 100);
  const controlOffL = Math.max(l - 18, 0);

  return {
    base: baseHex,
    surface: hslToCss(h, s, Math.max(l - 4, 0)),
    bg: hslToCss(h, s, Math.max(l - 6, 0)),
    line: hslToCss(h, lineSh, lineSl),
    lineSoft: hslToCss(h, Math.min(s + 10, 100), lineSoftSl),
    lineHi: hslToCss(h, Math.min(s + 20, 100), lineHiSl),
    accent: hslToCss(h, accentS, accentL),
    accentSoft: hslToCss(h, accentS, accentSoftL),
    text: baseHex,
    textMuted: hslToCss(h, Math.min(s + 20, 100), textMutedL),
    control: hslToCss(h, Math.min(s + 20, 100), controlL),
    controlOff: hslToCss(h, Math.min(s + 10, 100), controlOffL),
    material: _material,
  };
}

// --- Registry entries --------------------------------------------------------

const ENTRIES: [string, Category, BlockPalette, Material, Record<string, BlockAnchor>?][] = [
  [
    "amp.basic",
    "Modelling",
    {
      base: "#241a12", surface: "#1a120c", bg: "#151008",
      line: "#d8b46b", lineSoft: "#a88352", lineHi: "#f0c886",
      accent: "#e8a33d", accentSoft: "#8a5f2a",
      text: "#1a120c", textMuted: "#d8b46b",
      control: "#f0c886", controlOff: "#5a3a1c",
      material: "metal",
    },
    "metal",
    {
      voice: { x: 17, y: 74 },
      gain: { x: 33, y: 74 },
      bass: { x: 50, y: 74 },
      mid: { x: 67, y: 74 },
      treble: { x: 83, y: 74 },
      level: { x: 50, y: 88, anchor: "bottom-right" },
    },
  ],
  [
    "distortion.stompbox",
    "Distortion",
    {
      base: "#1c1214", surface: "#140c0e", bg: "#0f080c",
      line: "#c8566a", lineSoft: "#a34556", lineHi: "#e8849a",
      accent: "#d45a74", accentSoft: "#7a3040",
      text: "#140c0e", textMuted: "#c8566a",
      control: "#e8849a", controlOff: "#5a2434",
      material: "metal",
    },
    "metal",
  ],
  [
    "cab.basic",
    "Modelling",
    {
      base: "#181818", surface: "#101010", bg: "#0a0a0a",
      line: "#999", lineSoft: "#666", lineHi: "#bbb",
      accent: "#8888cc", accentSoft: "#444466",
      text: "#101010", textMuted: "#999",
      control: "#bbb", controlOff: "#444",
      material: "wood",
    },
    "wood",
  ],
  [
    "noiseGate.drum",
    "Dynamics",
    {
      base: "#141a14", surface: "#0e140e", bg: "#080e08",
      line: "#6bba6b", lineSoft: "#4a9a4a", lineHi: "#84d884",
      accent: "#5ab85a", accentSoft: "#2a6a2a",
      text: "#0e140e", textMuted: "#6bba6b",
      control: "#84d884", controlOff: "#2a5a2a",
      material: "metal",
    },
    "metal",
  ],
  [
    "pitch.shift",
    "Pitch",
    {
      base: "#14181c", surface: "#0c1014", bg: "#080a0e",
      line: "#6ba8c8", lineSoft: "#4a88a8", lineHi: "#84c8e8",
      accent: "#48c8e8", accentSoft: "#286898",
      text: "#0c1014", textMuted: "#6ba8c8",
      control: "#84c8e8", controlOff: "#285878",
      material: "metal",
    },
    "metal",
  ],
  [
    "modulation.chorus",
    "Modulation",
    {
      base: "#181418", surface: "#100e14", bg: "#0e080e",
      line: "#b86ab8", lineSoft: "#984a98", lineHi: "#d884d8",
      accent: "#d848d8", accentSoft: "#882888",
      text: "#100e14", textMuted: "#b86ab8",
      control: "#d884d8", controlOff: "#682868",
      material: "metal",
    },
    "metal",
  ],
  [
    "dynamics.multiband_compressor",
    "Dynamics",
    {
      base: "#161a18", surface: "#0e1410", bg: "#080e0c",
      line: "#78b888", lineSoft: "#58a868", lineHi: "#94d8a4",
      accent: "#48c888", accentSoft: "#287848",
      text: "#0e1410", textMuted: "#78b888",
      control: "#94d8a4", controlOff: "#286838",
      material: "metal",
    },
    "metal",
  ],
  [
    "modulation.phaser",
    "Modulation",
    {
      base: "#18141c", surface: "#141014", bg: "#0e0a0e",
      line: "#a86ab8", lineSoft: "#884a98", lineHi: "#c884d8",
      accent: "#d848c8", accentSoft: "#882880",
      text: "#141014", textMuted: "#a86ab8",
      control: "#c884d8", controlOff: "#682870",
      material: "metal",
    },
    "metal",
  ],
  [
    "time.delay",
    "Time",
    {
      base: "#161818", surface: "#0e1010", bg: "#080a0a",
      line: "#78a8a8", lineSoft: "#588888", lineHi: "#94c8c8",
      accent: "#48a8a8", accentSoft: "#287878",
      text: "#0e1010", textMuted: "#78a8a8",
      control: "#94c8c8", controlOff: "#287878",
      material: "metal",
    },
    "metal",
  ],
  [
    "time.reverb",
    "Time",
    {
      base: "#16181a", surface: "#0e1014", bg: "#080a0e",
      line: "#8888b8", lineSoft: "#686898", lineHi: "#a4a4d8",
      accent: "#8888d8", accentSoft: "#484888",
      text: "#0e1014", textMuted: "#8888b8",
      control: "#a4a4d8", controlOff: "#484888",
      material: "glass",
    },
    "glass",
  ],
  [
    "pitch.whammy",
    "Pitch",
    {
      base: "#18161c", surface: "#101014", bg: "#0a0a0e",
      line: "#c86a98", lineSoft: "#a84a88", lineHi: "#e884b8",
      accent: "#d848a8", accentSoft: "#882878",
      text: "#101014", textMuted: "#c86a98",
      control: "#e884b8", controlOff: "#682878",
      material: "metal",
    },
    "metal",
  ],
  [
    "utility.noise",
    "Utility",
    {
      base: "#181818", surface: "#141414", bg: "#0e0e0e",
      line: "#999", lineSoft: "#777", lineHi: "#bbb",
      accent: "#aaa", accentSoft: "#555",
      text: "#141414", textMuted: "#999",
      control: "#bbb", controlOff: "#555",
      material: "metal",
    },
    "metal",
  ],
  [
    "tone.eq_parametric",
    "Tone",
    {
      base: "#181814", surface: "#141410", bg: "#0e0e0a",
      line: "#b8b068", lineSoft: "#989058", lineHi: "#d8d084",
      accent: "#c8b848", accentSoft: "#786a28",
      text: "#141410", textMuted: "#b8b068",
      control: "#d8d084", controlOff: "#6a6028",
      material: "metal",
    },
    "metal",
  ],
  [
    "tone.eq_graphic",
    "Tone",
    {
      base: "#181814", surface: "#141410", bg: "#0e0e0a",
      line: "#b8b068", lineSoft: "#989058", lineHi: "#d8d084",
      accent: "#c8b848", accentSoft: "#786a28",
      text: "#141410", textMuted: "#b8b068",
      control: "#d8d084", controlOff: "#6a6028",
      material: "metal",
    },
    "metal",
  ],
];

// --- Registry class ----------------------------------------------------------

function makeTheme(category: Category, palette: BlockPalette, material: Material, anchors?: Record<string, BlockAnchor>): BlockTheme {
  return {
    palette,
    anchors,
    vars: {
      [BlockVar.hue]: `--block-${categoryHue(category)}`,
      [BlockVar.material]: `--block-${material}`,
      [BlockVar.state]: "--block-state",
      [BlockVar.palette]: `--block-palette-${categoryHue(category)}`,
    },
  };
}

export class ThemeRegistry implements BlockThemeRegistry {
  private _map = new Map<string, BlockTheme>();
  private _categories = new Map<Category, BlockTheme>();

  constructor(entries?: [string, Category, BlockPalette, Material, Record<string, BlockAnchor>?][]) {
    for (const [type, category, palette, material, anchors] of entries ?? ENTRIES) {
      const theme = makeTheme(category, palette, material, anchors);
      this._map.set(type, theme);
      if (!this._categories.has(category)) {
        this._categories.set(category, theme);
      }
    }
  }

  get(type: string): BlockTheme | undefined {
    return this._map.get(type);
  }

  byCategory(category: Category): BlockTheme {
    return this._categories.get(category) ?? this.fallback("", category);
  }

  fallback(_type: string, category: Category): BlockTheme {
    const palette = buildPalette(categoryHue(category));
    return makeTheme(category, palette, palette.material);
  }
}

// --- Public API --------------------------------------------------------------

export const registry = new ThemeRegistry();

export function blockTheme(type: string, category: Category): BlockTheme {
  return registry.get(type) ?? registry.byCategory(category) ?? registry.fallback(type, category);
}
