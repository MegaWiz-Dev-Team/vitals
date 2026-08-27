//! What a candidate is allowed to know while the clock is running.
//!
//! Every fix in this file answers the same audit finding: the star is only a measurement if the
//! candidate could not have read the answer off the wire. The page is not the seal — a Network
//! tab is one keystroke away, and a `curl` before the exam starts is not even that — so each
//! property here is asserted against the JSON the server actually serves.
//!
//! The seal is always *until the bell*, never forever: the mark sheet and the debrief are the
//! whole point of an unlimited-retry model, and they get the full text. What is refused is
//! knowing it while it can still be used.

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
        let state = std::env::temp_dir().join(format!("vitals-integrity-{}-{n}", std::process::id()));
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

/// Every bank id the station set declares, read off the same table the server serves from —
/// so a member added later is covered without anyone remembering to add it here.
fn bank_ids() -> Vec<String> {
    let src = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/main.rs"),
    )
    .expect("main.rs");
    let mut out = Vec::new();
    for line in src.lines() {
        let Some(rest) = line.trim().strip_prefix("SetMember { id: ") else { continue };
        let Some(c) = rest.split("case: \"").nth(1) else { continue };
        if let Some(id) = c.split('"').next() {
            out.push(id.to_string());
        }
    }
    assert!(out.len() >= 12, "the set table stopped parsing: {out:?}");
    out
}

/// **The worst leak the audit found.** `/api/chain` is unauthenticated, needs no session, and is
/// the first call the lobby makes. It used to carry `case` and `specialty` for all twelve
/// members: `ddx-anaphylaxis-1` names the diagnosis in the id, and `eir-gastroenterology` names
/// the organ the mark sheet is asking about. One GET, before sitting anything, answered every
/// station on the shelf — which undid the stem rewrite, the band chip and the nudge seal in a
/// single request.
#[test]
fn the_shelf_endpoint_names_no_disease_and_no_organ() {
    let s = Server::start();
    let chain = s.json("/api/chain");
    let body = chain.to_string();

    for bank in bank_ids() {
        assert!(
            !body.contains(&bank),
            "/api/chain still hands out the bank id {bank}, which spells the diagnosis: {body}"
        );
    }
    // The Eir specialty names the organ. The circuit band ("emergency", "paediatrics") is what
    // a real circuit puts on the door and is the only field the shelf may wear.
    assert!(!body.contains("eir-"), "/api/chain still hands out the specialty: {body}");

    // And the fields themselves are gone rather than merely emptied, so nothing can refill them.
    for st in chain["sets"].as_array().expect("sets") {
        for m in st["members"].as_array().expect("members") {
            assert!(m["case"].is_null(), "a member still carries `case`: {m}");
            assert!(m["specialty"].is_null(), "a member still carries `specialty`: {m}");
            assert!(m["band"].is_string(), "a member lost the band the shelf draws from: {m}");
            assert!(m["title"].is_string(), "a member lost its stem: {m}");
        }
    }
}

/// Where they went. Provenance is worth printing — it is what lets a reader check the case came
/// from a bank and not from us — but only once it can no longer be used, which is exactly the
/// gate `/api/marks` already holds: an outcome, or nothing.
#[test]
fn provenance_arrives_with_the_mark_sheet_and_not_before() {
    let s = Server::start();
    let id = s.open("osce-a");

    let early = s.json(&format!("/api/marks?id={id}"));
    assert_eq!(early["sealed"], serde_json::json!(true), "the sheet opened early: {early}");
    assert!(early["bank_case"].is_null(), "provenance leaked through the seal: {early}");
    assert!(early["specialty"].is_null(), "the specialty leaked through the seal: {early}");

    s.order(&id, "adrenaline im");
    s.play_out(&id);

    let after = s.json(&format!("/api/marks?id={id}"));
    assert_eq!(
        after["bank_case"].as_str(),
        Some("ddx-anaphylaxis-1"),
        "the debrief lost the provenance line: {after}"
    );
    assert_eq!(
        after["specialty"].as_str(),
        Some("eir-emergency"),
        "the debrief lost the specialty: {after}"
    );

    // An episode is not converted from anything, so it has no line and does not invent one.
    let ep = s.open("ep1");
    s.order(&ep, "let her stand up");
    s.play_out(&ep);
    let m = s.json(&format!("/api/marks?id={ep}"));
    assert!(m["bank_case"].is_null(), "an episode invented a provenance line: {m}");
}

/// The page reads it from the sealed answer now, not from the shelf table — one call site, and
/// it is inside the mark-sheet fetch.
#[test]
fn the_page_prints_provenance_from_the_sealed_sheet() {
    let page = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static/index.html"),
    )
    .expect("the page");
    assert!(
        page.contains("function showProvenance(m)"),
        "showProvenance no longer takes the sealed payload"
    );
    assert!(
        page.contains("m&&m.bank_case"),
        "showProvenance is not reading the bank id off the mark sheet"
    );
    assert!(
        !page.contains("info.m.case"),
        "the page still reads the bank id off the unauthenticated shelf table"
    );
    assert!(
        !page.contains("BANDFOR"),
        "the page still keeps a specialty→band table for a field the server no longer sends"
    );
}

/// The other end of the same leak: whatever a station file says about itself in prose, none of
/// it may reach the wire. The scenario's `_note` carries the bank id as repo provenance and the
/// scenario is never served — only its sha256 is — so this pins the payloads a running case
/// actually produces rather than the file on disk, which cannot be edited without revaluing
/// every leaf already anchored against it.
#[test]
fn a_live_run_never_puts_the_bank_id_on_the_wire() {
    let s = Server::start();
    let banks = bank_ids();
    for ep in ["osce-a", "osce-c", "osce-d4"] {
        let id = s.open(ep);
        let mut seen = s.json(&format!("/api/new?ep={ep}")).to_string();
        seen.push_str(&s.order(&id, "oxygen").to_string());
        seen.push_str(&s.tick(&id, 30.0).to_string());
        seen.push_str(&s.json(&format!("/api/tape?id={id}")).to_string());
        seen.push_str(&s.json(&format!("/api/kit?ep={ep}")).to_string());
        for bank in &banks {
            assert!(!seen.contains(bank.as_str()), "{ep}: a live payload names the bank id {bank}");
        }
        assert!(!seen.contains("eir-"), "{ep}: a live payload names the specialty");
    }
}
