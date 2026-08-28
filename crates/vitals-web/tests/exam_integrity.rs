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
            // The shipped window is six new runs a minute per address, which is right for a
            // public bay and wrong for a test that has to walk the whole shelf in one process.
            // The limit itself has its own tests in `meter`.
            .env("VITALS_TURNS_PER_MIN", "600")
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

    /// A raw body, unparsed — the bytes a browser or a `curl` actually receives.
    fn text(&self, path: &str) -> String {
        let url = format!("http://127.0.0.1:{}{path}", self.port);
        ureq::get(&url).call().map(|r| r.into_string().unwrap_or_default()).unwrap_or_else(|e| {
            match e {
                ureq::Error::Status(_, r) => r.into_string().unwrap_or_default(),
                other => panic!("{url}: {other}"),
            }
        })
    }

    /// The device feed, fetched the way a pane fetches it: one header, naming a session — or
    /// naming none, which is the first thing anybody poking at this would try.
    fn feed(&self, sid: Option<&str>) -> String {
        let url = format!("http://127.0.0.1:{}/device/vitals", self.port);
        let req = ureq::get(&url);
        let req = match sid {
            Some(s) => req.set("x-embla-session", s),
            None => req,
        };
        req.call().map(|r| r.into_string().unwrap_or_default()).unwrap_or_else(|e| match e {
            ureq::Error::Status(_, r) => r.into_string().unwrap_or_default(),
            other => panic!("{url}: {other}"),
        })
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

/// The two sentences the ventilator pane is allowed to say only outside the seal, read out of
/// the server's own source so a reworded const cannot quietly stop being tested.
fn vent_reads() -> Vec<String> {
    let src = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/main.rs"),
    )
    .expect("main.rs");
    let mut out = Vec::new();
    for name in ["VENT_READ_WIDE", "VENT_READ_NARROW"] {
        let at = src.find(&format!("const {name}: &str = \"")).unwrap_or_else(|| panic!("{name} is gone"));
        let body = &src[at + src[at..].find('"').unwrap() + 1..];
        let end = body.find("\";").expect("an unterminated const");
        // Rust's line continuation: a trailing backslash eats the newline and the indent after it.
        let mut text = String::new();
        let mut chars = body[..end].chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\\' {
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
            } else {
                text.push(c);
            }
        }
        out.push(text);
    }
    assert_eq!(out.len(), 2, "the vent read constants stopped parsing: {out:?}");
    out
}

/// **A gate the candidate holds is not a gate.**
///
/// `device/vent.html` shipped the interpretation of the peak-to-plateau gap — *think
/// bronchospasm or a blocked tube* — as a string in the file, and chose whether to render it
/// from `P.get('exam') === '1'`. The pane is an iframe whose URL the page builds, so the
/// candidate did not have to defeat anything: the sentence was in view-source whatever the
/// branch decided, and the branch itself was steered by a parameter they could delete.
///
/// Reading the peak-to-plateau gap is the mark at a ventilator station. So the pane holds no
/// interpretation at all now: the words are the server's, they ride in on the feed the pane
/// already polls, and [`Session::sealed`] — the same predicate the harm list, the feed and the
/// chart are withheld by — decides whether they are sent. Sealed means the key is absent.
///
/// Everything below is asserted against the bytes, because the bytes are what a `curl` gets.
#[test]
fn a_sealed_station_is_never_handed_the_ventilator_read() {
    let s = Server::start();
    let reads = vent_reads();
    // Paraphrases too: the exact const is pinned above, and these catch a rewrite that keeps
    // the answer while changing the wording.
    let fragments = ["think bronchospasm", "not stiff lungs", "blocked tube"];

    // ── the pane, as the browser receives it ────────────────────────────────────────────────
    // No session, no headers, no cookie — this is the whole file, served to anyone.
    for pane in ["vent", "monitor", "pump"] {
        let html = s.text(&format!("/device/{pane}"));
        assert!(html.len() > 500, "/device/{pane} served nothing: {html:?}");
        for r in &reads {
            assert!(!html.contains(r.as_str()), "/device/{pane} ships the read in its own bytes");
        }
        for f in fragments {
            assert!(!html.contains(f), "/device/{pane} ships {f:?} in its own bytes");
        }
        // A pane that can read whether it is in an exam is a pane that can be told it is not.
        // The word may appear in prose; what may not appear is any way of *asking* the URL.
        for gate in ["get('exam')", "get(\"exam\")", "exam=1", "exam'] ", "EXAM ="] {
            assert!(!html.contains(gate), "/device/{pane} reads its own seal from {gate:?}");
        }
    }

    // ── a station, mid-run ──────────────────────────────────────────────────────────────────
    // Sealed by definition: `osce-a` is a set member, so this holds on a bay with no chain
    // configured at all — which is exactly the deployment a visitor reaches first.
    let id = s.open("osce-a");
    let sealed = s.feed(Some(&id));
    assert!(sealed.contains("\"hr\""), "the instrument stopped reporting: {sealed}");
    assert!(!sealed.contains("vent_read"), "a sealed feed carries the key: {sealed}");
    for r in &reads {
        assert!(!sealed.contains(r.as_str()), "a sealed feed carries the read: {sealed}");
    }

    // ── the bypasses ────────────────────────────────────────────────────────────────────────
    // No session named at all, and a session id that never existed: both answer the empty
    // document, so there is no unsealed default to fall into.
    for (what, body) in [("no session", s.feed(None)), ("a forged id", s.feed(Some(&"f".repeat(32))))] {
        assert!(!body.contains("vent_read"), "{what} was handed the read: {body}");
        for r in &reads {
            assert!(!body.contains(r.as_str()), "{what} was handed the read: {body}");
        }
    }
    // The old flag, forged onto the pane URL and onto the feed. Neither is read by anything.
    for q in ["?exam=0", "?exam=1"] {
        let html = s.text(&format!("/device/vent{q}"));
        for r in &reads {
            assert!(!html.contains(r.as_str()), "/device/vent{q} answered differently");
        }
    }

    // ── practice keeps everything ───────────────────────────────────────────────────────────
    // The sentence is why the panel was built. An episode is not a station and is never sealed,
    // so it is handed both branches and picks between them itself.
    let ep1 = s.open("ep1");
    let practice = s.feed(Some(&ep1));
    assert!(practice.contains("vent_read"), "practice lost its teaching: {practice}");
    for r in &reads {
        assert!(practice.contains(r.as_str()), "practice lost a branch of the read: {practice}");
    }

    // ── the bell, not forever ───────────────────────────────────────────────────────────────
    // Sealing lasts exactly as long as the clock, the same way the mark sheet does. The query
    // parameter never did this: it was fixed when the iframe was built and stayed fixed.
    s.play_out(&id);
    let after = s.feed(Some(&id));
    assert!(after.contains("vent_read"), "the read stayed sealed after the outcome: {after}");
    for r in &reads {
        assert!(after.contains(r.as_str()), "the read stayed sealed after the outcome: {after}");
    }
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
    // Not even the token. `harm:sealed` used to travel here and the feed drew it as
    // "⚠ harm recorded" the instant the depressor went in — the sentence withheld and the
    // verdict delivered anyway, on screen, where it did not even need a Network tab.
    let beats: Vec<&str> = v["beats"].as_array().expect("beats").iter()
        .filter_map(|b| b.as_str()).filter(|b| b.starts_with("harm")).collect();
    assert!(beats.is_empty(), "a sealed reply still carries a harm beat: {beats:?}");
    // And the chart does not carry the row at all — see `a_sealed_chart_carries_no_harm_row`,
    // which is the property in its own right. A redacted row on a timestamped record hands over
    // the timing, and the timing is the answer.
    assert_eq!(
        v["chart"].as_array().expect("chart").iter().filter(|c| c["kind"] == "harm").count(),
        0,
        "the sealed chart still carries a harm row: {v}"
    );

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

// ── the row, not only the sentence ───────────────────────────────────────────
//
// The seal used to redact the harm line and keep the row:
//
//     0:12 | ORDER | IV-push adrenaline
//     0:12 | HARM  | ⚠ harm recorded
//
// The sentence was withheld and the *timing* was not, on the same second as the order that
// caused it. A candidate does not have to read the sentence to learn what the seal exists to
// withhold — a marker landing the instant they act is the whole of what the mark sheet will say
// later. The three below assert the row is gone, that nothing took its place, and that the bell
// still hands everything back.

/// Every station's own scenario file, and the orders that reach an intervention in it.
///
/// Read off the disk rather than listed here, so a station added tomorrow is covered without
/// anyone remembering to come back. Never *written* — a `.sce.json`'s sha256 is the case's
/// identity on chain and proofs are anchored against these bytes.
fn stations_and_their_harms() -> Vec<(String, Vec<String>)> {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../demo/stations");
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .expect("demo/stations")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().ends_with(".sce.json"))
        .collect();
    files.sort();

    let mut out = Vec::new();
    for path in files {
        let ep = path.file_name().unwrap().to_string_lossy().replace(".sce.json", "");
        let sce: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("a case")).expect("json");
        let mut orders = Vec::new();
        for iv in sce["interventions"].as_array().into_iter().flatten() {
            // Harm is declared two ways and both have to be swept: on the intervention, and
            // inside an effect. `osce-d4` writes every one of its five the second way and would
            // read as a harmless station to anyone who only checked the first.
            let declared = iv.get("harm").is_some_and(|h| !h.is_null());
            let in_effects = iv["effects"].to_string().contains("\"harm\"");
            if !(declared || in_effects) {
                continue;
            }
            if let Some(q) = query_for(iv) {
                orders.push(q);
            }
        }
        out.push((ep, orders));
    }
    assert!(out.len() >= 12, "the shelf stopped being read: {out:?}");
    assert!(
        out.iter().filter(|(_, o)| !o.is_empty()).count() >= 12,
        "a station stopped offering a reachable harm, so this test would pass by not testing: {out:?}"
    );
    out
}

/// Free text that reaches this intervention, built from the case's own matcher.
///
/// The engine matches on substrings of the lower-cased order, first declaration wins. Taking the
/// author's own keywords is the only way to be sure the order lands somewhere real without this
/// test carrying guesses about what a station will accept.
fn query_for(iv: &serde_json::Value) -> Option<String> {
    let words = |v: &serde_json::Value| -> Vec<String> {
        v.as_array().into_iter().flatten().filter_map(|x| x.as_str()).map(str::to_lowercase).collect()
    };
    let m = &iv["match"];
    let not = words(&m["not_kw"]);
    let clean = |k: &String| !not.iter().any(|n| k.contains(n.as_str()));

    let mut parts: Vec<String> = Vec::new();
    for g in m["all_groups"].as_array().into_iter().flatten() {
        parts.push(words(g).into_iter().find(clean)?);
    }
    let any = words(&m["any_kw"]);
    if !any.is_empty() {
        parts.push(any.into_iter().find(clean)?);
    }
    if parts.is_empty() {
        return None;
    }
    let text = parts.join(" ");
    // The join can manufacture an excluded keyword across a boundary (" iv"). Better to fire no
    // order than one the author excluded.
    (!not.iter().any(|n| text.contains(n.as_str()))).then_some(text)
}

/// **The regression, over the wire, on every station.** Order everything each case declares
/// harmful and read the JSON a browser receives. No row of kind `harm` may be on it — not the
/// sentence, not `harm:sealed`, not a placeholder. Nothing.
#[test]
fn a_sealed_chart_carries_no_harm_row() {
    let s = Server::start();
    let mut charted = 0usize;
    let mut fired = 0usize;

    for (ep, orders) in stations_and_their_harms() {
        let id = s.open(&ep);
        for q in &orders {
            let v = s.order(&id, q);
            // A harm that ends the case is no longer sealed — the bell is exactly when it
            // stops being. Assert on the frames where the clock is still running.
            if !v["outcome"].is_null() {
                break;
            }
            let v = s.tick(&id, 3.0);
            if !v["outcome"].is_null() {
                break;
            }
            fired += 1;
            let chart = v["chart"].as_array().expect("chart");
            charted += chart.len();
            let harm: Vec<&serde_json::Value> =
                chart.iter().filter(|c| c["kind"] == "harm").collect();
            assert!(
                harm.is_empty(),
                "{ep}: ordering {q:?} put a harm row on a sealed chart: {harm:?} — the sentence \
                 is withheld and the timing is not, one row under the order that caused it, which \
                 is the answer the seal exists to keep"
            );
            // Nor by any other name. The token itself may not appear anywhere on the chart.
            for c in chart {
                let text = c["text"].as_str().unwrap_or_default();
                assert!(
                    !text.contains("harm"),
                    "{ep}: {text:?} is a harm marker wearing another kind"
                );
            }
        }
    }
    assert!(fired > 20, "only {fired} harmful orders actually landed; the drive did not drive");
    assert!(charted > 40, "only {charted} chart rows were produced at all");
}

/// The row is gone, and nothing moved in to replace it. Two runs of `osce-d3` — the station where
/// the choice *is* the assessment: 0.2 mg to the kilo for a twenty-kilo six-year-old, or the
/// adult 0.5. Same drug, same route, same second, one of them harm.
///
/// A candidate who can diff the two responses must not be able to tell which. So this asserts on
/// the shape rather than the content: the key set, the number of chart rows, their kinds, their
/// clock. What is *allowed* to differ is the dose the candidate typed — that is their own order
/// read back — and the patient, who is a different patient after the wrong dose and shows it on
/// the monitor, which is the one place an exam has always been allowed to answer.
#[test]
fn the_two_doses_produce_charts_of_the_same_shape() {
    let s = Server::start();

    let shape = |order: &str| -> (Vec<String>, Vec<String>, Vec<String>, usize) {
        let id = s.open("osce-d3");
        s.order(&id, order);
        let v = s.tick(&id, 12.0);
        assert!(v["outcome"].is_null(), "the run ended before the assertion: {v}");
        let chart = v["chart"].as_array().expect("chart");
        (
            v.as_object().expect("an object").keys().cloned().collect(),
            chart.iter().map(|c| c["kind"].as_str().unwrap_or_default().to_string()).collect(),
            chart.iter().map(|c| c["t"].to_string()).collect(),
            v["harm"].as_array().expect("harm").len(),
        )
    };

    let (k_wrong, kinds_wrong, t_wrong, harm_wrong) = shape("adrenaline 0.5 mg im");
    let (k_right, kinds_right, t_right, harm_right) = shape("adrenaline 0.2 mg im");

    assert_eq!(k_wrong, k_right, "the two answers do not even carry the same fields");
    assert_eq!(
        kinds_wrong, kinds_right,
        "the wrong dose produced a different set of chart rows from the right one — a candidate \
         can read which was the trap off the shape of the reply alone"
    );
    assert_eq!(t_wrong, t_right, "the two charts run on different clocks");
    assert_eq!((harm_wrong, harm_right), (0, 0), "the harm list is not sealed");
    // And neither chart mentions harm in any field at all.
    for order in ["adrenaline 0.5 mg im", "adrenaline 0.2 mg im"] {
        let id = s.open("osce-d3");
        s.order(&id, order);
        let body = s.tick(&id, 12.0)["chart"].to_string();
        assert!(!body.contains("harm"), "{order}: the chart still says harm: {body}");
    }
}

/// Harm does not only come from an order. A case that is left alone deteriorates and fires harm
/// off its own clock, through a different code path, and a seal that only covered ordered harm
/// would have handed the same marker to a candidate who did nothing.
#[test]
fn a_harm_the_clock_fired_is_sealed_the_same_way() {
    let s = Server::start();
    let id = s.open("osce-a");
    for frame in 0..40 {
        let v = s.tick(&id, 30.0);
        if !v["outcome"].is_null() {
            // The bell. Everything comes back, including the rows this test watched for.
            let rows: Vec<&str> = v["chart"]
                .as_array()
                .expect("chart")
                .iter()
                .filter(|c| c["kind"] == "harm")
                .filter_map(|c| c["text"].as_str())
                .collect();
            assert!(!rows.is_empty(), "the bell never gave the harm rows back: {v}");
            for r in &rows {
                assert_ne!(*r, "harm:sealed", "a row came back still redacted: {rows:?}");
            }
            assert!(
                v["harm"].as_array().is_some_and(|a| !a.is_empty()),
                "the harm list stayed sealed after the outcome: {v}"
            );
            assert!(frame > 0, "the case ended before a single sealed frame was read");
            return;
        }
        // The feed's channel is sealed on the same rule as the chart, so a harm the clock fired
        // is as invisible mid-run as one an order caused. That a harm *did* fire is established
        // at the bell above, where the rows and the list both have to be non-empty.
        assert!(
            !v["beats"].as_array().expect("beats").iter().any(|b| b.as_str().is_some_and(|b| b.starts_with("harm"))),
            "a harm the clock fired reached a sealed feed: {v}"
        );
        assert_eq!(
            v["chart"].as_array().expect("chart").iter().filter(|c| c["kind"] == "harm").count(),
            0,
            "a harm the clock fired reached a sealed chart: {v}"
        );
    }
    panic!("the run never ended");
}

/// The other half of the rule, and the one it would be easiest to break by accident: an episode
/// is a lesson, not an exam. EP1–EP5 are never sealed, and the harm marker there is the teaching
/// — a coach who will not say what went wrong is not coaching. If the filter above ever widens
/// past the seal, this is what fails.
#[test]
fn a_practice_episode_still_charts_its_harm_while_it_plays() {
    let s = Server::start();
    for (ep, order, needle) in [
        ("ep1", "let her stand up", "collapse"),
        ("ep2", "nitrate", "nitrate"),
        ("ep3", "tongue depressor", "laryngospasm"),
        ("ep4", "thrombolysis", "thrombolysis"),
        ("ep5", "crystalloid", "crystalloid"),
    ] {
        let id = s.open(ep);
        s.order(&id, order);
        let v = s.tick(&id, 10.0);
        assert!(v["outcome"].is_null(), "{ep}: the episode ended before the assertion: {v}");

        let rows: Vec<&str> = v["chart"]
            .as_array()
            .expect("chart")
            .iter()
            .filter(|c| c["kind"] == "harm")
            .filter_map(|c| c["text"].as_str())
            .collect();
        assert!(
            !rows.is_empty(),
            "{ep}: the practice chart lost its harm row — that row is the lesson: {v}"
        );
        assert!(
            rows.iter().any(|r| r.to_lowercase().contains(needle)),
            "{ep}: the harm row no longer says what happened, expected something about {needle:?}: {rows:?}"
        );
        assert!(
            v["harm"].as_array().is_some_and(|a| !a.is_empty()),
            "{ep}: a practice run lost its harm list mid-play: {v}"
        );
    }
}

/// The page has to survive the row not being there. It renders the chart off `kind`, and the
/// only branch that ever read a harm row was the one printing the sealed token into it.
#[test]
fn the_page_no_longer_needs_a_sealed_harm_row_to_render() {
    let page = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static/index.html"),
    )
    .expect("the page");
    // The chart renderer never indexes; it maps whatever rows arrive. An empty chart already
    // had its own branch long before this, for a run where nothing has been ordered yet.
    assert!(
        page.contains("nothing recorded yet"),
        "the chart lost its empty branch, so a chart with no rows would render as nothing at all"
    );
    // And the feed still holds the harm *beats*, which is a different surface with a different
    // promise: `unsealHarm()` rewrites those lines from position at the bell, so they must keep
    // arriving one per harm. See HARM_SEALED in main.rs.
    assert!(
        page.contains("function unsealHarm(v)"),
        "the feed lost the unseal it does at the bell"
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
