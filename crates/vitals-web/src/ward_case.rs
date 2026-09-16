//! The cases the ward plays, and the door they arrive through.
//!
//! **The ward carries none of the season** (founder, 16 ก.ย.: *"ผมไม่ได้ให้เอาเคสของ vitals เดิมมา
//! ใช้ใน world"*). Its patients come from the patient factory and its cases from the case factory:
//! a pack compiled out of embla-cases carrying the scenario the engine already runs, the mark
//! sheet, and the patient's own words for what she is asked.
//!
//! A pack is stored whole and served back whole. This module is the door's opinion about one —
//! what makes a case playable, scorable and checkable on a ward strangers walk into — and nothing
//! else: the fields the compiler writes that the ward does not read are kept untouched rather than
//! modelled, because a type that has to know every field is a type that breaks when the compiler
//! learns a new one.

use serde_json::Value;

/// Where the packs live.
pub const CASE_STORE: &str = "ward_case";

/// The levels the ward publishes, and the only ones it can offer a patient at.
pub const LEVELS: [&str; 3] = ["student", "intern", "resident"];

/// What the ward needs to know about a pack without opening it again.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CaseSummary {
    pub case_id: String,
    /// What kind of deterioration this case is — `haemorrhagic_shock`, `airway_obstruction`. The
    /// compiler's own word, and the one thing a reader of the catalogue wants before opening it.
    pub archetype: String,
    /// ISO 3166-1 alpha-3, or `None` for a case that belongs to no country in particular.
    pub country: Option<String>,
    pub difficulty: String,
    pub endemic: bool,
    /// Compiled but not clinically reviewed. A provisional case may be replaced; a reviewed one
    /// may not, because somebody has played it and the chain carries what they did.
    pub provisional: bool,
    pub version: String,
    pub title: String,
    /// The age the case is written about, and the sex it is written for — the pack's own
    /// `patient{age, sex}`, passed through as the compiler spells it (`male`, `female`).
    ///
    /// The factory matches a person against these: the sex is what the dialogue, the examination
    /// and the differential assume, and the age is what the physiology was tuned for. `None` where
    /// a pack does not say, because "about a man" and "nobody wrote it down" are different answers.
    pub patient_age: Option<u32>,
    pub patient_sex: Option<String>,
}

/// Is this pack one the ward can admit a patient onto?
///
/// Every rule here is a way a case could reach a public ward and be unplayable, unscorable or
/// uncheckable on it. The sentences are written for the person running the compiler, because they
/// are the only person who can fix any of them.
pub fn validate_case(pack: &Value) -> Result<CaseSummary, String> {
    let s = |k: &str| pack.get(k).and_then(Value::as_str).unwrap_or_default().to_string();

    // ── the id ──────────────────────────────────────────────────────────────
    // It is a store key and it is rendered into a page, so it is the narrow shape both can carry.
    let case_id = s("case_id");
    // 120 rather than 64: the compiler names a library case after the case itself, and
    // `embla-hepatic-encephalopathy-precipitated-by-gi-bleeding-resident` is 65 characters of
    // perfectly good id. The limit is here to keep it a store key and a path segment, not to
    // impose a house style on somebody else's library.
    if case_id.is_empty()
        || case_id.len() > 120
        || !case_id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(format!(
            "{case_id:?} is not a case id — lower case, digits and hyphens, up to 120 of them"
        ));
    }

    // ── the season ──────────────────────────────────────────────────────────
    if let Some(found) = season_marker(pack) {
        return Err(format!(
            "this pack names {found:?}, which belongs to the season. The ward plays what the case \
             factory compiles and nothing of vitals.academy's own"
        ));
    }

    // ── who it is offered to ────────────────────────────────────────────────
    let difficulty = s("difficulty");
    if !LEVELS.contains(&difficulty.as_str()) {
        return Err(format!("difficulty {difficulty:?} is not one of {LEVELS:?}"));
    }
    let country = match pack.get("country") {
        None | Some(Value::Null) => None,
        Some(Value::String(c))
            if c.len() == 3 && c.bytes().all(|b| b.is_ascii_uppercase()) =>
        {
            Some(c.clone())
        }
        Some(other) => {
            return Err(format!(
                "country {other} is not ISO 3166-1 alpha-3 — the globe matches on THA, NPL, ETH, \
                 and free text matches nothing"
            ))
        }
    };
    let version = s("version");
    if version.is_empty() {
        return Err("a pack carries the version it was compiled at".into());
    }

    // ── the case itself ─────────────────────────────────────────────────────
    let sce = pack.get("sce").ok_or("a pack carries the scenario the engine runs")?;
    let sce_json = serde_json::to_string(sce).map_err(|e| format!("the scenario will not serialise: {e}"))?;
    let engine = vitals_sce::Sce::from_json(&sce_json)
        .map_err(|e| format!("the scenario does not parse, so nobody could play it: {e}"))?;

    // A stay ends when the engine ends it: a case that cannot end is a bed that never frees, and
    // one that can only end badly is a ward where nothing a stranger does matters.
    let kinds: Vec<&str> = sce
        .get("outcomes")
        .and_then(Value::as_array)
        .map(|o| o.iter().filter_map(|x| x.get("kind").and_then(Value::as_str)).collect())
        .unwrap_or_default();
    if !kinds.contains(&"win") {
        return Err("this case has no way to survive it".into());
    }
    if !kinds.contains(&"death") {
        return Err("this case has no way to die of it".into());
    }

    // ── the mark sheet ──────────────────────────────────────────────────────
    let mut known: std::collections::BTreeSet<String> =
        engine.interventions.iter().map(|i| i.id.clone()).collect();
    if let Some(o) = sce.get("outcomes").and_then(Value::as_array) {
        known.extend(o.iter().filter_map(|x| x.get("id").and_then(Value::as_str)).map(str::to_string));
    }
    // Harm sentences, which a `no_harm` item matches instead of an id. They are the scenario's own
    // words — `debrief.avoid[].why` and any `harm` an intervention produces — and an item paying
    // for a sentence the case never says pays nobody, exactly like a missing id.
    let mut harms: std::collections::BTreeSet<String> = Default::default();
    collect_harms(sce, &mut harms);

    let items = pack
        .get("rubric")
        .and_then(|r| r.get("items"))
        .and_then(Value::as_array)
        .ok_or("a pack carries a mark sheet with items in it")?;
    for item in items {
        let kind = item.get("type").and_then(Value::as_str).unwrap_or_default();
        let needle = item.get("needle").and_then(Value::as_str);
        // Each kind of item names a different kind of thing, and checking them all against the
        // intervention ids is how "did not give a sedative before the airway" — a harm sentence —
        // came back as an id the scenario does not have.
        match kind {
            "no_harm" => {
                if let Some(n) = needle {
                    if !harms.contains(n) {
                        return Err(format!(
                            "the mark sheet pays for not causing {n:?} and this case never says \
                             that, so nobody can avoid it"
                        ));
                    }
                }
            }
            "no_unindicated" => {
                for allowed in item.get("allow").and_then(Value::as_array).into_iter().flatten() {
                    let Some(a) = allowed.as_str() else { continue };
                    if !known.contains(a) {
                        return Err(format!(
                            "the mark sheet clears {a:?} as indicated and the scenario has no \
                             intervention by that name"
                        ));
                    }
                }
            }
            _ => {
                let mut needles: Vec<&str> = needle.into_iter().collect();
                if let Some(any) = item.get("any_of").and_then(Value::as_array) {
                    needles.extend(any.iter().filter_map(Value::as_str));
                }
                for n in needles {
                    if !known.contains(n) {
                        return Err(format!(
                            "the mark sheet pays for {n:?} and the scenario has no intervention or \
                             outcome by that name, so nobody can ever earn it"
                        ));
                    }
                }
            }
        }
    }

    Ok(CaseSummary {
        case_id,
        archetype: s("archetype"),
        country,
        difficulty,
        endemic: pack.get("endemic").and_then(Value::as_bool).unwrap_or(false),
        provisional: pack.get("provisional").and_then(Value::as_bool).unwrap_or(true),
        version,
        title: s("title"),
        patient_age: pack.get("patient").and_then(|p| p.get("age")).and_then(Value::as_u64).map(|a| a as u32),
        patient_sex: pack
            .get("patient")
            .and_then(|p| p.get("sex"))
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

/// A name from the season, anywhere in the pack.
///
/// Whole words: `EP1` through `EP5`, `OSCE`, and the sixteen ids the bay shelves its own cases
/// under. A clinical string may perfectly well contain "ep" or "osce" inside another word, and a
/// door that refused those would be refusing medicine to protect a naming rule.
fn season_marker(pack: &Value) -> Option<String> {
    fn walk(v: &Value, out: &mut Option<String>) {
        if out.is_some() {
            return;
        }
        match v {
            Value::String(s) => {
                for word in s.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')) {
                    let w = word.to_ascii_lowercase();
                    let seasonal = crate::ward::CATALOGUE.contains(&w.as_str())
                        || w == "osce"
                        || (w.len() == 3 && w.starts_with("ep") && w.as_bytes()[2].is_ascii_digit());
                    if seasonal {
                        *out = Some(word.to_string());
                        return;
                    }
                }
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            Value::Object(o) => o.values().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    let mut found = None;
    walk(pack, &mut found);
    found
}

/// Every sentence this case can say about harm.
///
/// `debrief.avoid[].why` is where the compiler puts them and an intervention may carry one of its
/// own. Walked rather than listed: a scenario that grows a second place to say "this hurt her"
/// should not quietly stop being checkable.
fn collect_harms(sce: &Value, out: &mut std::collections::BTreeSet<String>) {
    match sce {
        Value::Object(o) => {
            for (k, v) in o {
                if (k == "why" || k == "harm") && v.is_string() {
                    out.insert(v.as_str().unwrap_or_default().to_string());
                }
                collect_harms(v, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| collect_harms(x, out)),
        _ => {}
    }
}

/// Every case this ward holds, as the catalogue reads them.
///
/// A pack that no longer validates is left out rather than listed: the door refused its kind on
/// the way in, and a case the ward would not accept today is not one it should offer a patient.
pub fn all(store: &crate::store::Store) -> Vec<CaseSummary> {
    store
        .list::<serde_json::Value>(CASE_STORE)
        .into_iter()
        .filter_map(|(_, pack)| validate_case(&pack).ok())
        .collect()
}

/// The name this case is filed under.
///
/// The case id where the store can take it, and a hash of the id where it cannot. A library case
/// is called what its library calls it — `embla-hepatic-encephalopathy-precipitated-by-gi-bleeding-resident`
/// is 65 characters of perfectly good name — and the store's keys are file names capped at 64.
/// Which of those two facts gives way is not a medical question, so it is this one: the filing is
/// ours and the id is theirs. The pack keeps its own `case_id` inside the document, so every
/// reader still sees the library's name and nothing has to know about this.
pub fn key_for(case_id: &str) -> String {
    if crate::store::is_safe_key(case_id) {
        return case_id.to_string();
    }
    use sha2::{Digest, Sha256};
    let h = Sha256::digest(case_id.as_bytes());
    format!("c-{}", h[..20].iter().map(|b| format!("{b:02x}")).collect::<String>())
}

/// The scenario one of the ward's own cases runs, as the engine takes it.
///
/// `None` for anything this ward did not accept through the door — a season id, a case that was
/// never sent, a pack that no longer validates. The caller then says so rather than reaching for a
/// file: `demo/**` is the season's and a ward patient must never be playing one by accident.
pub fn sce_of(store: &crate::store::Store, case_id: &str) -> Option<String> {
    let pack: serde_json::Value = store.get(CASE_STORE, &key_for(case_id))?;
    validate_case(&pack).ok()?;
    serde_json::to_string(pack.get("sce")?).ok()
}

/// Which case the next patient is admitted onto.
///
/// `wanted` is her own pack's choice, honoured whenever the ward holds it — the patient factory
/// names the case it built her for, and it fitted her to it. Everything below is what happens when
/// it named none, or named one this ward has not been sent.
///
/// **The ward fits her before it places her.** It placed Nusrat Jahan, 64, a woman, on a typhoid
/// case written about a 26-year-old man because they were both Bangladeshi, and the case's own
/// presentation opens "Young man from Bangladesh" — the wrong-patient error the ward has made
/// twice already in other forms. So a fallback case must be written for her sex and near her age,
/// and her country is a preference on top of that rather than a reason to ignore it.
///
/// `None` means nobody is admitted this tick, and that is the right answer to "nothing here fits
/// her": an empty bed says nothing false, and a mismatch says several — the dialogue, the
/// examination, the differential and the picture are all about somebody else.
pub fn choose_case<'a>(
    cases: &'a [CaseSummary],
    wanted: Option<&str>,
    who: &crate::ward::Persona,
    difficulty: Option<&str>,
) -> Option<&'a CaseSummary> {
    if let Some(id) = wanted {
        if let Some(hit) = cases.iter().find(|c| c.case_id == id) {
            return Some(hit);
        }
    }
    let newest = |a: &&CaseSummary, b: &&CaseSummary| {
        // Version strings from one compiler, compared piece by piece so 0.10.0 is after 0.9.0.
        let parts = |v: &str| -> Vec<u64> { v.split('.').map(|p| p.parse().unwrap_or(0)).collect() };
        parts(&b.version).cmp(&parts(&a.version)).then(a.case_id.cmp(&b.case_id))
    };
    // `None` is "any level", which is where the ward is today: the patient pack does not carry one
    // yet, so the ticker asks for her country and takes what there is.
    let at_level = |c: &&CaseSummary| difficulty.is_none_or(|d| c.difficulty == d);
    let fits = |c: &&CaseSummary| fits_patient(c, who);
    // Her country first: a Nepali woman on a Nepali case is the whole point of the endemic work.
    // It is a preference among the cases that fit her, never a reason to take one that does not.
    let mut hers: Vec<&CaseSummary> = cases
        .iter()
        .filter(|c| c.country.as_deref() == Some(who.country.as_str()))
        .filter(at_level)
        .filter(fits)
        .collect();
    hers.sort_by(newest);
    if let Some(first) = hers.first() {
        return Some(first);
    }
    let mut any: Vec<&CaseSummary> = cases.iter().filter(at_level).filter(fits).collect();
    any.sort_by(newest);
    any.first().copied()
}

/// Is this case written about somebody like her?
///
/// The sex is the dialogue's, the examination's and the differential's, so it has to match. The
/// age is the physiology's: twelve years either way, and a child only with a child — sixteen and
/// six are not a near miss, and a case tuned for one of them alarms wrongly on the other.
///
/// The two vocabularies meet here and nowhere else: a persona says `f`, a compiled case says
/// `female`, and one of them has to translate. A case that does not say who it is about cannot be
/// fitted to anybody, so it answers `false` — it can still be named outright by a factory that
/// knows what it is doing.
fn fits_patient(c: &CaseSummary, who: &crate::ward::Persona) -> bool {
    let (Some(age), Some(sex)) = (c.patient_age, c.patient_sex.as_deref()) else {
        return false;
    };
    let same_sex = match sex.to_ascii_lowercase().as_str() {
        "male" | "m" => who.sex.eq_ignore_ascii_case("m"),
        "female" | "f" => who.sex.eq_ignore_ascii_case("f"),
        _ => false,
    };
    if !same_sex {
        return false;
    }
    let hers = who.age as i64;
    let theirs = age as i64;
    if (hers < 16) != (theirs < 16) {
        return false;
    }
    (hers - theirs).abs() <= 12
}
