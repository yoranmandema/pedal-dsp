//! In-memory `listModels` query over a loaded [`Index`]: search, facets,
//! sorting and paging, as specified in `docs/MODEL-BROWSE-AND-CACHE.md`
//! §3 (search), §4 (filter taxonomy), §6 (sorting) and §10 (wire contract).
//!
//! Everything here is pure: no I/O, no network, no clock. The engine calls it
//! from the `listModels` IPC command; the UI never sees the index itself.
//!
//! # Search
//!
//! Case-insensitive, diacritic-folded, split on whitespace. A record matches
//! when **every** token is a substring of at least one searched field. The
//! per-field weights only order results; they never exclude a record on their
//! own.
//!
//! # Facets
//!
//! Multi-select, OR within a facet, AND across facets. Each facet's value list
//! and counts are computed over the result set filtered by the query and every
//! *other* facet (standard faceted search), before paging.

use std::cmp::Ordering;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

use crate::index::{Format, Index, Kind, ModelRecord};

/// `pageSize` when the caller does not send one.
pub const DEFAULT_PAGE_SIZE: u32 = 100;
/// Hard ceiling for `pageSize`, per spec §10.
pub const MAX_PAGE_SIZE: u32 = 500;

// ---------------------------------------------------------------------------
// Wire enums
// ---------------------------------------------------------------------------

/// Sort order. Wire values are frozen by spec §10.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sort {
    #[default]
    Relevance,
    Name,
    Author,
    Newest,
    Downloads,
    Favorites,
    Updated,
    Added,
}

impl Sort {
    pub fn as_wire(self) -> &'static str {
        match self {
            Sort::Relevance => "relevance",
            Sort::Name => "name",
            Sort::Author => "author",
            Sort::Newest => "newest",
            Sort::Downloads => "downloads",
            Sort::Favorites => "favorites",
            Sort::Updated => "updated",
            Sort::Added => "added",
        }
    }
}

/// The Cab/IR facet. Wire values are frozen by spec §10.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CabIr {
    WithCab,
    WithoutCab,
    IrOnly,
}

impl CabIr {
    pub fn as_wire(self) -> &'static str {
        match self {
            CabIr::WithCab => "with-cab",
            CabIr::WithoutCab => "without-cab",
            CabIr::IrOnly => "ir-only",
        }
    }
}

/// Download state facet. `All` is accepted on the wire and is a no-op.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    All,
    Downloaded,
    NotDownloaded,
    Stale,
}

impl State {
    pub fn as_wire(self) -> &'static str {
        match self {
            State::All => "all",
            State::Downloaded => "downloaded",
            State::NotDownloaded => "not-downloaded",
            State::Stale => "stale",
        }
    }
}

// ---------------------------------------------------------------------------
// Query
// ---------------------------------------------------------------------------

/// A `listModels` query. Build one directly or with [`Query::from_value`].
#[derive(Clone, Debug, Default)]
pub struct Query {
    pub query: Option<String>,
    pub kinds: Vec<Kind>,
    pub manufacturers: Vec<String>,
    pub cab_ir: Vec<CabIr>,
    pub tags: Vec<String>,
    pub formats: Vec<Format>,
    pub states: Vec<State>,
    /// `None` means "use the spec default": relevance when a query is present,
    /// most-downloaded otherwise.
    pub sort: Option<Sort>,
    pub page: u32,
    pub page_size: u32,
}

impl Query {
    /// Parse the frozen `listModels` wire payload (camelCase keys).
    ///
    /// Unknown enum values are rejected rather than silently dropped, so a UI
    /// sending a bad `sort` gets a clear error instead of a wrong ordering.
    pub fn from_value(value: &Value) -> Result<Self, String> {
        let obj = value
            .as_object()
            .ok_or_else(|| "listModels payload must be a JSON object".to_string())?;

        let query = match obj.get("query") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(s.clone()),
            Some(_) => return Err("query must be a string".to_string()),
        };

        let sort = match obj.get("sort") {
            None | Some(Value::Null) => None,
            Some(v) => {
                Some(serde_json::from_value::<Sort>(v.clone()).map_err(|e| format!("sort: {e}"))?)
            }
        };

        Ok(Self {
            query,
            kinds: parse_enum_array(obj.get("kinds"), "kinds")?,
            manufacturers: parse_string_array(obj.get("manufacturers"), "manufacturers")?,
            cab_ir: parse_enum_array(obj.get("cabIr").or_else(|| obj.get("cab_ir")), "cabIr")?,
            tags: parse_string_array(obj.get("tags"), "tags")?,
            formats: parse_enum_array(obj.get("formats"), "formats")?,
            states: parse_enum_array(obj.get("states"), "states")?,
            sort,
            page: parse_u32(obj.get("page"), "page")?.unwrap_or(1),
            page_size: parse_u32(
                obj.get("pageSize").or_else(|| obj.get("page_size")),
                "pageSize",
            )?
            .unwrap_or(DEFAULT_PAGE_SIZE),
        })
    }

    /// Clamp paging to the spec limits and drop no-op filters.
    pub fn normalized(&self) -> Self {
        let mut q = self.clone();
        if q.page < 1 {
            q.page = 1;
        }
        if q.page_size == 0 {
            q.page_size = DEFAULT_PAGE_SIZE;
        }
        if q.page_size > MAX_PAGE_SIZE {
            q.page_size = MAX_PAGE_SIZE;
        }
        q.states.retain(|s| *s != State::All);
        q.tags = q
            .tags
            .iter()
            .map(|t| fold(t))
            .filter(|t| !t.is_empty())
            .collect();
        q.manufacturers = q
            .manufacturers
            .iter()
            .map(|m| fold(m))
            .filter(|m| !m.is_empty())
            .collect();
        q
    }

    fn resolved_sort(&self, has_query: bool) -> Sort {
        self.sort.unwrap_or(if has_query {
            Sort::Relevance
        } else {
            Sort::Downloads
        })
    }
}

fn parse_enum_array<T: for<'de> Deserialize<'de>>(
    value: Option<&Value>,
    field: &str,
) -> Result<Vec<T>, String> {
    match value {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| serde_json::from_value::<T>(v.clone()).map_err(|e| format!("{field}: {e}")))
            .collect(),
        Some(_) => Err(format!("{field} must be an array")),
    }
}

fn parse_string_array(value: Option<&Value>, field: &str) -> Result<Vec<String>, String> {
    match value {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| format!("{field} must contain only strings"))
            })
            .collect(),
        Some(_) => Err(format!("{field} must be an array")),
    }
}

fn parse_u32(value: Option<&Value>, field: &str) -> Result<Option<u32>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => n
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .map(Some)
            .ok_or_else(|| format!("{field} must be a non-negative integer")),
        Some(_) => Err(format!("{field} must be a non-negative integer")),
    }
}

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FacetValue {
    pub value: String,
    pub count: usize,
}

/// Facet value lists and counts, computed before paging. Serialized with the
/// frozen wire key `cabIr`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Facets {
    pub kind: Vec<FacetValue>,
    pub manufacturer: Vec<FacetValue>,
    pub cab_ir: Vec<FacetValue>,
    pub tags: Vec<FacetValue>,
    pub format: Vec<FacetValue>,
    pub state: Vec<FacetValue>,
}

/// One page of `listModels` results.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub total: usize,
    pub page: u32,
    pub page_size: u32,
    pub models: Vec<ModelRecord>,
    pub facets: Facets,
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

const WEIGHT_NAME: u32 = 10;
const WEIGHT_AUTHOR: u32 = 8;
const WEIGHT_TAG: u32 = 6;
const WEIGHT_MAKE: u32 = 5;
const WEIGHT_TITLE: u32 = 4;
const WEIGHT_DESCRIPTION: u32 = 2;

/// Case-fold and strip diacritics so `mesa` matches `Mesa` and `Mesa` matches
/// a record typed as `Mésar`.
pub fn fold(input: &str) -> String {
    input
        .nfd()
        .filter(|c| !is_combining_mark(*c))
        .collect::<String>()
        .to_lowercase()
}

fn is_combining_mark(c: char) -> bool {
    matches!(c,
        '\u{0300}'..='\u{036F}'
        | '\u{1AB0}'..='\u{1AFF}'
        | '\u{1DC0}'..='\u{1DFF}'
        | '\u{20D0}'..='\u{20FF}'
        | '\u{FE20}'..='\u{FE2F}')
}

/// Whitespace-split the free-text query into folded tokens. `None`/empty means
/// "no text filter".
pub fn tokenize(query: Option<&str>) -> Vec<String> {
    let Some(query) = query else {
        return Vec::new();
    };
    fold(query)
        .split_whitespace()
        .map(str::to_string)
        .filter(|t| !t.is_empty())
        .collect()
}

/// Score a record against the tokens. `None` means at least one token matched
/// nothing, so the record is excluded.
fn search_score(record: &ModelRecord, tokens: &[String]) -> Option<u32> {
    if tokens.is_empty() {
        return Some(0);
    }
    let metadata = &record.metadata;
    let mut fields: Vec<(u32, String)> = Vec::with_capacity(8);
    fields.push((WEIGHT_NAME, fold(&metadata.name)));
    fields.push((WEIGHT_AUTHOR, fold(&metadata.author.name)));
    if let Some(username) = &metadata.author.username {
        fields.push((WEIGHT_AUTHOR, fold(username)));
    }
    for tag in &metadata.tags {
        fields.push((WEIGHT_TAG, fold(tag)));
    }
    if let Some(value) = &metadata.manufacturer {
        fields.push((WEIGHT_MAKE, fold(value)));
    }
    if let Some(value) = &metadata.gear_model {
        fields.push((WEIGHT_MAKE, fold(value)));
    }
    if let Some(value) = &metadata.tone_title {
        fields.push((WEIGHT_TITLE, fold(value)));
    }
    if let Some(value) = &metadata.description {
        fields.push((WEIGHT_DESCRIPTION, fold(value)));
    }

    let mut total = 0;
    for token in tokens {
        total += fields
            .iter()
            .filter(|(_, field)| field.contains(token.as_str()))
            .map(|(weight, _)| *weight)
            .max()?;
    }
    Some(total)
}

// ---------------------------------------------------------------------------
// Filters
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Facet {
    Kind,
    Manufacturer,
    CabIr,
    Tags,
    Format,
    State,
}

fn cab_ir_matches(record: &ModelRecord, value: CabIr) -> bool {
    let metadata = &record.metadata;
    match value {
        CabIr::WithCab => metadata.with_cab == Some(true),
        CabIr::WithoutCab => metadata.with_cab == Some(false),
        CabIr::IrOnly => metadata.kind == Kind::Ir,
    }
}

fn state_matches(record: &ModelRecord, value: State) -> bool {
    match value {
        State::All => true,
        State::Downloaded => record.local.is_some(),
        State::NotDownloaded => record.local.is_none(),
        State::Stale => record.local.as_ref().map(|l| l.stale).unwrap_or(false),
    }
}

/// `skip` is the facet currently being counted: its own filter is not applied,
/// so a selected value does not collapse its own facet to just that value.
fn matches_filters(record: &ModelRecord, query: &Query, skip: Option<Facet>) -> bool {
    let metadata = &record.metadata;

    if skip != Some(Facet::Kind) && !query.kinds.is_empty() && !query.kinds.contains(&metadata.kind)
    {
        return false;
    }

    if skip != Some(Facet::Manufacturer) && !query.manufacturers.is_empty() {
        let manufacturer = metadata
            .manufacturer
            .as_deref()
            .map(fold)
            .unwrap_or_default();
        if !query.manufacturers.contains(&manufacturer) {
            return false;
        }
    }

    if skip != Some(Facet::CabIr)
        && !query.cab_ir.is_empty()
        && !query.cab_ir.iter().any(|c| cab_ir_matches(record, *c))
    {
        return false;
    }

    if skip != Some(Facet::Tags) && !query.tags.is_empty() {
        let tags: Vec<String> = metadata.tags.iter().map(|t| fold(t)).collect();
        if !query.tags.iter().any(|t| tags.contains(t)) {
            return false;
        }
    }

    if skip != Some(Facet::Format)
        && !query.formats.is_empty()
        && !query.formats.contains(&metadata.format)
    {
        return false;
    }

    if skip != Some(Facet::State)
        && !query.states.is_empty()
        && !query.states.iter().any(|s| state_matches(record, *s))
    {
        return false;
    }

    true
}

// ---------------------------------------------------------------------------
// Sorting
// ---------------------------------------------------------------------------

fn compare(a: &(u32, &ModelRecord), b: &(u32, &ModelRecord), sort: Sort) -> Ordering {
    let ordering = match sort {
        Sort::Relevance => b.0.cmp(&a.0),
        Sort::Name => fold(&a.1.metadata.name).cmp(&fold(&b.1.metadata.name)),
        Sort::Author => fold(&a.1.metadata.author.name).cmp(&fold(&b.1.metadata.author.name)),
        Sort::Newest => cmp_timestamp_desc(
            a.1.metadata.published_at.as_deref(),
            b.1.metadata.published_at.as_deref(),
        ),
        // Spec §6: "Most downloaded" and "Most favorited" are **descending**
        // (highest first), like Newest/Updated. Ascending would make the
        // no-query default order surface the least-used captures first.
        Sort::Downloads => stat(b.1, |s| s.downloads).cmp(&stat(a.1, |s| s.downloads)),
        Sort::Favorites => stat(b.1, |s| s.favorites).cmp(&stat(a.1, |s| s.favorites)),
        Sort::Updated => cmp_timestamp_desc(
            a.1.metadata.updated_at.as_deref(),
            b.1.metadata.updated_at.as_deref(),
        ),
        Sort::Added => cmp_timestamp_desc(
            a.1.local.as_ref().map(|l| l.downloaded_at.as_str()),
            b.1.local.as_ref().map(|l| l.downloaded_at.as_str()),
        ),
    };
    // Every sort is stable and ties break on the record id.
    ordering.then_with(|| a.1.id.cmp(&b.1.id))
}

fn stat(record: &ModelRecord, pick: impl Fn(&crate::index::Stats) -> u64) -> u64 {
    record.metadata.stats.as_ref().map(pick).unwrap_or(0)
}

/// Newest first; a missing timestamp sorts last.
fn cmp_timestamp_desc(a: Option<&str>, b: Option<&str>) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) => y.cmp(x),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

// ---------------------------------------------------------------------------
// Facets
// ---------------------------------------------------------------------------

fn compute_facets(index: &Index, query: &Query, tokens: &[String]) -> Facets {
    Facets {
        kind: facet_counts(index, query, tokens, Facet::Kind, |r| {
            vec![r.metadata.kind.as_wire().to_string()]
        }),
        manufacturer: facet_counts(index, query, tokens, Facet::Manufacturer, |r| {
            r.metadata
                .manufacturer
                .clone()
                .filter(|m| !m.trim().is_empty())
                .into_iter()
                .collect()
        }),
        cab_ir: facet_counts(index, query, tokens, Facet::CabIr, cab_ir_values),
        tags: facet_counts(index, query, tokens, Facet::Tags, |r| {
            r.metadata.tags.clone()
        }),
        format: facet_counts(index, query, tokens, Facet::Format, |r| {
            vec![r.metadata.format.as_wire().to_string()]
        }),
        state: facet_counts(index, query, tokens, Facet::State, state_values),
    }
}

fn facet_counts<F>(
    index: &Index,
    query: &Query,
    tokens: &[String],
    facet: Facet,
    values: F,
) -> Vec<FacetValue>
where
    F: Fn(&ModelRecord) -> Vec<String>,
{
    let mut counts: HashMap<String, usize> = HashMap::new();
    for record in &index.models {
        if !matches_filters(record, query, Some(facet)) {
            continue;
        }
        if search_score(record, tokens).is_none() {
            continue;
        }
        for value in values(record) {
            *counts.entry(value).or_insert(0) += 1;
        }
    }
    let mut out: Vec<FacetValue> = counts
        .into_iter()
        .map(|(value, count)| FacetValue { value, count })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.value.cmp(&b.value)));
    out
}

fn cab_ir_values(record: &ModelRecord) -> Vec<String> {
    let metadata = &record.metadata;
    if metadata.kind == Kind::Ir {
        return vec![CabIr::IrOnly.as_wire().to_string()];
    }
    match metadata.with_cab {
        Some(true) => vec![CabIr::WithCab.as_wire().to_string()],
        Some(false) => vec![CabIr::WithoutCab.as_wire().to_string()],
        None => Vec::new(),
    }
}

fn state_values(record: &ModelRecord) -> Vec<String> {
    match &record.local {
        None => vec![State::NotDownloaded.as_wire().to_string()],
        Some(local) => {
            let mut out = vec![State::Downloaded.as_wire().to_string()];
            if local.stale {
                out.push(State::Stale.as_wire().to_string());
            }
            out
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Run a `listModels` query against an index.
pub fn list_models(index: &Index, query: &Query) -> Page {
    let query = query.normalized();
    let tokens = tokenize(query.query.as_deref());
    let sort = query.resolved_sort(!tokens.is_empty());

    let mut matched: Vec<(u32, &ModelRecord)> = index
        .models
        .iter()
        .filter(|record| matches_filters(record, &query, None))
        .filter_map(|record| search_score(record, &tokens).map(|score| (score, record)))
        .collect();
    matched.sort_by(|a, b| compare(a, b, sort));

    let total = matched.len();
    let facets = compute_facets(index, &query, &tokens);

    let start = (query.page as usize - 1).saturating_mul(query.page_size as usize);
    let models = matched
        .iter()
        .skip(start)
        .take(query.page_size as usize)
        .map(|(_, record)| (*record).clone())
        .collect();

    Page {
        total,
        page: query.page,
        page_size: query.page_size,
        models,
        facets,
    }
}

impl Index {
    /// Convenience wrapper so callers can write `index.list_models(&query)`.
    pub fn list_models(&self, query: &Query) -> Page {
        list_models(self, query)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{Author, LocalState, Metadata, ModelRecord, Stats};

    fn record(id: &str, name: &str, kind: Kind, with_cab: Option<bool>) -> ModelRecord {
        ModelRecord {
            id: id.to_string(),
            provider: "t3k".to_string(),
            tone_id: "28199".to_string(),
            source_url: "https://www.tone3000.com/tones/x-28199".to_string(),
            download_url: None,
            content_hash: None,
            fetched_at: "2026-11-15T09:00:00Z".to_string(),
            local_path: None,
            local: None,
            metadata: Metadata {
                name: name.to_string(),
                tone_title: Some(name.to_string()),
                author: Author {
                    name: format!("Author {id}"),
                    username: Some(format!("author_{id}")),
                    url: None,
                    avatar_url: None,
                    verified: Some(true),
                },
                description: None,
                kind,
                format: Format::Nam,
                variant: crate::index::Variant::A2,
                with_cab,
                manufacturer: Some("Mesa Boogie".to_string()),
                gear_model: Some("Dual Rectifier".to_string()),
                cab: None,
                mics: None,
                tags: vec!["high gain".to_string(), "metal".to_string()],
                sample_rate: Some(48000),
                loudness_db: None,
                gain_db: None,
                license: None,
                links: Vec::new(),
                thumbnail: None,
                stats: Some(Stats {
                    favorites: 10,
                    downloads: 100,
                }),
                published_at: Some("2026-05-04T20:49:14Z".to_string()),
                updated_at: None,
            },
        }
    }

    fn downloaded(mut record: ModelRecord, downloaded_at: &str, stale: bool) -> ModelRecord {
        record.local_path = Some(format!("t3k/28199/{}.nam", record.id));
        record.local = Some(LocalState {
            file_name: format!("{}.nam", record.id),
            size_bytes: 10,
            checksum: "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                .to_string(),
            downloaded_at: downloaded_at.to_string(),
            stale,
        });
        record
    }

    fn index() -> Index {
        let mut mesa = record("t3k:1", "Mesa Boogie Dual Rectifier", Kind::Amp, Some(true));
        mesa.metadata.tags = vec!["high gain".to_string(), "metal".to_string()];
        // Every record must differ in maker/model/tags, otherwise substring
        // search and the tag facet cannot be distinguished from a no-op.
        let mut fender = record("t3k:2", "Fender Super Reverb", Kind::Amp, Some(true));
        let mut pedal = record("t3k:3", "OD808 Overdrive", Kind::Pedal, Some(false));
        let mut ir = record("t3k:4", "4x12 Cabinet IR", Kind::Ir, Some(false));
        ir.metadata.format = Format::Ir;
        let mut other = record("t3k:5", "Mesa Car Réctifier", Kind::Other, None);
        other.metadata.manufacturer = Some("Mesa Boogie".to_string());

        let variants: [(&str, &str, &str, &str); 5] = [
            ("Mesa Boogie", "Dual Rectifier", "high gain", "metal"),
            ("Fender", "Super Reverb", "clean", "blackface"),
            ("Maxon", "OD808", "overdrive", "boost"),
            ("OwnHammer", "4x12", "cabinet", "ir"),
            ("Mesa Boogie", "Réctifier", "rectifier", "high gain"),
        ];
        for (record, (maker, model, tag_a, tag_b)) in
            [&mut mesa, &mut fender, &mut pedal, &mut ir, &mut other]
                .into_iter()
                .zip(variants)
        {
            record.metadata.manufacturer = Some(maker.to_string());
            record.metadata.gear_model = Some(model.to_string());
            record.metadata.tags = vec![tag_a.to_string(), tag_b.to_string()];
        }

        Index {
            schema: 1,
            provider: "t3k".to_string(),
            generated_at: "2026-11-15T09:00:00Z".to_string(),
            count: 5,
            models: vec![
                downloaded(mesa, "2026-11-14T10:00:00Z", false),
                fender,
                pedal,
                ir,
                other,
            ],
        }
    }

    fn ids(page: &Page) -> Vec<&str> {
        page.models.iter().map(|r| r.id.as_str()).collect()
    }

    #[test]
    fn fold_strips_diacritics_and_case() {
        assert_eq!(fold("Mesa Boogie"), "mesa boogie");
        assert_eq!(fold("Mésar"), "mesar");
        assert_eq!(fold("RÉCTIFIER"), "rectifier");
    }

    #[test]
    fn tokenize_splits_on_whitespace() {
        assert_eq!(tokenize(Some("  high   gain ")), vec!["high", "gain"]);
        assert!(tokenize(None).is_empty());
        assert!(tokenize(Some("   ")).is_empty());
    }

    #[test]
    fn search_requires_every_token_and_ignores_diacritics() {
        let page = list_models(&index(), &Query::default());
        assert_eq!(page.total, 5);

        let q = Query {
            query: Some("mesa boogie".to_string()),
            ..Query::default()
        };
        // Both tokens must match at least one field: t3k:1 and t3k:5 carry
        // "Mesa Boogie" as manufacturer, the other three do not.
        let page = list_models(&index(), &q);
        assert_eq!(page.total, 2);
        assert!(ids(&page).contains(&"t3k:1"));
        assert!(ids(&page).contains(&"t3k:5"));

        let q = Query {
            query: Some("rectif".to_string()),
            ..Query::default()
        };
        // Matches "Rectifier" in the name of t3k:1 and "Réctifier" in t3k:5.
        let page = list_models(&index(), &q);
        assert_eq!(page.total, 2);
        assert!(ids(&page).contains(&"t3k:1"));
        assert!(ids(&page).contains(&"t3k:5"));

        // A token that only matches one field (gear_model, weight 5) is enough.
        let q = Query {
            query: Some("od808".to_string()),
            ..Query::default()
        };
        assert_eq!(list_models(&index(), &q).total, 1);

        let q = Query {
            query: Some("mesa od808".to_string()),
            ..Query::default()
        };
        // AND across tokens: no record has both "mesa" and "od808".
        assert_eq!(list_models(&index(), &q).total, 0);
    }

    #[test]
    fn relevance_ranks_name_above_description() {
        let mut idx = index();
        idx.models
            .push(record("t3k:6", "Something Else", Kind::Pedal, Some(false)));
        let last = idx.models.last_mut().unwrap();
        last.metadata.description = Some("mentions mesa boogie in passing".to_string());
        idx.count = idx.models.len();

        let q = Query {
            query: Some("mesa".to_string()),
            sort: Some(Sort::Relevance),
            ..Query::default()
        };
        let page = list_models(&idx, &q);
        // Name matches (weight 10) outrank description matches (weight 2).
        assert_eq!(&page.models[0].id, "t3k:1");
        assert_eq!(&page.models[1].id, "t3k:5");
        assert_eq!(&page.models[2].id, "t3k:6");
    }

    #[test]
    fn filters_are_or_within_a_facet_and_and_across_facets() {
        let idx = index();

        let q = Query {
            kinds: vec![Kind::Amp, Kind::Pedal],
            ..Query::default()
        };
        let page = list_models(&idx, &q);
        assert_eq!(page.total, 3);
        assert!(ids(&page).iter().all(|id| *id != "t3k:4"));

        // AND across facets: amp + ir-only cannot both hold.
        let q = Query {
            kinds: vec![Kind::Amp],
            cab_ir: vec![CabIr::IrOnly],
            ..Query::default()
        };
        assert_eq!(list_models(&idx, &q).total, 0);

        // Cab/IR facet.
        let q = Query {
            cab_ir: vec![CabIr::WithCab],
            ..Query::default()
        };
        assert_eq!(list_models(&idx, &q).total, 2);

        let q = Query {
            cab_ir: vec![CabIr::WithoutCab],
            ..Query::default()
        };
        assert_eq!(list_models(&idx, &q).total, 2);

        let q = Query {
            cab_ir: vec![CabIr::IrOnly],
            ..Query::default()
        };
        assert_eq!(list_models(&idx, &q).total, 1);

        // Tags facet, case-insensitive.
        let q = Query {
            tags: vec!["METAL".to_string()],
            ..Query::default()
        };
        assert_eq!(list_models(&idx, &q).total, 1);

        // Format facet.
        let q = Query {
            formats: vec![Format::Ir],
            ..Query::default()
        };
        assert_eq!(list_models(&idx, &q).total, 1);
    }

    #[test]
    fn state_facet_separates_downloaded_not_downloaded_and_stale() {
        let mut idx = index();
        idx.models[1] = downloaded(idx.models[1].clone(), "2026-11-10T10:00:00Z", true);

        let q = Query {
            states: vec![State::Downloaded],
            ..Query::default()
        };
        assert_eq!(list_models(&idx, &q).total, 2);

        let q = Query {
            states: vec![State::NotDownloaded],
            ..Query::default()
        };
        assert_eq!(list_models(&idx, &q).total, 3);

        let q = Query {
            states: vec![State::Stale],
            ..Query::default()
        };
        assert_eq!(list_models(&idx, &q).total, 1);

        // `all` is a no-op.
        let q = Query {
            states: vec![State::All],
            ..Query::default()
        };
        assert_eq!(list_models(&idx, &q).total, 5);
    }

    #[test]
    fn facets_ignore_their_own_filter_but_honour_the_others() {
        let idx = index();
        let q = Query {
            kinds: vec![Kind::Amp],
            ..Query::default()
        };
        let page = list_models(&idx, &q);

        // The kind facet still lists the values excluded by the kind filter.
        let kind_values: Vec<&str> = page.facets.kind.iter().map(|f| f.value.as_str()).collect();
        assert!(kind_values.contains(&"amp"));
        assert!(kind_values.contains(&"pedal"));
        assert!(kind_values.contains(&"other"));

        // Counts come from the whole match set (no filters): 2 amps.
        let amp = page.facets.kind.iter().find(|f| f.value == "amp").unwrap();
        assert_eq!(amp.count, 2);

        // The manufacturer facet honours the kind filter: only the Mesa amp
        // remains, so the Mesa Boogie count is 1.
        let mesa = page
            .facets
            .manufacturer
            .iter()
            .find(|f| f.value == "Mesa Boogie")
            .unwrap();
        assert_eq!(mesa.count, 1);
    }

    #[test]
    fn paging_applies_after_faceting_and_is_clamped() {
        let idx = index();
        let mut q = Query {
            page: 1,
            page_size: 2,
            ..Query::default()
        };
        let page = list_models(&idx, &q);
        assert_eq!(page.total, 5);
        assert_eq!(page.models.len(), 2);

        q.page = 3;
        let page = list_models(&idx, &q);
        assert_eq!(page.models.len(), 1);

        q.page = 99;
        let page = list_models(&idx, &q);
        assert!(page.models.is_empty());
        assert_eq!(page.total, 5);

        // Over-max page sizes are clamped, not rejected.
        let q = Query {
            page_size: 5000,
            ..Query::default()
        };
        assert_eq!(list_models(&idx, &q).page_size, MAX_PAGE_SIZE);

        // A missing page size defaults to 100.
        assert_eq!(
            list_models(&idx, &Query::default()).page_size,
            DEFAULT_PAGE_SIZE
        );
    }

    #[test]
    fn sorts_are_stable_and_tie_break_on_id() {
        let idx = index();

        let q = Query {
            sort: Some(Sort::Name),
            ..Query::default()
        };
        let page = list_models(&idx, &q);
        let names: Vec<&str> = page
            .models
            .iter()
            .map(|r| r.metadata.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec![
                "4x12 Cabinet IR",
                "Fender Super Reverb",
                "Mesa Boogie Dual Rectifier",
                "Mesa Car Réctifier",
                "OD808 Overdrive",
            ]
        );

        // Default with no query is most-downloaded; all ties here, so id order.
        let page = list_models(&idx, &Query::default());
        assert_eq!(
            ids(&page),
            vec!["t3k:1", "t3k:2", "t3k:3", "t3k:4", "t3k:5"]
        );
    }

    #[test]
    fn downloads_and_favorites_sorts_put_the_highest_stats_first() {
        // Spec §6: "Most downloaded" / "Most favorited" are descending.
        let mut idx = index();
        for (i, r) in idx.models.iter_mut().enumerate() {
            let stats = r.metadata.stats.get_or_insert(Stats {
                favorites: 0,
                downloads: 0,
            });
            stats.downloads = 100 + i as u64 * 10;
            stats.favorites = 200 - i as u64 * 10;
        }
        let page = list_models(
            &idx,
            &Query {
                sort: Some(Sort::Downloads),
                ..Query::default()
            },
        );
        assert_eq!(ids(&page), vec!["t3k:5", "t3k:4", "t3k:3", "t3k:2", "t3k:1"]);

        let page = list_models(
            &idx,
            &Query {
                sort: Some(Sort::Favorites),
                ..Query::default()
            },
        );
        assert_eq!(ids(&page), vec!["t3k:1", "t3k:2", "t3k:3", "t3k:4", "t3k:5"]);
    }

    #[test]
    fn added_sort_puts_downloaded_records_first_by_date() {
        let idx = index();
        let q = Query {
            sort: Some(Sort::Added),
            ..Query::default()
        };
        let page = list_models(&idx, &q);
        assert_eq!(page.models[0].id, "t3k:1");
        // Everything else has no download date and ties on id.
        assert_eq!(page.models[1].id, "t3k:2");
    }

    #[test]
    fn wire_payload_parses_and_rejects_unknown_values() {
        let value = serde_json::json!({
            "query": "mesa",
            "kinds": ["amp", "pedal"],
            "manufacturers": ["Mesa Boogie"],
            "cabIr": ["with-cab"],
            "tags": ["metal"],
            "formats": ["nam"],
            "states": ["downloaded"],
            "sort": "newest",
            "page": 2,
            "pageSize": 50
        });
        let q = Query::from_value(&value).expect("valid payload");
        assert_eq!(q.page, 2);
        assert_eq!(q.page_size, 50);
        assert_eq!(q.sort, Some(Sort::Newest));
        assert_eq!(q.cab_ir, vec![CabIr::WithCab]);
        assert_eq!(q.kinds, vec![Kind::Amp, Kind::Pedal]);

        let bad = serde_json::json!({ "sort": "popularity" });
        assert!(Query::from_value(&bad).is_err());

        let bad = serde_json::json!({ "kinds": ["amp", "wobble"] });
        assert!(Query::from_value(&bad).is_err());

        let bad = serde_json::json!({ "pageSize": -1 });
        assert!(Query::from_value(&bad).is_err());

        let bad = serde_json::json!({ "states": "downloaded" });
        assert!(Query::from_value(&bad).is_err());
    }

    #[test]
    fn page_serializes_with_the_frozen_camel_case_wire_keys() {
        let page = list_models(&index(), &Query::default());
        let value = serde_json::to_value(&page).expect("serializes");
        assert!(value.get("pageSize").is_some());
        assert!(value.get("page_size").is_none());
        assert!(value.get("total").is_some());
        assert!(value.get("models").is_some());
        assert!(value.get("facets").is_some());
        assert!(value["facets"].get("cabIr").is_some());
        assert!(value["facets"].get("cab_ir").is_none());
    }
}
