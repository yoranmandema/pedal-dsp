//! The local model index, and the preset `model` references that use it.
//!
//! A preset's NAM block names its model either by **index id** (`"t3k:379978"`)
//! or by a **legacy path** (`"../models/foo.nam"`). At load time
//! [`resolver`] maps an id through the cached index's `local_path`; anything the
//! index does not know is resolved as a path, exactly as before. See
//! `docs/MODEL-BROWSE-AND-CACHE.md` §10.1.
//!
//! The index itself is owned by the model-index builder; this module only reads
//! the four fields preset loading needs, so it keeps working against a newer
//! index schema.

use std::path::{Path, PathBuf};

use dsp_graph::{Doc, IndexResolver, ModelIndex, ModelRecord};
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};

/// The directory downloaded models live in: `$PEDAL_MODELS`, else
/// `$PEDAL_HOME/models`, else the bundled `models/` beside the presets.
pub fn models_dir() -> PathBuf {
    if let Some(v) = non_empty_env("PEDAL_MODELS") {
        return PathBuf::from(v);
    }
    match home() {
        Some(h) => h.join("models"),
        // `PEDAL_PRESETS` can point anywhere; the bundled models still sit next
        // to the preset directory.
        None => crate::store::preset_dir()
            .parent()
            .map(|p| p.join("models"))
            .unwrap_or_else(|| PathBuf::from("models")),
    }
}

/// The cached index file: `$PEDAL_HOME/index/models.json`.
///
/// `None` when no home can be determined at all.
pub fn index_file() -> Option<PathBuf> {
    home().map(|h| index_file_in(&h))
}

/// The index path for an explicit home (tests, and callers that manage their
/// own root).
pub fn index_file_in(home: &Path) -> PathBuf {
    home.join("index").join("models.json")
}

/// `PEDAL_HOME`, else the dev checkout root (an ancestor of the executable that
/// contains `presets/`, `models/` and `Cargo.toml`).
pub fn home() -> Option<PathBuf> {
    if let Some(v) = non_empty_env("PEDAL_HOME") {
        return Some(PathBuf::from(v));
    }
    dev_home()
}

fn dev_home() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    for dir in exe.ancestors().skip(1) {
        if dir.join("presets").is_dir()
            && dir.join("models").is_dir()
            && dir.join("Cargo.toml").is_file()
        {
            return Some(dir.to_path_buf());
        }
    }
    None
}

/// Load the cached index. An absent or unreadable index is an empty index, so
/// preset loading falls back to paths; the caller decides whether to log it.
pub fn load_index() -> ModelIndex {
    let Some(path) = index_file() else {
        return ModelIndex::default();
    };
    match ModelIndex::load(&path) {
        Ok(Some(index)) => index,
        Ok(None) => ModelIndex::default(),
        Err(e) => {
            eprintln!("pedal: ignoring unreadable model index: {e}");
            ModelIndex::default()
        }
    }
}

/// The resolver preset loading uses: index ids through `local_path`, anything
/// else as a legacy path.
pub fn resolver() -> IndexResolver {
    IndexResolver::new(load_index(), models_dir())
}

/// What [`migrate_preset_models`] did, for logging.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MigrationReport {
    /// Preset stems whose `model` references were rewritten to an index id.
    pub migrated: Vec<String>,
    /// `(preset, reference)` pairs left as paths because the index has no
    /// downloaded record for that model.
    pub skipped: Vec<(String, String)>,
}

impl MigrationReport {
    pub fn is_empty(&self) -> bool {
        self.migrated.is_empty() && self.skipped.is_empty()
    }
}

/// Rewrite path-based `model` references in every preset under `presets_dir` to
/// the index id of the model they point at.
///
/// A reference is migrated only when the index holds a **downloaded** record for
/// that model, so a migrated preset can always be resolved. A model that is not
/// in the index yet keeps its path and is reported, so the caller can log it.
pub fn migrate_preset_models(presets_dir: &Path, index: &ModelIndex) -> MigrationReport {
    let mut report = MigrationReport::default();
    let mut files: Vec<PathBuf> = match std::fs::read_dir(presets_dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect(),
        Err(_) => return report,
    };
    files.sort();
    for path in files {
        let stem = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(mut doc) = Doc::from_json(&text) else {
            continue;
        };
        let mut changed = false;
        for node in &mut doc.nodes {
            let Some(reference) = node.model.clone() else {
                continue;
            };
            if index.contains(&reference) {
                continue;
            }
            match lookup_model(&reference, presets_dir, index) {
                Some(id) => {
                    node.model = Some(id);
                    changed = true;
                }
                None => report.skipped.push((stem.clone(), reference)),
            }
        }
        if changed {
            if std::fs::write(&path, doc.to_json()).is_ok() {
                report.migrated.push(stem);
            }
        }
    }
    report
}

/// The index id for a legacy `model` path, when the file can be matched to a
/// downloaded record. Matched by the `.nam` file's `metadata.weights_hash`
/// (provider-independent) or, failing that, its `metadata.t3k_id`.
fn lookup_model(reference: &str, presets_dir: &Path, index: &ModelIndex) -> Option<String> {
    let path = if Path::new(reference).is_absolute() {
        PathBuf::from(reference)
    } else {
        presets_dir.join(reference)
    };
    let text = std::fs::read_to_string(path).ok()?;
    let file: serde_json::Value = serde_json::from_str(&text).ok()?;
    let metadata = file.get("metadata")?;

    let downloaded = |r: &ModelRecord| r.local_path.is_some();

    if let Some(hash) = metadata.get("weights_hash").and_then(|v| v.as_str()) {
        if let Some(record) = index.by_content_hash(hash) {
            if downloaded(record) {
                return Some(record.id.clone());
            }
        }
    }
    let model_id = match metadata.get("t3k_id") {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(serde_json::Value::Number(n)) => n.as_i64().map(|v| v.to_string()),
        _ => None,
    };
    if let Some(record) = index.by_provider_model("t3k", model_id.as_deref()?) {
        if downloaded(record) {
            return Some(record.id.clone());
        }
    }
    None
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal `.nam` with the two metadata fields migration matches on.
    fn nam(weights_hash: &str, t3k_id: i64) -> String {
        serde_json::json!({
            "version": "0.7.0",
            "metadata": {
                "name": "Test",
                "weights_hash": weights_hash,
                "t3k_id": t3k_id,
            },
            "architecture": "WaveNet",
            "config": {},
            "weights": [],
            "sample_rate": 48000.0,
        })
        .to_string()
    }

    fn index() -> ModelIndex {
        ModelIndex::from_json(
            &serde_json::json!({
                "schema": 1,
                "provider": "t3k",
                "count": 2,
                "models": [
                    {
                        "id": "t3k:872412",
                        "provider": "t3k",
                        "local_path": "t3k/106343/872412_a2.nam",
                        "content_hash": "nam-content-v1:8cbd",
                    },
                    {
                        "id": "t3k:999999",
                        "provider": "t3k",
                        "local_path": null,
                        "content_hash": "nam-content-v1:ffff",
                    }
                ]
            })
            .to_string(),
        )
        .unwrap()
    }

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pedal-migrate-{label}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn preset(model: &str) -> String {
        serde_json::json!({
            "name": "nam",
            "nodes": [
                {"id": "in", "block": "io.input"},
                {"id": "nam", "block": "mod.nam", "model": model},
                {"id": "out", "block": "io.output"}
            ],
            "edges": [
                {"from": ["in", "out"], "to": ["nam", "in"]},
                {"from": ["nam", "out"], "to": ["out", "in"]}
            ]
        })
        .to_string()
    }

    #[test]
    fn migrates_a_path_to_the_index_id() {
        let dir = temp_dir("hit");
        let presets = dir.join("presets");
        std::fs::create_dir_all(&presets).unwrap();
        std::fs::create_dir_all(dir.join("models")).unwrap();
        std::fs::write(
            dir.join("models/MES DRECF.nam"),
            nam("nam-content-v1:8cbd", 872412),
        )
        .unwrap();
        std::fs::write(
            presets.join("07-nam.json"),
            preset("../models/MES DRECF.nam"),
        )
        .unwrap();

        let report = migrate_preset_models(&presets, &index());
        assert_eq!(report.migrated, vec!["07-nam".to_string()]);
        assert!(report.skipped.is_empty());
        let migrated = std::fs::read_to_string(presets.join("07-nam.json")).unwrap();
        let doc = Doc::from_json(&migrated).unwrap();
        assert_eq!(
            doc.node("nam").unwrap().model.as_deref(),
            Some("t3k:872412")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn matches_by_t3k_id_when_the_hash_is_absent() {
        let dir = temp_dir("t3k-id");
        let presets = dir.join("presets");
        std::fs::create_dir_all(&presets).unwrap();
        std::fs::write(presets.join("x.nam"), nam("nam-content-v1:other", 872412)).unwrap();
        std::fs::write(presets.join("p.json"), preset("x.nam")).unwrap();

        let report = migrate_preset_models(&presets, &index());
        assert_eq!(report.migrated, vec!["p".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn leaves_a_model_the_index_lacks_and_reports_it() {
        let dir = temp_dir("miss");
        let presets = dir.join("presets");
        std::fs::create_dir_all(&presets).unwrap();
        std::fs::create_dir_all(dir.join("models")).unwrap();
        std::fs::write(
            dir.join("models/unknown.nam"),
            nam("nam-content-v1:dead", 4242),
        )
        .unwrap();
        std::fs::write(
            presets.join("08-other.json"),
            preset("../models/unknown.nam"),
        )
        .unwrap();
        let before = std::fs::read_to_string(presets.join("08-other.json")).unwrap();

        let report = migrate_preset_models(&presets, &index());
        assert!(report.migrated.is_empty());
        assert_eq!(
            report.skipped,
            vec![("08-other".to_string(), "../models/unknown.nam".to_string())]
        );
        assert_eq!(
            std::fs::read_to_string(presets.join("08-other.json")).unwrap(),
            before
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn does_not_migrate_a_record_that_is_not_downloaded() {
        let dir = temp_dir("not-downloaded");
        let presets = dir.join("presets");
        std::fs::create_dir_all(&presets).unwrap();
        std::fs::create_dir_all(dir.join("models")).unwrap();
        // Hash matches t3k:999999, but that record has no local_path.
        std::fs::write(dir.join("models/y.nam"), nam("nam-content-v1:ffff", 999999)).unwrap();
        std::fs::write(presets.join("p.json"), preset("../models/y.nam")).unwrap();

        let report = migrate_preset_models(&presets, &index());
        assert!(report.migrated.is_empty());
        assert_eq!(report.skipped.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The shipped OD808 preset, once migrated, resolves through the index and
    /// builds — the whole pipeline against the real bundled model.
    #[test]
    fn shipped_od808_preset_migrates_and_resolves_through_the_index() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let real_model = repo.join("models/MES DRECF RD Gain2 BAL O808 CAB.nam");
        let real_preset = repo.join("presets/07-nam-od808.json");
        if !real_model.is_file() || !real_preset.is_file() {
            return; // the capture is not bundled in this checkout
        }
        let file: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&real_model).unwrap()).unwrap();
        let hash = file["metadata"]["weights_hash"]
            .as_str()
            .unwrap()
            .to_string();
        let t3k_id = file["metadata"]["t3k_id"].as_i64().unwrap();

        let dir = temp_dir("od808");
        let presets = dir.join("presets");
        let models = dir.join("models");
        std::fs::create_dir_all(&presets).unwrap();
        std::fs::create_dir_all(models.join("t3k/106343")).unwrap();
        std::fs::copy(
            &real_model,
            models.join("MES DRECF RD Gain2 BAL O808 CAB.nam"),
        )
        .unwrap();
        std::fs::write(
            presets.join("07-nam-od808.json"),
            std::fs::read_to_string(&real_preset).unwrap(),
        )
        .unwrap();

        let index = ModelIndex::from_json(
            &serde_json::json!({
                "schema": 1,
                "provider": "t3k",
                "count": 1,
                "models": [{
                    "id": format!("t3k:{t3k_id}"),
                    "provider": "t3k",
                    "local_path": format!("t3k/106343/{t3k_id}_a2.nam"),
                    "content_hash": hash,
                }]
            })
            .to_string(),
        )
        .unwrap();

        let report = migrate_preset_models(&presets, &index);
        assert_eq!(report.migrated, vec!["07-nam-od808".to_string()]);

        // The model is now downloaded at the index's `local_path`.
        std::fs::copy(
            &real_model,
            models.join(format!("t3k/106343/{t3k_id}_a2.nam")),
        )
        .unwrap();

        let migrated = std::fs::read_to_string(presets.join("07-nam-od808.json")).unwrap();
        let doc = Doc::from_json(&migrated).unwrap();
        assert_eq!(
            doc.node("nam").unwrap().model.as_deref(),
            Some(format!("t3k:{t3k_id}").as_str())
        );

        let resolver = IndexResolver::new(index, models.clone());
        let reg = dsp_graph::blocks::builtin_registry();
        doc.build_in_with(&reg, &presets, &resolver)
            .expect("migrated preset resolves through the index");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn index_path_is_under_home() {
        assert_eq!(
            index_file_in(Path::new("/home/u/app.pedal")),
            PathBuf::from("/home/u/app.pedal/index/models.json")
        );
    }
}

/// One `local` block of an index record, matching
/// docs/MODEL-BROWSE-AND-CACHE.md §9.3. `checksum` is the sha256 of the
/// downloaded file, stored with its `sha256:` prefix.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalState {
    pub file_name: String,
    pub size_bytes: u64,
    pub checksum: String,
    pub downloaded_at: String,
    #[serde(default)]
    pub stale: bool,
}

/// One full cached index record, matching
/// docs/MODEL-BROWSE-AND-CACHE.md §9.3. This is the shape the UI and the
/// engine itself only reads `model`; the browser API serialises this record
/// verbatim over IPC.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FullModelRecord {
    pub id: String,
    pub provider: String,
    pub tone_id: String,
    pub source_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    pub fetched_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local: Option<LocalState>,
    /// `metadata` is kept as JSON so a record round-trips unchanged even though
    /// the engine itself only reads `model`; the browser API serialises this
    /// record verbatim over IPC.
    pub metadata: Value,
}

/// The full cached index document, matching §9.1/§9.3. This is what is written
/// to `index/models.json`; it survives refresh and download updates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct FullIndex {
    pub schema: u32,
    pub provider: String,
    pub generated_at: String,
    pub count: usize,
    pub models: Vec<FullModelRecord>,
}

impl FullIndex {
    /// Parse the index document at `path`; `Ok(None)` when it does not exist.
    pub fn load(path: &Path) -> Result<Option<Self>, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_str(&text).map(Some),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("reading index {}: {e}", path.display())),
        }
    }

    fn from_str(text: &str) -> Result<Self, String> {
        let doc: Value = serde_json::from_str(text).map_err(|e| format!("index parse: {e}"))?;
        let inst = FullIndex::try_from(&doc)?;
        if inst.count != inst.models.len() {
            return Err(format!(
                "index count {} != models len {}",
                inst.count,
                inst.models.len()
            ));
        }
        Ok(inst)
    }

    /// Atomically replace the index file with `index` (`models.json.tmp` → rename
    /// over the old file; `models.json.bak` keeps the previous good generation,
    /// like the refresh).
    pub fn save_atomic(self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("index dir: {e}"))?;
        }
        let tmp = path.with_file_name(format!(
            "{}.tmp",
            path.file_name().unwrap_or_default().to_string_lossy()
        ));
        let bak = path.with_file_name(format!(
            "{}.bak",
            path.file_name().unwrap_or_default().to_string_lossy()
        ));
        let text =
            serde_json::to_string_pretty(&self).map_err(|e| format!("index serialize: {e}"))?;
        std::fs::write(&tmp, text).map_err(|e| format!("index write tmp: {e}"))?;
        // Revoke the previous good generation, then publish the new one.
        let _ = std::fs::rename(path, &bak);
        std::fs::rename(&tmp, path).map_err(|e| format!("index publish: {e}"))?;
        // Drop the rollback copy once we have published.
        let _ = std::fs::remove_file(&bak);
        Ok(())
    }
}

impl TryFrom<&Value> for FullIndex {
    type Error = String;

    fn try_from(value: &Value) -> Result<Self, Self::Error> {
        let schema = value
            .get("schema")
            .and_then(|v| v.as_u64())
            .ok_or("missing schema")? as u32;
        let provider = value
            .get("provider")
            .and_then(|v| v.as_str())
            .ok_or("missing provider")?
            .to_string();
        let generated_at = value
            .get("generated_at")
            .and_then(|v| v.as_str())
            .ok_or("missing generated_at")?
            .to_string();
        let count = value
            .get("count")
            .and_then(|v| v.as_u64())
            .ok_or("missing count")? as usize;
        let models = value
            .get("models")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let models = models
            .into_iter()
            .map(|m| -> Result<FullModelRecord, String> {
                let rec: FullModelRecord =
                    serde_json::from_value(m).map_err(|e| format!("record: {e}"))?;
                Ok(rec)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            schema,
            provider,
            generated_at,
            count,
            models,
        })
    }
}

/// Load the cached index as the full document. An absent or unreadable index
/// is an empty index.
pub fn load_full_index() -> Result<FullIndex, String> {
    let Some(path) = crate::model_index::index_file() else {
        return Ok(FullIndex::default());
    };
    match FullIndex::load(&path) {
        Ok(Some(index)) => Ok(index),
        Ok(None) => Ok(FullIndex::default()),
        Err(e) => Err(e),
    }
}

/// Record a successful download in the in-memory `FullIndex` (the caller then
/// writes it back). `id` is the record id; false if there is no such record.
/// `rel` is the path relative to `$PEDAL_MODELS` (POSIX); `local` carries
/// `file_name`, `size_bytes`, `checksum`, `downloaded_at` and a fresh `stale`.
pub fn record_downloaded(index: &mut FullIndex, id: &str, rel: &str, local: &LocalState) -> bool {
    match index.models.iter_mut().find(|r| r.id == id) {
        Some(rec) => {
            rec.local_path = Some(rel.to_string());
            rec.local = Some(local.clone());
            true
        }
        None => false,
    }
}

/// Current time as RFC 3339 UTC with a `Z` suffix, e.g.
/// `2026-11-15T09:00:00Z`.
pub fn now_utc() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let hh = rem / 3600;
    let mm = (rem % 3600) / 60;
    let ss = rem % 60;
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// Write the `full_index` document to `path` atomically (tmp → publish → remove
/// bak), mirroring the refresh persistence.
pub fn save_full_index(index: &FullIndex, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("index dir: {e}"))?;
    }
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "models.json".to_string());
    let tmp = path.with_file_name(format!("{file_name}.tmp"));
    let bak = path.with_file_name(format!("{file_name}.bak"));
    let text = serde_json::to_string_pretty(index).map_err(|e| format!("index serialize: {e}"))?;
    std::fs::write(&tmp, text).map_err(|e| format!("index write tmp: {e}"))?;
    let _ = std::fs::rename(path, &bak);
    std::fs::rename(&tmp, path).map_err(|e| format!("index publish: {e}"))?;
    let _ = std::fs::remove_file(&bak);
    Ok(())
}

/// Days since 1970-01-01 to a civil date (Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let y = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * y + y / 4 - y / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = y + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}
