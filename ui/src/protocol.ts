// Wire types for docs/IPC-PROTOCOL.md. Shared by every shell.

export type Category =
  | "I/O"
  | "Split/Merge"
  | "Dynamics"
  | "Tone"
  | "Distortion"
  | "Modulation"
  | "Pitch"
  | "Time"
  | "Modelling"
  | "Utility"
  | "Control";

export interface ParamSpec {
  id: string;
  name: string;
  kind: "number" | "choice";
  min: number;
  max: number;
  default: number;
  unit: string;
  choices?: string[];
}

export interface BlockSpec {
  name: string;
  category: Category;
  params: ParamSpec[];
}

export interface Block {
  id: string;
  type: string;
  name: string;
  bypass: boolean;
  params: Record<string, number>;
}

export interface Channel {
  id: string;
  name: string;
  volume: number;
  sources: string[];
  blocks: Block[];
}

/** One end of a cross-channel routing: a block instance in a channel. */
export interface RoutingEndpoint {
  channel: string;
  block: string;
}

/**
 * A tap from one block's output to a block in another channel. Not an edge:
 * the source keeps its own downstream wire, and the destination sums the two
 * feeds. `gain` is linear.
 */
export interface Routing {
  id: string;
  source: RoutingEndpoint;
  dest: RoutingEndpoint;
  enabled: boolean;
  gain: number;
}

export interface State {
  /** Monotonic revision. Bumped by the engine on every rebuild; the UI drops
   *  snapshots that are not newer, so a late delivery cannot roll it back. */
  rev: number;
  preset: string;
  presets: string[];
  source: "riff" | "live" | "silence";
  registry: Record<string, BlockSpec>;
  channels: Channel[];
  routings: Routing[];
  /** Global input trim, dB, from the header knob. */
  inputGain: number;
  /** Global output volume, dB, from the header knob. */
  masterOutVolume: number;
}

export interface Meters {
  in: number;
  out: number;
  cpu: number;
  xruns: number;
  /** Per-band gain reduction for an active multiband block (absent otherwise). */
  bands?: ReadonlyArray<{ band: string; gr_db: number; out_db: number }>;
}

// --- Messages --------------------------------------------------------------

export interface Request {
  type: "req";
  id: number;
  cmd: string;
  args?: Record<string, unknown>;
}

export interface Response {
  type: "res";
  id: number;
  ok: boolean;
  payload?: unknown;
  error?: string;
}

export interface StateEvent {
  type: "event";
  event: "state";
  payload: State;
}

export interface MetersEvent {
  type: "event";
  event: "meters";
  payload: Meters;
}

export interface Tuner {
  active: boolean;
  freq: number;
  note: string;
  octave: number;
  cents: number;   // -50...+50
  clarity: number; // 0...1
  reference: number;
}

export interface TunerEvent {
  type: "event";
  event: "tuner";
  payload: Tuner;
}

export function isTuner(msg: ServerMessage): msg is TunerEvent {
  return msg.type === "event" && msg.event === "tuner";
}

export interface ErrorEvent {
  type: "event";
  event: "error";
  payload: { message: string };
}

export interface DownloadItem {
  id: string;
  status: "queued" | "running" | "done" | "failed" | "cancelled";
  received: number;
  total: number;
  percent: number; // 0...100
  local_path: string;
  error: string | null;
  retries: number;
}

export interface DownloadEvent {
  type: "event";
  event: "download";
  payload: DownloadItem;
}

export function isDownload(msg: ServerMessage): msg is DownloadEvent {
  return msg.type === "event" && msg.event === "download";
}

export type ServerMessage = Response | StateEvent | MetersEvent | TunerEvent | DownloadEvent | ErrorEvent;

export function isState(msg: ServerMessage): msg is StateEvent {
  return msg.type === "event" && msg.event === "state";
}

export function isMeters(msg: ServerMessage): msg is MetersEvent {
  return msg.type === "event" && msg.event === "meters";
}

export function isResponse(msg: ServerMessage): msg is Response {
  return msg.type === "res";
}

export function asState(payload: unknown): State {
  return payload as State;
}
