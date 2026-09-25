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
