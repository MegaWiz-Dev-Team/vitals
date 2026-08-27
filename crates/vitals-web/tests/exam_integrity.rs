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

/// **The seal that was only CSS.** `view()` never read `exam_mode`, so the harm sentence went
/// out on every tick in three fields at once and the page greyed one copy of it out. Station C's
/// whole lesson is *do not put the depressor in*; the run that put it in was being told, in
/// text, in the tab beside the game, exactly what putting it in did.
#[test]
fn a_station_withholds_the_harm_sentence_until_the_bell() {
    const FULL: &str = "the tongue depressor goes in";
    let s = Server::start();
    let id = s.open("osce-c");

    s.order(&id, "look in the throat");
    let v = s.tick(&id, 10.0);
    let body = v.to_string();

    assert!(!body.contains(FULL), "the harm sentence is still on the wire mid-run: {body}");
    assert_eq!(v["harm"].as_array().map(Vec::len), Some(0), "the harm list is not sealed: {v}");
    let beats: Vec<&str> = v["beats"].as_array().expect("beats").iter()
        .filter_map(|b| b.as_str()).filter(|b| b.starts_with("harm")).collect();
    assert_eq!(beats, vec!["harm:sealed"], "a harm beat says more than the token: {beats:?}");
    let chart: Vec<&str> = v["chart"].as_array().expect("chart").iter()
        .filter(|c| c["kind"] == "harm").filter_map(|c| c["text"].as_str()).collect();
    assert_eq!(chart, vec!["harm:sealed"], "the chart still prints the harm sentence: {chart:?}");
    // The line itself is kept — that something went wrong, at this second, is on the monitor
    // anyway. Withholding the fact as well would be lying to the candidate rather than sealing.
    assert_eq!(v["chart"].as_array().expect("chart").iter().filter(|c| c["kind"] == "harm").count(), 1);

    // And the bell hands all of it back, in every field, because that is what the debrief is.
    let end = s.play_out(&id);
    let after = end.to_string();
    assert!(after.contains(FULL), "the debrief never got the harm sentence back: {after}");
    assert!(
        end["harm"].as_array().is_some_and(|a| a.iter().any(|h| h.as_str().is_some_and(|h| h.contains(FULL)))),
        "the harm list stayed sealed after the outcome: {end}"
    );
    assert!(
        end["beats"].as_array().is_some_and(|a| a.iter().any(|b| b.as_str().is_some_and(|b| b.contains(FULL)))),
        "the beats stayed sealed after the outcome: {end}"
    );
    assert!(
        end["chart"].as_array().is_some_and(|a| a.iter().any(|c| c["text"].as_str().is_some_and(|t| t.contains(FULL)))),
        "the chart stayed sealed after the outcome: {end}"
    );
}

/// The seal must not travel by translation either. `tr` is the beat table read in the language
/// the page asked for, and a translated harm line is the harm line.
#[test]
fn the_seal_holds_in_every_language() {
    let s = Server::start();
    let id = s.open("osce-c");
    s.order(&id, "look in the throat");
    let v = s.json(&format!("/api/step?id={id}&tick=10&lang=th"));
    let body = v.to_string();
    assert!(!body.contains("the tongue depressor"), "the English leaked under a Thai run: {body}");
    for key in v["tr"].as_object().map(|m| m.keys().cloned().collect::<Vec<_>>()).unwrap_or_default() {
        assert!(!key.starts_with("harm:") || key == "harm:sealed", "a harm beat was translated: {key}");
    }
}

/// An episode is drama, not an exam. Nothing is withheld there, and the seal must not have
/// wandered into the season on its way to the circuit.
#[test]
fn an_episode_is_never_sealed() {
    let s = Server::start();
    let id = s.open("ep1");
    s.order(&id, "let her stand up");
    let v = s.tick(&id, 10.0);
    assert!(
        v["harm"].as_array().is_some_and(|a| !a.is_empty()),
        "an episode lost its harm feedback mid-run: {v}"
    );
}

/// **The rubric's needles, printed on the chart.** The engine records an order by intervention
/// id because that is what replay and the scorer need, and the chart printed the id straight:
/// `adrenaline_undosed`, `dx_epiglottitis`, `exam_throat`. Those are the mark sheet's own
/// needles — they name what is being marked, and `_undosed` and `dx_` name the shape of the
/// mistake it is waiting for. The id stays on the tape; it may not reach a screen.
#[test]
fn the_chart_prints_what_was_ordered_and_never_the_rubric_needle() {
    let s = Server::start();
    // osce-d3 is where this is worst: the paediatric anaphylaxis station marks the *dose*, and
    // the id says so out loud.
    let id = s.open("osce-d3");
    for order in ["adrenaline", "oxygen", "look in the throat", "anaphylaxis"] {
        s.order(&id, order);
        s.tick(&id, 10.0);
    }
    let v = s.tick(&id, 10.0);
    let chart = v["chart"].as_array().expect("chart").clone();
    assert!(!chart.is_empty(), "nothing reached the chart: {v}");

    for c in &chart {
        let kind = c["kind"].as_str().unwrap_or_default();
        if kind != "action" && kind != "action_refused" {
            continue;
        }
        let text = c["text"].as_str().unwrap_or_default();
        assert!(
            !text.contains('_'),
            "the chart is printing an intervention id: {text:?} — that is the rubric's needle"
        );
    }
    // The whole payload, not just the chart: an id anywhere on the wire is an id the candidate
    // can read. (The three the audit named, on the two stations that define them.)
    let body = v.to_string();
    for needle in ["adrenaline_undosed", "dx_anaphylaxis", "exam_throat"] {
        assert!(!body.contains(needle), "{needle} is still on the wire: {body}");
    }

    // And the tape keeps every id, because the leaf and the scorer are built from it.
    let tape = s.json(&format!("/api/tape?id={id}")).to_string();
    assert!(tape.contains("adrenaline"), "the tape lost the resolved order: {tape}");
}

/// The same translation must not swallow the lines the engine does not write as ids. A harm, an
/// outcome and a status are prose already, and a renderer that assumed every chart line was an
/// intervention id would replace them with a shrug.
#[test]
fn the_lines_the_engine_writes_as_prose_survive_the_translation() {
    let s = Server::start();
    let id = s.open("ep1");
    s.order(&id, "let her stand up");
    let end = s.play_out(&id);
    let chart = end["chart"].as_array().expect("chart");
    let prose: Vec<&str> = chart
        .iter()
        .filter(|c| c["kind"] == "harm" || c["kind"] == "outcome" || c["kind"] == "status")
        .filter_map(|c| c["text"].as_str())
        .collect();
    assert!(!prose.is_empty(), "no non-order lines to check: {end}");
    for line in prose {
        assert!(!line.is_empty(), "a chart line was translated into nothing");
    }
    // And an order that *is* an id still reads as the case's own label rather than the id.
    let orders: Vec<&str> = chart
        .iter()
        .filter(|c| c["kind"] == "action")
        .filter_map(|c| c["text"].as_str())
        .collect();
    for o in orders {
        assert!(!o.contains('_'), "an episode is printing an intervention id too: {o:?}");
    }
}

/// **The clock a candidate could stop.** `hold` freezes the run and `easy/hard` picks how fast
/// the patient crumples. Both are right for practice and neither belongs in something that ends
/// in an anchored claim: half of every station's rubric is timed, so a candidate who can freeze
/// the patient is inside every window by construction, and choosing the deterioration rate is
/// choosing how much of the mark sheet is reachable — after the run was declared to the chain.
#[test]
fn a_station_has_no_hold_button_and_no_speed_dial() {
    let page = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static/index.html"),
    )
    .expect("the page");

    // One place decides, and it hides both.
    let at = page.find("function examControls()").expect("examControls is gone");
    let body = &page[at..at + 400];
    for id in ["#pause", "#diff"] {
        assert!(
            body.contains(&format!("$('{id}').classList.toggle('hide',x)")),
            "examControls no longer hides {id}"
        );
    }
    assert!(body.contains("hard=false"), "examControls no longer pins the speed");

    // A hidden button is still a button, so the handlers refuse as well.
    for handler in ["$('#pause').onclick=()=>{ if(examMode())return;", "$('#diff').onclick=()=>{ if(examMode())return;"] {
        assert!(page.contains(handler), "a control still answers during an exam: {handler}");
    }

    // And it is called from all three moments exam-ness can change: entering a case, starting
    // the clock, and walking back out to the shelf.
    assert!(
        page.matches("examControls()").count() >= 4,
        "examControls is declared and barely called"
    );

    // Nothing else may re-enable the hold button behind its back.
    assert!(
        !page.contains("$('#pause').disabled=false"),
        "something re-enables hold without asking whether this is an exam"
    );
}

/// **The endpoint that gave away more than the harm text.** `/api/debrief` was not sealed at
/// all. `expected` is the scenario's own model answer — every intervention the case wanted, its
/// label, the reason it wanted it and the second it wanted it by — and `harms` carries the full
/// sentence together with the intervention id that caused it. One GET mid-run was the whole
/// station, in order, with timings, which walked straight around the seal the view now holds.
#[test]
fn the_debrief_stays_shut_until_the_case_is_over() {
    let s = Server::start();
    let id = s.open("osce-c");
    s.order(&id, "look in the throat");
    s.tick(&id, 10.0);

    let mid = s.json(&format!("/api/debrief?id={id}"));
    assert_eq!(mid["sealed"], serde_json::json!(true), "the debrief opened mid-run: {mid}");
    let body = mid.to_string();
    assert!(!body.contains("tongue depressor"), "the harm sentence came out of the debrief: {body}");
    assert!(!body.contains("exam_throat"), "the intervention id came out of the debrief: {body}");
    assert!(mid["expected"].is_null(), "the model answer leaked mid-run: {mid}");
    assert!(mid["harms"].is_null(), "the harm list leaked mid-run: {mid}");

    // And the bell opens it, because everything above is what a debrief is for.
    s.play_out(&id);
    let after = s.json(&format!("/api/debrief?id={id}"));
    assert!(after["sealed"].is_null(), "the debrief stayed shut after the outcome: {after}");
    assert!(
        after["harms"].as_array().is_some_and(|a| !a.is_empty()),
        "the debrief lost the harms it exists to explain: {after}"
    );
}
