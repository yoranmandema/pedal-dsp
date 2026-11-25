// Model record types and query/facet types for the model browser.
//
// The shapes mirror docs/model-index.schema.json exactly. The engine owns the
// index and answers every browsing query over it (listModels, getModel,
// refreshIndex); the UI never holds its own copy of the index.

// --- Wire (frozen) enums -------------------------------------------

/** `sort` argument to `listModels`. */
export type Sort =
  | "relevance"
  | "name"
  | "author"
  | "newest"
  | "downloads"
  | "favorites"
  | "updated"
  | "added";

/** `kinds[]` argument. */
export type Kind = "amp" | "pedal" | "outboard" | "ir" | "fullrig" | "other";

/** `cabIr[]` argument. */
export type CabIr = "with-cab" | "without-cab" | "ir-only";

/** `formats[]` argument. */
export type Format = "nam" | "ir";

/** `states[]` argument. */
export type ModelState = "all" | "downloaded" | "not-downloaded" | "stale";

// --- Records ---------------------------------------------------------

export interface Author {
  name: string;
  username?: string;
  url?: string;
  avatar_url?: string;
  verified?: boolean;
}

export interface Stats {
  favorites: number;
  downloads: number;
}

export interface Metadata {
  name: string;
  tone_title?: string;
  author: Author;
  description?: string;
  kind: Kind;
  format: Format;
  variant: "a1" | "a2" | "custom";
  with_cab?: boolean;
  manufacturer?: string | null;
  gear_model?: string | null;
  cab?: string | null;
  mics?: string | null;
  tags: string[];
  sample_rate?: number | null;
  loudness_db?: number | null;
  gain_db?: number | null;
  license?: string | null;
  links: string[];
  thumbnail?: string | null;
  stats?: Stats;
  published_at?: string;
  updated_at?: string;
}

export interface LocalState {
  file_name: string;
  size_bytes: number;
  checksum: string; // "sha256:…"
  downloaded_at: string; // RFC3339
  stale: boolean;
}

export interface ModelRecord {
  id: string;
  provider: string;
  tone_id: string;
  source_url: string;
  download_url?: string | null;
  content_hash?: string | null;
  fetched_at: string;
  local_path?: string | null;
  local?: LocalState | null;
  metadata: Metadata;
}

export interface ModelIndex {
  schema: 1;
  provider: string;
  generated_at: string;
  count: number;
  models: ModelRecord[];
}

export interface Facet {
  value: string;
  count: number;
}

export interface Facets {
  kind?: Facet[];
  manufacturer?: Facet[];
  cabIr?: Facet[];
  tags?: Facet[];
  format?: Facet[];
  state?: Facet[];
}

export interface ModelPage {
  total: number;
  page: number;
  pageSize: number;
  models: ModelRecord[];
  facets: Facets;
}

/** Top-level `listModels` request. */
export interface ListModelsArgs {
  query?: string;
  kinds: Kind[];
  manufacturers: string[];
  cabIr: CabIr[];
  tags: string[];
  formats: Format[];
  states: ModelState[];
  sort: Sort;
  page: number;
  pageSize: number;
}

// --- Helpers -------------------------------------------------------

/** The fields a search token may match, in ranking order (heavier = first). */
export const SEARCH_FIELDS: Array<
  keyof Pick<Metadata, "name" | "tone_title" | "author" | "manufacturer" | "gear_model" | "tags">
> = ["name", "tone_title", "author", "manufacturer", "gear_model", "tags"];

/** Resolved author name: author.name, or username if the name is absent. */
export function authorName(m: ModelRecord): string {
  const a = m.metadata.author;
  return a.name || a.username || "";
}

/** All author text to search: name + username. */
export function authorText(m: ModelRecord): string {
  const a = m.metadata.author;
  const u = a.username && a.username.toLowerCase() !== (a.name ?? "").toLowerCase() ? a.username : "";
  return [a.name, u].filter(Boolean).join(" · ");
}

/** Top-3 tags for the list row, lowercase. */
export function topTags(m: ModelRecord): string[] {
  return m.metadata.tags.slice(0, 3);
}

/** Normalized slug for a model id, for safe display/download naming. */
export function modelSlug(id: string): string {
  return id.replace(/[^a-z0-9._-]+/g, "-").replace(/-+/g, "-").replace(/^-|-$/g, "");
}

/** Prefix of `id` before the first ":" (the provider) — stable across refreshes. */
export function modelProvider(id: string): string {
  return id.split(":")[0];
}

/** Flag indicating a model is not downloaded. */
export function isDownloaded(m: ModelRecord): boolean {
  return !!m.local_path;
}

/** Staleness of a locally cached model. */
export function isStale(m: ModelRecord): boolean {
  return m.local?.stale === true;
}

/** Human-readable file size. */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** Default page size. */
export const PAGE_SIZE = 100;

/** Ceiling enforced on the server. */
export const MAX_PAGE_SIZE = 500;
