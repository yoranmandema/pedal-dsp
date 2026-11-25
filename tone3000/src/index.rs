//! The cached model index: typed records, JSON-Schema validation, atomic
//! persistence, and the refresh builder.
//!
//! Contracts implemented here:
//!
//! * `docs/MODEL-BROWSE-AND-CACHE.md` §9.1 (paths), §9.2 (download layout),
//!   §9.3 (record schema), §9.4 (refresh/merge/staleness).
//! * `docs/model-index.schema.json` (embedded verbatim and used to validate
//!   every candidate index before it is written).
//!
//! # Sources and terms
//!
//! [`Source::FreeFeeds`] uses only the public/bounded feeds (`trending`,
//! `latest`) and is the default. [`Source::Catalog`] walks the whole
//! `/tones/search` catalog, which the TONE3000 free-tier terms forbid caching;
//! it therefore requires an explicit [`RefreshConfig::catalog_agreement`].

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::api::{Architecture, Format as ApiFormat, Gear, Model, Tone};
use crate::client::Client;
use crate::paths::Paths;

/// The agreed JSON Schema, embedded so validation never touches the network.
pub const INDEX_SCHEMA: &str = include_str!("../../docs/model-index.schema.json");
/// The agreed worked example, used by tests as a known-good index.
pub const EXAMPLE_INDEX: &str = include_str!("../../docs/example-models.json");

const SCHEMA_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    #[error("index does not match the schema: {0}")]
    Invalid(String),
    #[error("embedded schema could not be compiled: {0}")]
    Compile(String),
    #[error("could not serialize the index: {0}")]
    Serialize(#[from] serde_json::Error),
    /// A query string could not be encoded (a key or value was not valid).
    #[error("could not encode the request URL: {0}")]
    Encode(String),
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Api(#[from] crate::client::Error),
    #[error(transparent)]
    Schema(#[from] SchemaError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid index: {0}")]
    Invalid(String),
    #[error("could not serialize the index: {0}")]
    Serialize(#[from] serde_json::Error),
}

// ---------------------------------------------------------------------------
// Timestamps
// ---------------------------------------------------------------------------

/// Current time as RFC 3339 UTC with a `Z` suffix, e.g. `2026-11-15T09:00:00Z`.
///
/// Sub-second precision is dropped on purpose: the agreed schema and the
/// worked example both use whole seconds, and keeping the fraction would make
/// every record fail the `"Z$"`-only reading of the timestamp format.
pub fn now_utc() -> String {
    truncate_subseconds(OffsetDateTime::now_utc())
}

/// Normalize an upstream timestamp to RFC 3339 UTC `Z`, whole seconds only.
/// Returns `None` when the value cannot be parsed, so a bad optional timestamp
/// is omitted rather than failing the whole refresh.
pub fn normalize_timestamp(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    OffsetDateTime::parse(value, &Rfc3339)
        .ok()
        .map(truncate_subseconds)
}

/// Re-format in UTC with no fractional seconds: `+00:00` becomes `Z` and a
/// non-zero offset is converted rather than merely relabelled.
fn truncate_subseconds(dt: OffsetDateTime) -> String {
    let dt = dt.replace_nanosecond(0).expect("0 ns is always in range");
    let dt = dt.to_offset(time::UtcOffset::UTC);
    dt.format(&Rfc3339)
        .expect("RFC 3339 formatting cannot fail")
}

// ---------------------------------------------------------------------------
// Record types (wire shape = schema shape)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Other,
    Amp,
    Pedal,
    Outboard,
    Ir,
    Fullrig,
}

impl Kind {
    pub fn as_wire(self) -> &'static str {
        match self {
            Kind::Amp => "amp",
            Kind::Pedal => "pedal",
            Kind::Outboard => "outboard",
            Kind::Ir => "ir",
            Kind::Fullrig => "fullrig",
            Kind::Other => "other",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    #[default]
    Nam,
    Ir,
}

impl Format {
    pub fn as_wire(self) -> &'static str {
        match self {
            Format::Nam => "nam",
            Format::Ir => "ir",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Variant {
    #[default]
    Custom,
    A1,
    A2,
}

impl Variant {
    pub fn as_wire(self) -> &'static str {
        match self {
            Variant::A1 => "a1",
            Variant::A2 => "a2",
            Variant::Custom => "custom",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Author {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stats {
    pub favorites: u64,
    pub downloads: u64,
}

/// One `metadata` object.
///
/// Only the schema's required set is mandatory here (`name`, `author`, `kind`,
/// `format`, `variant`); every other field defaults when absent so the agreed
/// worked example — which omits several optional keys — deserializes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Metadata {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone_title: Option<String>,
    #[serde(default)]
    pub author: Author,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub kind: Kind,
    pub format: Format,
    pub variant: Variant,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub with_cab: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gear_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cab: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mics: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loudness_db: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gain_db: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats: Option<Stats>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalState {
    pub file_name: String,
    pub size_bytes: u64,
    pub checksum: String,
    pub downloaded_at: String,
    pub stale: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelRecord {
    pub id: String,
    pub provider: String,
    pub tone_id: String,
    pub source_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    pub fetched_at: String,
    pub local_path: Option<String>,
    pub local: Option<LocalState>,
    pub metadata: Metadata,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Index {
    pub schema: u32,
    pub provider: String,
    pub generated_at: String,
    pub count: usize,
    pub models: Vec<ModelRecord>,
}

impl Index {
    pub fn find(&self, id: &str) -> Option<&ModelRecord> {
        self.models.iter().find(|r| r.id == id)
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

fn validator() -> &'static jsonschema::Validator {
    static VALIDATOR: OnceLock<jsonschema::Validator> = OnceLock::new();
    VALIDATOR.get_or_init(|| {
        let schema: Value =
            serde_json::from_str(INDEX_SCHEMA).expect("the embedded index schema is valid JSON");
        // `format` is an assertion here, matching the schema's own review
        // (a `uri` or `date-time` that does not parse is rejected, not merely
        // annotated). The schema is embedded, so no retrieval is involved.
        jsonschema::options()
            .should_validate_formats(true)
            .build(&schema)
            .expect("the embedded index schema compiles")
    })
}

/// Validate an already-serialized index against the embedded schema.
pub fn validate_value(value: &Value) -> Result<(), SchemaError> {
    let errors: Vec<String> = validator()
        .iter_errors(value)
        .map(|e| format!("{}: {}", e.instance_path(), e))
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(SchemaError::Invalid(errors.join("; ")))
    }
}

/// Validate a typed index against the embedded schema.
///
/// The schema cannot express cross-field equality, so the §9.3 invariant
/// `count == models.len()` is checked here as well.
pub fn validate_index(index: &Index) -> Result<(), SchemaError> {
    let value = serde_json::to_value(index)?;
    validate_value(&value)?;
    if index.count != index.models.len() {
        return Err(SchemaError::Invalid(format!(
            "count is {} but models has {} record(s)",
            index.count,
            index.models.len()
        )));
    }
    let mut ids = std::collections::HashSet::new();
    for m in &index.models {
        if !ids.insert(m.id.as_str()) {
            return Err(SchemaError::Invalid(format!(
                "duplicate record id {}",
                m.id
            )));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Derivation from upstream fields (spec §4.1)
// ---------------------------------------------------------------------------

/// Normalize `gear` + `format` into the stored [`Kind`] and the explicit
/// `with_cab` flag. A known gear that contradicts the format yields
/// [`Kind::Other`] (never a fake kind).
pub fn derive_kind(gear: Option<Gear>, format: Option<ApiFormat>) -> (Kind, Option<bool>) {
    let format = format.unwrap_or(ApiFormat::Other);
    match gear {
        Some(Gear::AmpCab) if format == ApiFormat::Nam => (Kind::Amp, Some(true)),
        Some(Gear::Amp) if format == ApiFormat::Nam => (Kind::Amp, Some(false)),
        Some(Gear::Pedal) if format == ApiFormat::Nam => (Kind::Pedal, Some(false)),
        Some(Gear::Outboard) if format == ApiFormat::Nam => (Kind::Outboard, Some(false)),
        Some(Gear::FullRig) if format == ApiFormat::Nam => (Kind::Fullrig, Some(true)),
        Some(Gear::Space) if format == ApiFormat::Ir => (Kind::Ir, Some(false)),
        Some(_) => (Kind::Other, None),
        None => match format {
            ApiFormat::Ir => (Kind::Ir, Some(false)),
            _ => (Kind::Other, None),
        },
    }
}

/// `architecture_version` `"2"` → A2, `"1"` → A1, anything else → custom. When
/// the version is absent, a `_a1` / `_a2` filename suffix is used.
pub fn derive_variant(architecture_version: Option<&str>, model_url: Option<&str>) -> Variant {
    match architecture_version {
        Some("2") => Variant::A2,
        Some("1") => Variant::A1,
        Some(_) => Variant::Custom,
        None => {
            let url = model_url.unwrap_or("");
            if url.ends_with("_a1.nam") {
                Variant::A1
            } else if url.ends_with("_a2.nam") {
                Variant::A2
            } else {
                Variant::Custom
            }
        }
    }
}

fn non_empty(value: &str) -> Option<&str> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

/// `non_empty` for an optional upstream string: trims, rejects blanks, and
/// returns the original (untrimmed) slice so callers keep the exact casing.
fn non_empty_opt(value: Option<&str>) -> Option<&str> {
    value.and_then(non_empty)
}

/// Only absolute http(s) URLs are kept: the schema validates `format: uri` on
/// these fields, so an internal locator must never leak into a record.
fn http_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

// ---------------------------------------------------------------------------
// Record construction
// ---------------------------------------------------------------------------

/// Build one index record from a tone + model pair. Returns `None` when the
/// pair cannot produce a schema-valid record (unknown format, no name, …).
pub fn build_record(
    provider: &str,
    tone: &Tone,
    model: &Model,
    fetched_at: &str,
) -> Option<ModelRecord> {
    let format = match tone.format? {
        ApiFormat::Nam => Format::Nam,
        ApiFormat::Ir => Format::Ir,
        ApiFormat::Other => return None,
    };
    let (kind, with_cab) = derive_kind(tone.gear, tone.format);
    let variant = derive_variant(
        model.architecture_version.as_deref(),
        model.model_url.as_deref(),
    );

    let name = non_empty_opt(model.name.as_deref())
        .or_else(|| non_empty_opt(tone.title.as_deref()))?
        .to_string();

    let user = tone.user.clone().unwrap_or_default();
    let author = Author {
        name: non_empty_opt(user.display_name.as_deref())
            .or_else(|| non_empty_opt(user.username.as_deref()))
            .unwrap_or("Unknown")
            .to_string(),
        username: non_empty_opt(user.username.as_deref()).map(str::to_string),
        url: non_empty_opt(user.url.as_deref())
            .filter(|v| http_url(v))
            .map(str::to_string),
        avatar_url: non_empty_opt(user.avatar_url.as_deref())
            .filter(|v| http_url(v))
            .map(str::to_string),
        verified: user.is_verified,
    };

    let first_make = tone
        .makes
        .first()
        .and_then(|m| m.name())
        .and_then(non_empty)
        .map(str::to_string);

    let manufacturer = model
        .meta_string("gear_make")
        .or_else(|| model.meta_string("manufacturer"))
        .and_then(non_empty)
        .map(str::to_string)
        .or(first_make.clone());
    let gear_model = model
        .meta_string("gear_model")
        .and_then(non_empty)
        .map(str::to_string)
        .or(first_make);

    let mut tags: Vec<String> = tone
        .tags
        .iter()
        .filter_map(|t| t.name())
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    tags.sort();
    tags.dedup();

    let links: Vec<String> = tone
        .links
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter(|l| http_url(l))
        .cloned()
        .collect();

    let thumbnail = tone
        .images
        .as_ref()
        .and_then(|images| images.first())
        .and_then(|v| non_empty(v))
        .filter(|v| http_url(v))
        .map(str::to_string);

    let stats = match (tone.favorites_count, tone.downloads_count) {
        (None, None) => None,
        (favorites, downloads) => Some(Stats {
            favorites: favorites.unwrap_or(0),
            downloads: downloads.unwrap_or(0),
        }),
    };

    let source_url = tone
        .url
        .as_deref()
        .and_then(non_empty)
        .filter(|v| http_url(v))
        .map(str::to_string)
        .unwrap_or_else(|| format!("https://www.tone3000.com/tones/{}", tone.id));

    let metadata = Metadata {
        name,
        tone_title: non_empty_opt(tone.title.as_deref()).map(str::to_string),
        author,
        description: non_empty_opt(tone.description.as_deref()).map(str::to_string),
        kind,
        format,
        variant,
        with_cab,
        manufacturer,
        gear_model,
        // The API's model metadata does not expose cabinet/mic strings; leave
        // them absent rather than inventing values.
        cab: None,
        mics: None,
        tags,
        sample_rate: model
            .meta_u64("sample_rate")
            .or_else(|| model.meta_u64("sampleRate"))
            .map(|v| v as u32),
        loudness_db: model.meta_f64("loudness"),
        gain_db: model.meta_f64("gain"),
        license: non_empty_opt(tone.license.as_deref()).map(str::to_string),
        links,
        thumbnail,
        stats,
        published_at: tone.published_at.as_deref().and_then(normalize_timestamp),
        updated_at: tone.updated_at.as_deref().and_then(normalize_timestamp),
    };

    Some(ModelRecord {
        id: format!("{provider}:{}", model.id),
        provider: provider.to_string(),
        tone_id: tone.id.to_string(),
        source_url,
        download_url: non_empty_opt(model.model_url.as_deref()).map(str::to_string),
        content_hash: non_empty_opt(model.content_hash.as_deref()).map(str::to_string),
        fetched_at: fetched_at.to_string(),
        local_path: None,
        local: None,
        metadata,
    })
}

// ---------------------------------------------------------------------------
// Merge (spec §9.4)
// ---------------------------------------------------------------------------

/// Merge freshly built candidates into the existing index.
///
/// * `local` / `local_path` are always carried forward from the previous
///   record, so a refresh can never orphan a download.
/// * `local.stale` is set when both the previous and the new `content_hash` are
///   present and differ; if either is missing the record is never marked stale.
/// * Records absent from the candidates are dropped, but their files are never
///   touched (deletion is an index-only operation).
pub fn merge(existing: Option<&Index>, candidates: Vec<ModelRecord>, generated_at: &str) -> Index {
    let provider = existing
        .map(|index| index.provider.clone())
        .unwrap_or_else(|| crate::PROVIDER.to_string());
    let previous: HashMap<String, ModelRecord> = existing
        .map(|index| {
            index
                .models
                .iter()
                .map(|record| (record.id.clone(), record.clone()))
                .collect()
        })
        .unwrap_or_default();

    let mut models = Vec::with_capacity(candidates.len());
    for mut record in candidates {
        if let Some(prev) = previous.get(&record.id) {
            let stale = match (prev.content_hash.as_deref(), record.content_hash.as_deref()) {
                (Some(a), Some(b)) => a != b,
                _ => false,
            };
            record.local = prev.local.clone();
            record.local_path = prev.local_path.clone();
            if let Some(local) = record.local.as_mut() {
                local.stale = stale;
            }
        }
        models.push(record);
    }

    let count = models.len();
    Index {
        schema: SCHEMA_VERSION,
        provider,
        generated_at: generated_at.to_string(),
        count,
        models,
    }
}

// ---------------------------------------------------------------------------
// Persistence (spec §9.1)
// ---------------------------------------------------------------------------

/// Load and validate the index. `Ok(None)` when no index exists yet.
pub fn load_index(paths: &Paths) -> Result<Option<Index>, Error> {
    match std::fs::read_to_string(&paths.index_file) {
        Ok(text) => {
            let index: Index = serde_json::from_str(&text)
                .map_err(|e| Error::Invalid(format!("index file is not valid JSON: {e}")))?;
            validate_index(&index)?;
            Ok(Some(index))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Validate, then atomically replace `index/models.json` (tmp → bak → rename).
pub fn write_index(paths: &Paths, index: &Index) -> Result<(), Error> {
    validate_index(index)?;
    paths.ensure_dirs()?;
    let text = serde_json::to_string_pretty(index)?;
    std::fs::write(&paths.tmp_file, text.as_bytes())?;
    if paths.index_file.exists() {
        std::fs::rename(&paths.index_file, &paths.bak_file)?;
    }
    if let Err(e) = std::fs::rename(&paths.tmp_file, &paths.index_file) {
        // Put the previous generation back rather than leaving no index.
        if paths.bak_file.exists() {
            let _ = std::fs::rename(&paths.bak_file, &paths.index_file);
        }
        return Err(e.into());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Refresh
// ---------------------------------------------------------------------------

/// Which tone listing the index is built from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Source {
    /// Bounded feeds: `trending` (public) + `latest` (Bearer). Permitted by the
    /// free-tier terms, but `latest` still needs the user's credential.
    #[default]
    FreeFeeds,
    /// Full `/tones/search` walk. Requires a commercial TONE3000 agreement
    /// (the free-tier terms forbid bulk catalog listing and caching).
    Catalog,
}

impl Source {
    pub fn is_catalog(self) -> bool {
        matches!(self, Source::Catalog)
    }
}

#[derive(Clone, Debug)]
pub struct RefreshConfig {
    pub source: Source,
    /// Explicit acknowledgement that a commercial/approved-partner agreement
    /// covers the bulk catalog walk. Must be `true` for [`Source::Catalog`].
    pub catalog_agreement: bool,
    /// `page_size` for `/tones/search` (API cap: 25).
    pub search_page_size: u32,
    /// `page_size` for `/models` (API cap: 300).
    pub models_page_size: u32,
}

impl Default for RefreshConfig {
    fn default() -> Self {
        Self {
            source: Source::FreeFeeds,
            catalog_agreement: false,
            search_page_size: 25,
            models_page_size: 300,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RefreshOutcome {
    pub generated_at: String,
    pub count: usize,
    pub tones: usize,
    pub skipped: usize,
}

pub struct IndexBuilder<'a> {
    client: &'a Client,
    config: RefreshConfig,
}

impl<'a> IndexBuilder<'a> {
    pub fn new(client: &'a Client, config: RefreshConfig) -> Self {
        Self { client, config }
    }

    /// Fetch every tone, then every model of every tone, merge into the
    /// existing index, validate, and atomically persist.
    ///
    /// All-or-nothing: a failure on any page aborts before the old index is
    /// touched.
    pub async fn refresh(&self, paths: &Paths) -> Result<RefreshOutcome, Error> {
        if self.config.source.is_catalog() && !self.config.catalog_agreement {
            return Err(Error::Invalid(
                "full catalog refresh (source = catalog) requires a commercial TONE3000 \
                 agreement; the free-tier terms forbid bulk catalog listing and caching. \
                 Set catalog_agreement = true once that agreement is in place, or use \
                 Source::FreeFeeds."
                    .to_string(),
            ));
        }

        let tones = self.fetch_tones().await?;
        let tone_count = tones.len();
        let fetched_at = now_utc();
        let mut candidates = Vec::new();
        let mut skipped = 0usize;

        for tone in &tones {
            let mut seen_models: HashSet<u64> = HashSet::new();
            for architecture in architectures_for(tone) {
                let query = vec![
                    ("tone_id".to_string(), tone.id.to_string()),
                    (
                        "architecture".to_string(),
                        architecture.as_str().to_string(),
                    ),
                ];
                let models: Vec<Model> = self
                    .client
                    .fetch_all_pages("models", &query, self.config.models_page_size)
                    .await?;
                for model in models {
                    if !seen_models.insert(model.id) {
                        continue;
                    }
                    match build_record(crate::PROVIDER, tone, &model, &fetched_at) {
                        Some(record) => candidates.push(record),
                        None => skipped += 1,
                    }
                }
            }
        }

        let existing = load_index(paths)?;
        let index = merge(existing.as_ref(), candidates, &fetched_at);
        validate_index(&index)?;
        write_index(paths, &index)?;

        Ok(RefreshOutcome {
            generated_at: index.generated_at,
            count: index.count,
            tones: tone_count,
            skipped,
        })
    }

    async fn fetch_tones(&self) -> Result<Vec<Tone>, crate::client::Error> {
        match self.config.source {
            Source::FreeFeeds => {
                let mut tones = self.client.get_tone_feed("tones/trending", &[]).await?;
                tones.extend(self.client.get_tone_feed("tones/latest", &[]).await?);
                Ok(dedupe_tones(tones))
            }
            Source::Catalog => {
                self.client
                    .fetch_all_pages("tones/search", &[], self.config.search_page_size)
                    .await
            }
        }
    }
}

/// Drop repeated tones (a tone can be both trending and latest), keeping the
/// first occurrence and the original order.
fn dedupe_tones(tones: Vec<Tone>) -> Vec<Tone> {
    let mut seen = std::collections::HashSet::new();
    tones.into_iter().filter(|t| seen.insert(t.id)).collect()
}

/// Which architecture listings to request for one tone. The API's
/// `architecture` default silently excludes A2, so the indexer always sends an
/// explicit value and only asks for architectures the tone reports.
fn architectures_for(tone: &Tone) -> Vec<Architecture> {
    let mut out = Vec::new();
    if tone.a1_models_count.unwrap_or(0) > 0 {
        out.push(Architecture::A1);
    }
    if tone.a2_models_count.unwrap_or(0) > 0 {
        out.push(Architecture::A2);
    }
    if tone.custom_models_count.unwrap_or(0) > 0 {
        out.push(Architecture::Custom);
    }
    if out.is_empty() && tone.models_count.unwrap_or(0) > 0 {
        // Counts were absent but the tone does have models: ask for everything
        // and dedupe by model id.
        out.extend([Architecture::A1, Architecture::A2, Architecture::Custom]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::EmbeddedUser;

    fn tone() -> Tone {
        Tone {
            id: 28199,
            title: Some("API 512c".to_string()),
            description: Some("Dual channels on an API 512c preamp.".to_string()),
            gear: Some(Gear::Outboard),
            format: Some(ApiFormat::Nam),
            user: Some(EmbeddedUser {
                username: Some("tone3000".to_string()),
                display_name: Some("TONE3000".to_string()),
                avatar_url: Some("https://assets.tone3000.com/a.png".to_string()),
                url: Some("https://www.tone3000.com/tone3000".to_string()),
                is_verified: Some(true),
            }),
            tags: vec![crate::api::Tag {
                id: Some(1),
                name: Some("Preamp".to_string()),
            }],
            makes: vec![crate::api::Make {
                id: Some(2),
                name: Some("API 512c".to_string()),
            }],
            images: Some(vec!["https://assets.tone3000.com/t.jpg".to_string()]),
            links: Some(vec!["https://example.com/x".to_string()]),
            license: Some("t3k".to_string()),
            models_count: Some(1),
            a1_models_count: Some(0),
            a2_models_count: Some(1),
            custom_models_count: Some(0),
            irs_count: Some(0),
            downloads_count: Some(12),
            favorites_count: Some(3),
            published_at: Some("2026-05-04T20:49:14.123456+00:00".to_string()),
            updated_at: Some("2026-05-04T21:00:00+00:00".to_string()),
            created_at: None,
            url: Some("https://www.tone3000.com/tones/api-512c-28199".to_string()),
            is_favorite: None,
            is_public: Some(true),
        }
    }

    fn model() -> Model {
        Model {
            id: 379978,
            name: Some("API 512c - left mic gain 5".to_string()),
            model_url: Some("r2://models/7e9gmdzciqu_a2.nam".to_string()),
            tone_id: Some(28199),
            content_hash: Some("nam-content-v1:abc".to_string()),
            architecture_version: Some("2".to_string()),
            metadata: Some(serde_json::json!({
                "gear_make": "API",
                "gear_model": "API 512c",
                "sample_rate": 48000,
                "loudness": -22.98,
                "gain": 0.0
            })),
            size: None,
            created_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn embedded_schema_validates_the_example() {
        let example: Index = serde_json::from_str(EXAMPLE_INDEX).expect("example parses");
        assert_eq!(example.count, 2);
        validate_index(&example).expect("example validates");
    }

    #[test]
    fn built_record_validates_against_the_schema() {
        let record =
            build_record("t3k", &tone(), &model(), "2026-11-15T09:00:00Z").expect("record builds");
        let index = merge(None, vec![record], "2026-11-15T09:00:00Z");
        validate_index(&index).expect("built index validates");
        assert_eq!(index.models[0].metadata.kind, Kind::Outboard);
        assert_eq!(index.models[0].metadata.variant, Variant::A2);
        assert_eq!(index.models[0].metadata.with_cab, Some(false));
        assert_eq!(index.models[0].metadata.tags, vec!["preamp".to_string()]);
        assert_eq!(
            index.models[0].metadata.published_at.as_deref(),
            Some("2026-05-04T20:49:14Z")
        );
    }

    #[test]
    fn kind_derivation_follows_the_spec() {
        use ApiFormat::*;
        assert_eq!(
            derive_kind(Some(Gear::AmpCab), Some(Nam)),
            (Kind::Amp, Some(true))
        );
        assert_eq!(
            derive_kind(Some(Gear::Amp), Some(Nam)),
            (Kind::Amp, Some(false))
        );
        assert_eq!(
            derive_kind(Some(Gear::Pedal), Some(Nam)),
            (Kind::Pedal, Some(false))
        );
        assert_eq!(
            derive_kind(Some(Gear::FullRig), Some(Nam)),
            (Kind::Fullrig, Some(true))
        );
        assert_eq!(
            derive_kind(Some(Gear::Space), Some(Ir)),
            (Kind::Ir, Some(false))
        );
        // Contradictory pair is never presented as a real kind.
        assert_eq!(
            derive_kind(Some(Gear::Pedal), Some(Ir)),
            (Kind::Other, None)
        );
        assert_eq!(derive_kind(None, Some(Ir)), (Kind::Ir, Some(false)));
        assert_eq!(derive_kind(None, Some(Nam)), (Kind::Other, None));
        assert_eq!(derive_kind(None, None), (Kind::Other, None));
    }

    #[test]
    fn variant_derivation() {
        assert_eq!(derive_variant(Some("2"), None), Variant::A2);
        assert_eq!(derive_variant(Some("1"), None), Variant::A1);
        assert_eq!(derive_variant(Some("custom"), None), Variant::Custom);
        assert_eq!(derive_variant(None, Some("r2://x_a1.nam")), Variant::A1);
        assert_eq!(derive_variant(None, Some("r2://x_a2.nam")), Variant::A2);
        assert_eq!(derive_variant(None, Some("r2://x.nam")), Variant::Custom);
    }

    #[test]
    fn merge_carries_local_state_forward_and_marks_stale() {
        let mut record = build_record("t3k", &tone(), &model(), "2026-11-15T09:00:00Z").unwrap();
        record.local_path = Some("t3k/28199/379978_a2.nam".to_string());
        record.local = Some(LocalState {
            file_name: "379978_a2.nam".to_string(),
            size_bytes: 10,
            checksum: "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                .to_string(),
            downloaded_at: "2026-11-15T09:00:00Z".to_string(),
            stale: false,
        });
        let existing = merge(None, vec![record], "2026-11-15T09:00:00Z");

        // Same hash: local state survives, not stale.
        let unchanged = build_record("t3k", &tone(), &model(), "2026-11-15T10:00:00Z").unwrap();
        let refreshed = merge(Some(&existing), vec![unchanged], "2026-11-15T10:00:00Z");
        let r = &refreshed.models[0];
        assert_eq!(r.local_path.as_deref(), Some("t3k/28199/379978_a2.nam"));
        assert!(!r.local.as_ref().unwrap().stale);

        // Different hash: still carried forward, now stale.
        let mut changed = model();
        changed.content_hash = Some("nam-content-v1:def".to_string());
        let changed = build_record("t3k", &tone(), &changed, "2026-11-15T10:00:00Z").unwrap();
        let refreshed = merge(Some(&existing), vec![changed], "2026-11-15T10:00:00Z");
        let r = &refreshed.models[0];
        assert_eq!(r.local_path.as_deref(), Some("t3k/28199/379978_a2.nam"));
        assert!(r.local.as_ref().unwrap().stale);

        // Missing hash on either side: never stale.
        let mut no_hash = model();
        no_hash.content_hash = None;
        let no_hash = build_record("t3k", &tone(), &no_hash, "2026-11-15T10:00:00Z").unwrap();
        let refreshed = merge(Some(&existing), vec![no_hash], "2026-11-15T10:00:00Z");
        assert!(!refreshed.models[0].local.as_ref().unwrap().stale);
    }

    #[test]
    fn merge_drops_records_absent_from_the_new_listing() {
        let record = build_record("t3k", &tone(), &model(), "2026-11-15T09:00:00Z").unwrap();
        let existing = merge(None, vec![record.clone()], "2026-11-15T09:00:00Z");
        let refreshed = merge(Some(&existing), vec![], "2026-11-15T10:00:00Z");
        assert_eq!(refreshed.count, 0);
        assert!(refreshed.models.is_empty());
    }

    #[test]
    fn normalize_timestamp_handles_offsets_and_fractions() {
        assert_eq!(
            normalize_timestamp("2026-05-04T20:49:14.123456+00:00").as_deref(),
            Some("2026-05-04T20:49:14Z")
        );
        assert_eq!(
            normalize_timestamp("2026-05-04T20:49:14Z").as_deref(),
            Some("2026-05-04T20:49:14Z")
        );
        assert_eq!(normalize_timestamp("nonsense"), None);
        assert_eq!(normalize_timestamp(""), None);
    }

    #[test]
    fn architectures_only_requests_reported_ones() {
        let mut t = tone();
        t.a2_models_count = Some(2);
        assert_eq!(architectures_for(&t), vec![Architecture::A2]);
        t.a2_models_count = None;
        t.a1_models_count = None;
        t.custom_models_count = None;
        t.models_count = Some(5);
        assert_eq!(
            architectures_for(&t),
            vec![Architecture::A1, Architecture::A2, Architecture::Custom]
        );
    }

    // ---------------------------------------------------------------
    // Refresh end-to-end against a mock transport
    // ---------------------------------------------------------------

    use crate::client::{Client, Request, Response, RetryPolicy, TransportError};
    use crate::mock::{self, Handler, MockTransport};
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn fast_retry() -> RetryPolicy {
        RetryPolicy {
            max_attempts: 1,
            initial_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(2),
            jitter: false,
            min_interval: Duration::ZERO,
        }
    }

    fn client_with(handler: Handler) -> (Client, MockTransport) {
        let transport = MockTransport::new(handler);
        let client = Client::new(
            Arc::new(transport.clone()),
            "https://api.test/v1",
            None,
            fast_retry(),
        );
        (client, transport)
    }

    fn temp_home(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pedal-index-{label}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn query_value(request: &Request, key: &str) -> Option<String> {
        request
            .query
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    }

    fn tone_json(id: u64, a2_models: u64) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "title": format!("Tone {id}"),
            "gear": "amp-cab",
            "format": "nam",
            "user": {"username": "tone3000", "display_name": "TONE3000"},
            "tags": [{"id": 1, "name": "High Gain"}],
            "makes": [{"id": 2, "name": "Mesa Boogie"}],
            "images": ["https://assets.tone3000.com/t.jpg"],
            "models_count": a2_models,
            "a1_models_count": 0,
            "a2_models_count": a2_models,
            "custom_models_count": 0,
            "downloads_count": 10,
            "favorites_count": 1,
            "published_at": "2026-05-04T20:49:14.123456+00:00",
            "url": format!("https://www.tone3000.com/tones/tone-{id}")
        })
    }

    fn model_json(id: u64, tone_id: u64, architecture: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "name": format!("Model {id}"),
            "model_url": format!("https://www.tone3000.com/api/files/models/{id}/m{id}_{architecture}.nam"),
            "tone_id": tone_id,
            "content_hash": format!("nam-content-v1:{id}"),
            "architecture_version": architecture,
            "metadata": {
                "gear_make": "Mesa Boogie",
                "gear_model": "Dual Rectifier",
                "sample_rate": 48000,
                "loudness": -22.98,
                "gain": 0.0
            }
        })
    }

    /// trending = tones 1,2; latest = tone 3; one A2 model per tone.
    fn free_feeds_handler(request: &Request) -> Result<Response, TransportError> {
        let url = request.url.clone();
        if url.contains("tones/trending") {
            return Ok(mock::ok_json(
                &url,
                mock::tone_list(&[tone_json(1, 1), tone_json(2, 1)]),
            ));
        }
        if url.contains("tones/latest") {
            return Ok(mock::ok_json(&url, mock::tone_list(&[tone_json(3, 1)])));
        }
        if url.contains("/models") {
            let tone_id = query_value(request, "tone_id").unwrap_or_default();
            let architecture = query_value(request, "architecture").unwrap_or_default();
            let id = 100 + tone_id.parse::<u64>().unwrap_or(0);
            return Ok(mock::ok_json(
                &url,
                mock::paginated(
                    &[model_json(id, tone_id.parse().unwrap_or(0), &architecture)],
                    1,
                    300,
                    1,
                ),
            ));
        }
        Err(TransportError::Network(format!("unexpected URL {url}")))
    }

    #[tokio::test]
    async fn refresh_from_free_feeds_persists_a_valid_index() {
        let home = temp_home("free");
        let paths = Paths::from_home(&home);
        let (client, transport) = client_with(Arc::new(free_feeds_handler));
        let builder = IndexBuilder::new(&client, RefreshConfig::default());

        let outcome = builder.refresh(&paths).await.expect("refresh succeeds");
        assert_eq!(outcome.count, 3);
        assert_eq!(outcome.tones, 3);
        assert_eq!(outcome.skipped, 0);

        // The file exists, loads, and validates against the embedded schema.
        assert!(paths.index_file.is_file());
        let loaded = load_index(&paths).expect("loads").expect("present");
        assert_eq!(loaded.count, 3);
        assert_eq!(loaded.models.len(), 3);
        validate_index(&loaded).expect("persisted index validates");

        let record = loaded.find("t3k:101").expect("record present");
        assert_eq!(record.metadata.kind, Kind::Amp);
        assert_eq!(record.metadata.variant, Variant::A2);
        assert_eq!(record.metadata.with_cab, Some(true));
        assert_eq!(record.metadata.tags, vec!["high gain".to_string()]);
        assert_eq!(record.tone_id, "1");
        assert!(record.local.is_none());
        assert!(record.local_path.is_none());

        // Every /models call sent an explicit architecture (never omitted).
        for request in transport.requests() {
            if request.url.contains("/models") {
                assert!(
                    query_value(&request, "architecture").is_some(),
                    "architecture must always be sent: {}",
                    request.url
                );
            }
        }

        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn overlapping_feeds_do_not_duplicate_records() {
        let home = temp_home("overlap");
        let paths = Paths::from_home(&home);
        // tone 1 is in both trending and latest.
        let (client, _t) = client_with(Arc::new(|request: &Request| {
            let url = request.url.clone();
            if url.contains("tones/trending") || url.contains("tones/latest") {
                return Ok(mock::ok_json(&url, mock::tone_list(&[tone_json(1, 1)])));
            }
            let tone_id = query_value(request, "tone_id").unwrap_or_default();
            let architecture = query_value(request, "architecture").unwrap_or_default();
            Ok(mock::ok_json(
                &url,
                mock::paginated(
                    &[model_json(101, tone_id.parse().unwrap_or(0), &architecture)],
                    1,
                    300,
                    1,
                ),
            ))
        }));
        let builder = IndexBuilder::new(&client, RefreshConfig::default());
        let outcome = builder.refresh(&paths).await.expect("refresh succeeds");
        assert_eq!(outcome.count, 1);
        let loaded = load_index(&paths).unwrap().unwrap();
        assert_eq!(loaded.models.len(), 1);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn validate_index_rejects_duplicate_record_ids() {
        let home = temp_home("dupid");
        let paths = Paths::from_home(&home);
        let (client, _t) = client_with(Arc::new(free_feeds_handler));
        let builder = IndexBuilder::new(&client, RefreshConfig::default());
        builder.refresh(&paths).await.expect("refresh succeeds");
        let mut index = load_index(&paths).unwrap().unwrap();
        let first = index.models[0].clone();
        index.models.push(first);
        index.count = index.models.len();
        assert!(validate_index(&index).is_err());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn refresh_is_all_or_nothing() {
        let home = temp_home("atomic");
        let paths = Paths::from_home(&home);

        // A previous good generation.
        let previous = merge(
            None,
            vec![build_record("t3k", &tone(), &model(), "2026-11-15T09:00:00Z").unwrap()],
            "2026-11-15T09:00:00Z",
        );
        write_index(&paths, &previous).expect("seed writes");
        let before = std::fs::read_to_string(&paths.index_file).unwrap();

        // Fail the models call for tone 2.
        let handler = Arc::new(|request: &Request| {
            if request.url.contains("/models") {
                if query_value(request, "tone_id").as_deref() == Some("2") {
                    return Ok(mock::response(
                        request.url.as_str(),
                        500,
                        serde_json::json!({"error": "boom"}).to_string(),
                    ));
                }
                let tone_id = query_value(request, "tone_id").unwrap_or_default();
                let architecture = query_value(request, "architecture").unwrap_or_default();
                let id = 100 + tone_id.parse::<u64>().unwrap_or(0);
                return Ok(mock::ok_json(
                    request.url.as_str(),
                    mock::paginated(
                        &[model_json(id, tone_id.parse().unwrap_or(0), &architecture)],
                        1,
                        300,
                        1,
                    ),
                ));
            }
            free_feeds_handler(request)
        });
        let (client, _transport) = client_with(handler);
        let builder = IndexBuilder::new(&client, RefreshConfig::default());

        let error = builder
            .refresh(&paths)
            .await
            .expect_err("refresh must fail");
        assert!(
            matches!(
                error,
                Error::Api(crate::client::Error::Http { status: 500, .. })
            ),
            "{error:?}"
        );

        // The old index is untouched, and no temp file is left behind.
        let after = std::fs::read_to_string(&paths.index_file).unwrap();
        assert_eq!(before, after, "a failed refresh must not touch the index");
        assert!(!paths.tmp_file.exists(), "no half-written temp file");

        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn catalog_source_requires_an_explicit_agreement() {
        let home = temp_home("catalog-gate");
        let paths = Paths::from_home(&home);
        let (client, transport) = client_with(Arc::new(free_feeds_handler));
        let config = RefreshConfig {
            source: Source::Catalog,
            catalog_agreement: false,
            ..RefreshConfig::default()
        };

        let error = IndexBuilder::new(&client, config)
            .refresh(&paths)
            .await
            .expect_err("catalog refresh without an agreement must refuse");
        assert!(matches!(error, Error::Invalid(_)), "{error:?}");
        assert!(
            error.to_string().contains("agreement"),
            "the refusal must say why: {error}"
        );
        assert!(
            transport.requests().is_empty(),
            "the guard must fire before any request"
        );
        assert!(!paths.index_file.exists());

        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn catalog_source_walks_the_search_endpoint() {
        let home = temp_home("catalog");
        let paths = Paths::from_home(&home);

        let handler = Arc::new(|request: &Request| {
            if request.url.contains("tones/search") {
                let page = query_value(request, "page")
                    .and_then(|v| v.parse::<u32>().ok())
                    .unwrap_or(1);
                let tones = if page == 1 {
                    vec![tone_json(1, 1), tone_json(2, 1)]
                } else {
                    vec![tone_json(3, 1)]
                };
                return Ok(mock::ok_json(
                    request.url.as_str(),
                    mock::paginated(&tones, page, 2, 3),
                ));
            }
            free_feeds_handler(request)
        });
        let (client, transport) = client_with(handler);
        let config = RefreshConfig {
            source: Source::Catalog,
            catalog_agreement: true,
            search_page_size: 2,
            models_page_size: 300,
        };

        let outcome = IndexBuilder::new(&client, config)
            .refresh(&paths)
            .await
            .expect("catalog refresh succeeds");
        assert_eq!(outcome.tones, 3);
        assert_eq!(outcome.count, 3);
        assert_eq!(load_index(&paths).expect("loads").unwrap().count, 3);

        let mut search_pages: Vec<String> = transport
            .requests()
            .iter()
            .filter(|r| r.url.contains("tones/search"))
            .filter_map(|r| query_value(r, "page"))
            .collect();
        search_pages.sort();
        assert_eq!(search_pages, vec!["1", "2"]);

        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn refresh_carries_local_downloads_forward_and_marks_stale() {
        let home = temp_home("merge");
        let paths = Paths::from_home(&home);

        let mut seeded = build_record("t3k", &tone(), &model(), "2026-11-15T09:00:00Z").unwrap();
        seeded.local_path = Some("t3k/28199/379978_a2.nam".to_string());
        seeded.local = Some(LocalState {
            file_name: "379978_a2.nam".to_string(),
            size_bytes: 301225,
            checksum: "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                .to_string(),
            downloaded_at: "2026-11-15T09:00:00Z".to_string(),
            stale: false,
        });
        write_index(&paths, &merge(None, vec![seeded], "2026-11-15T09:00:00Z"))
            .expect("seed writes");

        // Upstream now reports a different content hash for the same model.
        let handler = Arc::new(|request: &Request| {
            if request.url.contains("/models") {
                let mut model = model_json(379978, 28199, "2");
                model["content_hash"] = serde_json::json!("nam-content-v1:changed");
                return Ok(mock::ok_json(
                    request.url.as_str(),
                    mock::paginated(&[model], 1, 300, 1),
                ));
            }
            if request.url.contains("tones/trending") {
                return Ok(mock::ok_json(
                    request.url.as_str(),
                    mock::tone_list(&[tone()]),
                ));
            }
            if request.url.contains("tones/latest") {
                return Ok(mock::ok_json(
                    request.url.as_str(),
                    mock::tone_list(&Vec::<crate::api::Tone>::new()),
                ));
            }
            Err(TransportError::Network(format!(
                "unexpected URL {}",
                request.url
            )))
        });
        let (client, _transport) = client_with(handler);
        let outcome = IndexBuilder::new(&client, RefreshConfig::default())
            .refresh(&paths)
            .await
            .expect("refresh succeeds");
        assert_eq!(outcome.count, 1);

        let loaded = load_index(&paths).unwrap().unwrap();
        let record = loaded.find("t3k:379978").expect("record survives") as &ModelRecord;
        assert_eq!(
            record.local_path.as_deref(),
            Some("t3k/28199/379978_a2.nam"),
            "a refresh must never orphan a download"
        );
        assert!(record.local.as_ref().expect("local state").stale);
        assert_eq!(
            record.content_hash.as_deref(),
            Some("nam-content-v1:changed")
        );

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn schema_rejects_records_the_schema_forbids() {
        let record = build_record("t3k", &tone(), &model(), "2026-11-15T09:00:00Z").unwrap();
        let index = merge(None, vec![record], "2026-11-15T09:00:00Z");
        validate_index(&index).expect("the built record is valid");

        // A non-URI source_url must be rejected (format is an assertion here).
        let mut bad = index.clone();
        bad.models[0].source_url = "not a uri".to_string();
        assert!(validate_index(&bad).is_err(), "bad uri accepted");

        // A timestamp that is not RFC 3339 UTC must be rejected.
        let mut bad = index.clone();
        bad.models[0].fetched_at = "yesterday".to_string();
        assert!(validate_index(&bad).is_err(), "bad timestamp accepted");

        // local_path without local (or the reverse) must be rejected.
        let mut bad = index.clone();
        bad.models[0].local_path = Some("t3k/1/2_a2.nam".to_string());
        assert!(
            validate_index(&bad).is_err(),
            "local_path without local accepted"
        );

        // count must match models.length.
        let mut bad = index;
        bad.count = 99;
        assert!(validate_index(&bad).is_err(), "wrong count accepted");
    }

    #[tokio::test]
    async fn written_index_round_trips_through_the_schema() {
        let home = temp_home("roundtrip");
        let paths = Paths::from_home(&home);
        let (client, _transport) = client_with(Arc::new(free_feeds_handler));
        IndexBuilder::new(&client, RefreshConfig::default())
            .refresh(&paths)
            .await
            .expect("refresh succeeds");

        let text = std::fs::read_to_string(&paths.index_file).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).expect("file is JSON");
        validate_value(&value).expect("the file on disk validates against the schema");
        assert!(paths.bak_file.exists() || paths.index_file.exists());

        let _ = std::fs::remove_dir_all(&home);
    }
}
