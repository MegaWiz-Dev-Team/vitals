//! Ending a station, over the wire.
//!
//! The bay had no way to say "I have finished". A station ended when the scenario's own triggers
//! ended it, which on `osce-b2` and `osce-c` is never — an audit ran both to sixty simulated
//! minutes with no outcome and the mark sheet still sealed — and on `osce-c2` is never either if
//! the drug goes in and the peak flow is not repeated.
//!
//! `/api/finish` is the control, and the thing to be careful about is what it must not be. A
//! finish that froze the clock and marked the current state would be a cheat code: press it a
//! second before the arrest and `death_cap` never fires. So it does not freeze anything. It
//! stops taking input and runs the encounter on — the patient does not stop existing because
//! the candidate has stopped acting — and the tape it leaves is the tape of a candidate who
//! stood at the bedside and did nothing until the same moment.
//!
//! `vitals_replay::bell` holds that property over every case and every duration, and
//! `vitals_osce`'s `early_finish` holds it over the mark sheet. This file holds it over the
//! wire, where the seal, the leaf and the mark sheet actually live.

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
        let state = std::env::temp_dir().join(format!("vitals-finish-{}-{n}", std::process::id()));
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
    fn finish(&self, id: &str) -> serde_json::Value {
        self.json(&format!("/api/finish?id={id}"))
    }
    fn marks(&self, id: &str) -> serde_json::Value {
        self.json(&format!("/api/marks?id={id}"))
    }
    fn tape(&self, id: &str) -> serde_json::Value {
        self.json(&format!("/api/tape?id={id}"))
    }
    /// Stand at the bedside, two seconds at a time, exactly as the page's own loop does at a
    /// station — and stop when the run is over, exactly as the page's own loop does.
    fn stand_there(&self, id: &str, minutes: f64) -> serde_json::Value {
        let mut v = self.tick(id, 2.0);
        for _ in 1..((minutes * 60.0 / 2.0) as usize) {
            if v["over"] == serde_json::json!(true) {
                break;
            }
            v = self.tick(id, 2.0);
        }
        v
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

/// **Finish with nothing done at all.** The commonest thing a lost candidate does, and the case
/// that used to hang: `osce-c` declares no ending a candidate can reach by standing still.
///
/// Time is called at the ten minutes the card advertises, the run is over with no terminal
/// outcome — which is the truthful answer, because the case never decided anything about her —
/// and the mark sheet opens on a column of misses.
#[test]
fn finishing_with_nothing_done_ends_the_station_and_opens_the_sheet() {
    let s = Server::start();
    let id = s.open("osce-c");
    assert!(s.marks(&id)["sealed"] == serde_json::json!(true), "the sheet was open before play");

    let v = s.finish(&id);
    assert_eq!(v["over"], serde_json::json!(true), "the station did not end: {v}");
    assert!(v["outcome"].is_null(), "a terminal outcome was invented: {v}");
    assert_eq!(v["elapsed"], serde_json::json!(600.0), "time was called somewhere else: {v}");
    assert!(v["leaf"].as_str().is_some_and(|l| l.len() == 64), "no leaf for a finished run: {v}");

    let m = s.marks(&id);
    assert!(m["sealed"].is_null(), "the sheet stayed shut after the bell: {m}");
    assert!(m["max"].as_u64().unwrap_or(0) > 0, "the sheet has no marks on it: {m}");
    assert_eq!(m["cleared"], serde_json::json!(false), "doing nothing cleared the bar: {m}");
    // Every item that pays for an *action* is a miss. The two that are hit are `no_harm`
    // avoidances, which a candidate who touched nothing does satisfy — that is the rubric's own
    // arithmetic and not something the bell decided, and it is a long way below the bar.
    let items = m["items"].as_array().expect("a sheet");
    for i in items.iter().filter(|i| i["kind"] == serde_json::json!("action")) {
        assert_eq!(i["earned"], serde_json::json!(0), "an action was paid for on an empty run: {i}");
    }
    assert!(m["bps"].as_u64().unwrap_or(9999) < 7_000, "an empty run scored a pass: {m}");
}

/// **The clock, ringing on its own.** No button pressed: the candidate simply plays until the
/// advertised time is up. Both stations that had no ending now end, and both open their sheets.
#[test]
fn the_clock_ends_the_two_stations_that_had_no_ending() {
    for ep in ["osce-b2", "osce-c"] {
        let s = Server::start();
        let id = s.open(ep);
        // Twelve minutes of standing there against a ten-minute card. The loop stops itself,
        // because the server stops the run.
        let v = s.stand_there(&id, 12.0);
        assert_eq!(v["over"], serde_json::json!(true), "{ep} never ended: {v}");
        assert_eq!(v["elapsed"], serde_json::json!(600.0), "{ep} ended off the clock: {v}");
        assert_eq!(v["limit"], serde_json::json!(600.0), "{ep} advertises something else: {v}");
        let m = s.marks(&id);
        assert!(m["sealed"].is_null(), "{ep}: the sheet stayed sealed: {m}");
    }
}

/// **The third way a station stranded.** `osce-c2` has an ending, and it is unreachable if the
/// candidate gives the salbutamol and never repeats the peak flow: the case moves to
/// `responding`, the wheeze settles, and `settled_home` waits for a reassessment that is never
/// coming. The patient is comfortable and the run is infinite.
///
/// She is not abandoned there. The clock ends the station at the advertised ten minutes, and
/// what the sheet says is what the candidate did — the neb paid, the steroid and the
/// reassessment did not.
#[test]
fn the_station_that_stranded_on_a_missing_reassessment_ends_too() {
    let s = Server::start();
    let id = s.open("osce-c2");
    s.order(&id, "salbutamol nebuliser");
    let v = s.stand_there(&id, 20.0);
    assert_eq!(v["over"], serde_json::json!(true), "osce-c2 stranded in `responding`: {v}");
    assert!(v["outcome"].is_null(), "an ending was invented for a case that has none here: {v}");
    let m = s.marks(&id);
    assert!(m["sealed"].is_null(), "the sheet stayed sealed: {m}");
    let paid = |label: &str| -> u64 {
        m["items"].as_array().expect("a sheet").iter()
            .find(|i| i["label"].as_str().unwrap_or("").contains(label))
            .map(|i| i["earned"].as_u64().unwrap_or(0)).unwrap_or(0)
    };
    assert!(paid("Salbutamol") > 0, "the neb went in and was not paid for: {m}");
    assert_eq!(paid("Systemic steroid"), 0, "a steroid nobody gave was paid for: {m}");
    assert_eq!(paid("Home with a plan"), 0, "an ending nobody reached was paid for: {m}");
}

/// **Finishing mid-run on a stable patient.** `osce-b2` is the pericarditis: the anti-inflammatory
/// settles him, and from there the case wants two and a half minutes of observation before it
/// will send him home. A candidate who has done the work and pressed finish at ninety seconds
/// gets the ending their management earned, not the one the clock caught them at.
#[test]
fn finishing_on_a_settled_patient_still_plays_the_ending_out() {
    let s = Server::start();
    let id = s.open("osce-b2");
    s.order(&id, "nsaid");
    let mid = s.stand_there(&id, 1.5);
    assert!(mid["outcome"].is_null(), "the case ended before the finish was pressed: {mid}");
    assert_eq!(mid["over"], serde_json::json!(false), "the run was already over: {mid}");

    let v = s.finish(&id);
    assert_eq!(v["outcome"], serde_json::json!("WinDischarge"), "the ending was cut short: {v}");
    assert!(
        v["elapsed"].as_f64().unwrap_or(0.0) < 600.0,
        "a case that reached its own ending was still held to the bell: {v}"
    );
}

/// **Finishing on a patient who is going to arrest.** The cheat this control had to be built
/// around: `osce-a` is anaphylaxis with no adrenaline given, and she arrests at 6:48. A finish
/// pressed at thirty seconds must not save her.
///
/// The comparison is the honest one: the same run, ended at once, against the same run played
/// out by hand. Same outcome, same leaf, same mark sheet.
#[test]
fn finishing_before_the_arrest_does_not_dodge_it() {
    let played = {
        let s = Server::start();
        let id = s.open("osce-a");
        s.order(&id, "any allergies?");
        let v = s.stand_there(&id, 12.0);
        (v["outcome"].clone(), v["leaf"].clone(), s.marks(&id)["score"].clone(), v["elapsed"].clone())
    };
    assert_eq!(played.0, serde_json::json!("DeathArrest"), "the worked example stopped arresting");

    let s = Server::start();
    let id = s.open("osce-a");
    s.order(&id, "any allergies?");
    s.tick(&id, 2.0);
    let v = s.finish(&id);
    assert_eq!(v["outcome"], played.0, "an early finish changed the ending: {v}");
    assert_eq!(v["elapsed"], played.3, "an early finish changed when it ended: {v}");
    assert_eq!(v["leaf"], played.1, "an early finish is a different run: {v}");
    assert_eq!(s.marks(&id)["score"], played.2, "an early finish is marked differently");
}

/// **Nothing lands on a run the bell has ended.** A browser left open used to go on ticking,
/// and every tick was pushed onto the tape whether the engine did anything with it or not —
/// so the leaf of a finished run drifted with how long somebody left the tab up.
#[test]
fn a_finished_run_takes_nothing_more_onto_its_tape() {
    let s = Server::start();
    let id = s.open("osce-c");
    let v = s.finish(&id);
    let leaf = v["leaf"].clone();
    let before = s.tape(&id)["tape"].as_array().map(Vec::len);

    for _ in 0..20 {
        s.tick(&id, 30.0);
    }
    s.order(&id, "adrenaline 0.5 mg IM");
    let after = s.tape(&id);
    assert_eq!(after["tape"].as_array().map(Vec::len), before, "the tape grew after the bell");
    assert_eq!(s.tick(&id, 2.0)["leaf"], leaf, "the leaf moved after the bell");
}

/// **Practice gets the same courtesy.** An episode is a lesson rather than an exam, and a
/// learner who has finished with a patient should be able to say so and read the debrief —
/// which is the same debrief, computed the same way. What practice does *not* get is a
/// different physiology: EP1's patient arrests on her own timetable here exactly as she does
/// at a station.
#[test]
fn an_episode_can_be_finished_too() {
    let s = Server::start();
    let id = s.open("ep1");
    assert_eq!(s.json(&format!("/api/debrief?id={id}"))["sealed"], serde_json::json!(true));
    let v = s.finish(&id);
    assert_eq!(v["over"], serde_json::json!(true), "an episode cannot be finished: {v}");
    assert_eq!(v["outcome"], serde_json::json!("DeathArrest"), "the ending was skipped: {v}");
    let d = s.json(&format!("/api/debrief?id={id}"));
    assert!(d["sealed"].is_null(), "the debrief stayed shut: {d}");
}

/// **The leaf a run of this shape has always produced.**
///
/// The one thing this change was not allowed to do is move a hash. `sce_hash`, the tape encoding
/// and `leaf()` are all untouched, and the pinned pair below is what the build before any of this
/// existed produced for the same scripted run — captured by playing it against `HEAD` and
/// against this working tree and comparing the two.
///
/// A run that reaches its own ending never meets the bell at all, which is why this test plays
/// one that does: the ending is the scenario's, the tape is the same tape, and the leaf is the
/// same 64 characters.
#[test]
fn a_run_that_ends_on_its_own_hashes_exactly_as_it_always_did() {
    let s = Server::start();
    let id = s.open("osce-a");
    for order in ["any allergies?", "adrenaline 0.5 mg IM", "oxygen 15 L non-rebreather",
                  "lay her flat and lift her legs", "IV fluids 500 ml"] {
        s.order(&id, order);
        s.tick(&id, 10.0);
    }
    let v = s.stand_there(&id, 12.0);
    assert_eq!(v["outcome"], serde_json::json!("WinDischarge"), "the scripted run changed: {v}");
    assert_eq!(v["sce_hash"], serde_json::json!(SCE_HASH), "osce-a's identity moved");
    assert_eq!(v["leaf"], serde_json::json!(LEAF), "the leaf of an unchanged run moved");
}

/// `osce-a`'s scenario hash — the case's identity on chain, and untouched by anything here.
const SCE_HASH: &str = "ac52be1cda7ea6199664b25759217dcb8a04a7ac65adaeaca572ccf202828798";
/// The leaf of the scripted run above.
const LEAF: &str = "a9a0e0f4021dd2f22c2123effc21c2325ac107ab46ee827228b7b9a03f67dd6a";
