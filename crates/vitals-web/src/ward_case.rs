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

// ── the case's own words, told about the person in the bed ──────────────────

/// Fill a pack's placeholders from the persona.
///
/// The compiler writes `{age}`, `{sex_word}`, `{he_she}`, `{his_her}`, `{him_her}`,
/// `{himself_herself}` and capitalised forms of each, because the ward renames every patient it
/// admits and moves her country: prose with "26-year-old man" baked into it is prose about
/// somebody else the moment she is placed. A capital first letter means "capitalise the fill".
///
/// **Filled where prose leaves the pack, never stored filled.** The pack in the store is the
/// compiler's own bytes — the ones the admission committed to — and a rendered copy would be a
/// second version of the case with nobody's name on it.
///
/// A brace word this build does not know is left exactly as written. A pack that invents one
/// should look wrong on the page rather than quietly lose a word.
pub fn fill_persona(text: &str, who: &crate::ward::Persona) -> String {
    let female = who.sex.eq_ignore_ascii_case("f");
    let male = who.sex.eq_ignore_ascii_case("m");
    let child = who.age < 16;
    let pick = |f: &'static str, m: &'static str, neither: &'static str| -> &'static str {
        if female { f } else if male { m } else { neither }
    };
    let fill = |name: &str| -> Option<String> {
        Some(match name {
            "age" => who.age.to_string(),
            // The one word that turns on age as well as sex: a six-year-old is a girl, not a woman.
            "sex_word" => pick(
                if child { "girl" } else { "woman" },
                if child { "boy" } else { "man" },
                "patient",
            )
            .to_string(),
            "he_she" => pick("she", "he", "they").to_string(),
            "his_her" => pick("her", "his", "their").to_string(),
            "him_her" => pick("her", "him", "them").to_string(),
            "himself_herself" => pick("herself", "himself", "themselves").to_string(),
            _ => return None,
        })
    };

    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('}').map(|i| open + i) else {
            break;
        };
        let name = &rest[open + 1..close];
        // A leading capital asks for a capitalised fill: `{Sex_word}` at the start of a sentence.
        let capitalise = name.starts_with(|c: char| c.is_ascii_uppercase());
        let lower = name.to_ascii_lowercase();
        match fill(&lower) {
            Some(value) if !name.is_empty() => {
                if capitalise {
                    let mut cs = value.chars();
                    if let Some(first) = cs.next() {
                        out.push_str(&first.to_uppercase().to_string());
                        out.push_str(cs.as_str());
                    }
                } else {
                    out.push_str(&value);
                }
            }
            // Not ours: left as written, braces and all.
            _ => out.push_str(&rest[open..=close]),
        }
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out
}

/// What the ward's page is given about the case in front of it.
///
/// Every word from the pack and every person from the persona. The page had been reading the
/// season's own table for all of this, which holds the sixteen cases of vitals.academy and nothing
/// the factory compiles — so a World-case patient was rendered as EP1's.
/// What is said when the case wrote no words for this question.
///
/// The pack's own sentence where it has one — it is the case author's patient and they may have
/// written how she refuses — and otherwise the ward's. The pronoun is the pronoun of the person in
/// the bed: the ward admits men, and a hard-coded "she" over Rafael Moreira is the page
/// contradicting its own chart. An unknown sex is answered the way patient.rs `pronouns()` answers
/// it — "this patient" rather than a singular "they", which turns this sentence and the twelve
/// like it into typos.
fn no_answer(pack: &Value, who: &crate::ward::Persona) -> String {
    if let Some(own) = pack.get("no_answer").and_then(Value::as_str) {
        return fill_persona(own, who);
    }
    let subject = match who.sex.to_ascii_uppercase().as_str() {
        "F" => "she",
        "M" => "he",
        _ => "this patient",
    };
    format!("— {subject} does not answer that, and the case does not say why")
}

/// What the patient said, and what the ward understood the question to be.
pub struct Answer {
    /// The `ask_` intervention this question was understood as, when the case knows it. `None` is
    /// not an error: it is a question this case was never written to answer, and the ward says so.
    pub matched: Option<String>,
    /// What is shown at the bedside. Always something: silence at a bed reads as a broken page.
    pub words: String,
}

/// What she says when she is asked, on a ward where nothing answers but the case file.
///
/// In the bay her voice is a language model with her persona in front of it. A public ward cannot
/// have that: inference costs money per question and is metered per month, so a fair's worth of
/// strangers would spend the ceiling in an afternoon and everybody after them would be told the
/// patient has nothing to say. The ward host runs with no gateway at all.
///
/// It does not need one. The compiler writes a voice entry for every `ask_` intervention — her own
/// words, by the case's author, against that exact finding — so the answer here is a lookup, and a
/// lookup is a better answer than an improvisation with more ways to be wrong.
///
/// The question is matched the way the engine matches an order (`runtime::match_intervention`):
/// canonicalised on both sides so an IME's full-width text and a case authored in Japanese meet in
/// the middle, the author's `not_kw` exclusions kept, first match in the pack's own order. Only
/// `ask_` interventions: the ask bar is a conversation, and "crystalloid bolus" belongs to the
/// tray.
pub fn answer(pack: &Value, who: &crate::ward::Persona, q: &str) -> Answer {
    let matched = asked_about(pack, q);
    let words = matched
        .as_deref()
        .and_then(|id| pack.get("voice")?.get(id)?.get("words")?.as_str())
        .map(|w| fill_persona(w, who))
        .unwrap_or_else(|| no_answer(pack, who));
    Answer { matched, words }
}

/// Which `ask_` this question is, in the case's own words for it.
fn asked_about(pack: &Value, q: &str) -> Option<String> {
    let asks = pack
        .get("sce")?
        .get("interventions")?
        .as_array()?
        .iter()
        .filter(|iv| iv.get("id").and_then(Value::as_str).is_some_and(|id| id.starts_with("ask_")));

    let t = vitals_sce::text::canon(q).to_lowercase();
    let has = |k: &str| !k.is_empty() && t.contains(&vitals_sce::text::canon(k).to_lowercase());
    let words = |m: Option<&Value>, k: &str| -> Vec<String> {
        m.and_then(|m| m.get(k))
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default()
    };

    for iv in asks {
        let id = iv.get("id").and_then(Value::as_str).unwrap_or_default();
        // The chip presses the intervention itself. Most packs list the id among the keywords and
        // would match below anyway; a pack that does not still has to answer its own chip.
        if q.trim() == id {
            return Some(id.to_string());
        }
        let m = iv.get("match");
        let any_kw = words(m, "any_kw");
        let not_kw = words(m, "not_kw");
        let groups: Vec<Vec<String>> = m
            .and_then(|m| m.get("all_groups"))
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|g| g.as_array().map(|g| g.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default())
                    .collect()
            })
            .unwrap_or_default();

        let has_positive = !any_kw.is_empty() || !groups.is_empty();
        let any_ok = any_kw.is_empty() || any_kw.iter().any(|k| has(k));
        let groups_ok = groups.iter().all(|g| g.iter().any(|k| has(k)));
        let not_ok = !not_kw.iter().any(|k| has(k));
        if has_positive && any_ok && groups_ok && not_ok {
            return Some(id.to_string());
        }
    }
    None
}

pub fn case_view(pack: &Value, who: &crate::ward::Persona) -> Value {
    let say = |v: Option<&Value>| v.and_then(Value::as_str).map(|s| fill_persona(s, who));
    let presentation = pack.get("presentation");

    // The chips, in the vocabulary the compiler uses for intervention ids. A row with nothing in
    // it is a row the page does not draw: this case simply has no labs, rather than the ward
    // hiding them.
    let mut chips: std::collections::BTreeMap<&str, Vec<Value>> = Default::default();
    for i in pack.get("sce").and_then(|s| s.get("interventions")).and_then(Value::as_array).into_iter().flatten() {
        let (Some(id), Some(label)) = (i.get("id").and_then(Value::as_str), say(i.get("label"))) else {
            continue;
        };
        let row = match id.split('_').next().unwrap_or_default() {
            "ask" => "ask",
            "exam" => "exam",
            "ix" => "lab",
            "tx" => "treat",
            "dx" => "dx",
            _ => continue,
        };
        chips.entry(row).or_default().push(serde_json::json!({ "id": id, "label": label }));
    }

    // What she says when she is asked. The words are the case author's, in her person; a question
    // the case never wrote an answer for gets the pack's own line, or the ward's if it has none.
    let voice: serde_json::Map<String, Value> = pack
        .get("voice")
        .and_then(Value::as_object)
        .map(|v| {
            v.iter()
                .filter_map(|(k, entry)| {
                    say(entry.get("words")).map(|w| (k.clone(), Value::String(w)))
                })
                .collect()
        })
        .unwrap_or_default();

    serde_json::json!({
        "case_id": pack.get("case_id").and_then(Value::as_str).unwrap_or_default(),
        // Who is in the bed, in the one string the page reads three ways: the bed label prints it,
        // `ageOf` parses the number for the monitor's alarm limits, and `pro()` takes the
        // patient's pronoun out of it. On a station the case's author writes it; here the person
        // is the ward's, and a card carrying the author's patient gets all three wrong.
        "who": format!("{} · {} {}", who.name, who.sex.to_ascii_uppercase(), who.age),
        "title": say(pack.get("title")).unwrap_or_default(),
        "presents": presentation.and_then(|p| say(p.get("chief_complaint"))).unwrap_or_default(),
        "story": presentation.and_then(|p| say(p.get("hpi"))).unwrap_or_default(),
        "setting": presentation.and_then(|p| say(p.get("setting"))),
        // The care setting and *not* the specialty. "eir-surgery" printed over a three-week fever
        // with sudden abdominal pain tells a candidate it is not the heart, which is the question
        // this case is asking — the season took the organ specialty off the endpoint that feeds
        // the sheet for exactly that reason (bay.js `bandOf`), and a public ward that marks a
        // diagnosis is no different. Where the patient presented leaks nothing: everybody arrives
        // somewhere.
        "care_setting": pack.get("care_setting").and_then(Value::as_str),
        "difficulty": pack.get("difficulty").and_then(Value::as_str).unwrap_or_default(),
        "archetype": pack.get("archetype_label").and_then(Value::as_str),
        "chips": chips,
        "voice": voice,
        "no_answer": no_answer(pack, who),
    })
}
