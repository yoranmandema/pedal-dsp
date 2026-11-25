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
  /** I/O buffer size, in frames (e.g. 128, 256, 512). */
  bufferFrames: number;
  /** Sample rate the engine is running at (48_000, 44_100, …). */
  sampleRate: number;
  /** One-way latency in ms, as measured by the engine. */
  oneWayLatencyMs: number;
  /** Round-trip latency in ms (the sum of input and output one-way latency). */
  roundTripLatencyMs: number;
  /** Looper transport state, 0..4 (Idle 0 / Recording 1 / Playing 2 /
   * Overdubbing 3 / Stopped 4). Sent on every state event; the UI reads it
   * for the banner and the trigger buttons. */
  looper_state?: number;
}

/** Engine's config endpoint response: the I/O buffer-size config and latency. */
export interface Config {
  bufferFrames: number;
  sampleRate: number;
  supported: number[];
  oneWayLatencyMs: number;
  roundTripLatencyMs: number;
}

export interface Meters {
  in: number;
  out: number;
  cpu: number;
  xruns: number;
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

export type ServerMessage = Response | StateEvent | MetersEvent | LooperEvent | TunerEvent | ErrorEvent | DownloadEvent;

export function isState(msg: ServerMessage): msg is StateEvent {
  return msg.type === "event" && msg.event === "state";
}

export function isMeters(msg: ServerMessage): msg is MetersEvent {
  return msg.type === "event" && msg.event === "meters";
}

export function isResponse(msg: ServerMessage): msg is Response {
  return msg.type === "res";
}

/** Looper position/state event, sent ~10 Hz while the looper is Playing or
 * Overdubbing. `state` is the engine's looper state (0..4); `frac` is the
 * fraction of the take already played (0..1). */
export interface LooperEvent {
  type: "event";
  event: "looper";
  payload: { state: number; frac: number };
}

export function isLooper(msg: ServerMessage): msg is LooperEvent {
  return msg.type === "event" && msg.event === "looper";
}

/** Model download item reported by the engine. */
export interface DownloadItem {
  /** Unique download id (UUID). */
  id: string;
  /** Model name or path. */
  name: string;
  /** One of: queued | progress | completed | paused | error | cancelled. */
  status: string;
  /** Progress fraction 0..1, or null when not applicable. */
  progress: number | null;
  /** Bytes received so far, or null. */
  receivedBytes: number | null;
  /** Bytes total, or null if unknown. */
  totalBytes: number | null;
  /** Resolved local file path (present when status is completed). */
  path: string | null;
  /** Error message (present when status is error). */
  error: string | null;
  /** Model index id (the key for the model index entry). */
  modelId: string;
}

/** Download status event, emitted whenever a download changes. */
export interface DownloadEvent {
  type: "event";
  event: "download";
  payload: DownloadItem;
}

export function isDownload(msg: ServerMessage): msg is DownloadEvent {
  return msg.type === "event" && msg.event === "download";
}

export function asState(payload: unknown): State {
  return payload as State;
}
