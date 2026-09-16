//! The faces already made: `~/.vitals/world/portraits.json`.
//!
//! Keyed on the person — `<ISO3>-<index>` — with her pictures by state. Seeded from the sixty
//! bases the batch made (at 28, 45 and 63, by index, per `docs/internal/portraits/batch.py`) and
//! the nine full state sets; grown by this job as it makes more.
//!
//! A face has an age, and the door has a band. A base made at 45 does not serve a case written
//! for a man of 25, so a face that fits no band is not "the persona's face" — it is one of her
//! faces, and another is made at the age the pack draws and recorded under `<key>@<age>`. The
//! seeded entries carry no age; theirs is the batch's, by index, and [`batch_age`] says so.

use crate::pool::Person;
use std::collections::BTreeMap;
use std::ops::RangeInclusive;
use std::path::Path;

/// The ages the batch made the sixty bases at, by index within a country.
pub const BATCH_AGES: [u16; 3] = [28, 45, 63];

/// One person's pictures.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct Entry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sex: Option<String>,
    /// The age the base was made at. Absent on the seeded sixty, whose age is the batch's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub age: Option<u16>,
    /// State → url, in the bucket's one shape.
    #[serde(default)]
    pub portrait: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Manifest {
    pub entries: BTreeMap<String, Entry>,
}

/// A base that fits: which entry, at what age, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Base {
    pub key: String,
    pub age: u16,
    pub url: String,
}

/// The batch's age for a seeded key, by index: `THA-0` → 28, `THA-1` → 45, `THA-2` → 63.
/// `None` for an age-keyed entry (`THA-1@71`), which carries its own.
pub fn batch_age(key: &str) -> Option<u16> {
    if key.contains('@') {
        return None;
    }
    let idx: usize = key.rsplit('-').next()?.parse().ok()?;
    Some(BATCH_AGES[idx % BATCH_AGES.len()])
}

impl Manifest {
    /// Both shapes: the seeded `{name, country, sex, portrait: {state: url}}` and the brief's
    /// bare `{state: url}`. A value with a `portrait` field is the first; anything else whose
    /// values are all strings is the second.
    pub fn parse(json: &str) -> Result<Manifest, String> {
        let raw: BTreeMap<String, serde_json::Value> = serde_json::from_str(json).map_err(|e| format!("portraits.json: {e}"))?;
        let mut entries = BTreeMap::new();
        for (key, v) in raw {
            let entry = if v.get("portrait").is_some() || v.get("age").is_some() {
                serde_json::from_value::<Entry>(v).map_err(|e| format!("portraits.json {key}: {e}"))?
            } else {
                let flat: BTreeMap<String, String> = serde_json::from_value(v).map_err(|e| format!("portraits.json {key}: neither shape: {e}"))?;
                Entry { portrait: flat, ..Entry::default() }
            };
            entries.insert(key, entry);
        }
        Ok(Manifest { entries })
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(&self.entries).expect("a map of strings serialises")
    }

    /// A missing file is an empty manifest: the job's first run on a machine has no faces yet.
    pub fn load(path: &Path) -> Result<Manifest, String> {
        match std::fs::read_to_string(path) {
            Ok(s) => Manifest::parse(&s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Manifest::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// Written whole through a temp file and a rename, so a crash mid-write leaves the old file.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        write_atomically(path, &self.to_json())
    }

    /// The age an entry's base was made at: recorded, or the batch's by index.
    pub fn age_of(key: &str, e: &Entry) -> Option<u16> {
        e.age.or_else(|| batch_age(key))
    }

    /// The face of this person that fits the band, if any: her seeded entry, or one made later
    /// at an age inside the band. The nearest to the band's middle when several fit.
    pub fn base_for(&self, key: &str, band: &RangeInclusive<u16>) -> Option<Base> {
        let mid = (u32::from(*band.start()) + u32::from(*band.end())) / 2;
        let prefix = format!("{key}@");
        self.entries
            .iter()
            .filter(|(k, _)| k.as_str() == key || k.starts_with(&prefix))
            .filter_map(|(k, e)| {
                let age = Manifest::age_of(k, e)?;
                let url = e.portrait.get("stable")?;
                band.contains(&age).then(|| Base { key: k.clone(), age, url: clone(url) })
            })
            .min_by_key(|b| (u32::from(b.age).abs_diff(mid), b.key.clone()))
    }

    /// Record a base made for this person at this age. Under her own key when the batch age is
    /// hers and the entry is empty; under `<key>@<age>` otherwise.
    pub fn record_base(&mut self, key: &str, age: u16, url: &str, who: &Person) {
        let slot = match self.entries.get(key) {
            None if batch_age(key) == Some(age) => key.to_string(),
            Some(e) if e.portrait.is_empty() && Manifest::age_of(key, e) == Some(age) => key.to_string(),
            _ => format!("{key}@{age}"),
        };
        let e = self.entries.entry(slot).or_default();
        e.name.get_or_insert_with(|| who.name.clone());
        e.country.get_or_insert_with(|| who.country.clone());
        e.sex.get_or_insert_with(|| who.sex.letter().to_string());
        e.age = Some(age);
        e.portrait.insert("stable".into(), url.to_string());
    }

    /// Add one state's picture to an entry. Add only, like the door.
    pub fn record_state(&mut self, key: &str, state: &str, url: &str) -> bool {
        let e = self.entries.entry(key.to_string()).or_default();
        if e.portrait.contains_key(state) {
            return false;
        }
        e.portrait.insert(state.into(), url.into());
        true
    }

    /// The entry whose `stable` is this url — how a patient on the board is traced back to the
    /// face she was given, whichever key it was recorded under.
    pub fn entry_with_stable(&self, url: &str) -> Option<(&String, &Entry)> {
        self.entries.iter().find(|(_, e)| e.portrait.get("stable").map(String::as_str) == Some(url))
    }
}

fn clone(s: &str) -> String {
    s.to_string()
}

/// Temp file beside the target, then rename: the reader sees the old file or the new one.
pub fn write_atomically(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}
