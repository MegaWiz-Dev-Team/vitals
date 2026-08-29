//! NEWS2 is an adult score, and five of the cases on this shelf are children.
//!
//! `osce-b3` is Pim, three years old, mild croup. Her bedside monitor gets it right — `3–5 YR`,
//! `HR 80–140`, no alarm, `MONITORING` — and her banner says `Stable`. The NEWS2 panel on the
//! same screen read **`7 · HIGH RISK · emergency response`**, off vitals (HR 118, RR 28, SpO2 98%
//! on air, BP 98/62) that are entirely normal for three. The adult table charged her 3 for the
//! respiration rate, 2 for the pulse and 2 for the systolic, and the Royal College of Physicians
//! says in the publication itself that the table is not validated under 16.
//!
//! The commit titled *"a three-year-old is not a small adult"* banded the monitor's alarm limits
//! by age and missed this path: the view built
//! `news2::Obs` without an age at all, and `news2` had no way to receive one. The audit named
//! four cases; there are five. `osce-b2` is Tan, fourteen, and the adult table scored him
//! `2 · LOW RISK · routine observations` — which is the same instrument being wrong in the more
//! dangerous direction.
//!
//! What this file asserts is the *whole* of the fix, including the part that is not about
//! numbers: a child gets no score, the panel still exists so nothing on the page reads a missing
//! score as a death, and what stands in its place says which instrument is missing rather than
//! going quiet. No paediatric score is invented here — PEWS is a different instrument and
//! choosing one is a clinical decision, not a rendering one.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

struct Server {
    child: Child,
    port: u16,
    state: PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.state);
    }
}

impl Server {
    fn start() -> Server {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let state = std::env::temp_dir().join(format!("vitals-paeds-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&state);

        let mut child = Command::new(env!("CARGO_BIN_EXE_vitals-web"))
            .env("VITALS_WEB_BIND", "127.0.0.1:0")
            .env("VITALS_STATE_DIR", &state)
            // Seventeen cases opened back to back; the shipped window is six a minute.
            .env("VITALS_TURNS_PER_MIN", "600")
            .env_remove("VITALS_PROGRAM_ID")
            .env_remove("VITALS_TOKEN")
            .env_remove("HEIMDALL_API_KEY")
            .stdout(Stdio::piped())
            .spawn()
            .expect("start vitals-web");
        let out = child.stdout.take().expect("stdout");
        let mut me = Server { child, port: 0, state };
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
        let body = ureq::get(&url)
            .call()
            .map(|r| r.into_string().unwrap_or_default())
            .unwrap_or_else(|e| match e {
                ureq::Error::Status(_, r) => r.into_string().unwrap_or_default(),
                other => panic!("{url}: {other}"),
            });
        serde_json::from_str(&body).unwrap_or(serde_json::Value::Null)
    }

    /// A fresh run of `ep`, one tick in — the first frame a candidate actually sees.
    fn opened(&self, ep: &str) -> serde_json::Value {
        let id = self.json(&format!("/api/new?ep={ep}"))["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{ep}: no session id"))
            .to_string();
        self.json(&format!("/api/step?id={id}&tick=1"))
    }
}

/// Who is a child here. Named rather than derived, so removing a case from the server's table
/// cannot quietly remove it from the test as well.
const CHILDREN: &[(&str, &str)] = &[
    ("ep3", "Khaopun, 5"),
    ("osce-b2", "Tan, 14"),
    ("osce-b3", "Pim, 3"),
    ("osce-c", "Fon, 6"),
    ("osce-d3", "Beam, 6"),
];

const ADULTS: &[&str] = &[
    "ep1", "ep2", "ep4", "ep5", "osce-a", "osce-a2", "osce-b", "osce-c2", "osce-c3", "osce-d",
    "osce-d2", "osce-d4",
];

/// **The report's own case.** Pim's screen no longer carries a high-risk score beside a monitor
/// that says she is fine.
#[test]
fn the_three_year_old_with_croup_is_not_scored() {
    let s = Server::start();
    let v = s.opened("osce-b3");

    // The vitals the panel was scoring — unchanged, because they were never the problem.
    assert_eq!(v["hr"], 118.0);
    assert_eq!(v["rr"], 28.0);
    assert_eq!(v["spo2"], 98.0);
    assert_eq!(v["sbp"], 98.0);
    assert_eq!(v["status"], "Stable");

    let n = &v["news"];
    assert!(!n.is_null(), "a living patient lost her panel entirely — the page reads that as a death");
    assert_eq!(n["applies"], false, "NEWS2 still claims to cover a three-year-old: {n}");
    assert!(n["total"].is_null(), "a score was still sent for a three-year-old: {n}");
    assert!(n["worst"].is_null(), "a worst observation was still sent: {n}");
    assert_eq!(n["band"], "none");

    // And nothing anywhere in the payload says "high risk" about her any more.
    let body = v.to_string();
    for gone in ["high", "emergency response", "urgent review"] {
        assert!(!body.contains(gone), "the panel still says {gone:?} about a well three-year-old: {body}");
    }
}

/// Every child on the shelf, including the fourteen-year-old the original audit missed.
#[test]
fn no_case_under_sixteen_is_given_an_adult_score() {
    let s = Server::start();
    for (ep, who) in CHILDREN {
        let v = s.opened(ep);
        let n = &v["news"];
        assert!(!n.is_null(), "{ep} ({who}): the panel vanished instead of declining to score");
        assert_eq!(n["applies"], false, "{ep} ({who}) was scored on the adult table: {n}");
        assert!(n["total"].is_null(), "{ep} ({who}) still carries a number: {n}");
    }
}

/// The score is not switched off for everybody. Adults still get it, and it is the same score.
#[test]
fn an_adult_still_gets_the_score_a_ward_escalates_on() {
    let s = Server::start();
    for ep in ADULTS {
        let v = s.opened(ep);
        let n = &v["news"];
        assert_eq!(n["applies"], true, "{ep}: an adult lost the score: {n}");
        assert!(n["total"].as_u64().is_some(), "{ep}: an adult has no total: {n}");
        assert!(
            ["low", "medium", "high"].contains(&n["band"].as_str().unwrap_or_default()),
            "{ep}: an adult was banded {}",
            n["band"]
        );
    }
}

/// What replaces the number has to be honest in both directions: it may not claim a score, and it
/// may not read as "nothing to worry about" over a child who might be very sick. A blank does the
/// second thing, which is why the field is a sentence.
#[test]
fn what_replaces_the_score_does_not_reassure() {
    let s = Server::start();
    for (ep, who) in CHILDREN {
        let said = s.opened(ep)["news"]["response"].as_str().unwrap_or_default().to_lowercase();
        assert!(!said.is_empty(), "{ep} ({who}): the panel says nothing at all where a score was");
        assert!(said.contains("not validated"), "{ep} ({who}): {said:?}");
        assert!(said.contains("16"), "{ep} ({who}): {said:?} does not say who it does not cover");
        for soothing in ["routine", "stable", "normal", "low risk", "no concern"] {
            assert!(!said.contains(soothing), "{ep} ({who}): {said:?} reads as reassurance");
        }
    }
}

/// The panel does not become a null on the way to the bell. A null means *dead* — the page reads
/// one and raises the result panel — so a child who is merely unscoreable may never be sent
/// through that branch, tick after tick, for as long as she is alive.
#[test]
fn a_living_child_keeps_her_panel_for_the_whole_run() {
    let s = Server::start();
    let id = s.json("/api/new?ep=osce-b3")["id"].as_str().expect("a session").to_string();
    // The whole visit in one syrup — the steroid is what this station is about, and a treated
    // Pim stays well, which is the run that has to hold.
    s.json(&format!("/api/step?id={id}&do=dexamethasone%20by%20mouth"));

    for _ in 0..40 {
        let v = s.json(&format!("/api/step?id={id}&tick=30"));
        assert_ne!(v["status"], "Dead", "the treated run killed her: {v}");
        assert!(!v["news"].is_null(), "a living child lost her panel mid-run: {v}");
        assert_eq!(v["news"]["applies"], false, "she was scored partway through: {v}");
        assert!(v["news"]["total"].is_null(), "a number appeared mid-run: {v}");
    }
}

/// Death still empties the panel, for a child exactly as for an adult. The age gate is a
/// statement about which patients the *score* covers; it does not touch the older rule that an
/// early warning score has nothing left to warn about.
#[test]
fn death_still_removes_the_panel_for_a_child_too() {
    let s = Server::start();
    let id = s.json("/api/new?ep=osce-b3")["id"].as_str().expect("a session").to_string();
    let mut last = serde_json::Value::Null;
    for _ in 0..60 {
        last = s.json(&format!("/api/step?id={id}&tick=30"));
        if last["status"] == "Dead" {
            break;
        }
    }
    assert_eq!(last["status"], "Dead", "the untreated run never arrested: {last}");
    assert!(last["news"].is_null(), "a dead child was still given a panel: {last}");
}
