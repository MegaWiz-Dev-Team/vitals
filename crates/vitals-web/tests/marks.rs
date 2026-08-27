//! The mark sheet: after the bell, from the same tape, adding up to the same number.
//!
//! A station that fails used to end with "Death · arrest" and a hash. The player was told the
//! verdict and never the reasoning, which turns an unlimited-retry model into unlimited guessing
//! — so `/api/marks` opens the rubric item by item once the case is over.
//!
//! Three properties are worth a test, and all three are the kind that break quietly:
//!
//!   * **The seal.** The sheet names every action the rubric pays for, with its window. Served
//!     one second early it is the answer key. The page refuses to ask; this refuses to answer.
//!   * **The arithmetic.** The sheet is `sheet_for_run`, which is `det_for_run`'s own body — so
//!     what the debrief shows and what the chain carries have to be the same walk over the same
//!     tape. The test re-derives the det score off the served tape and demands they match.
//!   * **The ordering.** Worst first, or the sheet is a list rather than a lesson.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use vitals_replay::Step;

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
        let state = std::env::temp_dir().join(format!("vitals-marks-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&state);

        let mut child = Command::new(env!("CARGO_BIN_EXE_vitals-web"))
            .env("VITALS_WEB_BIND", "127.0.0.1:0")
            .env("VITALS_STATE_DIR", &state)
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

    fn json(&self, path: &str) -> serde_json::Value {
        let url = format!("http://127.0.0.1:{}{path}", self.port);
        let body = ureq::get(&url).call().map(|r| r.into_string().unwrap_or_default()).unwrap_or_else(
            |e| match e {
                ureq::Error::Status(_, r) => r.into_string().unwrap_or_default(),
                other => panic!("{url}: {other}"),
            },
        );
        serde_json::from_str(&body).unwrap_or(serde_json::Value::Null)
    }

    fn open(&self, ep: &str) -> String {
        self.json(&format!("/api/new?ep={ep}")).get("id").and_then(|v| v.as_str())
            .expect("a session id").to_string()
    }

    fn order(&self, id: &str, text: &str) -> serde_json::Value {
        self.json(&format!("/api/step?id={id}&do={}", enc(text)))
    }

    fn tick(&self, id: &str, dt: f64) -> serde_json::Value {
        self.json(&format!("/api/step?id={id}&tick={dt}"))
    }

    /// Tick until the automaton reaches a terminal state, so a test never asserts against a run
    /// that merely ran out of patience.
    fn play_out(&self, id: &str) -> serde_json::Value {
        let mut v = serde_json::Value::Null;
        for _ in 0..40 {
            v = self.tick(id, 30.0);
            if !v["outcome"].is_null() {
                return v;
            }
        }
        panic!("the run never ended: {v}");
    }
}

fn enc(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => c.to_string(),
            other => format!("%{:02X}", other as u32),
        })
        .collect()
}

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/");

/// The tape as `/api/tape` publishes it, back into the shape the scorer takes. This is the whole
/// point of the endpoint's shape: a player hands the JSON to a stranger and the leaf — and now
/// the mark sheet — is re-derived off this machine entirely.
fn steps_from(tape: &serde_json::Value) -> Vec<Step> {
    tape.as_array()
        .expect("a tape")
        .iter()
        .map(|s| {
            if let Some(t) = s.get("tick").and_then(|v| v.as_f64()) {
                Step::Tick(t)
            } else if let Some(a) = s.get("act").and_then(|v| v.as_str()) {
                Step::Act { text: s["do"].as_str().unwrap_or_default().into(), id: a.into() }
            } else if let Some(d) = s.get("do").and_then(|v| v.as_str()) {
                Step::Do(d.into())
            } else if let Some(q) = s.get("ask").and_then(|v| v.as_str()) {
                Step::Ask(q.into())
            } else if let Some(d) = s.get("set").and_then(|v| v.as_str()) {
                Step::Set(d.into(), s["to"].as_f64().unwrap_or_default())
            } else {
                Step::Off(s["off"].as_str().unwrap_or_default().into())
            }
        })
        .collect()
}

/// The competent station-A tape, in the chip texts the page actually sends.
fn competent(s: &Server, id: &str) {
    for (order, dt) in [
        ("any allergies?", 15.0),
        ("what did you eat before this?", 15.0),
        ("adrenaline im", 10.0),
        ("oxygen mask", 10.0),
        ("serum tryptase", 5.0),
        ("12-lead ecg", 5.0),
        ("anaphylaxis", 5.0),
    ] {
        s.order(id, order);
        s.tick(id, dt);
    }
}

/// The antihistamine-and-wait tape: the window shuts, she arrests, and the run drops the ten
/// points the station exists to teach.
fn hesitation(s: &Server, id: &str) {
    s.order(id, "chlorpheniramine");
}

/// The one that matters. A sheet served mid-run is the answer key read off the wall.
#[test]
fn the_mark_sheet_stays_shut_until_the_case_is_over() {
    let s = Server::start();
    let id = s.open("osce-a");

    // Nothing done yet.
    let early = s.json(&format!("/api/marks?id={id}"));
    assert_eq!(early["sealed"], serde_json::json!(true), "the sheet opened before play: {early}");
    assert!(early["items"].is_null(), "an item leaked through the seal: {early}");

    // Half a case in, with the drug already given — the moment a leak would be most useful.
    competent(&s, &id);
    let mid = s.json(&format!("/api/marks?id={id}"));
    assert_eq!(mid["sealed"], serde_json::json!(true), "the sheet opened mid-run: {mid}");
    assert!(mid["items"].is_null(), "an item leaked mid-run: {mid}");
    assert!(mid["score"].is_null() && mid["max"].is_null(), "the score leaked mid-run: {mid}");

    // And the moment the bell rings, it opens.
    s.play_out(&id);
    let after = s.json(&format!("/api/marks?id={id}"));
    assert!(after["sealed"].is_null(), "the sheet stayed shut after the outcome: {after}");
    assert!(after["items"].as_array().is_some_and(|a| !a.is_empty()), "no sheet: {after}");
}

/// The page must not be able to ask for one either — the seal is cheap to keep on both sides,
/// and the file anyone can read is the one that gets edited.
#[test]
fn the_page_only_asks_for_the_sheet_once_the_run_is_over() {
    let page = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static/index.html"),
    )
    .expect("the page");
    // Exactly one call site, and it is inside showMarks(). (Prose mentions of the endpoint are
    // not call sites, so the count is of the fetch itself.)
    assert_eq!(
        page.matches("fetch('/api/marks").count(),
        1,
        "the mark sheet is fetched from more than one place — every one of them is a seal to keep"
    );
    let at = page.find("async function showMarks").expect("showMarks is gone");
    let body = &page[at..at + 900];
    assert!(body.contains("/api/marks"), "showMarks no longer fetches the sheet");
    assert!(body.contains("if(!id||!over) return;"), "showMarks lost its guard — it would render mid-run");
    // And it is raised from the bell, not from paint().
    assert!(page.contains("showMarks();"), "showMarks is not raised by finish()");
    assert!(
        page.contains("showDebrief(); showProvenance(null); showMarks();"),
        "the bell no longer clears provenance before re-filling it from the sealed sheet"
    );
}

/// The sheet and the star are one walk over one tape, and the test proves it the hard way:
/// take the tape the server publishes, re-derive the det score off the disk files, and demand
/// the served sheet re-add to exactly that.
#[test]
fn the_sheet_adds_up_to_the_det_score_the_chain_would_carry() {
    let s = Server::start();
    let sce = std::fs::read_to_string(format!("{ROOT}stations/osce-a.sce.json")).expect("scenario");
    let rubric = std::fs::read_to_string(format!("{ROOT}rubrics/osce-a.json")).expect("rubric");

    for (name, drive) in [
        ("competent", competent as fn(&Server, &str)),
        ("hesitation", hesitation as fn(&Server, &str)),
    ] {
        let id = s.open("osce-a");
        drive(&s, &id);
        let view = s.play_out(&id);
        let outcome = view["outcome"].as_str().unwrap_or_default().to_string();

        let tape = s.json(&format!("/api/tape?id={id}"));
        let steps = steps_from(&tape["tape"]);
        let (det, max, _) = vitals_osce::det_for_run(&sce, &steps, &rubric).expect("det");

        let m = s.json(&format!("/api/marks?id={id}"));
        assert_eq!(m["score"].as_u64(), Some(det as u64), "{name}: served score ≠ det ({m})");
        assert_eq!(m["max"].as_u64(), Some(max as u64), "{name}: served max ≠ det max ({m})");

        let items = m["items"].as_array().expect("items");
        let earned: u64 = items.iter().map(|i| i["earned"].as_u64().unwrap_or_default()).sum();
        let points: u64 = items.iter().map(|i| i["points"].as_u64().unwrap_or_default()).sum();
        assert_eq!(earned, det as u64, "{name}: the items do not add up to the score ({m})");
        assert_eq!(points, max as u64, "{name}: the item maxima do not add up to the max ({m})");
        assert_eq!(items.len(), 10, "{name}: station A has ten rubric items");

        // Worst first — the top of the sheet is what to practise.
        let lost: Vec<u64> = items.iter().map(|i| i["lost"].as_u64().unwrap_or_default()).collect();
        assert!(lost.windows(2).all(|w| w[0] >= w[1]), "{name}: the sheet is not worst-first: {lost:?}");

        // And every zero says which kind of zero it is.
        for it in items {
            let mark = it["mark"].as_str().unwrap_or_default();
            let earned = it["earned"].as_u64().unwrap_or_default();
            assert_eq!(
                mark == "hit",
                earned > 0 || it["points"].as_u64() == Some(0),
                "{name}: {it} marks and points disagree"
            );
        }

        match name {
            "competent" => {
                assert!(outcome.starts_with("Win"), "the competent tape did not win: {outcome}");
                assert_eq!(det, max, "the competent tape is the full forty");
                assert_eq!(m["cleared"], serde_json::json!(true));
            }
            _ => {
                assert_eq!(m["cleared"], serde_json::json!(false), "hesitation must not clear");
                // The station's whole lesson, at the top of the sheet with its window quoted.
                let top = &items[0];
                assert!(
                    top["label"].as_str().unwrap_or_default().contains("Adrenaline"),
                    "the ten-point hole is not the first thing the player reads: {top}"
                );
                assert_eq!(top["mark"], serde_json::json!("miss"));
                assert_eq!(top["within"].as_f64(), Some(300.0), "the window is not quoted: {top}");
                assert!(top["at"].is_null(), "it was never given, so there is no time to quote");
            }
        }
    }
}

/// Practice is where the feedback is worth most, so it gets the same sheet. Exam mode changes
/// nothing about the debrief — only about what is shown while the clock is running.
#[test]
fn a_practice_run_gets_the_same_sheet_as_an_exam() {
    let s = Server::start();
    let id = s.open("osce-a");
    competent(&s, &id);
    s.play_out(&id);
    let m = s.json(&format!("/api/marks?id={id}"));
    // Never committed, so the server never bound this run as an exam.
    assert_eq!(m["exam"], serde_json::json!(false), "this run was not declared an exam: {m}");
    assert_eq!(m["items"].as_array().map(|a| a.len()), Some(10), "practice was refused a sheet: {m}");
    assert_eq!(m["score"], m["max"], "the competent tape is full marks in practice too: {m}");
}

/// A case with no rubric has no mark sheet, and that is a fact rather than an error — the page
/// prints nothing and the episode plays exactly as before.
#[test]
fn a_case_without_a_rubric_simply_has_no_sheet() {
    let s = Server::start();
    let id = s.open("ep1");
    s.order(&id, "let her stand up");
    s.play_out(&id);
    let m = s.json(&format!("/api/marks?id={id}"));
    assert!(m["error"].is_null(), "a rubric-less case must not error: {m}");
    assert_eq!(m["items"].as_array().map(|a| a.len()), Some(0), "{m}");
}

/// A stranger holding a live session id must not be handed its mark sheet either — every route
/// that reaches into a session asks the same question.
#[test]
fn a_stranger_cannot_read_someone_elses_sheet() {
    let s = Server::start();
    const A: &str = "AStZxZ8XgH9nSKarLT4MzUrY8HM5LtExzDaN9SDoaKiq";
    const B: &str = "3yHJZLgfBs2KWiRnJx65YejtFtkbSdJCezJEYGMcyYMZ";
    let id = s.json(&format!("/api/new?ep=osce-a&player={A}"))["id"].as_str().expect("id").to_string();
    let r = s.json(&format!("/api/marks?id={id}&player={B}"));
    assert_eq!(r["error"], serde_json::json!("no such session"), "a stranger read the sheet: {r}");
}
