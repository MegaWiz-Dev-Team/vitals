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
    assert!(review["content"]["chips"]["ask"].is_array(), "its own questions: {review}");
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
        let key = "1".repeat(44);
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
