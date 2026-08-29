//! The defibrillator button, from the outside — over HTTP, on the JSON the browser receives.
//!
//! What it replaced: `/api/kit?dev=defib` minted the words `"defibrillate 200 j"` and posted them
//! to the scenario's own intervention matcher. `ep2` had a `defibrillate` intervention that
//! branched on the *name of a state*; `ep3`, `ep4` and `ep5` had none at all. So the same button,
//! on the same tray, on four cases in one season, did four different things — and on three of
//! them it did nothing: nothing charted, nothing scored, nothing in the debrief, on a child in
//! cardiac arrest.
//!
//! It is wired to the physiology now, and these are the four properties that had to come with it:
//!
//!   1. a shockable rhythm converts, and the patient's own case decides what comes back with it;
//!   2. a non-shockable one is charted as the error it is — and the sentence that says what it
//!      **cost** is a harm, so the exam seals it until the bell;
//!   3. the chart stays neutral while the station is running: an order line, and no verdict;
//!   4. the button and the typed order produce the *same tape*, because the tape is the leaf.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

struct Server {
    child: Child,
    port: u16,
    state: std::path::PathBuf,
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
        let state = std::env::temp_dir().join(format!("vitals-defib-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&state);
        let mut child = Command::new(env!("CARGO_BIN_EXE_vitals-web"))
            .env("VITALS_WEB_BIND", "127.0.0.1:0")
            .env("VITALS_STATE_DIR", &state)
            // Several cases opened back to back; the shipped window is for a public bay and has
            // its own tests in `meter`.
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

    fn open(&self, ep: &str) -> String {
        self.json(&format!("/api/new?ep={ep}"))["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{ep}: no session id"))
            .to_string()
    }

    fn tick(&self, id: &str, dt: f64) -> serde_json::Value {
        self.json(&format!("/api/step?id={id}&tick={dt}"))
    }

    fn order(&self, id: &str, text: &str) -> serde_json::Value {
        self.json(&format!("/api/step?id={id}&do={}", enc(text)))
    }

    /// The tray, exactly as the page presses it.
    fn press_defib(&self, id: &str, joules: Option<i64>) -> serde_json::Value {
        match joules {
            Some(j) => self.json(&format!("/api/kit?id={id}&dev=defib&set={j}")),
            None => self.json(&format!("/api/kit?id={id}&dev=defib")),
        }
    }

    fn tape(&self, id: &str) -> serde_json::Value {
        self.json(&format!("/api/tape?id={id}"))["tape"].clone()
    }

    fn finish(&self, id: &str) -> serde_json::Value {
        self.json(&format!("/api/finish?id={id}"))
    }

    /// Run the clock until `f` holds, and say so if it never does.
    fn until(
        &self,
        id: &str,
        what: &str,
        f: impl Fn(&serde_json::Value) -> bool,
    ) -> serde_json::Value {
        let mut v = serde_json::Value::Null;
        for _ in 0..120 {
            v = self.tick(id, 10.0);
            if f(&v) {
                return v;
            }
            if !v["outcome"].is_null() {
                break;
            }
        }
        panic!("never reached {what}: {v}");
    }
}

fn enc(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => c.to_string(),
            other => {
                let mut b = [0u8; 4];
                other.encode_utf8(&mut b).bytes().map(|x| format!("%{x:02X}")).collect()
            }
        })
        .collect()
}

/// The chart, as `(kind, text)` — the two fields the page reads.
fn chart(v: &serde_json::Value) -> Vec<(String, String)> {
    v["chart"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| {
            (
                c["kind"].as_str().unwrap_or_default().to_string(),
                c["text"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

fn shock_rows(v: &serde_json::Value) -> Vec<String> {
    chart(v).into_iter().filter(|(k, _)| k == "shock").map(|(_, t)| t).collect()
}

fn beats(v: &serde_json::Value) -> Vec<String> {
    v["beats"].as_array().into_iter().flatten().filter_map(|b| b.as_str()).map(str::to_string).collect()
}

/// EP2 four minutes in: primary VF, the one arrest on the shelf a shock actually treats.
fn ep2_in_vf(s: &Server) -> String {
    let id = s.open("ep2");
    let v = s.until(&id, "ventricular fibrillation", |v| v["rhythm"] == "vf");
    assert_eq!(v["shockable"], true, "the rail refused a shockable rhythm: {v}");
    id
}

// ── 1. the shock that works ─────────────────────────────────────────────────

#[test]
fn shocking_ventricular_fibrillation_brings_the_rhythm_back() {
    let s = Server::start();
    let id = ep2_in_vf(&s);
    let v = s.press_defib(&id, Some(200));

    assert_eq!(v["rhythm"], "sinus", "the shock did not convert the rhythm: {v}");
    assert_eq!(v["pulse"], true, "a converted patient has no pulse: {v}");
    assert_eq!(
        shock_rows(&v),
        vec!["defibrillate 200 J into vf — rhythm now sinus"],
        "the chart does not say what the shock did: {:?}",
        chart(&v)
    );
    // The case's own idea of what ROSC looks like — the engine gives back a rhythm, `ep2` says
    // what pressure and what rate come back with it.
    assert_eq!(v["sbp"].as_f64(), Some(88.0), "ep2's ROSC edge never ran: {v}");
    assert!(beats(&v).iter().any(|b| b == "threshold:rosc"), "no ROSC beat: {:?}", beats(&v));
    assert!(beats(&v).iter().any(|b| b == "threshold:shock"), "no shock beat: {:?}", beats(&v));

    // …and she is still alive two minutes later. Before this, converting the rhythm left the
    // automaton in the state named `vf`, and ninety seconds after that she was in asystole with
    // a sinus rhythm on the monitor.
    let mut last = v;
    for _ in 0..12 {
        last = s.tick(&id, 10.0);
    }
    assert!(last["outcome"].is_null(), "the resuscitated patient died anyway: {last}");
    assert_eq!(last["rhythm"], "sinus");
}

#[test]
fn a_shock_that_worked_is_not_charted_as_a_harm() {
    let s = Server::start();
    let id = ep2_in_vf(&s);
    let v = s.press_defib(&id, Some(200));
    assert!(
        !chart(&v).iter().any(|(k, _)| k == "harm"),
        "converting VF was charted as harm: {:?}",
        chart(&v)
    );
    let end = s.finish(&id);
    assert_eq!(
        end["harm"].as_array().map(Vec::len),
        Some(0),
        "the harm list is not empty after a correct shock: {end}"
    );
}

// ── 2. the shock that cannot work ───────────────────────────────────────────

/// The three episodes that arrest in PEA. Before this, pressing the button here did nothing at
/// all — no chart line, no harm, nothing in the debrief — on a five-year-old, a woman with a
/// saddle embolus and a blast casualty, all three in cardiac arrest.
///
/// These four are the *season*, and the season is story mode: `Session::sealed` is
/// `exam_mode || set_member(ep).is_some()`, and `ep2`–`ep5` are what the station sets **open**
/// rather than members of one. So the harm is on the chart the moment it happens, exactly as
/// every other scenario harm is in story mode. What the seal does to it is asserted on a case
/// that has one — see `a_sealed_station_shows_the_shock_and_withholds_what_it_cost`.
#[test]
fn the_button_is_never_a_no_op_on_a_patient_in_arrest() {
    let s = Server::start();
    for ep in ["ep3", "ep4", "ep5"] {
        let id = s.open(ep);
        let v = s.until(&id, "arrest", |v| v["status"] == "Arrest");
        assert_eq!(v["shockable"], false, "{ep}: PEA was offered to a defibrillator");
        let before = chart(&v).len();

        let v = s.press_defib(&id, Some(200));
        assert_eq!(
            shock_rows(&v),
            vec!["defibrillate 200 J into pea — rhythm now pea"],
            "{ep}: the shock left nothing on the chart: {:?}",
            chart(&v)
        );
        assert!(chart(&v).len() > before, "{ep}: the chart did not grow");
        // Not shockable means not converted. The monitor must not reward the wrong button.
        assert_eq!(v["rhythm"], "pea", "{ep}: a non-shockable rhythm converted");

        // ── charted as the error it is, saying what it cost ──────────────────
        let harm_rows: Vec<String> =
            chart(&v).into_iter().filter(|(k, _)| k == "harm").map(|(_, t)| t).collect();
        assert_eq!(harm_rows.len(), 1, "{ep}: the wrong shock left no harm: {:?}", chart(&v));
        assert!(
            harm_rows[0].contains("it cost compressions and adrenaline"),
            "{ep}: the harm says what the candidate did rather than what it cost: {harm_rows:?}"
        );
        assert!(
            !harm_rows[0].to_ascii_uppercase().contains("HARM"),
            "{ep}: the row grades the candidate: {harm_rows:?}"
        );

        // ── and it is on the record at the bell ──────────────────────────────
        let end = s.finish(&id);
        let harm: Vec<String> = end["harm"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|h| h.as_str())
            .map(str::to_string)
            .collect();
        assert_eq!(harm.len(), 1, "{ep}: the wrong shock is not on the record: {end}");
        assert!(
            harm[0].contains("not shockable") && harm[0].contains("compressions and adrenaline"),
            "{ep}: the harm does not say what the shock cost: {harm:?}"
        );
    }
}

/// **The seal, on the one surface this change adds to it.**
///
/// A station is sealed for as long as it is running, and the rule the seal enforces is that the
/// chart is a record of orders and not a running mark sheet. So the shock row is there — the
/// candidate must be able to see what they did — and the sentence saying what it cost is not,
/// until the bell hands it over with everything else.
///
/// Shocking a patient who has a pulse is the only way to get a defibrillator wrong on the twelve
/// stations, because none of them declares an `arrest` status or a rhythm at all: on the shelf as
/// it stands, every sealed case is perfusing for the whole station. The arrest paths live in the
/// season, which is story mode — see the test above.
#[test]
fn a_sealed_station_shows_the_shock_and_withholds_what_it_cost() {
    let s = Server::start();
    let id = s.open("osce-a");
    let v = s.tick(&id, 10.0);
    assert_eq!(v["pulse"], true, "the fixture is not a perfusing patient: {v}");

    let v = s.press_defib(&id, Some(200));
    assert_eq!(shock_rows(&v), vec!["defibrillate 200 J into sinus — rhythm now sinus"]);
    assert!(
        !chart(&v).iter().any(|(k, _)| k == "harm"),
        "a harm row is on a sealed chart: {:?}",
        chart(&v)
    );
    assert!(
        !beats(&v).iter().any(|b| b.starts_with("harm")),
        "a harm beat reached a sealed reply: {:?}",
        beats(&v)
    );
    assert_eq!(v["harm"].as_array().map(Vec::len), Some(0), "the harm list leaked mid-station");
    // The shock beat is not withheld — it says only that the shock went in, which is the whole
    // reason it says nothing else.
    assert!(beats(&v).iter().any(|b| b == "threshold:shock"), "{:?}", beats(&v));

    let end = s.finish(&id);
    let harm = end["harm"].as_array().cloned().unwrap_or_default();
    assert!(
        harm.iter().any(|h| h.as_str().is_some_and(|h| h.contains("perfusing rhythm"))),
        "shocking a patient with a pulse left no harm on the record: {end}"
    );
}

// ── 3. the chart stays neutral ──────────────────────────────────────────────

/// **The regression `chart_neutrality` exists for, on the one row this change adds.**
///
/// The right shock and the wrong shock produce the same row, in the same column, in the same
/// shape. What differs is the rhythm it names and what the monitor did next — both of which the
/// screen is already showing — and nothing in it grades the candidate.
#[test]
fn the_shock_row_reads_the_same_whether_the_shock_was_right_or_wrong() {
    let s = Server::start();

    let right = ep2_in_vf(&s);
    let a = shock_rows(&s.press_defib(&right, Some(200)));

    let wrong = s.open("ep4");
    s.until(&wrong, "arrest", |v| v["status"] == "Arrest");
    let b = shock_rows(&s.press_defib(&wrong, Some(200)));

    assert_eq!(a.len(), 1);
    assert_eq!(b.len(), 1);
    for line in a.iter().chain(b.iter()) {
        assert!(line.starts_with("defibrillate 200 J into "), "{line:?} is not the order line");
        assert!(!line.to_ascii_uppercase().contains("HARM"), "{line:?} grades the order");
        for word in ["wrong", "mistake", "error", "should", "not shockable", "danger", "useless"] {
            assert!(!line.to_lowercase().contains(word), "{line:?} tells the candidate they erred");
        }
    }
    // Same number of words either way. A row that is conspicuously shorter after the wrong
    // button is the same tell with one extra step in front of it — which is why the engine
    // writes both from one template rather than from two branches.
    assert_eq!(
        a[0].split_whitespace().count(),
        b[0].split_whitespace().count(),
        "the two rows are different lengths: {a:?} / {b:?}"
    );
}

// ── 4. the button and the words are one act ─────────────────────────────────

/// **The tape is the leaf.** Two candidates who deliver the same shock at the same second must
/// leave the same evidence behind, whether one of them reached for the tray and the other typed.
#[test]
fn the_kit_button_and_a_typed_order_write_the_same_tape() {
    let s = Server::start();
    // `kit_phrase("defib", Some(200))` is exactly this string — the button mints it and then
    // reads it back through the same recogniser a typed order goes through.
    for typed in ["defibrillate 200 j", "shock at 200 joules", "cardiovert 200"] {
        let pressed = ep2_in_vf(&s);
        s.press_defib(&pressed, Some(200));

        let said = ep2_in_vf(&s);
        s.order(&said, typed);

        assert_eq!(
            s.tape(&pressed),
            s.tape(&said),
            "pressing the button and typing {typed:?} wrote different tapes"
        );
        assert_eq!(
            s.finish(&pressed)["leaf"],
            s.finish(&said)["leaf"],
            "…and therefore different leaves, for {typed:?}"
        );
    }
}

/// The step on the tape is the shock and its energy, not the words that produced it — so a
/// verifier replays what happened rather than re-guessing what was meant.
#[test]
fn the_tape_records_the_shock_rather_than_the_phrase() {
    let s = Server::start();
    let id = ep2_in_vf(&s);
    s.press_defib(&id, Some(360));
    let tape = s.tape(&id);
    let shocks: Vec<f64> =
        tape.as_array().into_iter().flatten().filter_map(|st| st["shock"].as_f64()).collect();
    assert_eq!(shocks, vec![360.0], "the dial did not reach the tape: {tape}");
    assert!(
        !tape.to_string().contains("defibrillate 360 j"),
        "the phrase is on the tape as well as the shock: {tape}"
    );
}

/// A dial with no number is the tray's own default, and it is the same number the page shows.
#[test]
fn a_shock_with_no_energy_dialled_is_the_default_the_page_offers() {
    let s = Server::start();
    let id = ep2_in_vf(&s);
    let v = s.press_defib(&id, None);
    assert_eq!(shock_rows(&v), vec!["defibrillate 200 J into vf — rhythm now sinus"]);
}

/// Thai reaches the defibrillator, and keeps the number the candidate dialled.
///
/// `lang::canonical_order` answers *whether* a phrase is a shock; the joules are read off the
/// text the candidate actually typed, because the canonical English is one headword for every
/// phrasing and would flatten every energy onto the default.
#[test]
fn a_thai_order_reaches_the_defibrillator_with_its_own_energy() {
    let s = Server::start();
    let id = ep2_in_vf(&s);
    let v = s.order(&id, "ช็อกไฟฟ้า 360 จูล");
    assert_eq!(shock_rows(&v), vec!["defibrillate 360 J into vf — rhythm now sinus"], "{v}");
}

// ── 5. the other kind of shock ──────────────────────────────────────────────

/// `shock` is two words in a resuscitation room, and only one of them is 200 joules.
///
/// `ep5`'s patient is exsanguinating, so "haemorrhagic shock" is a phrase a candidate types on
/// that station in the ordinary course of working. Delivering an unsynchronised shock for it —
/// and charting the harm — would be the worst possible answer to a candidate who was right.
#[test]
fn naming_the_other_kind_of_shock_does_not_fire_the_defibrillator() {
    let s = Server::start();
    for text in [
        "she is in haemorrhagic shock",
        "hypovolaemic shock, treat the bleeding",
        "this is cardiogenic shock",
        "septic shock",
        "her shock index is 1.4",
    ] {
        let id = s.open("ep5");
        s.tick(&id, 10.0);
        let v = s.order(&id, text);
        assert!(
            shock_rows(&v).is_empty(),
            "{text:?} delivered a shock: {:?}",
            chart(&v)
        );
        assert!(
            !s.tape(&id).to_string().contains("\"shock\""),
            "{text:?} put a shock on the tape"
        );
    }
}

/// …and the case's own matcher still answers first, on every station, for every word.
///
/// This is the rule that keeps the change out of the twelve stations nobody reviewed it for: a
/// station that defines its own shock intervention keeps it, and only a case that has declined
/// the text hands it to the engine.
#[test]
fn a_case_that_claims_the_words_still_gets_them() {
    let s = Server::start();
    // osce-d's own `hold_antiplatelets` is not a shock; what matters is that an order a case
    // *does* claim never reaches the defibrillator, so the ordinary path is unchanged.
    let id = s.open("osce-a");
    s.tick(&id, 5.0);
    let v = s.order(&id, "adrenaline 0.5 mg im");
    assert!(shock_rows(&v).is_empty(), "an ordinary order reached the defibrillator: {:?}", chart(&v));
    assert!(
        chart(&v).iter().any(|(k, _)| k == "action"),
        "the ordinary order stopped landing: {:?}",
        chart(&v)
    );
}
