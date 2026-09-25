//! **The ward's cases come through a door, like its patients.**
//!
//! "ผมไม่ได้ให้เอาเคสของ vitals เดิมมาใช้ใน world" — the founder, 16 ก.ย. The ward stops playing the
//! season's sixteen and plays what the case factory compiles from embla-cases: a pack carrying the
//! scenario the engine already runs, the mark sheet, and the patient's own words.
//!
//! The door's job is to refuse everything that would put a case on a public ward that nobody can
//! play, score or check. Each rule below is one of those.

use serde_json::{json, Value};
use vitals_web::ward_case::{validate_case, CaseSummary};

/// A pack with the shape the compiler emits and the smallest contents that can be true: one
/// treatment, one way to live, one way to die, and a mark sheet that pays for the treatment.
fn a_pack() -> Value {
    json!({
        "case_id": "auth-demo-1",
        "source": { "repo": "embla-cases", "ref": "worktree", "sha256": "a".repeat(64) },
        "title": "ทดสอบ",
        "country": "THA",
        "difficulty": "resident",
        "clinical_tier": 3,
        "specialty": "eir-emergency",
        "care_setting": "ER",
        "language": "th",
        "tags": ["test"],
        "endemic": false,
        "provisional": true,
        "version": "0.1.0",
        "archetype": "haemorrhagic_shock",
        "archetype_label": "haemorrhagic shock",
        "patient": { "age": 62, "sex": "male" },
        "presentation": { "chief_complaint": "อาเจียนเป็นเลือด", "hpi": "…", "setting": null },
        "sce": {
            "tick_seconds": 1.0,
            "setting": "ER",
            "initial_state": "presenting",
            "vitals0": { "hr": 110.0, "sbp": 90.0, "dbp": 60.0, "spo2": 97.0, "rr": 18.0, "temp": 37.0, "gcs": 15 },
            "variables": {},
            "states": [
                { "id": "presenting", "status": "critical", "bands": [], "dynamics": [], "transitions": [] },
                { "id": "stabilising", "status": "improving", "bands": [], "dynamics": [], "transitions": [] }
            ],
            "interventions": [
                { "id": "tx_fluids", "label": "Crystalloid bolus", "match": { "any_kw": ["fluids"] },
                  "effects": [{ "to_state": "stabilising" }] }
            ],
            "triggers": [
                { "id": "recovered", "once": true, "do": [{ "outcome": "win_discharge" }],
                  "when": { "all": [{ "in_state": "stabilising" }, { "op": "ge", "value": 60.0, "var": "t_in_state" }] } },
                { "id": "bled_out", "once": true, "do": [{ "outcome": "death_arrest" }],
                  "when": { "op": "ge", "value": 600.0, "var": "t_sec" } }
            ],
            "outcomes": [
                { "id": "win_discharge", "kind": "win", "label": "Treated in time" },
                { "id": "death_arrest", "kind": "death", "label": "Untreated too long" }
            ],
            "debrief": { "expect": [], "avoid": [] }
        },
        "rubric": {
            "case": "auth-demo-1",
            "pass_bps": 6000,
            "status": "provisional — compiled, not clinically reviewed",
            "items": [
                { "label": "Fluids", "type": "action", "needle": "tx_fluids", "points": 10 },
                { "label": "Survived", "type": "outcome", "any_of": ["win_discharge"], "points": 10 }
            ]
        },
        "voice": { "ask_hematemesis": { "finding": "Hematemesis", "present": true, "reveal": "volunteered", "words": "อ้วกเป็นเลือด" } },
        "management": [],
        "timed": {},
        "vitals_assumed": [],
        "replay": { "untreated_death_sec": 600.0, "win_path": [], "win_sec": 60.0, "win_outcome": "win_discharge",
                    "golden_score": { "earned": 20, "max": 20, "pass_bps": 6000 } },
        "compiler": { "name": "vitals-casefactory", "version": "0.9.4" }
    })
}

#[test]
fn a_compiled_pack_is_admitted() {
    let s: CaseSummary = validate_case(&a_pack()).expect("the compiler's own shape");
    assert_eq!(s.case_id, "auth-demo-1");
    assert_eq!(s.country.as_deref(), Some("THA"));
    assert_eq!(s.difficulty, "resident");
    assert!(s.provisional);
    assert_eq!(s.version, "0.1.0");
}

/// **A case the engine cannot run is a bed nobody can take.**
#[test]
fn the_scenario_has_to_parse() {
    let mut p = a_pack();
    p["sce"]["states"] = json!("not a list of states");
    let why = validate_case(&p).expect_err("refused");
    assert!(why.to_lowercase().contains("scenario"), "and says which half failed: {why}");
}

/// **A case with no way out is a patient who can only be abandoned.**
///
/// One win and one death, both, and for a reason the ward makes literal: a stay ends when the
/// engine ends it, so a case that cannot end is a bed that never frees — and one that can only
/// end badly is a ward where nothing a stranger does matters.
#[test]
fn a_case_must_be_survivable_and_fatal() {
    let mut p = a_pack();
    p["sce"]["outcomes"] = json!([{ "id": "win_discharge", "kind": "win", "label": "lived" }]);
    assert!(validate_case(&p).is_err(), "no way to die");

    let mut p = a_pack();
    p["sce"]["outcomes"] = json!([{ "id": "death_arrest", "kind": "death", "label": "died" }]);
    assert!(validate_case(&p).is_err(), "no way to live");
}

/// **A mark sheet that pays for something the scenario cannot produce pays nobody.**
#[test]
fn every_needle_names_something_in_the_scenario() {
    let mut p = a_pack();
    p["rubric"]["items"][0]["needle"] = json!("tx_a_drug_this_case_does_not_have");
    let why = validate_case(&p).expect_err("refused");
    assert!(why.contains("tx_a_drug_this_case_does_not_have"), "named: {why}");

    let mut p = a_pack();
    p["rubric"]["items"][1]["any_of"] = json!(["win_discharge", "win_a_third_ending"]);
    assert!(validate_case(&p).is_err(), "any_of is a list of needles and every one of them counts");
}

/// **Nothing of the season comes through this door.**
#[test]
fn a_pack_carrying_the_seasons_names_is_refused() {
    for (path, value) in [
        ("case_id", json!("osce-a2")),
        ("title", json!("OSCE station A2")),
    ] {
        let mut p = a_pack();
        p[path] = value.clone();
        assert!(validate_case(&p).is_err(), "{path} = {value} is the season's, not the ward's");
    }
}

/// **A case id is a store key and a page string**, so it is the narrow shape both can carry.
#[test]
fn the_case_id_is_one_plain_token() {
    for bad in ["", "Auth Demo", "auth/demo", "../etc", "auth.demo", &"a".repeat(200)] {
        let mut p = a_pack();
        p["case_id"] = json!(bad);
        assert!(validate_case(&p).is_err(), "{bad:?} is not a case id");
    }
}

/// The level decides who is offered her, so it is one of the three the ward publishes.
#[test]
fn the_level_is_one_the_ward_knows() {
    let mut p = a_pack();
    p["difficulty"] = json!("consultant");
    assert!(validate_case(&p).is_err());

    let mut p = a_pack();
    p["country"] = json!("Thailand");
    assert!(validate_case(&p).is_err(), "the globe matches on ISO3 and a free-text country matches nothing");

    let mut p = a_pack();
    p["country"] = Value::Null;
    assert!(validate_case(&p).is_ok(), "a case that belongs to no country in particular is allowed");
}

/// Every pack the compiler has actually produced, when this machine has them.
///
/// Skipped where they are absent — they are not in this repository and must not be: they are
/// compiled from embla-cases, which is the physicians' product and not public. This is the check
/// that the door and the compiler agree about the real thing rather than about a fixture.
#[test]
fn the_compilers_own_packs_are_admitted() {
    let dir = match std::env::var("HOME").map(|h| std::path::PathBuf::from(h).join(".vitals/world/cases")) {
        Ok(d) if d.is_dir() => d,
        _ => return,
    };
    let mut seen = 0;
    for entry in std::fs::read_dir(&dir).expect("the pack directory").flatten() {
        let path = entry.path();
        if !path.to_string_lossy().ends_with(".pack.json") {
            continue;
        }
        let raw = std::fs::read_to_string(&path).expect("a pack");
        let pack: Value = serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        validate_case(&pack).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        seen += 1;
    }
    assert!(seen > 0, "the directory is there and holds no packs: {}", dir.display());
}

// ── the door itself ─────────────────────────────────────────────────────────

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

struct Server {
    child: Child,
    port: u16,
    _state: std::path::PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self._state);
    }
}

impl Server {
    fn start() -> Server {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let state = std::env::temp_dir().join(format!("vitals-cases-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&state);
        let mut child = Command::new(env!("CARGO_BIN_EXE_vitals-web"))
            .env("VITALS_WEB_BIND", "127.0.0.1:0")
            .env("VITALS_STATE_DIR", &state)
            .env("VITALS_WORLD", "1")
            .env("VITALS_WARD_DOOR", "open")
            .env("VITALS_DOOR_TOKEN", "the-door-token")
            .env_remove("VITALS_PROGRAM_ID")
            .env_remove("VITALS_TOKEN")
            .env_remove("HEIMDALL_API_KEY")
            .stdout(Stdio::piped())
            .spawn()
            .expect("start vitals-web");
        let out = child.stdout.take().expect("stdout");
        let mut me = Server { child, port: 0, _state: state };
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if let Some(a) = line.split("http://").nth(1) {
                me.port = a.trim().rsplit(':').next().and_then(|p| p.parse().ok()).unwrap_or(0);
                break;
            }
        }
        assert!(me.port > 0, "server never said what port it took");
        me
    }

    /// Where this server keeps what it was given, so a test can read it back the way the ward
    /// does rather than through an endpoint that has already chosen what to show.
    fn state(&self) -> std::path::PathBuf {
        self._state.clone()
    }

    fn post(&self, path: &str, body: &Value) -> (u16, Value) {
        let url = format!("http://127.0.0.1:{}{path}", self.port);
        let r = ureq::post(&url)
            .set("Authorization", "Bearer the-door-token")
            .set("Content-Type", "application/json")
            .send_string(&body.to_string());
        match r {
            Ok(res) => (res.status(), res.into_json().unwrap_or(Value::Null)),
            Err(ureq::Error::Status(c, res)) => (c, res.into_json().unwrap_or(Value::Null)),
            Err(e) => panic!("{url}: {e}"),
        }
    }

    /// The same POST with somebody else's token — or none at all.
    fn post_with(&self, path: &str, body: &Value, token: Option<&str>) -> (u16, Value) {
        let url = format!("http://127.0.0.1:{}{path}", self.port);
        let mut r = ureq::post(&url).set("Content-Type", "application/json");
        if let Some(t) = token {
            r = r.set("Authorization", &format!("Bearer {t}"));
        }
        match r.send_string(&body.to_string()) {
            Ok(res) => (res.status(), res.into_json().unwrap_or(Value::Null)),
            Err(ureq::Error::Status(c, res)) => (c, res.into_json().unwrap_or(Value::Null)),
            Err(e) => panic!("{url}: {e}"),
        }
    }

    /// Every sentence in an answer reads as one — no hole in the middle of it.
    ///
    /// A Rust literal written across two lines without a `\` continuation keeps the source's own
    /// indentation, and what goes over the wire is a sentence with thirty spaces in it. It has
    /// happened three times now (the 401 from both doors, and "the pack stays in the store" in the
    /// answer this test was written beside), always in a sentence read by the one person who can
    /// act on it, in a log. Cheaper to catch here than to read every literal in the file.
    fn reads_as_sentences(v: &Value) {
        match v {
            Value::String(s) => assert!(!s.contains("  "), "a hole in the middle of it: {s:?}"),
            Value::Array(a) => a.iter().for_each(Self::reads_as_sentences),
            Value::Object(o) => o.values().for_each(Self::reads_as_sentences),
            _ => {}
        }
    }

    /// A GET carrying the door's secret, for the routes that are the operator's.
    fn post_get(&self, path: &str) -> (u16, Value) {
        let url = format!("http://127.0.0.1:{}{path}", self.port);
        match ureq::get(&url).set("Authorization", "Bearer the-door-token").call() {
            Ok(res) => (res.status(), res.into_json().unwrap_or(Value::Null)),
            Err(ureq::Error::Status(c, res)) => (c, res.into_json().unwrap_or(Value::Null)),
            Err(e) => panic!("{url}: {e}"),
        }
    }

    fn get(&self, path: &str) -> (u16, Value) {
        let url = format!("http://127.0.0.1:{}{path}", self.port);
        match ureq::get(&url).call() {
            Ok(res) => (res.status(), res.into_json().unwrap_or(Value::Null)),
            Err(ureq::Error::Status(c, res)) => (c, res.into_json().unwrap_or(Value::Null)),
            Err(e) => panic!("{url}: {e}"),
        }
    }
}

/// **A compiled case arrives, and the ward can say what it is holding.**
#[test]
fn a_case_comes_in_through_the_door_and_the_ward_lists_it() {
    let s = Server::start();
    let (code, body) = s.post("/api/ward/case", &a_pack());
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["stored"], "auth-demo-1");

    let (code, list) = s.get("/api/ward/cases");
    assert_eq!(code, 200);
    let cases = list["cases"].as_array().expect("a list of cases");
    assert_eq!(cases.len(), 1);
    assert_eq!(cases[0]["case_id"], "auth-demo-1");
    assert_eq!(cases[0]["country"], "THA");
    assert_eq!(cases[0]["difficulty"], "resident");
    assert_eq!(cases[0]["provisional"], true);
    assert_eq!(cases[0]["version"], "0.1.0");
    // The list is a catalogue, not the catalogue's contents: a pack is 20 KB of scenario and
    // nobody browsing the ward's cases needs it.
    assert!(cases[0].get("sce").is_none(), "the list does not carry the scenarios");
}

/// **Provisional means it can be recompiled. Reviewed means it cannot.**
///
/// A case somebody has already played is a case the chain carries shifts against, and replacing
/// it under the same id would rewrite what those shifts were about. So the moment a pack says it
/// has been reviewed, the door stops taking a second one.
#[test]
fn a_provisional_case_may_be_replaced_and_a_reviewed_one_may_not() {
    let s = Server::start();
    assert_eq!(s.post("/api/ward/case", &a_pack()).0, 200);

    let mut newer = a_pack();
    newer["version"] = json!("0.2.0");
    let (code, body) = s.post("/api/ward/case", &newer);
    assert_eq!(code, 200, "a provisional case is recompiled all day: {body}");
    assert_eq!(s.get("/api/ward/cases").1["cases"][0]["version"], "0.2.0");

    let mut reviewed = a_pack();
    reviewed["provisional"] = json!(false);
    reviewed["version"] = json!("1.0.0");
    assert_eq!(s.post("/api/ward/case", &reviewed).0, 200, "review lands like any other version");

    let mut after = a_pack();
    after["version"] = json!("1.0.1");
    let (code, body) = s.post("/api/ward/case", &after);
    assert_eq!(code, 409, "and nothing lands on top of it: {body}");
    let why = body["refused"].as_str().unwrap_or_default();
    assert!(why.contains("reviewed"), "the sentence says why: {why}");
    assert_eq!(s.get("/api/ward/cases").1["cases"][0]["version"], "1.0.0", "the reviewed one stands");
}

/// A pack the door refuses is named and the reason is the compiler's to act on.
#[test]
fn a_pack_the_ward_cannot_play_is_refused_with_the_reason() {
    let s = Server::start();
    let mut bad = a_pack();
    bad["sce"]["outcomes"] = json!([{ "id": "win_discharge", "kind": "win", "label": "lived" }]);
    let (code, body) = s.post("/api/ward/case", &bad);
    assert_eq!(code, 422, "{body}");
    assert!(body["refused"].as_str().unwrap_or_default().contains("die"), "{body}");
    assert_eq!(s.get("/api/ward/cases").1["cases"].as_array().map(Vec::len), Some(0),
               "and nothing was stored");
}

// ── which case a patient runs ───────────────────────────────────────────────

/// **The scenario comes from the pack the factory sent, never from `demo/**`.**
///
/// The ward's cases are compiled from embla-cases and arrive through the door; the season's
/// sixteen live on disk and belong to vitals.academy. A ward session reads its scenario out of the
/// case store, so what a stranger plays is what the compiler put on the record — the same bytes
/// the admission committed to on chain.
#[test]
fn a_wards_scenario_is_the_one_that_came_through_the_door() {
    use vitals_web::store::Store;
    use vitals_web::ward_case::{sce_of, CASE_STORE};

    let dir = std::env::temp_dir().join(format!("vitals-sce-of-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(dir.clone()).expect("a store");
    store.put(CASE_STORE, "auth-demo-1", &a_pack()).expect("stored");

    let sce = sce_of(&store, "auth-demo-1").expect("the case the door accepted");
    let engine = vitals_sce::Sce::from_json(&sce).expect("and it is what the engine runs");
    assert!(engine.interventions.iter().any(|i| i.id == "tx_fluids"));

    assert!(sce_of(&store, "osce-a2").is_none(),
            "a season id names nothing in the ward's own catalogue");
    assert!(sce_of(&store, "auth-demo-2").is_none(), "nor does a case nobody has sent");
    let _ = std::fs::remove_dir_all(&dir);
}

/// **Which case the next patient gets, and who it is written about.**
///
/// Her own pack names one when the patient factory knows what it wants, and that one is honoured:
/// the factory chose it with this same fit. When it names none, or names one this ward does not
/// hold, the ward places her — and placement is where a wrong patient gets made. It made one:
/// Nusrat Jahan, 64, a woman, was placed on a typhoid case written about a 26-year-old man, whose
/// own presentation opens "Young man from Bangladesh".
///
/// So the fallback fits her to the case: the sex the dialogue and the examination were written
/// for, and an age near the one the physiology was tuned for. Her country first, because a Nepali
/// woman on a Nepali case is the whole point of the endemic work, then any case that fits. No case
/// that fits means the bed waits — an empty bed says nothing false, and a mismatch says several.
#[test]
fn a_patient_is_placed_only_on_a_case_written_about_somebody_like_her() {
    use vitals_web::ward::Persona;
    use vitals_web::ward_case::{choose_case, CaseSummary};

    let case = |id: &str, country: Option<&str>, level: &str, version: &str, age: u32, sex: &str| CaseSummary {
        case_id: id.into(),
        archetype: "sepsis".into(),
        patient_age: Some(age),
        patient_sex: Some(sex.into()),
        country: country.map(str::to_string),
        difficulty: level.into(),
        endemic: country.is_some(),
        provisional: true,
        withdrawn: false,
        version: version.into(),
        title: id.into(),
    };
    let who = |country: &str, age: u16, sex: &str| Persona {
        name: "Nusrat Jahan".into(),
        country: country.into(),
        age,
        sex: sex.into(),
    };

    let cases = vec![
        case("typhoid-bgd", Some("BGD"), "intern", "0.1.0", 26, "male"),
        case("ugib-1", None, "resident", "0.1.0", 62, "male"),
        case("ugib-2", None, "resident", "0.2.0", 62, "male"),
        case("dengue-npl-1", Some("NPL"), "intern", "0.1.0", 34, "female"),
    ];
    let pick = |wanted: Option<&str>, p: &Persona, d: Option<&str>| {
        choose_case(&cases, wanted, p, d).map(|c| c.case_id.clone())
    };

    // The bug, as it happened: a woman of 64 from Bangladesh, and the only Bangladeshi case is
    // about a man of 26.
    assert_eq!(pick(None, &who("BGD", 64, "f"), None), None,
               "her country's case is written about a young man, so the bed waits rather than \
                putting her on it");

    // A fit in her own country is taken first.
    assert_eq!(pick(None, &who("NPL", 30, "f"), None), Some("dengue-npl-1".into()));
    // Same country, wrong sex for its only case: the country is a preference, so he falls through
    // to any case that fits him — and a man of 30 on a typhoid case written about a man of 26 is
    // exactly what fitting means. He does not get the Nepali dengue case written about a woman.
    assert_eq!(pick(None, &who("NPL", 30, "m"), None), Some("typhoid-bgd".into()));
    // No case at home, but one elsewhere she fits.
    assert_eq!(pick(None, &who("THA", 58, "m"), None), Some("ugib-2".into()),
               "newest version of the ones that fit");

    // Age: near the case's own, by the band that age is in — `age_fits`, and
    // `the_age_a_case_allows_follows_the_age_it_is_written_about` is where the table is read. These
    // two are the 40-and-over band, where near means ten years: the UGIB cases are written about a
    // man of 62.
    assert_eq!(pick(None, &who("THA", 52, "m"), None), Some("ugib-2".into()), "ten years is near");
    assert_eq!(pick(None, &who("THA", 51, "m"), None), None, "eleven is not");
    let paeds = vec![case("croup-1", None, "student", "0.1.0", 6, "female")];
    let kid = who("THA", 8, "f");
    assert_eq!(choose_case(&paeds, None, &kid, None).map(|c| c.case_id.clone()), Some("croup-1".into()));
    let grown = who("THA", 17, "f");
    assert_eq!(choose_case(&paeds, None, &grown, None), None,
               "a seventeen-year-old is not put on a case written about a six-year-old, however \
                close the years look");

    // Her pack's own choice is the factory's, made with this same fit, and is honoured as it is.
    assert_eq!(pick(Some("typhoid-bgd"), &who("BGD", 64, "f"), None), Some("typhoid-bgd".into()),
               "the factory named it and the factory is the one that fitted her to it");

    // The level still narrows, when the pack carries one.
    assert_eq!(pick(None, &who("THA", 58, "m"), Some("resident")), Some("ugib-2".into()));
    assert_eq!(pick(None, &who("THA", 58, "m"), Some("student")), None);

    // A case that does not say who it is about cannot be fitted to anybody, so the ward does not
    // place her on it — it can still be named by a factory that knows what it is doing.
    let silent = vec![CaseSummary { patient_age: None, patient_sex: None, ..case("quiet-1", None, "intern", "0.1.0", 40, "male") }];
    assert_eq!(choose_case(&silent, None, &who("THA", 40, "m"), None), None);
    assert!(choose_case(&silent, Some("quiet-1"), &who("THA", 40, "m"), None).is_some());
}

// ── the patient door, now that cases have one of their own ──────────────────

/// **A patient pack names a case the ward holds, or none at all.**
///
/// The factory built her for a case, or it did not and the ward will choose. What it may not do
/// any more is name one of the season's sixteen: those are vitals.academy's and the ward plays
/// what comes through `/api/ward/case`.
///
/// The check that the case *is held* belongs at the queue door rather than in the pack's own
/// shape, because it is a question about this ward at this moment — the same pack is valid the
/// minute after the compiler sends the case.
#[test]
fn a_patient_pack_names_a_case_this_ward_holds_or_none() {
    use vitals_web::store::Store;
    use vitals_web::ward::{Pack, Persona};
    use vitals_web::ward_case::CASE_STORE;
    use vitals_web::ward_chain::enqueue;

    let dir = std::env::temp_dir().join(format!("vitals-patient-door-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(dir.clone()).expect("a store");
    store.put(CASE_STORE, "auth-demo-1", &a_pack()).expect("a case the ward holds");

    // Somebody the fixture case is written about — it is a 62-year-old man's — because a pack that
    // contradicts the case it names is refused at this same door, which is
    // `a_patient_is_queued_only_onto_a_case_written_about_somebody_like_her` below. This test is
    // about whether the ward *holds* the case, and its packs must not fail for the other reason.
    let her = |case: &str| Pack {
        case: case.to_string(),
        difficulty: None,
        persona: Persona { name: "Anan Thepwong".into(), country: "NPL".into(), age: 60, sex: "m".into() },
        portrait: Default::default(),
        endemic: false,
    };

    let ok = enqueue(&store, vec![her("auth-demo-1")]);
    assert_eq!(ok.queued, 1, "{:?}", ok.rejected);

    let none = enqueue(&store, vec![her("")]);
    assert_eq!(none.queued, 1, "a pack with no case at all is the ward's to place: {:?}", none.rejected);

    let season = enqueue(&store, vec![her("osce-a2")]);
    assert_eq!(season.queued, 0, "the season's sixteen are refused at this door now");
    let why = season.rejected.first().cloned().unwrap_or_default();
    assert!(why.contains("/api/ward/case"), "and the sentence says where cases come from: {why}");

    let absent = enqueue(&store, vec![her("auth-demo-never-sent")]);
    assert_eq!(absent.queued, 0, "a case this ward does not hold is a patient nobody could open");
    let why = absent.rejected.first().cloned().unwrap_or_default();
    assert!(why.contains("auth-demo-never-sent"), "named: {why}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// And the level she was built for, when the factory knows it.
#[test]
fn a_patient_pack_may_say_which_level_she_was_built_for() {
    use vitals_web::ward::{Pack, Persona};

    let p = Pack {
        case: String::new(),
        difficulty: Some("intern".into()),
        persona: Persona { name: "Anita".into(), country: "NPL".into(), age: 34, sex: "f".into() },
        portrait: Default::default(),
        endemic: false,
    };
    assert!(vitals_web::ward_chain::validate_pack(&p).is_ok());

    let bad = Pack { difficulty: Some("consultant".into()), ..p.clone() };
    let why = vitals_web::ward_chain::validate_pack(&bad).expect_err("refused");
    assert!(why.contains("consultant"), "named: {why}");
}

/// **A library case is not renamed to fit our filing.**
///
/// `embla-hepatic-encephalopathy-precipitated-by-gi-bleeding-resident` is 65 characters, and the
/// store's keys are file names capped at 64. The door said yes and the store said no, as a 503
/// that reads like an outage — two of the compiler's sixty-six refused for a reason that has
/// nothing to do with medicine.
///
/// The id stays the library's and the filing is ours: a key the store cannot take becomes a hash
/// of that id, with the id itself kept in the document where it always was. Nothing that reads the
/// catalogue can tell, and nothing about the case changed to suit us.
#[test]
fn a_case_id_too_long_to_be_a_file_name_is_still_the_case_id() {
    use vitals_web::store::Store;
    use vitals_web::ward_case::{all, key_for, sce_of};

    let long = "embla-hepatic-encephalopathy-precipitated-by-gi-bleeding-resident";
    assert!(long.len() > 64, "the case that found this is {} characters", long.len());
    assert!(!vitals_web::store::is_safe_key(long), "and the store will not take it as a key");
    assert_ne!(key_for(long), long, "so it is filed under a name the store can hold");
    assert_eq!(key_for(long), key_for(long), "the same one every time, or it is lost");
    assert_ne!(key_for(long), key_for(&format!("{long}-2")), "and a different case is a different file");
    assert_eq!(key_for("auth-demo-1"), "auth-demo-1", "a short id is its own key and reads plainly");

    let dir = std::env::temp_dir().join(format!("vitals-longkey-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(dir.clone()).expect("a store");

    let mut pack = a_pack();
    pack["case_id"] = json!(long);
    pack["rubric"]["case"] = json!(long);
    store.put(vitals_web::ward_case::CASE_STORE, &key_for(long), &pack).expect("filed");

    assert!(sce_of(&store, long).is_some(), "and it is found again by the name the library gave it");
    let listed = all(&store);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].case_id, long, "the catalogue says the case's own id, not our file name");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The one word that says what kind of deterioration this is.
#[test]
fn the_catalogue_says_what_kind_of_case_it_is() {
    let s = validate_case(&a_pack()).expect("a pack");
    assert_eq!(s.archetype, "haemorrhagic_shock");
}

/// **The catalogue says who each case is written about.**
///
/// The pack carries `patient{age, sex}` — the malaria case is a man of 34 — and the patient
/// factory needs both to place somebody on it: the persona's sex must match what the dialogue and
/// the examination were written for, and her age should sit near the one the case assumes. Without
/// them in the catalogue every draw against the real sixty-six came back "no case_id", because the
/// chooser had nothing to match on.
///
/// Passed through as the compiler spells it — `male` and `female`, which is what its sixty-six say
/// — rather than translated into the ward's own single letter. Two vocabularies for one fact is
/// how a chooser silently matches nothing.
#[test]
fn the_catalogue_says_who_the_case_is_written_about() {
    let s = validate_case(&a_pack()).expect("a pack");
    assert_eq!(s.patient_age, Some(62));
    assert_eq!(s.patient_sex.as_deref(), Some("male"));

    // Absent rather than guessed: a case with no patient block says so, and the factory can tell
    // "this case is about a man" from "nobody wrote it down".
    let mut p = a_pack();
    p["patient"] = Value::Null;
    let s = validate_case(&p).expect("still a playable case");
    assert_eq!(s.patient_age, None);
    assert_eq!(s.patient_sex, None);
}

/// And it reaches the wire, which is where the factory reads it.
#[test]
fn the_catalogue_carries_the_patient_on_the_wire() {
    let s = Server::start();
    assert_eq!(s.post("/api/ward/case", &a_pack()).0, 200);
    let (_, list) = s.get("/api/ward/cases");
    let row = &list["cases"][0];
    assert_eq!(row["patient"]["age"], 62);
    assert_eq!(row["patient"]["sex"], "male");
}

// ── the case's own words, in the persona's person ───────────────────────────

/// **The prose is the case's; the person in it is the ward's.**
///
/// The compiler stopped baking "26-year-old" and "young man" into its text and writes placeholders
/// instead, because the ward renames every patient it admits. Nusrat Jahan, 64, was placed on a
/// case whose presentation opened "Young man from Bangladesh"; the placement rule stops the
/// mismatch and this fills what is left.
#[test]
fn a_cases_prose_is_told_about_the_person_in_the_bed() {
    use vitals_web::ward::Persona;
    use vitals_web::ward_case::fill_persona;

    let her = Persona { name: "Nusrat Jahan".into(), country: "BGD".into(), age: 64, sex: "f".into() };
    let him = Persona { name: "Rafael Moreira".into(), country: "BRA".into(), age: 26, sex: "m".into() };
    let kid = Persona { name: "Pim".into(), country: "THA".into(), age: 6, sex: "f".into() };

    assert_eq!(fill_persona("Young {sex_word} from Bangladesh, {age}-year-old", &her),
               "Young woman from Bangladesh, 64-year-old");
    assert_eq!(fill_persona("Young {sex_word} from Bangladesh, {age}-year-old", &him),
               "Young man from Bangladesh, 26-year-old");
    assert_eq!(fill_persona("{Sex_word}, {age}, says {he_she} vomited blood; {his_her} partner drove {him_her}.", &her),
               "Woman, 64, says she vomited blood; her partner drove her.");
    assert_eq!(fill_persona("{He_she} cannot settle {himself_herself}.", &him),
               "He cannot settle himself.");

    // A child is a boy or a girl, not a man or a woman — the one place the word turns on age.
    assert_eq!(fill_persona("a {sex_word} of {age}", &kid), "a girl of 6");
    assert_eq!(fill_persona("a {sex_word} of {age}", &her), "a woman of 64");

    // Nothing else is touched, and an unknown brace word is left exactly as written rather than
    // blanked: a pack that invents one should look wrong, not quietly lose a word.
    assert_eq!(fill_persona("no placeholders here", &her), "no placeholders here");
    assert_eq!(fill_persona("{not_a_placeholder} stays", &her), "{not_a_placeholder} stays");
    assert_eq!(fill_persona("อ้วกเป็นเลือด {age} ปี", &her), "อ้วกเป็นเลือด 64 ปี");
}

/// **What the ward's own page is given about a case.**
///
/// Every word of it from the pack, every person in it from the persona, and nothing at all from
/// the season's table — which is where the page was getting it: opening a World-case patient
/// showed EP1's name, EP1's questions and no title.
#[test]
fn the_ward_is_given_the_cases_own_words_to_render() {
    use vitals_web::ward::Persona;
    use vitals_web::ward_case::case_view;

    let mut pack = a_pack();
    pack["title"] = json!("Vomiting blood — a {sex_word} of {age}");
    pack["presentation"]["chief_complaint"] = json!("{He_she} vomited blood");
    pack["presentation"]["hpi"] = json!("A {sex_word} of {age} brought in by {his_her} partner.");
    pack["sce"]["interventions"] = json!([
        { "id": "ask_hematemesis", "label": "Ask: Hematemesis", "match": { "any_kw": ["hematemesis"] }, "effects": [] },
        { "id": "exam_conjunctiva", "label": "Look at the conjunctiva", "match": { "any_kw": ["conjunctiva"] }, "effects": [] },
        { "id": "ix_cbc", "label": "CBC", "match": { "any_kw": ["cbc"] }, "effects": [] },
        { "id": "tx_fluids", "label": "Crystalloid bolus for a {sex_word} of {age}", "match": { "any_kw": ["fluids"] }, "effects": [{ "to_state": "stabilising" }] },
        { "id": "dx_peptic_ulcer", "label": "Name the diagnosis", "match": { "any_kw": ["ulcer"] }, "effects": [] }
    ]);
    pack["voice"] = json!({
        "ask_hematemesis": { "finding": "Hematemesis", "present": true, "reveal": "volunteered",
                             "words": "I am {age} and I have never seen so much blood" }
    });

    let her = Persona { name: "Nusrat Jahan".into(), country: "BGD".into(), age: 64, sex: "f".into() };
    let v = case_view(&pack, &her);

    assert_eq!(v["title"], "Vomiting blood — a woman of 64");
    assert_eq!(v["presents"], "She vomited blood");
    assert_eq!(v["story"], "A woman of 64 brought in by her partner.");
    assert_eq!(v["difficulty"], "resident");

    // The quick questions are the case's own asks, in the case's own words.
    let asks = v["chips"]["ask"].as_array().expect("the asks");
    assert_eq!(asks.len(), 1);
    assert_eq!(asks[0]["id"], "ask_hematemesis");
    assert_eq!(asks[0]["label"], "Ask: Hematemesis");
    assert_eq!(v["chips"]["exam"][0]["id"], "exam_conjunctiva");
    assert_eq!(v["chips"]["lab"][0]["id"], "ix_cbc");
    assert_eq!(v["chips"]["treat"][0]["label"], "Crystalloid bolus for a woman of 64");
    assert_eq!(v["chips"]["dx"][0]["id"], "dx_peptic_ulcer");

    // And what she says when she is asked, in her own person.
    assert_eq!(v["voice"]["ask_hematemesis"], "I am 64 and I have never seen so much blood");
    assert!(v["no_answer"].as_str().is_some_and(|s| !s.is_empty()),
            "and something to say when she is asked about something this case never wrote down");

    // Nothing filled is stored: the pack is untouched by having been rendered.
    assert_eq!(pack["title"], "Vomiting blood — a {sex_word} of {age}");
}

/// **Who the page says is in the bed.**
///
/// The bay draws a card whose `who` reads "Name · SEX AGE", and three things read that one string:
/// the bed label prints it, `ageOf` parses it for the monitor's alarm limits (a three-year-old at
/// 118 and 28 is a normal three-year-old), and `pro()` takes the patient's pronoun out of it. On
/// the season it is written by the case's author. On the ward the person is the ward's, so the
/// view that replaces the season's card has to say who is actually there — otherwise the page
/// reads the case's author's patient for all three.
#[test]
fn the_view_names_the_person_in_the_bed() {
    use vitals_web::ward::Persona;
    use vitals_web::ward_case::case_view;

    let her = Persona { name: "Nusrat Jahan".into(), country: "BGD".into(), age: 64, sex: "f".into() };
    let him = Persona { name: "Rafael Moreira".into(), country: "BRA".into(), age: 26, sex: "m".into() };

    assert_eq!(case_view(&a_pack(), &her)["who"], "Nusrat Jahan · F 64");
    assert_eq!(case_view(&a_pack(), &him)["who"], "Rafael Moreira · M 26");

    // And the one sentence the ward writes itself takes his pronoun too: the ward admits men, and
    // "she does not answer that" over Rafael is the page contradicting its own chart.
    let his = case_view(&a_pack(), &him);
    let line = his["no_answer"].as_str().expect("something to say");
    assert!(line.split(|c: char| !c.is_ascii_alphabetic()).any(|w| w == "he"),
            "the no-answer line is written about a woman whoever is in the bed: {line:?}");
}

/// **On the ward the patient answers out of the case file, and out of nothing else.**
///
/// In the bay her voice is a language model with her persona in front of it: she improvises, and
/// the reveal gate is what keeps her from improvising away a fact the candidate has not earned.
/// A public ward cannot have that. It is free to the stranger, it scales to zero, and the spend is
/// per question — a ward under a fair's worth of traffic is a bill with no learner attached, and
/// the first thing that would go wrong is the tenth person of the day being told the patient has
/// nothing to say because the month's compute is gone.
///
/// The compiler already wrote what she says: every `ask_` intervention has a voice entry, in her
/// person, written by the case's author against that exact finding. So the ward's answer is a
/// lookup — the chip sends the intervention id, free typing goes through the case's own keywords,
/// and a question the case never wrote an answer for is told so plainly rather than answered by
/// something that does not know her.
#[test]
fn the_patient_answers_out_of_the_case_file() {
    use vitals_web::ward::Persona;
    use vitals_web::ward_case::answer;

    let mut pack = a_pack();
    pack["sce"]["interventions"] = json!([
        { "id": "ask_hematemesis", "label": "Ask: Vomiting blood",
          "match": { "any_kw": ["vomit", "blood", "ask_hematemesis"] }, "effects": [] },
        { "id": "ask_black_stool", "label": "Ask: Black stool",
          "match": { "any_kw": ["black stool", "melaena", "ask_black_stool"] }, "effects": [] },
        { "id": "ask_alcohol", "label": "Ask: Alcohol",
          "match": { "any_kw": ["alcohol", "drink"], "not_kw": ["water"] }, "effects": [] },
        { "id": "tx_fluids", "label": "Crystalloid bolus",
          "match": { "any_kw": ["fluids", "crystalloid"] },
          "effects": [{ "to_state": "stabilising" }] }
    ]);
    pack["voice"] = json!({
        "ask_hematemesis": { "finding": "Hematemesis", "present": true, "reveal": "volunteered",
                             "words": "I am {age} and I have never seen so much blood" },
        "ask_black_stool": { "finding": "Melaena", "present": true, "reveal": "on_direct_ask",
                             "words": "Yesterday. Black, sticky. I thought it was the medicine." }
    });

    let her = Persona { name: "Nusrat Jahan".into(), country: "BGD".into(), age: 64, sex: "f".into() };
    let him = Persona { name: "Rafael Moreira".into(), country: "BRA".into(), age: 26, sex: "m".into() };

    // The chip sends the intervention id — what fires, what lands on the tape, what is marked.
    let a = answer(&pack, &her, "ask_hematemesis");
    assert_eq!(a.matched.as_deref(), Some("ask_hematemesis"));
    assert_eq!(a.words, "I am 64 and I have never seen so much blood",
               "her words, about her: the placeholders are filled from the person in the bed");

    // And a stranger typing gets there through the case's own keywords, the same ones the engine
    // matches an order with.
    assert_eq!(answer(&pack, &her, "have you vomited any blood?").matched.as_deref(),
               Some("ask_hematemesis"));
    assert_eq!(answer(&pack, &her, "any melaena?").matched.as_deref(), Some("ask_black_stool"));
    assert_eq!(answer(&pack, &her, "ASK ABOUT THE BLACK STOOL").matched.as_deref(),
               Some("ask_black_stool"), "the question is matched however it is typed");

    // A matcher's exclusions are the author's and are kept: "do you drink water" is not a question
    // about alcohol, and answering it as one would be the patient agreeing to something she was
    // never asked.
    assert_eq!(answer(&pack, &her, "do you drink water?").matched, None);

    // An order is not a question. The ask bar is a conversation; "crystalloid bolus" belongs to
    // the tray, and a patient who answered it would be answering out of the wrong half of her own
    // case file.
    assert_eq!(answer(&pack, &her, "crystalloid bolus").matched, None);

    // A question the case never wrote an answer for, and a question it wrote no words for: both
    // are told so, in the pronoun of the person in the bed.
    let none = answer(&pack, &her, "do you have a dog at home?");
    assert_eq!(none.matched, None);
    assert!(none.words.contains("she"), "in her own pronoun: {:?}", none.words);
    assert!(!none.words.is_empty());
    let mute = answer(&pack, &him, "do you drink alcohol?");
    assert_eq!(mute.matched.as_deref(), Some("ask_alcohol"),
               "the case knows the question — it simply wrote no words for it");
    assert!(mute.words.contains(" he "), "and he is a man: {:?}", mute.words);

    // The pack may write its own sentence for that, and then it is the pack's.
    pack["no_answer"] = json!("{He_she} shakes {his_her} head.");
    assert_eq!(answer(&pack, &him, "do you have a dog at home?").words, "He shakes his head.");
}

/// **What the compiler writes, the ward holds — including the fields this build has no use for.**
///
/// The compiler now counts its own placeholders: `placeholders: {age, sex}` says how many of each
/// the prose carries, which is how a pack declares whether its text is about a person the ward can
/// rename. Nothing here reads it yet. That is exactly why it is worth a test — a door that
/// validates a shape tends to grow into a door that *keeps* only that shape, and the next field
/// the compiler adds would arrive here and quietly stop existing.
///
/// So the rule is that the door validates and stores; it does not edit. What comes out of the
/// store is what was posted, field for field.
#[test]
fn the_door_keeps_what_it_was_given() {
    let s = Server::start();
    let mut pack = a_pack();
    pack["case_id"] = json!("auth-placeholders-1");
    pack["placeholders"] = json!({ "age": 1, "sex": 2 });
    // A field no build has ever seen, to make the point that this is not about `placeholders`.
    pack["from_a_later_compiler"] = json!({ "nested": ["and", 3, true] });

    let (code, body) = s.post("/api/ward/case", &pack);
    assert_eq!(code, 200, "the door refused a pack for carrying a field it does not read: {body}");

    let store = vitals_web::store::Store::open(s.state()).expect("the ward's own store");
    let held: Value = store
        .get(vitals_web::ward_case::CASE_STORE, &vitals_web::ward_case::key_for("auth-placeholders-1"))
        .expect("the ward is holding it");

    assert_eq!(held["placeholders"], pack["placeholders"], "the compiler's count, untouched");
    assert_eq!(held["from_a_later_compiler"], pack["from_a_later_compiler"]);
    for field in ["case_id", "title", "sce", "rubric", "voice", "replay", "patient", "source"] {
        assert_eq!(held[field], pack[field], "{field} came back changed");
    }
}

/// **The door's refusals are sentences, and a sentence has one space between its words.**
///
/// The two refusals that guard the doors themselves — no door token configured, and a caller
/// presenting the wrong one — were written as single-line Rust literals with the source's own
/// indentation inside them, so what went over the wire had thirty-one spaces in the middle of each
/// sentence. They are read by the one person who can act on them, in a log, and a sentence with a
/// hole in it reads like a corrupted field rather than an instruction.
#[test]
fn the_doors_own_refusals_read_as_sentences() {
    let s = Server::start();
    let (code, body) = s.post_with("/api/ward/case", &a_pack(), Some("not-the-door-token"));
    assert_eq!(code, 401, "the door took somebody else's token: {body}");
    let said = body["error"].as_str().unwrap_or_default();
    assert!(!said.contains("  "), "the refusal has a hole in it: {said:?}");
    assert!(said.contains("VITALS_DOOR_TOKEN"),
            "and it names the variable the operator has to set: {said:?}");

    // The same door, with no token at all.
    let (code, body) = s.post_with("/api/ward/case", &a_pack(), None);
    assert_eq!(code, 401, "and an anonymous caller is not let in either: {body}");
    assert!(!body["error"].as_str().unwrap_or_default().contains("  "));
}

/// **A pack that names a case is held to the same rule as one the ward places.**
///
/// The ward will not *choose* a case written about somebody else — `fits_patient` is what stops it
/// — but a pack that names a case outright was checked for the season's names, a country code, a
/// name, an age in range, its portraits and its endemic claim, and then queued. So the rule held
/// for the patients the ward placed and not for the ones the factory placed, which is the half the
/// factory uses.
///
/// Demonstrated on 16 ก.ย. against a local ward holding staging's own sixty-six cases: a pack for
/// "Forseti Probe · F 66" naming `embla-dengue-shock-syndrome-child-intern` — a case written for a
/// child — came back `queued: 1`. On the ward that is a page telling a stranger they are treating
/// a child while the board beside it says sixty-six, and a physiology tuned for a child under an
/// adult's name.
///
/// The door refuses a *contradiction*, which is not quite the same test as the one placement uses:
/// a case that says nothing about its own patient contradicts nobody, and a factory that names it
/// outright is taking a decision the ward has no grounds to overrule. Every compiled pack says.
#[test]
fn a_patient_is_queued_only_onto_a_case_written_about_somebody_like_her() {
    use vitals_web::store::Store;
    use vitals_web::ward::{Pack, Persona};
    use vitals_web::ward_case::CASE_STORE;
    use vitals_web::ward_chain::enqueue;

    let dir = std::env::temp_dir().join(format!("vitals-fit-door-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(dir.clone()).expect("a store");

    // The pack fixture is written about a 62-year-old man.
    store.put(CASE_STORE, "auth-demo-1", &a_pack()).expect("a case the ward holds");
    // And one written about a child, which is the placement that made this a rule.
    let mut child = a_pack();
    child["case_id"] = json!("auth-demo-child");
    child["patient"] = json!({ "age": 7, "sex": "female" });
    store.put(CASE_STORE, "auth-demo-child", &child).expect("a child's case");
    // A case that says nothing about its patient: nothing to contradict.
    let mut silent = a_pack();
    silent["case_id"] = json!("auth-demo-silent");
    silent["patient"] = json!(null);
    store.put(CASE_STORE, "auth-demo-silent", &silent).expect("a case with no patient block");

    let pack = |case: &str, name: &str, age: u16, sex: &str| Pack {
        case: case.to_string(),
        difficulty: None,
        persona: Persona { name: name.into(), country: "NPL".into(), age, sex: sex.into() },
        portrait: Default::default(),
        endemic: false,
    };

    // Somebody like the case's own patient: queued, as it always was.
    let ok = enqueue(&store, vec![pack("auth-demo-1", "Anan Thepwong", 60, "m")]);
    assert_eq!(ok.queued, 1, "{:?}", ok.rejected);

    // The sex the dialogue, the examination and the differential are written for.
    let wrong_sex = enqueue(&store, vec![pack("auth-demo-1", "Anita Shrestha", 60, "f")]);
    assert_eq!(wrong_sex.queued, 0);
    let why = wrong_sex.rejected.first().cloned().unwrap_or_default();
    assert!(why.contains("auth-demo-1"), "the case is named: {why}");
    assert!(why.contains("man") && why.contains("woman"), "and both people are: {why}");

    // The age the physiology is tuned for: twelve years either way.
    let wrong_age = enqueue(&store, vec![pack("auth-demo-1", "Anan Thepwong", 30, "m")]);
    assert_eq!(wrong_age.queued, 0, "thirty-two years out is not a near miss");
    assert!(wrong_age.rejected.first().is_some_and(|w| w.contains("62")), "{:?}", wrong_age.rejected);

    // A child only on a child's case, and an adult never on one. Sixteen and six are not a near
    // miss: a case tuned for one of them alarms wrongly on the other from the first second.
    let adult_on_child = enqueue(&store, vec![pack("auth-demo-child", "Anita Shrestha", 19, "f")]);
    assert_eq!(adult_on_child.queued, 0, "{:?}", adult_on_child.rejected);
    let child_on_child = enqueue(&store, vec![pack("auth-demo-child", "Pim", 8, "f")]);
    assert_eq!(child_on_child.queued, 1, "{:?}", child_on_child.rejected);

    // And it reads like a sentence somebody wrote: "an 8-year-old girl", never "a 8-year-old".
    let child_words = enqueue(&store, vec![pack("auth-demo-child", "Anita Shrestha", 41, "f")]);
    let why = child_words.rejected.first().cloned().unwrap_or_default();
    assert!(why.contains("an 7-year-old") || why.contains("a 7-year-old"), "{why}");
    let eight = enqueue(&store, vec![pack("auth-demo-1", "Pim", 8, "f")]);
    assert!(eight.rejected.first().is_some_and(|w| w.contains("an 8-year-old")),
            "a reader who trips over the grammar trusts the rest of the sentence less: {:?}",
            eight.rejected);

    // A case that says nothing about its patient contradicts nobody, and the factory naming it
    // outright is taking a decision this ward has no grounds to overrule.
    let silent_ok = enqueue(&store, vec![pack("auth-demo-silent", "Anita Shrestha", 41, "f")]);
    assert_eq!(silent_ok.queued, 1, "{:?}", silent_ok.rejected);

    // And a pack that names no case at all is still the ward's to place.
    let none = enqueue(&store, vec![pack("", "Anita Shrestha", 34, "f")]);
    assert_eq!(none.queued, 1, "{:?}", none.rejected);

    let _ = std::fs::remove_dir_all(&dir);
}

/// **A pack's endemic claim is checked against the catalogue, because that is where cases live now.**
///
/// The first real factory tick, 17 ก.ย., staging 00033: the door refused
/// `embla-meningococcal-meningitis-septic-shock-intern` for Ousmane Garba, 26, from Niger — *"this
/// pack calls itself endemic, but the endemic list does not pair NER with
/// embla-meningococcal-…"*. The case is tagged endemic for NER in the catalogue, by the compiler,
/// and the door was reading `data/endemic.json`: a static file of six country→case pairs written
/// for the season's sixteen, empty today, that knows nothing of the case door. Eighteen of the
/// seventy-eight cases the ward holds are endemic, so every endemic patient the factory builds was
/// being turned away at the door.
///
/// The claim is a fact about the pairing, and the catalogue is where that fact is: a pack may call
/// itself endemic if the case it names is tagged endemic and its country is the patient's. A pack
/// naming no case may not claim it at all — the ward picks the case afterwards, and which case it
/// picks is what would make the claim true or false.
#[test]
fn an_endemic_claim_is_checked_against_the_catalogue() {
    use vitals_web::store::Store;
    use vitals_web::ward::{Pack, Persona};
    use vitals_web::ward_case::CASE_STORE;
    use vitals_web::ward_chain::enqueue;

    let dir = std::env::temp_dir().join(format!("vitals-endemic-door-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(dir.clone()).expect("a store");

    // The case from the tick, as the catalogue holds it: endemic in Niger, written about a boy of
    // sixteen.
    let mut meningo = a_pack();
    meningo["case_id"] = json!("embla-meningococcal-meningitis-septic-shock-intern");
    meningo["country"] = json!("NER");
    meningo["endemic"] = json!(true);
    meningo["patient"] = json!({ "age": 16, "sex": "male" });
    store.put(CASE_STORE, &vitals_web::ward_case::key_for("embla-meningococcal-meningitis-septic-shock-intern"), &meningo)
        .expect("the ward holds it");
    // And one that is endemic somewhere else.
    let mut dengue = a_pack();
    dengue["case_id"] = json!("embla-dengue-shock-child");
    dengue["country"] = json!("THA");
    dengue["endemic"] = json!(true);
    dengue["patient"] = json!({ "age": 20, "sex": "male" });
    store.put(CASE_STORE, &vitals_web::ward_case::key_for("embla-dengue-shock-child"), &dengue).expect("held");
    // And one that belongs to no place at all.
    store.put(CASE_STORE, &vitals_web::ward_case::key_for("auth-demo-1"), &a_pack()).expect("held");

    let pack = |case: &str, country: &str, age: u16, endemic: bool| Pack {
        case: case.to_string(),
        difficulty: None,
        persona: Persona { name: "Ousmane Garba".into(), country: country.into(), age, sex: "m".into() },
        portrait: Default::default(),
        endemic,
    };

    // The pack from the tick. Nothing about it is wrong.
    let tick = enqueue(&store, vec![pack("embla-meningococcal-meningitis-septic-shock-intern", "NER", 26, true)]);
    assert_eq!(tick.queued, 1,
               "the case is tagged endemic for Niger in the catalogue and this patient is from \
                Niger: {:?}", tick.rejected);

    // Endemic in Thailand is not endemic in Niger, whoever the patient is.
    let elsewhere = enqueue(&store, vec![pack("embla-dengue-shock-child", "NER", 26, true)]);
    assert_eq!(elsewhere.queued, 0);
    let why = elsewhere.rejected.first().cloned().unwrap_or_default();
    assert!(why.contains("THA") && why.contains("NER"), "both places are named: {why}");

    // A case that belongs to no place cannot be claimed as endemic anywhere.
    let nowhere = enqueue(&store, vec![pack("auth-demo-1", "NER", 60, true)]);
    assert_eq!(nowhere.queued, 0, "{:?}", nowhere.rejected);
    assert!(nowhere.rejected.first().is_some_and(|w| w.contains("auth-demo-1")), "{:?}", nowhere.rejected);

    // A pack naming no case may not claim it: the ward picks the case afterwards, and which case it
    // picks is exactly what would make the claim true or false.
    let unnamed = enqueue(&store, vec![pack("", "NER", 26, true)]);
    assert_eq!(unnamed.queued, 0);
    assert!(unnamed.rejected.first().is_some_and(|w| w.contains("names no case")),
            "{:?}", unnamed.rejected);

    // And a pack that claims nothing is queued whatever the case is tagged.
    let quiet = enqueue(&store, vec![pack("embla-meningococcal-meningitis-septic-shock-intern", "THA", 20, false)]);
    assert_eq!(quiet.queued, 1, "{:?}", quiet.rejected);

    let _ = std::fs::remove_dir_all(&dir);
}

/// **A case can be withdrawn, and a withdrawn case is not a deleted one.**
///
/// The compiler stopped accepting non-English cases on 17 ก.ย.: the library is 421 Thai of 433, and
/// the founder was shown "womanวัยกลางคน…" over a Japanese patient — a Thai case with English
/// placeholders filled into it, on a person from the wrong side of the world. Sixty of the cases
/// this ward holds are those, and the ticker will keep putting people on them until somebody says
/// otherwise.
///
/// Withdrawing is that sentence. It is not a delete: patients are mid-stay on some of these, their
/// shifts are on the chain, and the case is what their chart is rebuilt from — so the pack stays in
/// the store, readable, for as long as anybody is on it. What goes is its future: it is never
/// placed, and a pack naming it is refused at the queue door in the door's own words.
///
/// A reviewed case may not be withdrawn, for the same reason it may not be replaced: somebody has
/// played it and the chain carries what they did.
#[test]
fn a_provisional_case_can_be_withdrawn_and_is_still_readable() {
    use vitals_web::store::Store;
    use vitals_web::ward::{Pack, Persona};
    use vitals_web::ward_case::{all, choose_case, key_for, sce_of, CASE_STORE};
    use vitals_web::ward_chain::enqueue;

    let s = Server::start();
    let mut thai = a_pack();
    thai["case_id"] = json!("embla-thai-library-1");
    thai["language"] = json!("th");
    thai["patient"] = json!({ "age": 40, "sex": "female" });
    let (code, _) = s.post("/api/ward/case", &thai);
    assert_eq!(code, 200);

    // Withdrawn by the same door the case came through, and the answer says what happened.
    let (code, body) = s.post("/api/ward/case/embla-thai-library-1/withdraw", &json!({}));
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["withdrawn"], "embla-thai-library-1");
    Server::reads_as_sentences(&body);

    // The catalogue still lists it, and says so.
    let (code, list) = s.get("/api/ward/cases");
    assert_eq!(code, 200);
    let row = list["cases"].as_array().expect("the catalogue")
        .iter().find(|c| c["case_id"] == "embla-thai-library-1").expect("still listed").clone();
    assert_eq!(row["withdrawn"], true, "a withdrawn case is on the record, not gone: {row}");

    // And a reader that wants only what can be played can ask for that.
    let (_, placeable) = s.get("/api/ward/cases?placeable=1");
    assert!(placeable["cases"].as_array().expect("cases").iter()
                .all(|c| c["case_id"] != "embla-thai-library-1"),
            "?placeable=1 still offers a case nobody may be put on: {placeable}");

    // Nobody new is put on it: not by the ward choosing it…
    let store = Store::open(s.state()).expect("the ward's own store");
    let held = all(&store);
    let her = Persona { name: "Aoi Nakamura".into(), country: "JPN".into(), age: 40, sex: "f".into() };
    assert!(choose_case(&held, None, &her, None).is_none_or(|c| c.case_id != "embla-thai-library-1"),
            "the ward placed a patient on a withdrawn case");
    // …nor by a pack naming it outright.
    let named = enqueue(&store, vec![Pack {
        case: "embla-thai-library-1".into(),
        difficulty: None,
        persona: her.clone(),
        portrait: Default::default(),
        endemic: false,
    }]);
    assert_eq!(named.queued, 0, "{:?}", named.rejected);
    let why = named.rejected.first().cloned().unwrap_or_default();
    assert!(why.contains("withdrawn"), "and the sentence says which word it is: {why}");

    // But the patients already on it can still be opened: the pack is in the store, and the chart
    // of anybody mid-stay is rebuilt from it.
    assert!(store.get::<Value>(CASE_STORE, &key_for("embla-thai-library-1")).is_some(),
            "a withdrawn case is kept, or every patient on it becomes a blank screen at a bed");
    assert!(sce_of(&store, "embla-thai-library-1").is_some(),
            "and its scenario still loads, which is what a shift on her replays");

    // A reviewed case is not withdrawable, for the reason it is not replaceable.
    let mut reviewed = a_pack();
    reviewed["case_id"] = json!("embla-reviewed-1");
    reviewed["provisional"] = json!(false);
    assert_eq!(s.post("/api/ward/case", &reviewed).0, 200);
    let (code, body) = s.post("/api/ward/case/embla-reviewed-1/withdraw", &json!({}));
    assert_eq!(code, 409, "{body}");
    Server::reads_as_sentences(&body);
    assert!(body["refused"].as_str().is_some_and(|w| w.contains("reviewed")), "{body}");

    // A case nobody sent is a 404, and the door still takes only its own token.
    assert_eq!(s.post("/api/ward/case/embla-never-sent/withdraw", &json!({})).0, 404);
    assert_eq!(s.post_with("/api/ward/case/embla-thai-library-1/withdraw", &json!({}), None).0, 401,
               "the withdraw door is a door");
}


/// **The catalogue is read by people, so it carries no placeholders.**
///
/// A compiled title is written for whoever is put in the bed: "Elderly {sex_word} with wheezing
/// after COPD exacerbation". Filled at the bedside from the person there, and filled here from the
/// case's own patient — because the catalogue is a list of cases rather than of patients, and the
/// case's own patient is the one it is written about. The reviewer's list showed eighteen rows of
/// `{sex_word}` until this.
#[test]
fn the_catalogue_reads_as_prose_and_not_as_a_template() {
    let s = Server::start();
    let mut pack = a_pack();
    pack["case_id"] = json!("embla-placeholder-1");
    pack["title"] = json!("Elderly {sex_word} of {age} with wheezing");
    pack["patient"] = json!({ "age": 82, "sex": "male" });
    assert_eq!(s.post("/api/ward/case", &pack).0, 200);

    let (_, list) = s.get("/api/ward/cases");
    let row = list["cases"].as_array().expect("the catalogue")
        .iter().find(|c| c["case_id"] == "embla-placeholder-1").expect("listed").clone();
    assert_eq!(row["title"], "Elderly man of 82 with wheezing", "{row}");
    assert!(!row["title"].as_str().unwrap_or_default().contains('{'),
            "a reader of the catalogue is reading the compiler's plumbing: {row}");
}

/// **Twelve years either way is an adult's rule, and it let a one-year-old onto an eight-year-old's
/// case.**
///
/// The factory queued exactly that on 17 ก.ย. — a persona of 1 for a spontaneous pneumothorax
/// written about a child of 8 — and the door took it. `(1 - 8).abs() <= 12` is true, and the "child
/// only with a child" rule was satisfied because both are under sixteen. Everything the case
/// assumes about that patient is wrong: the airway, the doses, the words she uses, whether she
/// speaks at all.
///
/// So the tolerance follows the age it is about, which is how paediatrics works: an infant and a
/// toddler are different patients, a school-age child and a teenager are not interchangeable, and
/// two adults ten years apart usually are. The table is the director's, and the factory takes the
/// same one:
///
///   * under 5 — within one year, never below one
///   * 5 to 15 — within three, and never outside 5–15
///   * 16 to 39 — from 16, up to ten years older, eight years younger
///   * 40 and over — within ten
#[test]
fn the_age_a_case_allows_follows_the_age_it_is_written_about() {
    use vitals_web::ward::Persona;
    use vitals_web::ward_case::{contradicts, CaseSummary};

    let case = |age: u32| CaseSummary {
        case_id: "embla-age-1".into(),
        archetype: "pneumothorax".into(),
        patient_age: Some(age),
        patient_sex: Some("female".into()),
        country: None,
        difficulty: "intern".into(),
        endemic: false,
        provisional: true,
        withdrawn: false,
        version: "0.1.0".into(),
        title: "a case".into(),
    };
    let who = |age: u16| Persona {
        name: "Forseti Probe".into(), country: "THA".into(), age, sex: "f".into(),
    };
    let ok = |c: u32, p: u16| contradicts(&case(c), &who(p)).is_none();

    // The pair from the tick.
    let refused = contradicts(&case(8), &who(1)).expect("a one-year-old is not an eight-year-old");
    assert!(refused.contains('8') && refused.contains('1'), "with both ages in it: {refused}");

    // under 5: within one year, and never below one.
    assert!(ok(3, 2) && ok(3, 3) && ok(3, 4));
    assert!(!ok(3, 5) && !ok(3, 1), "two years is a different patient at three");
    assert!(ok(1, 1) && !ok(1, 0), "nobody is nought, and the door says so elsewhere too");

    // 5–15: within three, and never outside the band.
    assert!(ok(8, 5) && ok(8, 11) && !ok(8, 4) && !ok(8, 12));
    assert!(ok(15, 12) && !ok(15, 16), "sixteen is not a child's case");
    assert!(ok(5, 8) && !ok(5, 4), "and four is not on a five-year-old's case");

    // 16–39: sixteen at the youngest, ten years up, eight years down.
    assert!(ok(20, 16) && !ok(20, 15), "a child never plays an adult's case");
    assert!(ok(30, 40) && !ok(30, 41), "ten years older");
    assert!(ok(30, 22) && !ok(30, 21), "eight years younger");

    // 40 and over: within ten, which is the rule that was right all along.
    assert!(ok(62, 52) && ok(62, 72) && !ok(62, 51) && !ok(62, 73));
    assert!(ok(80, 74));
}

/// **What makes a case unreplaceable is a shift on the chain, not a clinician's signature.**
///
/// The rule above this one read `provisional`, and its comment in main.rs claimed "a reviewed case
/// is one the chain carries shifts against". That was an assumption written down as a fact, and it
/// was false: `provisional` is the compiler's word for clinical review, every case on this ward is
/// provisional, and so the check passed everything. On 23 ก.ย. a recompiled pack went over
/// `ddx-pneumonia-1-en` while the chain held a closure against it. `sce_hash` is an input to the
/// leaf, so the closure stopped re-deriving — not a different score, a shift that no longer
/// verifies at all.
///
/// The rule is narrower than "never replace a played case", because only some of a pack can hurt
/// an anchored shift. The **scored content** is the `sce` block, which the leaf commits to, and the
/// `rubric`, which the receipt's mark sheet is computed from. A title, a tag, a country or a
/// version is presentation: changing it cannot move a leaf or a sheet, and refusing it would be
/// theatre. So the door blocks a change to the scored content of a case the chain has shifts
/// against, and lets everything else through.
///
/// A fresh ward has no patient on this case at all, which is a definite "nothing is anchored"
/// rather than an unknown — the unknown case is the one where patients exist and the board cannot
/// be read, and that one refuses with 503 rather than guessing.
#[test]
fn a_case_the_chain_has_no_shifts_against_may_be_recompiled() {
    let s = Server::start();
    assert_eq!(s.post("/api/ward/case", &a_pack()).0, 200);

    // Presentation only: the scored content is untouched, so this is not the dangerous kind of
    // change and the catalogue takes it.
    let mut retitled = a_pack();
    retitled["title"] = json!("a better title");
    retitled["version"] = json!("0.1.1");
    let (code, body) = s.post("/api/ward/case", &retitled);
    assert_eq!(code, 200, "a title is not something a leaf commits to: {body}");

    // The scored content itself, on a ward where nobody is on this case: allowed, because there is
    // no anchored shift for it to rewrite.
    let mut recompiled = a_pack();
    recompiled["version"] = json!("0.2.0");
    recompiled["sce"]["interventions"][0]["match"]["any_kw"] = json!(["a new phrase"]);
    let (code, body) = s.post("/api/ward/case", &recompiled);
    assert_eq!(code, 200, "nothing is anchored against it, so a recompile lands: {body}");
}

/// **The same bytes twice is not a store, and the door says so instead of reporting one.**
///
/// A replace at an unchanged version with changed bytes is a silent edit, and after the pneumonia
/// incident it is a silent edit to something a leaf depends on. So the identifiers have to move
/// when the scored content does — `version` for an authored change, `compiler.commit` for a
/// recompile — and the refusal names both hashes so the caller can see for themselves that the
/// bytes differ.
#[test]
fn a_silent_edit_is_refused_and_an_identical_push_writes_nothing() {
    let s = Server::start();
    assert_eq!(s.post("/api/ward/case", &a_pack()).0, 200);

    // Byte for byte what is already held. Nothing to write, and the answer says that rather than
    // reporting a store that did not happen.
    let (code, body) = s.post("/api/ward/case", &a_pack());
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["unchanged"], json!(true),
               "an identical push writes nothing and says so: {body}");

    // Changed scored content, and every identifier standing still. This is the shape of the thing
    // that went wrong at 14:44 and the door has to name it.
    let mut edited = a_pack();
    edited["sce"]["interventions"][0]["match"]["any_kw"] = json!(["something else"]);
    let (code, body) = s.post("/api/ward/case", &edited);
    assert_eq!(code, 409, "a silent edit is refused: {body}");
    let why = body["refused"].as_str().unwrap_or_default();
    assert!(why.contains("version") && why.contains("compiler"),
            "and the refusal says which identifier should have moved: {why}");
    assert_ne!(body["sce_sha256_held"], body["sce_sha256_offered"],
               "with both hashes, so the caller can see the bytes differ: {body}");

    // The same change, with the compiler's commit moved: a recompile, and it lands.
    let mut rebuilt = edited.clone();
    rebuilt["compiler"] = json!({"name": "vitals-casefactory", "version": "0.9.4",
                                 "commit": "ccd73727b039"});
    let (code, body) = s.post("/api/ward/case", &rebuilt);
    assert_eq!(code, 200, "a recompile by a different compiler is not a silent edit: {body}");
}

/// **The case Amelia's shift is anchored against cannot be recompiled under her.**
///
/// This is the rule the other two do not test, and the one the incident was about. On 23 ก.ย. a
/// recompiled pack replaced `ddx-pneumonia-1-en` while the chain carried a ward closure against it;
/// `sce_hash` is an input to the leaf, so that closure stopped re-deriving. The check that let it
/// through was asking whether a clinician had reviewed the case.
///
/// Seeded through the ward's own `keep_board`, not a hand-built row — the same reason the factory
/// contract test calls `queue_block` instead of describing it. A board written by the writer is a
/// board the reader will accept, and a fixture of my own shape would only prove I can agree with
/// myself.
///
/// The presentation half is asserted in the same test on purpose: a title may still be corrected on
/// a case the chain has shifts against, because a title is not something a leaf or a mark sheet is
/// computed from. A rule that refuses everything is easy and wrong.
#[test]
fn the_scored_content_of_a_case_with_a_shift_on_chain_is_not_replaceable() {
    let s = Server::start();
    assert_eq!(s.post("/api/ward/case", &a_pack()).0, 200);

    // A patient on this case whom the chain has finished: one shift, and a closed slot. Whoever
    // signed it, it is an anchored shift and its leaf commits to these bytes.
    let store = vitals_web::store::Store::open(s.state()).expect("the ward's own store");
    let board = json!({
        "readable": true,
        "source": "devnet:test",
        "patients": [{
            "patient_id": 1790037060u64,
            "state": "died",
            "case": "auth-demo-1",
            "shifts": 1,
            "bed": null,
            "admitted_slot": 1u64,
            "closed_slot": 502381820u64,
        }],
    });
    assert!(vitals_web::ward_chain::keep_board(&store, &board, "test"),
            "a readable board is kept, and this test needs the ward to have one");

    // The dangerous change: the scored content, under a patient whose shift is on chain.
    let mut recompiled = a_pack();
    recompiled["version"] = json!("0.2.0");
    recompiled["sce"]["interventions"][0]["match"]["any_kw"] = json!(["pertussis"]);
    let (code, body) = s.post("/api/ward/case", &recompiled);
    assert_eq!(code, 409, "the chain carries a shift against it: {body}");
    let why = body["refused"].as_str().unwrap_or_default();
    assert!(why.contains("shift"),
            "and the refusal says why, in the terms that make it true: {why}");
    assert!(!why.contains("reviewed"),
            "and not in terms of clinical review, which is what the old rule wrongly asked: {why}");

    // The safe change, on the very same case: a title cannot move a leaf or a mark sheet, so it is
    // still allowed while she is on the board.
    let mut retitled = a_pack();
    retitled["title"] = json!("a corrected title");
    retitled["version"] = json!("0.1.1");
    let (code, body) = s.post("/api/ward/case", &retitled);
    assert_eq!(code, 200, "presentation is not scored content: {body}");
}

/// **Every version of a case's scored content is kept, addressed by its own bytes.**
///
/// A shift's leaf commits to `sce_hash(sce_json)`, and its mark sheet is computed from the rubric.
/// Both are read from whatever the case store holds *now*, so a corrected case rewrites what an
/// anchored shift is shown to have been played against — which is why 31 production cases are
/// pinned and cannot take the diagnosis fix at all.
///
/// The way out is the ordinary one: keep the bytes, address them by themselves, let the mutable
/// pointer move. This is the keeping half. A pack arriving at the door leaves its scored content in
/// the blob store under the hash of that content, so a later correction adds a blob rather than
/// replacing the one an anchored shift was played on.
///
/// Addressed by `sce` **and** `rubric` together, because both decide what a receipt says: the leaf
/// commits to the first and the sheet is computed from the second, and a rubric corrected under an
/// unchanged scenario would otherwise silently take the earlier blob's place — the same defect one
/// layer down. The `sce` hash is recorded inside the blob as well, because that is the one a leaf
/// can be matched against when working out which bytes an already-anchored shift was played on.
#[test]
fn the_door_keeps_every_version_of_what_a_case_scores() {
    let s = Server::start();
    assert_eq!(s.post("/api/ward/case", &a_pack()).0, 200);

    let store = vitals_web::store::Store::open(s.state()).expect("the ward's own store");
    let first = vitals_web::ward_case::played_id(&a_pack());
    let kept = vitals_web::ward_case::played_bytes(&store, &first)
        .expect("the bytes a shift on this version would have been played against");
    assert_eq!(kept.sce_sha256, vitals_web::ward_case::sce_sha256(&a_pack()),
               "a blob knows the sce hash a leaf would name");
    assert!(!kept.sce.is_empty() && !kept.rubric.is_empty(), "both halves are kept");

    // A correction: new scored content, a moved version, nothing anchored against the case.
    let mut fixed = a_pack();
    fixed["version"] = json!("0.2.0");
    fixed["sce"]["interventions"][0]["match"]["any_kw"] = json!(["pertussis", "whooping cough"]);
    assert_eq!(s.post("/api/ward/case", &fixed).0, 200);

    // **The first version is still there.** That is the whole point: an anchored shift played on it
    // can still be re-derived from the bytes it was played on, not from the correction.
    let second = vitals_web::ward_case::played_id(&fixed);
    assert_ne!(second, first, "the correction changed what the case scores");
    assert!(vitals_web::ward_case::played_bytes(&store, &first).is_some(),
            "the version an anchored shift was played on must survive the correction");
    assert!(vitals_web::ward_case::played_bytes(&store, &second).is_some(),
            "and the correction is kept too, for the shifts that come after it");

    // A rubric corrected under an unchanged scenario is a different blob, because the sheet a
    // receipt shows is computed from it. Addressing on `sce` alone would have lost this one.
    let mut regraded = a_pack();
    regraded["version"] = json!("0.3.0");
    regraded["rubric"]["items"][0]["points"] = json!(99);
    assert_ne!(vitals_web::ward_case::played_id(&regraded), first,
               "the rubric is part of what a case scores, so it is part of the address");

    // Pushing the same bytes twice is one blob: they are addressed by themselves.
    assert_eq!(s.post("/api/ward/case", &fixed).1["unchanged"], json!(true));
    assert!(vitals_web::ward_case::played_bytes(&store, &second).is_some());
}

/// **A shift says which bytes it was played against, and says nothing when it cannot know.**
///
/// Keeping the blobs is half the repair; the other half is that a shift anchored from here on
/// records the address of the bytes it ran on, so its receipt re-derives against those rather than
/// against whatever the case store holds when somebody opens it years later.
///
/// The address is only recorded where it is a fact. The hand-over path is the one writer that *is*
/// the play — it holds the scenario the session ran and the pack whose rubric its sheet comes from
/// — so the rule lives here as one function rather than as a filter chain inside the request
/// handler, and the cases where it must stay silent are tested as carefully as the case where it
/// speaks. Every other writer of a tape is reconstructing somebody else's anchored shift and does
/// not know what it was played on; those record nothing and are resolved by replay instead.
///
/// The load-bearing assertion is the last one: after a correction moves the case pointer, a shift
/// still holding the *old* scenario is not labelled with the new bytes. A guess there would be
/// indistinguishable from a fact, and would quietly re-create the defect the blobs exist to end.
#[test]
fn a_shift_records_the_bytes_it_was_played_against() {
    let s = Server::start();
    assert_eq!(s.post("/api/ward/case", &a_pack()).0, 200);
    let store = vitals_web::store::Store::open(s.state()).expect("the ward's own store");

    // A patient the ward is holding, queued for that case — what `packs` reads at hand-over.
    let patient: vitals_web::ward::Pack = serde_json::from_value(json!({
        "case": "auth-demo-1",
        "persona": { "name": "อารีย์", "age": 62, "sex": "f", "country": "THA" },
    }))
    .expect("a patient the factory queued");
    store
        .put(vitals_web::ward_chain::PERSONA_STORE, "p7", &patient)
        .expect("a bed on the board");

    // The scenario a shift on her actually runs, taken the way the ward takes it.
    let played = vitals_web::ward_case::sce_of(&store, "auth-demo-1")
        .expect("the scenario the ward plays for this case");

    let addr = vitals_web::ward_case::played_address(&store, 7, &played);
    assert_eq!(addr, vitals_web::ward_case::played_id(&a_pack()),
               "the shift is addressed to the bytes the door kept");
    assert!(vitals_web::ward_case::played_bytes(&store, &addr).is_some(),
            "and the address resolves to those bytes, which is the whole point of recording it");

    // Silent where it would be guessing. A scenario that is not this pack's own means the case
    // moved under the player: that shift is one prove-by-replay must resolve, not one to label.
    assert!(vitals_web::ward_case::played_address(&store, 7, "{\"tick_seconds\":1.0}").is_empty(),
            "a scenario that is not the pack's own is not evidence of anything");
    assert!(vitals_web::ward_case::played_address(&store, 99, &played).is_empty(),
            "and a patient this ward is not holding tells us nothing about what was played");

    // **While she is on the board the bytes cannot move at all.** The door refuses to change what a
    // case scores when it cannot prove no anchored shift is riding on it, which is why the
    // mislabelling below is rare rather than routine — but rare is not never, and it is already in
    // production: 31 cases took corrections before this rule existed.
    let mut fixed = a_pack();
    fixed["version"] = json!("0.2.0");
    fixed["sce"]["vitals0"]["hr"] = json!(124.0);
    let (code, refusal) = s.post("/api/ward/case", &fixed);
    assert_eq!(code, 503, "a patient is on this case: {refusal}");
    assert!(refusal["refused"].as_str().unwrap_or_default().contains("what that case scores"),
            "and it refuses on the scored content, not on the pack as a whole: {refusal}");

    // So reach the state those 31 are already in, the way they got there: the pointer moved while a
    // shift was riding on the older bytes. Nothing but the pointer changes; the blobs both stand.
    store
        .put(vitals_web::ward_case::CASE_STORE,
             &vitals_web::ward_case::key_for("auth-demo-1"), &fixed)
        .expect("the corrected case, as a pre-rule correction left it");

    let now = vitals_web::ward_case::sce_of(&store, "auth-demo-1").expect("the corrected scenario");
    assert_ne!(now, played, "the correction really did change the scenario");
    assert_eq!(vitals_web::ward_case::played_address(&store, 7, &now),
               vitals_web::ward_case::played_id(&fixed),
               "a shift that starts after the correction is addressed to the corrected bytes");
    assert!(vitals_web::ward_case::played_address(&store, 7, &played).is_empty(),
            "and a shift still holding the scenario from before it is never labelled with the new \
             bytes — that mislabelling is the defect the blobs exist to end");
}

/// **The address a shift records always resolves to bytes — even for a case the door never kept.**
///
/// A shift records the address of the bytes it was played against at hand-over. That address is
/// only worth recording if the bytes are there to be found: the receipt that re-derives from played
/// bytes, and the proof that decides which bytes produced a leaf, both look them up by it. The door
/// keeps a blob on every push and the seed keeps one for every case the store lists — but a pack can
/// reach the case store another way (every correction filed before the blob store existed did), and
/// a shift played on such a pack would record an address with nothing behind it, and read
/// unrebuildable the moment it was handed over. The test that precedes this one sets up exactly that
/// state and checks only that the shift is addressed, one assertion short of checking the address
/// resolves. This is that assertion.
///
/// So recording the address keeps the bytes. Keeping is idempotent and addressed by content, so for a
/// case the door already kept this writes the same blob again and changes nothing; it only closes the
/// gap for one it did not. It never touches what an existing leaf commits to.
#[test]
fn a_shift_keeps_the_bytes_it_records_even_on_a_case_the_door_never_kept() {
    let s = Server::start();
    assert_eq!(s.post("/api/ward/case", &a_pack()).0, 200);
    let store = vitals_web::store::Store::open(s.state()).expect("the ward's own store");

    let patient: vitals_web::ward::Pack = serde_json::from_value(json!({
        "case": "auth-demo-1",
        "persona": { "name": "อารีย์", "age": 62, "sex": "f", "country": "THA" },
    }))
    .expect("a patient the factory queued");
    store
        .put(vitals_web::ward_chain::PERSONA_STORE, "p7", &patient)
        .expect("a bed on the board");

    // A pack that reached the case store without passing the door — the way every correction filed
    // before the blob store existed did. Nobody kept its bytes.
    let mut unkept = a_pack();
    unkept["version"] = json!("0.2.0");
    unkept["sce"]["vitals0"]["hr"] = json!(124.0);
    store
        .put(vitals_web::ward_case::CASE_STORE,
             &vitals_web::ward_case::key_for("auth-demo-1"), &unkept)
        .expect("a case filed without the door");
    let unkept_id = vitals_web::ward_case::played_id(&unkept);
    assert!(vitals_web::ward_case::played_bytes(&store, &unkept_id).is_none(),
            "the premise: nobody kept these bytes when they were filed");

    // A stranger plays a shift on it, and the ward records what it was played on.
    let played = vitals_web::ward_case::sce_of(&store, "auth-demo-1")
        .expect("the scenario she plays");
    let addr = vitals_web::ward_case::played_address(&store, 7, &played);
    assert_eq!(addr, unkept_id, "the shift is addressed to the bytes it actually ran");

    // The load-bearing assertion. Before this, the address named bytes nobody had kept.
    assert!(vitals_web::ward_case::played_bytes(&store, &addr).is_some(),
            "an address a shift records must resolve to its bytes — otherwise the shift is \
             unrebuildable the moment it is handed over");
}

/// **The cases the ward already holds get blobs too**, or the repair only covers what arrives next.
///
/// The door keeps a blob on every push, so every case published from now on is addressed. The ward
/// in production is holding 106 cases that were filed before the blob store existed, and an
/// anchored shift on one of those has nothing to be matched against. Seeding walks what the store
/// holds and keeps each case's current scored content under its own address.
///
/// It only recovers the *current* version. A case corrected before the blob store existed has lost
/// its earlier bytes for good — that is the honest unrebuildable, and it is a fact to publish, not
/// a gap to fill with whatever the store holds today.
#[test]
fn the_ward_seeds_a_blob_for_every_case_it_already_holds() {
    let s = Server::start();
    let store = vitals_web::store::Store::open(s.state()).expect("the ward's own store");

    // Two cases filed the way the 106 were: straight into the store, before any blob was kept.
    let mut filed = vec![];
    for (id, hr) in [("pre-door-1", 110.0), ("pre-door-2", 96.0)] {
        let mut pack = a_pack();
        pack["case_id"] = json!(id);
        pack["sce"]["vitals0"]["hr"] = json!(hr);
        store
            .put(vitals_web::ward_case::CASE_STORE, &vitals_web::ward_case::key_for(id), &pack)
            .expect("a case filed before the blob store existed");
        assert!(vitals_web::ward_case::played_bytes(&store,
                    &vitals_web::ward_case::played_id(&pack)).is_none(),
                "no blob yet — this is the state the seeding exists to repair");
        filed.push(pack);
    }

    let kept = vitals_web::ward_case::seed_played_bytes(&store);
    assert!(kept >= 2, "every case the ward holds now has its bytes kept, not only these two: {kept}");
    for pack in &filed {
        assert!(vitals_web::ward_case::played_bytes(&store,
                    &vitals_web::ward_case::played_id(pack)).is_some(),
                "a case filed before the blob store existed can now be addressed");
    }

    // Idempotent because the address *is* the bytes: seeding twice keeps the same blobs.
    assert_eq!(vitals_web::ward_case::seed_played_bytes(&store), kept,
               "seeding again keeps the same blobs rather than a second copy of each");
}

/// **An already-anchored shift is matched to its bytes by replaying them**, not by being told.
///
/// Nothing on chain says which bytes a shift was played on — that is the whole defect. But the leaf
/// does: it commits to `sce_hash(sce_json)` and to the tape, so replaying a candidate blob's
/// scenario against the tape the ward kept either reproduces the anchored leaf or does not. A blob
/// that reproduces it is what this shift was played on, demonstrated rather than assumed.
///
/// Three outcomes, and the two that are not a clean match are the point of the exercise:
///
///   * one blob reproduces the leaf — proved, and its address can be recorded;
///   * several do — they share a scenario and differ only in rubric, which a leaf commits to
///     nothing about, so which rubric scored this shift cannot be told from the chain. A coin flip
///     here would put a number on a stranger's receipt that no evidence supports;
///   * none does — the bytes are not in this ward any more, and the chart cannot be rebuilt.
///
/// The ambiguous case is why recording the address forward at hand-over is not merely an
/// efficiency: for a shift anchored before that recording existed, on a case that later takes a
/// rubric-only correction, replay can never recover the second half of the address.
#[test]
fn an_anchored_shift_is_matched_to_its_bytes_by_replaying_them() {
    use vitals_web::ward_case::ForReceipt;

    let s = Server::start();
    assert_eq!(s.post("/api/ward/case", &a_pack()).0, 200);
    let store = vitals_web::store::Store::open(s.state()).expect("the ward's own store");

    // A real shift: somebody opened her chart, did nothing, and handed over. The tape is empty and
    // the leaf is as binding as any other.
    let played = vitals_web::ward_case::sce_of(&store, "auth-demo-1").expect("the scenario");
    let tape: Vec<vitals_replay::Step> = vec![];
    let anchored = leaf_the_receipts_way(&store, &played, &tape, LONE_SLOT);
    let shifts = lone(LONE_SLOT);
    let tape_of = |h: &str| vitals_web::ward_chain::tape_by_hash(&store, h);
    let deriving = vitals_web::ward_chain::Deriving {
        shifts: &shifts, this: &shifts[0], tape_of: &tape_of,
        admitted_slot: LONE_SLOT, dated: &ward_dated,
    };

    match vitals_web::ward_case::bytes_for_receipt(&store, &anchored, &tape, &deriving, None, None) {
        ForReceipt::These { played_id, .. } => assert_eq!(
            played_id, vitals_web::ward_case::played_id(&a_pack()),
            "the blob that reproduces the leaf is the one the door kept"),
        other => panic!("one blob reproduces this leaf and it should have been proved: {other:?}"),
    }

    // A rubric corrected under an unchanged scenario. Both blobs reproduce the leaf, because the
    // leaf commits to the scenario and the tape and to nothing about the rubric.
    let mut regraded = a_pack();
    regraded["version"] = json!("0.2.0");
    regraded["rubric"]["items"][0]["points"] = json!(99);
    vitals_web::ward_case::keep_played_bytes(&store, &regraded);

    match vitals_web::ward_case::bytes_for_receipt(&store, &anchored, &tape, &deriving, None, None) {
        ForReceipt::ScenarioOnly { candidates: ids, .. } => {
            assert_eq!(ids.len(), 2, "both rubrics fit the same leaf: {ids:?}");
            assert!(ids.contains(&vitals_web::ward_case::played_id(&a_pack()))
                    && ids.contains(&vitals_web::ward_case::played_id(&regraded)),
                    "and it names both rather than picking one: {ids:?}");
        }
        other => panic!("two blobs share this scenario; picking one would be a guess: {other:?}"),
    }

    // A leaf nothing here reproduces. Not an error and not a blank — a shift whose chart this ward
    // can no longer rebuild, which is a fact a reader is owed.
    assert!(matches!(vitals_web::ward_case::bytes_for_receipt(
                         &store, &"0".repeat(64), &tape, &deriving, None, None),
                     ForReceipt::Unrebuildable),
            "a leaf no kept bytes reproduce is unrebuildable, and says so");
}

/// **Every shift the chain holds, and what this ward can prove about the bytes behind it.**
///
/// This is the list to read before anything is repaired. One row per anchored shift: the leaf, the
/// patient, the case her pack names now, what the hand-over recorded if it recorded anything, and
/// what replaying the kept bytes against the kept tape actually demonstrates.
///
/// Both sources are reported, never reconciled. A recorded address and a proved one that name
/// different bytes is a finding — the case moved under a shift, or something wrote an address it
/// could not know — and the row says so and changes nothing. Every defect on this ward so far came
/// from a second source of truth added without asking what happens when it disagrees with the
/// first; here the answer is that the disagreement is published.
///
/// Four verdicts, because the ways a chart fails to rebuild are not the same thing: the bytes are
/// missing, or several fit equally, or the tape itself was never kept here — which is a different
/// problem with a different fix, and collapsing it into "unrebuildable" would hide it.
#[test]
fn the_ward_lists_what_it_can_prove_about_every_anchored_shift() {
    let s = Server::start();
    assert_eq!(s.post("/api/ward/case", &a_pack()).0, 200);
    let store = vitals_web::store::Store::open(s.state()).expect("the ward's own store");

    let tape: Vec<vitals_replay::Step> = vec![];

    // Three cases, so three distinct scenarios and three distinct leaves. A tape is filed under the
    // leaf and a leaf commits to nothing about who played it, so two patients on one scenario would
    // share a single tape record — true of the real ward, and not what this test is measuring.
    let case = |id: &str, hr: f64| -> (String, String) {
        let mut pack = a_pack();
        pack["case_id"] = json!(id);
        pack["sce"]["vitals0"]["hr"] = json!(hr);
        assert_eq!(s.post("/api/ward/case", &pack).0, 200, "the door takes {id}");
        let sce = vitals_web::ward_case::sce_of(&store, id).expect("its scenario");
        (leaf_the_receipts_way(&store, &sce, &tape, LONE_SLOT),
         vitals_web::ward_case::played_id(&pack))
    };
    let (leaf_a, addr_a) = case("auth-demo-a", 111.0);
    let (leaf_b, addr_b) = case("auth-demo-b", 112.0);
    let (leaf_c, addr_c) = case("auth-demo-c", 113.0);
    assert!(leaf_a != leaf_b && leaf_b != leaf_c, "three scenarios, three leaves");

    let anchor = |id: u64, leaf: &str, case_id: &str, recorded: &str, keep_the_tape: bool| {
        let shift = vitals_web::ward::ShiftOnChain {
            patient_id: id, signer: [1; 32], slot: LONE_SLOT,
            run_hash: {
                let mut b = [0u8; 32];
                for (i, c) in b.iter_mut().enumerate() {
                    *c = u8::from_str_radix(&leaf[i * 2..i * 2 + 2], 16).expect("hex");
                }
                b
            },
        };
        let mut seen = vitals_web::ward_chain::Seen::default();
        seen.absorb(vec![(shift, format!("sig{id}"))], Some((format!("sig{id}"), 100)));
        store.put(vitals_web::ward_chain::SHIFT_CACHE, &format!("p{id}"), &seen).expect("cached");
        let patient: vitals_web::ward::Pack = serde_json::from_value(json!({
            "case": case_id,
            "persona": { "name": "อารีย์", "age": 62, "sex": "f", "country": "THA" },
        })).expect("a patient");
        store.put(vitals_web::ward_chain::PERSONA_STORE, &format!("p{id}"), &patient).expect("bed");
        if keep_the_tape {
            vitals_web::ward_chain::keep_tape(&store, &vitals_web::ward_chain::StoredTape {
                patient_id: id,
                run_hash: leaf.to_string(),
                steps: tape.clone(),
                played_id: recorded.to_string(),
            })
            .expect("tape kept");
        }
    };

    anchor(1, &leaf_a, "auth-demo-a", &addr_a, true);  // proved, and the recording agrees
    anchor(2, &leaf_b, "auth-demo-b", "", true);       // proved, nothing recorded — pre-piece-two
    anchor(3, &"a".repeat(64), "auth-demo-a", "", true);   // a tape, but no bytes fit that leaf
    anchor(4, &"b".repeat(64), "auth-demo-a", "", false);  // no tape kept here at all
    // The row every reader is here for: a recorded address that is not what replay proves.
    anchor(5, &leaf_c, "auth-demo-c", &addr_a, true);

    // Her admission is the shift's own slot, so no idle time enters the derivation and the rows are
    // about which bytes were chosen.
    let admitted_of = |_id: u64| Some(LONE_SLOT);
    let as_it_stands_of = |case: &str| standing(&store, case);
    let whole = std::time::Duration::from_secs(60);
    let (rows, next) = vitals_web::ward_chain::bytes_behind_the_anchored_shifts(
        &store, &admitted_of, &as_it_stands_of, whole, None);
    assert_eq!(next, None, "a budget this generous finishes the ward in one call");
    let of = |id: u64| rows.iter().find(|r| r.patient_id == id).expect("a row per anchored shift");
    assert_eq!(rows.len(), 5, "one row per anchored shift, none dropped: {rows:#?}");

    assert_eq!(of(1).verdict, "proved");
    assert_eq!(of(1).proved.as_deref(), Some(addr_a.as_str()));
    assert_eq!(of(1).recorded.as_deref(), Some(addr_a.as_str()));
    assert!(!of(1).disagrees, "the hand-over recorded what replay proves");
    assert_eq!(of(1).case, "auth-demo-a", "the row carries the case her pack names, for a reader");

    assert_eq!(of(2).verdict, "proved", "a shift from before the recording existed is recoverable");
    assert_eq!(of(2).proved.as_deref(), Some(addr_b.as_str()));
    assert_eq!(of(2).recorded, None, "and it is honest that nothing was recorded");
    assert!(!of(2).disagrees, "nothing recorded is not a disagreement");

    assert_eq!(of(3).verdict, "unrebuildable", "a leaf no kept bytes reproduce");
    assert_eq!(of(3).proved, None);

    assert_eq!(of(4).verdict, "no tape", "a tape this ward never kept is its own problem");
    assert_eq!(of(4).proved, None);

    assert_eq!(of(5).verdict, "proved", "replay still proves which bytes reproduce the leaf");
    assert_eq!(of(5).proved.as_deref(), Some(addr_c.as_str()));
    assert_eq!(of(5).recorded.as_deref(), Some(addr_a.as_str()),
               "and the recorded address is reported as it stands, not quietly corrected");
    assert!(of(5).disagrees, "the two sources name different bytes, and the row says so");

    // **Paging must not change a verdict.** A full read of a ward with shifts whose bytes are gone
    // took 140 seconds on staging and held the only instance for all of it, so the call is bounded
    // and resumable. That is safe only if the answer does not depend on where the cut fell — a
    // verdict that moved with the page boundary would be a published fact decided by a stopwatch.
    let mut paged: Vec<(u64, &str)> = Vec::new();
    let mut cursor = None;
    for _ in 0..16 {
        let (page, stopped) = vitals_web::ward_chain::bytes_behind_the_anchored_shifts(
            &store, &admitted_of, &as_it_stands_of, std::time::Duration::ZERO, cursor);
        assert!(!page.is_empty(), "every call makes progress, or the cursor would never advance");
        paged.extend(page.iter().map(|r| (r.patient_id, r.verdict)));
        match stopped {
            Some(at) => cursor = Some(at),
            None => break,
        }
    }
    let at_once: Vec<(u64, &str)> = rows.iter().map(|r| (r.patient_id, r.verdict)).collect();
    assert_eq!(paged, at_once,
               "read a patient at a time, the ward reports exactly what it reports in one call");

    // And exactly once: a patient is never split across two calls, so no shift is counted twice.
    let mut ids: Vec<u64> = paged.iter().map(|(id, _)| *id).collect();
    let before = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), before, "no shift is read twice across the pages");
}

/// **A writer that does not know what a shift was played against must not erase the answer.**
///
/// Tapes are content-addressed by the leaf, so keeping the same tape twice is keeping it once — and
/// that is exactly the hazard. The hand-over is the only writer that knows the played address;
/// every reconstructing writer passes an empty one by design, because guessing would be worse. A
/// plain put means a rebuild of an already-anchored shift, months later, silently deletes the fact
/// the hand-over recorded, and the deletion looks identical to a shift that never recorded one.
///
/// Found by a test whose own setup gave two patients one leaf. The ward had this hole in it from the
/// moment the field was added.
#[test]
fn keeping_a_tape_again_never_erases_the_address_already_on_it() {
    use vitals_web::ward_chain::{keep_tape, StoredTape, TAPE_STORE};

    let dir = std::env::temp_dir().join(format!("vitals-erase-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = vitals_web::store::Store::open(dir).expect("a store");

    let leaf = "c".repeat(64);
    let recorded = "d".repeat(64);
    let tape = |played_id: &str| StoredTape {
        patient_id: 7,
        run_hash: leaf.clone(),
        steps: vec![],
        played_id: played_id.to_string(),
    };
    let on_it = |store: &vitals_web::store::Store| -> String {
        store.get::<StoredTape>(TAPE_STORE, &leaf).expect("the tape").played_id
    };

    keep_tape(&store, &tape(&recorded)).expect("the hand-over files it with its address");
    assert_eq!(on_it(&store), recorded);

    // A rebuild of the same shift. It does not know, and must not say so on the record.
    keep_tape(&store, &tape("")).expect("kept again");
    assert_eq!(on_it(&store), recorded, "a rebuild must not erase what the hand-over knew");

    // A later hand-over that does know, on a case whose rubric was corrected between the two, is a
    // writer with evidence — it replaces the address rather than being ignored.
    let later = "e".repeat(64);
    keep_tape(&store, &tape(&later)).expect("kept again");
    assert_eq!(on_it(&store), later, "a writer that knows may correct a writer that knew");

    // And a tape nobody has filed before keeps whatever it arrives with, including nothing.
    let other = StoredTape { run_hash: "f".repeat(64), ..tape("") };
    keep_tape(&store, &other).expect("kept");
    assert_eq!(store.get::<StoredTape>(TAPE_STORE, &other.run_hash).expect("it").played_id, "",
               "an empty address on a new tape is the ordinary state, not an erasure");
}

/// **Both the seed and the integrity list take the door's secret.**
///
/// Seeding writes, and an operator asks for it deliberately — the rule on this repair is that
/// nothing is fixed behind anybody's back, and a seed running by itself on every cold start would be
/// the first step of exactly that.
///
/// Reading changes nothing, and it was public for one deploy on the reasoning that a shift this ward
/// cannot rebuild is a fact the person holding that receipt is owed. That much is true; the surface
/// was wrong. Unseeded, the list answered "77 of 77 shifts unrebuildable" on production — a sentence
/// about this ward never having looked, published as a finding about the chain. The per-shift
/// honesty a stranger is owed belongs on their own receipt, where it sits next to the shift it is
/// about and can be checked; an aggregate that reads catastrophically before its own setup step has
/// run is a footgun whoever is holding it.
///
/// The shapes are pinned here because this is the surface somebody reads before deciding what to
/// repair. A count that quietly stopped being published would make a clean list out of a ward with
/// findings in it.
#[test]
fn the_seed_and_the_integrity_list_are_both_the_operators() {
    let s = Server::start();
    let store = vitals_web::store::Store::open(s.state()).expect("the ward's own store");

    // A case filed the way the 106 in production were: before any blob was kept.
    let mut pack = a_pack();
    pack["case_id"] = json!("pre-seed");
    store
        .put(vitals_web::ward_case::CASE_STORE, &vitals_web::ward_case::key_for("pre-seed"), &pack)
        .expect("a case filed before the blob store existed");
    let addr = vitals_web::ward_case::played_id(&pack);
    assert!(vitals_web::ward_case::played_bytes(&store, &addr).is_none(), "nothing kept for it yet");

    // It writes, so it is the factory's door and not the page's.
    assert_eq!(s.post_with("/api/ward/seed", &json!({}), None).0, 401,
               "a write with no secret is refused");
    assert_eq!(s.post_with("/api/ward/seed", &json!({}), Some("not-the-token")).0, 401,
               "and somebody else's secret is not this door's");

    let (code, body) = s.post("/api/ward/seed", &json!({}));
    assert_eq!(code, 200, "{body}");
    Server::reads_as_sentences(&body);
    assert!(body["cases_kept"].as_u64().unwrap_or(0) >= 1, "it says how many it kept: {body}");
    assert!(vitals_web::ward_case::played_bytes(&store, &addr).is_some(),
            "and the case filed before the blob store existed is now addressable by its own bytes");

    // One anchored shift, so the answer below carries a row and not only its counts. A consumer
    // reads `rows[].verdict`, and a fixture with an empty list pins the envelope and leaves every
    // field of a row unguarded.
    {
        let shift = vitals_web::ward::ShiftOnChain {
            patient_id: 4242, signer: [1; 32], slot: LONE_SLOT, run_hash: [0xab; 32],
        };
        let mut seen = vitals_web::ward_chain::Seen::default();
        seen.absorb(vec![(shift, "sig".to_string())], Some(("sig".to_string(), LONE_SLOT)));
        store.put(vitals_web::ward_chain::SHIFT_CACHE, "p4242", &seen).expect("cached");
        vitals_web::ward_chain::keep_tape(&store, &vitals_web::ward_chain::StoredTape {
            patient_id: 4242,
            run_hash: vitals_web::ward_chain::hex32(&[0xab; 32]),
            steps: vec![],
            played_id: String::new(),
        }).expect("tape kept");
    }

    // Reading is an operator's too — unseeded it would answer "every shift unrebuildable", which
    // is a sentence about this ward never having looked and not a fact about the chain.
    assert_eq!(s.get("/api/ward/bytes").0, 401, "the list is not a stranger's to read");
    let (code, body) = s.post_get("/api/ward/bytes");
    assert_eq!(code, 200, "{body}");
    Server::reads_as_sentences(&body);
    for named in ["shifts", "proved", "proved_as_it_stands", "ambiguous", "unrebuildable",
                  "no_tape", "not_asked", "disagreements", "rows"] {
        assert!(body.get(named).is_some(), "the published list names {named}: {body}");
        assert!(body["derivations"].get(named).is_some(),
                "and says how {named} is known, because a count nobody can check is a claim: {body}");
    }
    assert_eq!(body["shifts"], json!(1), "the one shift this ward has read");
    let row = &body["rows"][0];
    for named in ["patient_id", "leaf", "case", "recorded", "verdict", "proved", "candidates",
                  "disagrees"] {
        assert!(row.get(named).is_some(), "a row names {named}: {row}");
    }
    assert_eq!(row["patient_id"], json!(4242));
    // No chain in a test, so her admission slot cannot be learned and the honest verdict is that
    // nothing was asked about her bytes — not a claim that they are missing.
    assert_eq!(row["verdict"], json!("not asked"));
    assert_eq!(body["not_asked"], json!(1));

    // **The payload this test just read is written out for its consumers.**
    //
    // `scripts/bytes-read.sh` on cwf/ops pages this route and sums it, and it was built by hand
    // against the shape as described rather than as served — which is the shape that took the case
    // factory down for twelve hours in September: a hand-built consumer goes on passing while the
    // producer's field moves underneath it. So the answer this test actually received is written
    // where a consumer can assert against it, the same way the globe's card is tested against the
    // board the ward builds rather than a row somebody typed. Rename a field here and the consumer
    // fails loudly instead of quietly becoming fiction.
    let contract = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/contract");
    std::fs::create_dir_all(&contract).expect("somewhere to put the answer a reader is tested on");
    std::fs::write(contract.join("bytes.json"),
                   serde_json::to_string_pretty(&body).expect("the answer serialises"))
        .expect("the reader's fixture is written from the answer this test read");

    // Seeding again is the same ward, not a second copy of it.
    let (code, again) = s.post("/api/ward/seed", &json!({}));
    assert_eq!(code, 200);
    assert_eq!(again["cases_kept"], body_kept(&s), "asking twice keeps the same cases");
}

/// How many cases the seed says it keeps, asked afresh. Small helper so the assertion above reads
/// as the sentence it is rather than as two nested calls.
fn body_kept(s: &Server) -> Value {
    s.post("/api/ward/seed", &json!({})).1["cases_kept"].clone()
}

/// **A receipt derives from the bytes the shift was played on, or says it cannot.**
///
/// This is the fix the 31 pinned cases are waiting for. Today a receipt reads the scenario and the
/// rubric out of the case store as it stands, so correcting a case rewrites what an anchored shift
/// is shown to have been played against — which is why those cases cannot take the diagnosis fix at
/// all. Resolving the played bytes first is what unpins them.
///
/// The recorded address is checked, not trusted. It is a fact written by the only writer that knew,
/// and checking it costs one replay and makes the receipt's claim self-supporting: if the recorded
/// bytes do not reproduce the leaf the chain holds, the record is wrong about this shift and the
/// proof wins. A receipt that believed a bad record would be wrong in exactly the way this whole
/// mechanism exists to prevent, and would carry the authority of having been told.
#[test]
fn a_receipt_derives_from_the_bytes_the_shift_was_played_on_or_says_it_cannot() {
    use vitals_web::ward_case::ForReceipt;

    let s = Server::start();
    let store = vitals_web::store::Store::open(s.state()).expect("the ward's own store");
    let tape: Vec<vitals_replay::Step> = vec![];

    // A case, and the leaf a shift on it produces.
    let pack = a_pack();
    assert_eq!(s.post("/api/ward/case", &pack).0, 200);
    let sce = vitals_web::ward_case::sce_of(&store, "auth-demo-1").expect("its scenario");
    let leaf = leaf_the_receipts_way(&store, &sce, &tape, LONE_SLOT);
    let addr = vitals_web::ward_case::played_id(&pack);
    let shifts = lone(LONE_SLOT);
    let tape_of = |h: &str| vitals_web::ward_chain::tape_by_hash(&store, h);
    let deriving = vitals_web::ward_chain::Deriving {
        shifts: &shifts, this: &shifts[0], tape_of: &tape_of,
        admitted_slot: LONE_SLOT, dated: &ward_dated,
    };

    let file = |recorded: &str| {
        vitals_web::ward_chain::keep_tape(&store, &vitals_web::ward_chain::StoredTape {
            patient_id: 7, run_hash: leaf.clone(), steps: tape.clone(),
            played_id: recorded.to_string(),
        }).expect("tape kept");
    };

    // Nothing recorded — every shift anchored before the hand-over wrote an address. Replay finds
    // the bytes anyway, which is what makes the backlog recoverable.
    file("");
    match vitals_web::ward_case::bytes_for_receipt(&store, &leaf, &tape, &deriving, None, None) {
        ForReceipt::These { sce: got, rubric, played_id } => {
            assert_eq!(played_id, addr, "proved by replay, and named");
            assert_eq!(got, sce, "the scenario the leaf commits to");
            assert!(rubric.contains("items"), "and the rubric kept beside it: {rubric}");
        }
        other => panic!("the bytes are here and reproduce the leaf: {other:?}"),
    }

    // Recorded and correct: the same answer, reached by being told rather than by searching.
    file(&addr);
    assert!(matches!(vitals_web::ward_case::bytes_for_receipt(&store, &leaf, &tape, &deriving, None, None),
                     ForReceipt::These { ref played_id, .. } if *played_id == addr));

    // **Recorded and wrong.** Some other case's bytes, which do not reproduce this leaf. The record
    // loses to the proof rather than the receipt deriving a sheet from bytes nobody played.
    let mut other_case = a_pack();
    other_case["case_id"] = json!("auth-demo-other");
    other_case["sce"]["vitals0"]["hr"] = json!(141.0);
    assert_eq!(s.post("/api/ward/case", &other_case).0, 200);
    let wrong = vitals_web::ward_case::played_id(&other_case);
    assert_ne!(wrong, addr);
    file(&wrong);
    match vitals_web::ward_case::bytes_for_receipt(&store, &leaf, &tape, &deriving, None, None) {
        ForReceipt::These { played_id, .. } => assert_eq!(
            played_id, addr,
            "a record that does not reproduce the leaf is wrong about this shift, and the proof wins"),
        other => panic!("the real bytes are still here: {other:?}"),
    }

    // A rubric corrected under an unchanged scenario. The chart can be rebuilt and the sheet cannot,
    // and those are different claims: the scenario is proved, the rubric is unknowable from a leaf.
    let mut regraded = a_pack();
    regraded["version"] = json!("0.4.0");
    regraded["rubric"]["items"][0]["points"] = json!(3);
    vitals_web::ward_case::keep_played_bytes(&store, &regraded);
    file("");
    match vitals_web::ward_case::bytes_for_receipt(&store, &leaf, &tape, &deriving, None, None) {
        ForReceipt::ScenarioOnly { sce: got, candidates } => {
            assert_eq!(got, sce, "the chart is still rebuildable, because the scenario is proved");
            assert_eq!(candidates.len(), 2, "and both rubrics are named: {candidates:?}");
        }
        other => panic!("two rubrics fit one leaf; a sheet from either would be a guess: {other:?}"),
    }

    // A leaf whose bytes are gone. Not a blank sheet and not today's bytes — a refusal.
    assert_eq!(vitals_web::ward_case::bytes_for_receipt(&store, &"7".repeat(64), &tape, &deriving, None, None),
               ForReceipt::Unrebuildable,
               "the bytes this shift was played on are not here, and the honest answer says so");
}

/// **A case with no blob is still provable from the bytes it carries now.**
///
/// The 106 cases filed before the blob store existed have no kept version, and the ward plays some
/// cases from files that never went through the door at all. For a shift on one of those, the case
/// as it stands either reproduces the leaf the chain holds or it does not — and if it does, those
/// *are* the bytes that shift was played on, demonstrated by exactly the same replay a blob passes.
/// Calling it unrebuildable while its own bytes sat there answering the question would be a refusal
/// dressed as honesty.
///
/// Checked last and never first: a blob is a version somebody deliberately kept, while the live pack
/// is whatever the store holds today and earns its place only by reproducing the leaf. That ordering
/// is the whole difference between this and the `rubric_for` it replaces, which handed every receipt
/// the current rubric and is why cases with anchored shifts had to be pinned.
#[test]
fn a_case_with_no_blob_is_still_provable_from_the_bytes_it_carries_now() {
    use vitals_web::ward_case::ForReceipt;

    let s = Server::start();
    let store = vitals_web::store::Store::open(s.state()).expect("the ward's own store");
    let tape: Vec<vitals_replay::Step> = vec![];

    // Filed straight into the store, the way the 106 were: no blob kept for it.
    let mut pack = a_pack();
    pack["case_id"] = json!("no-blob");
    pack["sce"]["vitals0"]["hr"] = json!(103.0);
    store
        .put(vitals_web::ward_case::CASE_STORE, &vitals_web::ward_case::key_for("no-blob"), &pack)
        .expect("a case filed before the blob store existed");
    assert!(vitals_web::ward_case::played_bytes(&store,
                &vitals_web::ward_case::played_id(&pack)).is_none(), "and no blob for it");

    let sce = vitals_web::ward_case::sce_of(&store, "no-blob").expect("its scenario");
    let leaf = leaf_the_receipts_way(&store, &sce, &tape, LONE_SLOT);
    let shifts = lone(LONE_SLOT);
    let tape_of = |h: &str| vitals_web::ward_chain::tape_by_hash(&store, h);
    let deriving = vitals_web::ward_chain::Deriving {
        shifts: &shifts, this: &shifts[0], tape_of: &tape_of,
        admitted_slot: LONE_SLOT, dated: &ward_dated,
    };
    vitals_web::ward_chain::keep_tape(&store, &vitals_web::ward_chain::StoredTape {
        patient_id: 9, run_hash: leaf.clone(), steps: tape.clone(), played_id: String::new(),
    }).expect("tape kept");

    match vitals_web::ward_case::bytes_for_receipt(&store, &leaf, &tape, &deriving, standing(&store, "no-blob"), None) {
        ForReceipt::These { sce: got, rubric, played_id } => {
            assert_eq!(got, sce, "its own bytes reproduce the leaf, so they are the played bytes");
            assert!(rubric.contains("items"), "with the rubric they are filed beside: {rubric}");
            assert!(played_id.is_empty(),
                    "and no address is claimed, because no kept version was matched — the proof is \
                     the replay and not a blob somebody filed");
        }
        other => panic!("the case's own bytes answer this: {other:?}"),
    }

    // Correct the case, leaving the shift anchored on the older bytes. Now the live pack does not
    // reproduce the leaf, and there is no blob either — which is the honest unrebuildable.
    let mut fixed = pack.clone();
    fixed["sce"]["vitals0"]["hr"] = json!(104.0);
    store
        .put(vitals_web::ward_case::CASE_STORE, &vitals_web::ward_case::key_for("no-blob"), &fixed)
        .expect("a correction, as a pre-blob-store one landed");
    assert_eq!(vitals_web::ward_case::bytes_for_receipt(&store, &leaf, &tape, &deriving, standing(&store, "no-blob"), None),
               ForReceipt::Unrebuildable,
               "the version it was played on is gone, and the current one does not reproduce the \
                leaf, so nothing here can rebuild it");
}

/// **Trying the likely candidates first must not change the answer.**
///
/// Identifying the played scenario is the only part that costs a replay — re-deriving one candidate
/// replays the patient's whole history — so the search tries the address the hand-over recorded, a
/// sibling shift's resolved scenario, and the bytes the case carries now, before walking every kept
/// version. On staging the unordered walk took 198 seconds and held the ward's single instance for
/// all of it.
///
/// Ordering is exactly the kind of optimisation that quietly answers a different question: reach the
/// live bytes before the kept blob and a "proved" becomes a "proved as it stands", which is a
/// different published fact about whether that receipt is pinned. So this fixes the answer against
/// every combination of the shortcuts being available or not. Whatever the search is handed, it must
/// arrive at the same place.
///
/// The speed itself is not asserted here. A synthetic scenario replays far too fast on this machine
/// for a wall-clock bound to tell the ordered search from the unordered one, and a bound loose enough
/// not to be flaky would pass either — so the timing is measured on staging, against a real case
/// store, and a bound invented here would only look like a guarantee.
#[test]
fn trying_the_likely_candidates_first_does_not_change_the_answer() {
    let s = Server::start();
    assert_eq!(s.post("/api/ward/case", &a_pack()).0, 200);
    let store = vitals_web::store::Store::open(s.state()).expect("the ward's own store");
    let tape: Vec<vitals_replay::Step> = vec![];
    let sce = vitals_web::ward_case::sce_of(&store, "auth-demo-1").expect("its scenario");
    let addr = vitals_web::ward_case::played_id(&a_pack());
    let leaf = leaf_the_receipts_way(&store, &sce, &tape, LONE_SLOT);

    // A crowd of other kept versions for the walk to have to get through.
    for n in 0..24 {
        let mut other = a_pack();
        other["case_id"] = json!(format!("crowd-{n}"));
        other["sce"]["vitals0"]["hr"] = json!(60.0 + n as f64);
        vitals_web::ward_case::keep_played_bytes(&store, &other);
    }

    let shifts = lone(LONE_SLOT);
    let tape_of = |h: &str| vitals_web::ward_chain::tape_by_hash(&store, h);
    let deriving = vitals_web::ward_chain::Deriving {
        shifts: &shifts, this: &shifts[0], tape_of: &tape_of,
        admitted_slot: LONE_SLOT, dated: &ward_dated,
    };

    let answer = |recorded: &str, hint: Option<&str>, standing: bool| {
        vitals_web::ward_chain::keep_tape(&store, &vitals_web::ward_chain::StoredTape {
            patient_id: 7, run_hash: leaf.clone(), steps: tape.clone(),
            played_id: recorded.to_string(),
        }).expect("tape kept");
        vitals_web::ward_case::bytes_for_receipt(
            &store, &leaf, &tape, &deriving,
            standing.then(|| standing_of(&store, "auth-demo-1")).flatten(), hint)
    };

    // The walk alone, with nothing to shortcut it, is the reference answer.
    let reference = answer("", None, false);
    match &reference {
        vitals_web::ward_case::ForReceipt::These { played_id, .. } => assert_eq!(played_id, &addr),
        other => panic!("the kept version reproduces this leaf: {other:?}"),
    }

    // Every shortcut, alone and together. `keep_tape` will not let a later empty address erase an
    // earlier one, so the record is set once and the cases that want it absent come first.
    assert_eq!(answer("", Some(&sce), false), reference, "a sibling's scenario, tried first");
    assert_eq!(answer("", None, true), reference, "the live bytes, which must not win over a blob");
    assert_eq!(answer("", Some(&sce), true), reference, "both, and still the kept version");
    assert_eq!(answer(&addr, None, false), reference, "the recorded address");
    assert_eq!(answer(&addr, Some(&sce), true), reference, "everything available at once");

    // A record naming bytes that do not reproduce this leaf must not shortcut past the truth.
    let mut wrong_case = a_pack();
    wrong_case["case_id"] = json!("wrong");
    wrong_case["sce"]["vitals0"]["hr"] = json!(177.0);
    let wrong = vitals_web::ward_case::keep_played_bytes(&store, &wrong_case);
    assert_eq!(answer(&wrong, None, true), reference,
               "a wrong record is checked like any other candidate and loses to the walk");
}

/// The case's scored content as the store holds it now, for the tests that hand it to the proof.
fn standing_of(store: &vitals_web::store::Store, case: &str) -> Option<(String, String)> {
    standing(store, case)
}

/// **The offer route takes the door's secret, names its shift, and refuses without writing.**
///
/// The shapes are pinned because this is what an operator reads five times in a row while repairing
/// five shifts, and a refusal that looks like a success would be repaired-in-the-log and broken on
/// the ward.
#[test]
fn the_offer_route_is_the_operators_and_names_the_shift_it_is_for() {
    let s = Server::start();

    // It writes, so it is the factory's door and not the page's.
    assert_eq!(s.post_with("/api/ward/offer", &a_pack(), None).0, 401,
               "a write with no secret is refused");
    assert_eq!(s.post_with("/api/ward/offer", &a_pack(), Some("not-the-token")).0, 401,
               "and somebody else's secret is not this door's");

    // A leaf is required, and has to look like one.
    let (code, body) = s.post("/api/ward/offer", &a_pack());
    assert_eq!(code, 400, "{body}");
    Server::reads_as_sentences(&body);
    assert!(body["refused"].as_str().unwrap_or_default().contains("?leaf="),
            "it says what is missing and how to give it: {body}");
    assert_eq!(s.post("/api/ward/offer?leaf=nonsense", &a_pack()).0, 400);

    // A leaf this ward has read no shift under is a 404 and not a silent nothing: the operator has
    // the wrong leaf, and a 200 would read as a repair.
    let (code, body) = s.post(&format!("/api/ward/offer?leaf={}", "a".repeat(64)), &a_pack());
    assert_eq!(code, 404, "{body}");
    Server::reads_as_sentences(&body);
    assert!(body["refused"].as_str().unwrap_or_default().contains("no shift"), "{body}");
}

/// **Bytes may be offered for a shift that cannot be rebuilt, and the offer proves itself or is
/// refused.**
///
/// Production carries five anchored shifts whose case bytes were corrected before the blob store
/// existed. Their receipts now refuse, which is honest and not useful: the bytes are gone from the
/// store but not from the world, because the factory is deterministic and recompiling at the commit
/// that produced a pack gives the pack back byte for byte.
///
/// So an offer is the door minus the pointer move — same validation, same content-addressed blob, the
/// live catalogue untouched. Nothing is asserted by offering: the bytes count only if replaying them
/// reproduces the leaf the chain holds, which is the same arithmetic every other receipt rests on. A
/// recompile of the wrong commit is refused by that arithmetic rather than by anybody's judgement.
///
/// Aimed at one leaf rather than at the ward, for two reasons. It costs one derivation instead of one
/// per anchored shift, so it cannot stall the instance; and a pack that reproduces nothing is
/// attributable to the shift it was aimed at instead of being a failure somewhere in a batch.
#[test]
fn bytes_can_be_offered_for_a_shift_and_the_offer_proves_itself_or_is_refused() {
    use vitals_web::ward_case::Offered;

    let s = Server::start();
    let store = vitals_web::store::Store::open(s.state()).expect("the ward's own store");
    let tape: Vec<vitals_replay::Step> = vec![];

    // The case as it was played, and the leaf a shift on it produced — then the correction that
    // replaced it, the way the pre-blob-store ones did. The played bytes are now nowhere.
    let mut played = a_pack();
    played["case_id"] = json!("gone");
    played["sce"]["vitals0"]["hr"] = json!(101.0);
    store.put(vitals_web::ward_case::CASE_STORE,
              &vitals_web::ward_case::key_for("gone"), &played).expect("filed");
    let sce = vitals_web::ward_case::sce_of(&store, "gone").expect("its scenario");
    let leaf = leaf_the_receipts_way(&store, &sce, &tape, LONE_SLOT);
    let mut corrected = played.clone();
    corrected["sce"]["vitals0"]["hr"] = json!(102.0);
    store.put(vitals_web::ward_case::CASE_STORE,
              &vitals_web::ward_case::key_for("gone"), &corrected).expect("corrected");

    let shifts = lone(LONE_SLOT);
    let tape_of = |h: &str| vitals_web::ward_chain::tape_by_hash(&store, h);
    let deriving = vitals_web::ward_chain::Deriving {
        shifts: &shifts, this: &shifts[0], tape_of: &tape_of,
        admitted_slot: LONE_SLOT, dated: &ward_dated,
    };
    // Nothing here rebuilds it: that is the state the five are in.
    assert_eq!(vitals_web::ward_case::bytes_for_receipt(
                   &store, &leaf, &tape, &deriving, standing(&store, "gone"), None),
               vitals_web::ward_case::ForReceipt::Unrebuildable);

    // A recompile of the wrong thing does not reproduce the leaf, and is refused without writing.
    let mut wrong = played.clone();
    wrong["sce"]["vitals0"]["hr"] = json!(133.0);
    let before = vitals_web::ward_case::played_bytes(
        &store, &vitals_web::ward_case::played_id(&wrong)).is_none();
    assert!(before, "the wrong pack is not in the store to begin with");
    assert_eq!(vitals_web::ward_case::offer_bytes(&store, &wrong, &leaf, &tape, &deriving),
               Offered::DoesNotFit,
               "bytes that do not reproduce the leaf are refused by the arithmetic, not by judgement");
    assert!(vitals_web::ward_case::played_bytes(
                &store, &vitals_web::ward_case::played_id(&wrong)).is_none(),
            "and a refused offer writes nothing at all");

    // The right one reproduces the leaf and is kept.
    let addr = vitals_web::ward_case::played_id(&played);
    assert_eq!(vitals_web::ward_case::offer_bytes(&store, &played, &leaf, &tape, &deriving),
               Offered::Proved(addr.clone()));
    assert!(vitals_web::ward_case::played_bytes(&store, &addr).is_some(), "kept under its own bytes");

    // And the receipt rebuilds from them, while the live catalogue is untouched.
    match vitals_web::ward_case::bytes_for_receipt(
              &store, &leaf, &tape, &deriving, standing(&store, "gone"), None) {
        vitals_web::ward_case::ForReceipt::These { played_id, .. } => assert_eq!(played_id, addr),
        other => panic!("the offered bytes reproduce the leaf, so the receipt has them: {other:?}"),
    }
    assert_eq!(vitals_web::ward_case::sce_of(&store, "gone").as_deref(),
               Some(corrected["sce"].to_string().as_str()),
               "the case a new patient would be given is still the corrected one — an offer is not a \
                way to put an old version back on the ward");

    // Offering the same bytes again is not a second blob and does not pretend to be news.
    assert_eq!(vitals_web::ward_case::offer_bytes(&store, &played, &leaf, &tape, &deriving),
               Offered::AlreadyHere(addr.clone()));

    // **The hazard.** A rubric corrected under this scenario would leave two rubrics for one
    // scenario, and every receipt proved through it would publish no score at all. Refused, and the
    // address it collides with is named so somebody can see what they nearly did.
    let mut regraded = played.clone();
    regraded["rubric"]["items"][0]["points"] = json!(11);
    assert_eq!(vitals_web::ward_case::offer_bytes(&store, &regraded, &leaf, &tape, &deriving),
               Offered::WouldUnscore(vec![addr.clone()]),
               "an offer that would take the sheet off a receipt that has one is refused");
    assert!(vitals_web::ward_case::played_bytes(
                &store, &vitals_web::ward_case::played_id(&regraded)).is_none(),
            "and writes nothing");

    // A pack the door itself would refuse is refused here too: an offer is the door minus the
    // pointer move, not a way around the door.
    let mut junk = played.clone();
    junk["difficulty"] = json!("wizard");
    assert_eq!(vitals_web::ward_case::offer_bytes(&store, &junk, &leaf, &tape, &deriving),
               Offered::NotACase,
               "the door's own validation still applies");
}

/// The slot these one-shift fixtures put their shift in, and her admission — the same, so there is
/// no idle time to account for and the fixture stays about which bytes are chosen.
const LONE_SLOT: u64 = 120;

/// The chain's clock, as the ward asks it.
fn ward_dated(slot: u64) -> Option<i64> {
    (slot != 0).then(|| 1_789_000_000 + (slot as i64 * 2) / 5)
}

/// One anchored shift, standing alone.
fn lone(slot: u64) -> Vec<vitals_web::ward::ShiftOnChain> {
    vec![vitals_web::ward::ShiftOnChain {
        patient_id: 7, signer: [1; 32], slot, run_hash: [0; 32],
    }]
}

/// **The leaf a shift produces, derived the way `ward_chain::receipt` derives it** — `resumed` for
/// the state she began on, then the reduction on top.
///
/// Spelled out here rather than taken from `leaf_if_played_on` on purpose: a fixture that asked the
/// verifier for the answer it is about to check would agree with any arithmetic, including the wrong
/// one these tests exist to catch.
fn leaf_the_receipts_way(
    store: &vitals_web::store::Store,
    sce: &str,
    tape: &[vitals_replay::Step],
    slot: u64,
) -> String {
    let tape_of = |h: &str| vitals_web::ward_chain::tape_by_hash(store, h);
    let (mut st, _played) = vitals_web::ward_chain::resumed(
        sce, &[], &tape_of, slot, slot, &ward_dated, false,
    ).expect("she resumes");
    let r = vitals_replay::shift(&mut st, tape, 0.0);
    vitals_web::ward_chain::hex32(&vitals_replay::leaf(&vitals_replay::sce_hash(sce), tape, &r))
}

/// The case's scored content as the store holds it now, the way the ward hands it to the proof.
fn standing(store: &vitals_web::store::Store, case: &str) -> Option<(String, String)> {
    let pack: Value = store.get(vitals_web::ward_case::CASE_STORE,
                                &vitals_web::ward_case::key_for(case))?;
    Some((pack.get("sce")?.to_string(), pack.get("rubric")?.to_string()))
}

/// **The proof must re-derive a leaf the way the receipt does, or it calls good bytes unrebuildable.**
///
/// A shift's leaf commits to its reduction, and `ward_chain::receipt` takes that reduction from the
/// state after every prior shift *and* the idle time between the slots the chain dates. My proof
/// replayed the tape from the initial state instead — a second derivation of the one number the
/// whole mechanism turns on. With correct bytes in the store it still reports unrebuildable for any
/// shift that had anything happen before it, which is most of them.
///
/// Every other fixture in this file is one shift with an empty tape and no gap, which is precisely
/// why the suite was green. This one gives the patient two shifts and real idle gaps — the ordinary
/// shape of a ward patient — and asserts both prove against the bytes they were played on.
#[test]
fn the_proof_re_derives_a_leaf_the_way_the_receipt_does() {
    let s = Server::start();
    assert_eq!(s.post("/api/ward/case", &a_pack()).0, 200);
    let store = vitals_web::store::Store::open(s.state()).expect("the ward's own store");
    let sce = vitals_web::ward_case::sce_of(&store, "auth-demo-1").expect("its scenario");
    let addr = vitals_web::ward_case::played_id(&a_pack());

    // The chain's clock, as the ward asks it: a slot has a block time and idle time comes from it.
    let dated = |slot: u64| -> Option<i64> { (slot != 0).then(|| 1_789_000_000 + (slot as i64 * 2) / 5) };
    let tape_of = |h: &str| vitals_web::ward_chain::tape_by_hash(&store, h);
    let admitted = 50u64;

    // She is admitted at slot 50 and her first shift is at 100, so fifty slots of her illness
    // happened before anybody arrived. Her second is at 300.
    // **The tapes have to advance the clock or the fixture proves nothing.** With empty tapes the
    // reduction is the same whatever state the shift began on, so both leaves come out identical and
    // a verifier replaying from scratch looks correct. The first shift ticks 400 simulated seconds;
    // the second ticks 300 more, which crosses this case's `bled_out` trigger at 600 only because of
    // what happened before it. Replayed from the initial state that shift ends alive, and its leaf is
    // a different number — which is precisely the defect.
    let mut shifts: Vec<vitals_web::ward::ShiftOnChain> = vec![];
    let mut leaves: Vec<String> = vec![];
    for (slot, tick) in [(100u64, 400.0f64), (300u64, 90.0f64)] {
        let steps: Vec<vitals_replay::Step> = vec![
            vitals_replay::Step::Do("fluids".into()), vitals_replay::Step::Tick(tick),
        ];
        let before: Vec<vitals_web::ward::ShiftOnChain> =
            shifts.iter().filter(|x| x.slot < slot).copied().collect();
        // Exactly the call `receipt` makes, including the cap decision, because getting that wrong
        // moves the leaf.
        let (mut st, _played) = vitals_web::ward_chain::resumed(
            &sce, &before, &tape_of, admitted, slot, &dated,
            vitals_web::ward_chain::cap_on_arrival(
                slot, Some(&[1u8; 32]), vitals_web::ward::arrival_cap_from_slot(),
                vitals_web::ward_chain::ward_signer().as_ref()),
        ).expect("she resumes");
        let r = vitals_replay::shift(&mut st, &steps, 0.0);
        let leaf = vitals_web::ward_chain::hex32(
            &vitals_replay::leaf(&vitals_replay::sce_hash(&sce), &steps, &r));
        vitals_web::ward_chain::keep_tape(&store, &vitals_web::ward_chain::StoredTape {
            patient_id: 11, run_hash: leaf.clone(), steps: steps.clone(), played_id: String::new(),
        }).expect("tape kept");
        let mut b = [0u8; 32];
        for (i, c) in b.iter_mut().enumerate() {
            *c = u8::from_str_radix(&leaf[i * 2..i * 2 + 2], 16).expect("hex");
        }
        shifts.push(vitals_web::ward::ShiftOnChain {
            patient_id: 11, signer: [1; 32], slot, run_hash: b,
        });
        leaves.push(leaf);
    }
    assert_ne!(leaves[0], leaves[1], "two shifts on one patient are two leaves");

    // The second shift's leaf is *not* what a replay from the initial state produces. This is the
    // defect stated as a number: a verifier that starts over cannot reach it, however right the
    // bytes are.
    // The defect stated as a number. Her second shift's leaf is not what a replay from the initial
    // state produces: from the state she was actually in she is already recovered, so the order
    // raises no beats, while from scratch the same order raises two. Different reduction, different
    // leaf — and a verifier that starts over cannot reach the one the chain holds, however right the
    // bytes it is holding are.
    let from_scratch = {
        let steps = vec![
            vitals_replay::Step::Do("fluids".into()), vitals_replay::Step::Tick(90.0),
        ];
        let r = vitals_replay::replay(&sce, &steps).expect("it replays");
        vitals_web::ward_chain::hex32(
            &vitals_replay::leaf(&vitals_replay::sce_hash(&sce), &steps, &r))
    };
    assert_ne!(from_scratch, leaves[1],
               "a replay from the initial state reaches a different leaf, which is why the proof \
                must re-derive the way the receipt does");

    // Both were played on the bytes sitting in the store. Both must prove.
    for (n, leaf) in leaves.iter().enumerate() {
        let steps = tape_of(leaf).expect("her tape");
        let deriving = vitals_web::ward_chain::Deriving {
            shifts: &shifts, this: &shifts[n], tape_of: &tape_of,
            admitted_slot: admitted, dated: &dated,
        };
        let got = vitals_web::ward_case::bytes_for_receipt(
            &store, leaf, &steps, &deriving, None, None);
        match got {
            vitals_web::ward_case::ForReceipt::These { sce: ref got_sce, ref played_id, .. } => {
                assert_eq!(played_id, &addr,
                           "shift {} of 2 is addressed to the bytes it was played on", n + 1);
                assert_eq!(got_sce, &sce, "and to that scenario");
            }
            other => panic!("shift {} of 2 was played on the bytes in the store and must prove it: \
                             {other:?}", n + 1),
        }
    }
}
