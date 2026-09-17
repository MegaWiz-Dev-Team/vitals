//! **A review run: the case itself, opened by somebody reading the catalogue.**
//!
//! Eighteen cases are waiting for a clinical advisor and the ward has three beds. Reading them
//! through the board means waiting for a ticker to admit the one you wanted, on a public page,
//! beside patients strangers are treating — and a case that has never been placed cannot be read
//! at all.
//!
//! So a review run opens a held case directly: the compiled scenario, its own content, its voice,
//! its chips and its monitor, with a person invented from the case's own patient block. It is not
//! on the ward and not on the chain. No bed is taken, no lease exists, nothing is anchored, the
//! census does not move and the month's usage does not count it. The page says all of that at the
//! top, and these tests are what make it true underneath.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

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


/// **A review run plays the case and touches nothing.**
#[test]
fn a_review_run_opens_a_case_and_leaves_the_ward_alone() {
    let s = Server::start();
    let mut typhoid = a_pack();
    typhoid["case_id"] = json!("embla-typhoid-review-1");
    typhoid["title"] = json!("Nine days of fever — {sex_word} of {age}");
    typhoid["country"] = json!("BGD");
    typhoid["patient"] = json!({ "age": 64, "sex": "female" });
    typhoid["presentation"] = json!({
        "chief_complaint": "{He_she} has had a fever for nine days",
        "hpi": "A {sex_word} of {age}.",
        "setting": "a district hospital"
    });
    // A case with questions in it, because the tray is half of what a reviewer is reading.
    typhoid["sce"]["interventions"] = json!([
        { "id": "ask_fever_days", "label": "Ask how long the fever", "match": { "any_kw": ["fever"] }, "effects": [] },
        { "id": "tx_fluids", "label": "Crystalloid bolus", "match": { "any_kw": ["fluids"] },
          "effects": [{ "to_state": "stabilising" }] }
    ]);
    assert_eq!(s.post("/api/ward/case", &typhoid).0, 200);

    // The board before, and the board after, are the same board.
    let (_, before) = s.get("/api/ward");

    let (code, run) = s.get("/api/new?review=embla-typhoid-review-1");
    assert_eq!(code, 200, "{run}");
    assert!(run["id"].as_str().is_some_and(|i| !i.is_empty()), "a session to play: {run}");
    assert!(run["view"]["hr"].is_number(), "the monitor is the case's own: {run}");

    let review = &run["review"];
    assert_eq!(review["case"], "embla-typhoid-review-1");
    assert_eq!(review["is_review"], true, "the page has to know, and say so: {review}");
    assert_eq!(review["content"]["title"], "Nine days of fever — woman of 64",
               "the case's own words, filled from the person it is written about");
    assert_eq!(review["content"]["presents"], "She has had a fever for nine days");
    assert_eq!(review["content"]["chips"]["ask"][0]["id"], "ask_fever_days",
               "its own questions, in the case author's words: {review}");
    assert_eq!(review["title"], "Nine days of fever — woman of 64",
               "and no placeholder reaches the page: {review}");
    assert!(review["head"].is_null(), "no head: there is no chain here");
    assert!(review["patient_id"].is_null(), "and no patient: nobody was admitted");

    // Nothing moved on the ward.
    let (_, after) = s.get("/api/ward");
    assert_eq!(before["census"], after["census"], "a review run changed the census");
    assert_eq!(before["patients"], after["patients"], "a review run changed the board");

    // And nothing was counted as a run somebody played.
    let (_, usage) = s.get("/api/usage");
    assert_eq!(usage["runs"]["started"].as_u64().unwrap_or(0), 0,
               "a review run was counted in the month's usage: {usage}");

    // It plays: the engine steps, because the whole point is reading the case as a learner meets it.
    let id = run["id"].as_str().expect("the session");
    let (code, stepped) = s.get(&format!("/api/step?id={id}&tick=5"));
    assert_eq!(code, 200, "{stepped}");
    assert!(stepped["elapsed"].is_number(), "{stepped}");

    // What it can never do is reach the chain.
    for route in ["/api/ward/take", "/api/ward/declare", "/api/ward/anchor"] {
        // A well-formed key that signs nothing: the refusal has to be about what this run is, not
        // about the shape of the key asking.
        let key = "1".repeat(32);
        let (code, body) = s.get(&format!("{route}?id={id}&player={key}"));
        assert_eq!(code, 409, "{route} answered {code}: {body}");
        let why = body["error"].as_str().unwrap_or_default();
        assert!(why.contains("review"),
                "{route} refused without saying what this run is: {why:?}");
    }
}

/// A case nobody sent cannot be reviewed, and the refusal says so rather than opening something.
#[test]
fn a_review_run_of_a_case_the_ward_does_not_hold_is_refused() {
    let s = Server::start();
    let (code, body) = s.get("/api/new?review=embla-never-sent");
    assert_eq!(code, 404, "{body}");
    assert!(body["error"].as_str().is_some_and(|w| w.contains("embla-never-sent")), "{body}");
    assert!(body["view"].is_null(), "and nothing was opened: {body}");
}

/// A withdrawn case is still reviewable: it is in the store for the record, and the record is
/// exactly what a reviewer is reading.
#[test]
fn a_withdrawn_case_can_still_be_reviewed() {
    let s = Server::start();
    let mut pack = a_pack();
    pack["case_id"] = json!("embla-withdrawn-review-1");
    assert_eq!(s.post("/api/ward/case", &pack).0, 200);
    assert_eq!(s.post("/api/ward/case/embla-withdrawn-review-1/withdraw", &json!({})).0, 200);

    let (code, run) = s.get("/api/new?review=embla-withdrawn-review-1");
    assert_eq!(code, 200, "{run}");
    assert_eq!(run["review"]["withdrawn"], true,
               "and the page can say so, because that is what the reviewer is looking at: {run}");
}

/// **The mark sheet of a compiled case comes out of the pack.**
///
/// `/api/marks` reads a rubric off disk, beside the season's scenario files. A compiled case has no
/// file: the compiler sends the rubric inside the pack, through the case door, and the ward keeps
/// it in the store. So every shift on this ward, and every review run, has answered "no mark sheet
/// to open" — which reads as a fact about the case and is a fact about where we looked.
///
/// It is the whole point of a review run: the reviewer is reading what the case *pays for*, and it
/// is what a receipt has to show for a shift somebody played.
#[test]
fn the_mark_sheet_of_a_compiled_case_comes_out_of_the_pack() {
    let s = Server::start();
    let mut pack = a_pack();
    pack["case_id"] = json!("embla-marks-1");
    pack["rubric"]["case"] = json!("embla-marks-1");
    assert_eq!(s.post("/api/ward/case", &pack).0, 200);

    let (_, run) = s.get("/api/new?review=embla-marks-1");
    let id = run["id"].as_str().expect("a session").to_string();

    // Sealed until the case is over, as everywhere else.
    let (_, sealed) = s.get(&format!("/api/marks?id={id}"));
    assert_eq!(sealed["sealed"], true, "{sealed}");

    // Play it to an ending: the fixture's fluids reach the state its win is written from.
    let (_, _) = s.get(&format!("/api/step?id={id}&do=crystalloid%20fluids"));
    let (code, done) = s.get(&format!("/api/finish?id={id}"));
    assert_eq!(code, 200, "{done}");

    let (_, marks) = s.get(&format!("/api/marks?id={id}"));
    assert_eq!(marks["case"], "embla-marks-1", "the rubric's own case line: {marks}");
    assert_eq!(marks["max"], 20, "the pack's two items add to twenty: {marks}");
    assert_eq!(marks["pass_bps"], 6000);
    let items = marks["items"].as_array().expect("the rows a reviewer reads");
    assert_eq!(items.len(), 2, "{marks}");
    assert!(items.iter().any(|i| i["label"] == "Fluids"), "{marks}");
}
