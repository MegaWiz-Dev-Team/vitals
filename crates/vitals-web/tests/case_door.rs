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

    // Age: near the case's own, and never a child on an adult's physiology or the other way.
    assert_eq!(pick(None, &who("THA", 50, "m"), None), Some("ugib-2".into()), "twelve years is near");
    assert_eq!(pick(None, &who("THA", 49, "m"), None), None, "thirteen is not");
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

    let her = |case: &str| Pack {
        case: case.to_string(),
        difficulty: None,
        persona: Persona { name: "Anita Shrestha".into(), country: "NPL".into(), age: 34, sex: "f".into() },
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
