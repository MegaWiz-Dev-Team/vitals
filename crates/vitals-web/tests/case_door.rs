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

/// **Which case the next patient gets.**
///
/// Her own pack names one when the patient factory knows which it wants. Until it does, the ward
/// picks from what it holds: a case for her country if there is one — a Nepali woman on a Nepali
/// case is the whole point of the endemic work — and otherwise any case at her level, newest
/// first so a recompile is what the next patient plays.
#[test]
fn a_patient_is_given_a_case_from_the_wards_own_catalogue() {
    use vitals_web::ward_case::{choose_case, CaseSummary};

    let held = |id: &str, country: Option<&str>, level: &str, version: &str| CaseSummary {
        case_id: id.into(),
        country: country.map(str::to_string),
        difficulty: level.into(),
        endemic: country.is_some(),
        provisional: true,
        version: version.into(),
        title: id.into(),
    };
    let cases = vec![
        held("dengue-npl-1", Some("NPL"), "intern", "0.1.0"),
        held("ugib-1", None, "resident", "0.1.0"),
        held("ugib-2", None, "resident", "0.2.0"),
    ];

    assert_eq!(choose_case(&cases, Some("dengue-npl-1"), "NPL", Some("intern")).map(|c| c.case_id.clone()),
               Some("dengue-npl-1".into()), "the pack's own choice is honoured first");
    assert_eq!(choose_case(&cases, Some("not-here"), "NPL", Some("intern")).map(|c| c.case_id.clone()),
               Some("dengue-npl-1".into()),
               "a case the ward does not hold is not a reason to admit nobody");
    assert_eq!(choose_case(&cases, None, "NPL", Some("intern")).map(|c| c.case_id.clone()),
               Some("dengue-npl-1".into()), "her country's case, when there is one");
    assert_eq!(choose_case(&cases, None, "THA", Some("resident")).map(|c| c.case_id.clone()),
               Some("ugib-2".into()), "otherwise her level, newest version first");
    assert!(choose_case(&cases, None, "THA", Some("student")).is_none(),
            "and nothing at her level means nobody is admitted, rather than somebody admitted \
             onto a case written for a different learner");
    assert!(choose_case(&[], None, "THA", Some("resident")).is_none(), "an empty catalogue admits nobody");

    // No level asked for, which is where the ward is today: the patient pack does not carry one,
    // so the ticker asks for her country and takes the newest of whatever there is.
    assert_eq!(choose_case(&cases, None, "NPL", None).map(|c| c.case_id.clone()),
               Some("dengue-npl-1".into()));
    assert_eq!(choose_case(&cases, None, "THA", None).map(|c| c.case_id.clone()),
               Some("ugib-2".into()), "newest version, whatever level it was written for");
}
