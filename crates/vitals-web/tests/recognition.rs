//! What a learner's typed order is read as — the traps a substring matcher set, and proof that
//! closing them changed nothing a button or a Thai alias sends.
//!
//! Every row here was live in the bay (`/api/step` → `resolve_order`) until this change: the EP1–EP5
//! files are byte for byte embla-cloud's, which found these between 11 and 16 Nov 2026 and fixed
//! them in recognition only (`vitals_sce::recognise`). No scenario file changes, and a tape
//! replays by id as it always did.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use vitals_sce::{runtime::ACTION, runtime::ACTION_REFUSED, Sce, SceState};
use vitals_web::lang;
use vitals_web::order::{resolve_order, shock_order};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The file each case id plays, as `scenario_path` in `main.rs` resolves it.
fn path(id: &str) -> PathBuf {
    root().join(match id {
        "ep1" => "conformance/sce-anaphylaxis-ep1.json".to_string(),
        "ep2" => "demo/scenarios/ep2-stemi.json".to_string(),
        "ep3" => "demo/scenarios/ep3-epiglottitis.json".to_string(),
        "ep4" => "demo/scenarios/ep4-pulmonary-embolism.json".to_string(),
        "ep5" => "demo/scenarios/ep5-the-night-the-stars-fell.json".to_string(),
        station => format!("demo/stations/{station}.sce.json"),
    })
}

fn sce(id: &str) -> Sce {
    let p = path(id);
    Sce::from_json(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())))
        .unwrap_or_else(|e| panic!("{id}: {e}"))
}

/// Every case the bay plays: the five episodes and every station on disk.
fn cases() -> Vec<String> {
    let mut out: Vec<String> = ["ep1", "ep2", "ep3", "ep4", "ep5"].map(String::from).to_vec();
    let mut stations: Vec<String> = std::fs::read_dir(root().join("demo/stations"))
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().to_str()?.strip_suffix(".sce.json").map(String::from))
        .collect();
    stations.sort();
    assert_eq!(stations.len(), 12, "the twelve OSCE stations: {stations:?}");
    out.extend(stations);
    out
}

/// What the engine's substring rule makes of `text`: the order `apply` fires on a fresh run —
/// the rule `resolve` used before this change, and the one a `Step::Do` still replays through.
fn substring(sce: &Sce, text: &str) -> Option<String> {
    let mut st = SceState::new(sce.clone());
    let from = st.events().len();
    st.apply(text);
    st.events()[from..].iter().find(|e| e.kind == ACTION || e.kind == ACTION_REFUSED).map(|e| e.text.clone())
}

/// What the bay read these words as before: the scenario's substring match, then the language
/// layer's English order through the same rule.
fn before(id: &str, text: &str) -> String {
    let s = sce(id);
    substring(&s, text)
        .or_else(|| lang::canonical_order(text).and_then(|en| substring(&s, en)))
        .unwrap_or_default()
}

fn now(id: &str, text: &str) -> String {
    resolve_order(&SceState::new(sce(id)), text)
}

// ── the traps ───────────────────────────────────────────────────────────────────────────────────

#[test]
fn give_adrenaline_im_is_im_and_not_the_iv_push_harm() {
    for t in ["give adrenaline 0.5 mg im", "Give adrenaline IM in the thigh", "give adrenaline im"] {
        assert_eq!(before("ep1", t), "adrenaline_iv_push", "the trap as it was: {t}");
        assert_eq!(now("ep1", t), "adrenaline_im", "{t}");
    }
    // An order that really is an IV push is still read as one.
    for t in ["give adrenaline iv push", "adrenaline iv", "push adrenaline 1:1000", "อะดรีนาลีนเข้าเส้น"] {
        assert_eq!(now("ep1", t), "adrenaline_iv_push", "{t}");
    }
}

#[test]
fn naming_the_airway_in_thai_is_not_standing_her_up() {
    for t in ["ประเมินทางเดินหายใจ", "ดูแลทางเดินหายใจให้โล่ง", "ผื่นลุกลามทั้งตัว", "ซักประวัติการเดินทาง"] {
        assert_ne!(now("ep1", t), "stand", "{t}");
    }
    assert_eq!(before("ep1", "ประเมินทางเดินหายใจ"), "stand", "the trap as it was");
    // Standing her up is still standing her up.
    for t in ["ให้ผู้ป่วยลุกเดินไปห้องน้ำ", "let her stand up", "ลุกขึ้นนั่ง"] {
        assert_eq!(now("ep1", t), "stand", "{t}");
    }
}

#[test]
fn the_same_trap_in_the_other_episodes() {
    // `asa` (aspirin) inside "nasal".
    assert_eq!(before("ep2", "nasal cannula 2 l"), "aspirin");
    assert_eq!(now("ep2", "nasal cannula 2 l"), "oxygen");
    assert_eq!(now("ep2", "asa 300 mg chewed"), "aspirin");
    // `ent` inside "patient".
    assert_eq!(before("ep3", "calm the patient"), "call_airway_team");
    assert_ne!(now("ep3", "calm the patient"), "call_airway_team");
    assert_eq!(now("ep3", "call ent and anaesthesia"), before("ep3", "call ent and anaesthesia"));
}

#[test]
fn understanding_is_not_standing() {
    assert_eq!(before("ep1", "ok i understand"), "stand", "the trap as it was");
    assert_eq!(now("ep1", "ok i understand"), "");
    assert_eq!(now("ep1", "standard monitoring"), "");
}

// ── a negated order is not that order ───────────────────────────────────────────────────────────

/// (case, words, what they were read as before, what they are read as now). "" is nothing.
const NEGATED: &[(&str, &str, &str, &str)] = &[
    ("ep1", "ห้ามลุก", "stand", ""),
    ("ep1", "ห้ามลุกเดิน", "stand", ""),
    ("ep1", "อย่าให้ผู้ป่วยลุกจากเตียง", "stand", ""),
    ("ep1", "do not let her stand up", "stand", ""),
    ("ep1", "don't let her walk", "stand", ""),
    ("ep1", "no walking", "stand", ""),
    ("ep1", "อย่าเพิ่งให้กลับบ้าน", "discharge", ""),
    ("ep1", "ยังไม่ควรจำหน่าย", "discharge", ""),
    ("ep1", "don't discharge her yet", "discharge", ""),
    ("ep1", "not ready for discharge", "discharge", ""),
    ("ep1", "ห้ามให้อะดรีนาลีนเข้าเส้น", "adrenaline_iv_push", ""),
    ("ep1", "do not give adrenaline iv push", "adrenaline_iv_push", ""),
    // The route the learner ruled out no longer overrides the route the learner gave.
    ("ep1", "adrenaline 0.5 mg im, not iv", "adrenaline_iv_push", "adrenaline_im"),
    ("ep1", "give adrenaline im, do not push", "adrenaline_iv_push", "adrenaline_im"),
    ("ep3", "don't examine the throat", "tongue_depressor", ""),
    ("ep3", "do not separate him from his mother", "separate_mother", ""),
    ("ep3", "no iv access", "iv_access", ""),
    ("ep4", "this is not anxiety", "discharge_as_anxiety", ""),
    ("ep4", "do not discharge", "discharge_as_anxiety", ""),
    // ep4 keys discharge in English only: the Thai reaches it through the alias table, which had
    // the same trap one layer down.
    ("ep4", "อย่าเพิ่งให้กลับบ้าน", "discharge_as_anxiety", ""),
    ("ep5", "no crystalloid", "crystalloid_flood", ""),
    ("ep5", "avoid saline", "crystalloid_flood", ""),
    // ทำ and จำเป็น as filler: NFKC splits ำ, so the filler list must be canonical too.
    ("ep1", "ไม่จำเป็นต้องให้กลับบ้าน", "discharge", ""),
    ("ep1", "ไม่ต้องทำให้ลุก", "stand", ""),
    // Every other order too. "no aspirin" gave the aspirin, and the rubric credited it.
    ("ep1", "ไม่ต้องใส่ท่อ", "intubate", ""),
    ("ep1", "don't intubate", "intubate", ""),
    ("ep1", "no need to intubate", "intubate", ""),
    ("ep1", "ห้ามให้อะดรีนาลีน", "adrenaline_im", ""),
    ("ep1", "no oxygen", "oxygen", ""),
    ("ep1", "ไม่ต้องให้น้ำเกลือ", "fluids", ""),
    ("ep1", "do not give chlorpheniramine", "antihistamine", ""),
    ("ep2", "no aspirin", "aspirin", ""),
    ("ep2", "don't give aspirin", "aspirin", ""),
    ("ep2", "ไม่ต้องให้แอสไพริน", "aspirin", ""),
    ("ep2", "no nitrate", "nitrate", ""),
    ("ep2", "no heparin", "heparin", ""),
    ("ep3", "no antibiotics", "ceftriaxone", ""),
    ("ep3", "don't intubate", "secure_airway", ""),
    ("ep4", "ไม่ต้องทำ ecg", "ecg", ""),
    ("ep4", "no anticoagulation", "anticoagulate", ""),
    ("ep5", "no tourniquet", "tourniquet", ""),
    ("ep5", "ไม่ต้องให้เลือด", "transfuse", ""),
    ("ep5", "no chest drain", "chest_decompression", ""),
];

#[test]
fn saying_not_to_is_not_doing_it() {
    for (ep, t, was, is) in NEGATED {
        assert_eq!(before(ep, t), *was, "{ep}: the trap as it was: {t}");
        assert_eq!(now(ep, t), *is, "{ep}: {t}");
    }
}

#[test]
fn an_order_with_no_negation_in_reach_is_still_that_order() {
    for (ep, t, is) in [
        ("ep1", "ให้ผู้ป่วยลุกเดินไปห้องน้ำ", "stand"),
        ("ep1", "let her stand up, no rush", "stand"),
        ("ep1", "ไม่มีหวีด ให้กลับบ้าน", "discharge"),
        ("ep1", "ไม่ต้องรอ ให้กลับบ้านได้", "discharge"),
        ("ep1", "no wheeze, discharge home", "discharge"),
        ("ep1", "give adrenaline iv push", "adrenaline_iv_push"),
        ("ep1", "not improving after im, give adrenaline iv", "adrenaline_iv_push"),
        ("ep4", "ให้กลับบ้าน", "discharge_as_anxiety"),
        ("ep4", "reassure and discharge", "discharge_as_anxiety"),
        ("ep5", "no improvement, give saline", "crystalloid_flood"),
        ("ep1", "ไม่ดีขึ้น ให้อะดรีนาลีนซ้ำ", "adrenaline_im"),
        ("ep1", "ไม่ไหวแล้ว ใส่ท่อ", "intubate"),
        ("ep1", "intubate without delay", "intubate"),
        ("ep2", "no contraindication, give aspirin", "aspirin"),
        ("ep2", "no allergy give aspirin 300 mg", "aspirin"),
        ("ep2", "ไม่แพ้ยา ให้แอสไพริน", "aspirin"),
    ] {
        assert_eq!(now(ep, t), is, "{ep}: {t}");
    }
}

#[test]
fn an_order_said_in_negations_keeps_them() {
    // EP3's position of comfort keys on "do not disturb": its negation is the order, not against it.
    for (ep, t, is) in [
        ("ep3", "do not disturb him", "position_of_comfort"),
        ("ep3", "do not disturb him, position of comfort", "position_of_comfort"),
    ] {
        assert_eq!(before(ep, t), is, "{ep}: {t}");
        assert_eq!(now(ep, t), is, "{ep}: {t}");
    }
}

#[test]
fn hold_and_stop_negate_only_an_order_that_does_harm() {
    // "hold pressure" and "stop the bleeding" are orders, so "hold", "stop" and "หยุด" negate only
    // an order that does harm or sends the patient home — "hold the discharge".
    for (ep, t, is) in [
        ("ep5", "hold pressure on the wound", "tourniquet"),
        ("ep5", "stop the bleeding", "tourniquet"),
        ("ep5", "stop the bleeding with a tourniquet", "tourniquet"),
        ("ep5", "apply direct pressure", "tourniquet"),
    ] {
        assert_eq!(before(ep, t), is, "{ep}: {t}");
        assert_eq!(now(ep, t), is, "{ep}: {t}");
    }
    for (ep, t) in [("ep1", "hold the discharge"), ("ep1", "stop, don't stand her up"), ("ep4", "hold the discharge")] {
        assert_ne!(before(ep, t), "", "{ep}: the trap as it was: {t}");
        assert_eq!(now(ep, t), "", "{ep}: {t}");
    }
}

// ── the defibrillator ───────────────────────────────────────────────────────────────────────────

#[test]
fn saying_not_to_shock_is_not_a_shock() {
    // The bay's own shock route opens when the scenario declines the words, on every case — so a
    // negated shock delivered 200 J on any of them. The most dangerous misreading on the page.
    for t in ["do not shock", "don't shock", "no defib", "do not defibrillate", "ห้ามช็อกไฟฟ้า", "ไม่ต้องช็อกไฟฟ้า"] {
        assert_eq!(shock_order(t), None, "{t}");
    }
    for (t, j) in [
        ("shock", 200.0),
        ("defibrillate 150 j", 150.0),
        ("shock 200j", 200.0),
        ("ช็อกไฟฟ้า 360", 360.0),
        ("no pulse, shock now", 200.0),
        ("defibrillate 200 j", 200.0), // the tray's own phrase
    ] {
        assert_eq!(shock_order(t), Some(j), "{t}");
    }
    // The other kind of shock was never a defibrillator, and still is not.
    for t in ["septic shock", "she is in shock", "shock index 1.2"] {
        assert_eq!(shock_order(t), None, "{t}");
    }
}

// ── nothing a button or an alias sends has moved ────────────────────────────────────────────────

/// Every chip the page offers, per case, read off `static/index.html` (comments stripped).
fn page_chips() -> Vec<(String, String)> {
    let page = std::fs::read_to_string(root().join("crates/vitals-web/static/index.html")).unwrap();
    let start = page.find("const CHIPS={").expect("the page has CHIPS");
    let body = &page[start + "const CHIPS={".len()..];
    let end = body.find("\n};").expect("CHIPS closes");
    let mut text = body[..end].to_string();
    while let Some(a) = text.find("/*") {
        let b = text[a..].find("*/").map(|b| a + b + 2).unwrap_or(text.len());
        text.replace_range(a..b, "");
    }
    let mut out = Vec::new();
    let mut case = String::new();
    for line in text.lines() {
        let mut rest = line.trim_start();
        // A case opens its own line: `ep1:{…` or `'osce-a':{…`.
        if let Some(i) = rest.find(":{") {
            let key = rest[..i].trim_matches('\'');
            if key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                case = key.to_string();
                rest = &rest[i + 2..];
            }
        }
        while let Some(i) = rest.find('\'') {
            let after = &rest[i + 1..];
            let Some(j) = after.find('\'') else { break };
            out.push((case.clone(), after[..j].to_string()));
            rest = &after[j + 1..];
        }
    }
    out
}

#[test]
fn every_chip_on_every_case_is_read_exactly_as_before() {
    let chips = page_chips();
    let on: std::collections::BTreeSet<&str> = chips.iter().map(|(c, _)| c.as_str()).collect();
    let all = cases();
    assert_eq!(on.len(), all.len(), "the parser saw {on:?}");
    assert!(chips.len() >= 250, "only {} chips read off the page — the parser went blind", chips.len());
    let mut moved = Vec::new();
    for (case, chip) in &chips {
        assert!(all.contains(case), "{case} is not a case");
        let (b, a) = (before(case, chip), now(case, chip));
        if b != a {
            moved.push((case.clone(), chip.clone(), b, a));
        }
    }
    assert_eq!(moved, Vec::<(String, String, String, String)>::new(), "chips moved: {moved:#?}");
}

/// Phrases the language layer offers whose old reading was a short keyword inside a longer word —
/// each one a trap closed, read now as nothing. (case, the English phrase, what it was read as.) A
/// Thai label that translates to one of these moved with it. Every other alias, on every case,
/// reads exactly as before.
const ALIASES_MOVED: &[(&str, &str, &str)] = &[
    ("ep1", "where is the pain — what makes it better?", "intubate"), // ett in "better"
    ("ep3", "where is the pain — what makes it better?", "secure_airway"),
    ("ep5", "where is the pain — what makes it better?", "intubate"),
    ("ep3", "can you finish a sentence?", "call_airway_team"), // ent in "sentence"
    ("osce-b3", "immunisations?", "exam_sats"),                // sat inside "immunisations"
    ("osce-c2", "broad-spectrum antibiotics now", "ics_controller"), // ics in "antibiotics"
    // osce-d3 keys its exposure history on "ate" and "eat": asking about breathing, or ordering a
    // tube or a shock, scored the history mark — and kept the shock from ever being delivered.
    ("osce-d3", "any fever or a cold lately?", "ask_exposure"),
    ("osce-d3", "are her shots up to date?", "ask_exposure"),
    ("osce-d3", "can you breathe all right?", "ask_exposure"),
    ("osce-d3", "can you breathe?", "ask_exposure"),
    ("osce-d3", "does breathing change it?", "ask_exposure"),
    ("osce-d3", "defibrillate", "ask_exposure"),
    ("osce-d3", "intubate, secure the airway", "ask_exposure"),
];

#[test]
fn every_thai_alias_and_its_english_order_is_read_exactly_as_before() {
    let aliases: Vec<(&str, &str)> = lang::order_aliases().iter().copied().chain(lang::ask_aliases()).collect();
    assert!(aliases.len() >= 60, "{}", aliases.len());
    let mut want: Vec<(String, String, String, String)> = Vec::new();
    for (case, en, was) in ALIASES_MOVED {
        want.push((case.to_string(), en.to_string(), was.to_string(), String::new()));
        for (th, _) in aliases.iter().filter(|(_, e)| e == en) {
            want.push((case.to_string(), th.to_string(), was.to_string(), String::new()));
        }
    }
    want.sort();
    want.dedup();
    let mut moved = Vec::new();
    for case in cases() {
        for (th, en) in &aliases {
            for p in [th, en] {
                let (b, a) = (before(&case, p), now(&case, p));
                if b != a {
                    moved.push((case.clone(), p.to_string(), b, a));
                }
            }
        }
    }
    moved.sort();
    moved.dedup();
    assert_eq!(moved, want, "aliases moved: {moved:#?}");
}

// ── the stations' keyword lists ─────────────────────────────────────────────────────────────────

#[test]
fn the_stations_short_keywords_read_as_their_authors_meant() {
    // A short keyword on a station is mostly an abbreviation (`abg`, `cxr`, `ics`), and a few are
    // the start of a word (`sat`, `neb`, `eat` — `recognise::STEMS`). Either way it no longer
    // counts from inside a longer word.
    for (case, t, was, is) in [
        ("osce-c2", "broad-spectrum antibiotics", "ics_controller", ""),
        ("osce-b3", "she collapsed", "keep_calm", ""),
        ("osce-a", "intubate without delay", "supine", ""),
        ("osce-d3", "what is her date of birth", "ask_exposure", ""),
    ] {
        assert_eq!(before(case, t), was, "{case}: the trap as it was: {t}");
        assert_eq!(now(case, t), is, "{case}: {t}");
    }
    for (case, t, is) in [
        ("osce-b3", "check the saturations", "exam_sats"),
        ("osce-c", "sats on air", "exam_sats"),
        ("osce-c", "put a sat probe on", "exam_sats"),
        ("osce-c2", "nebulised salbutamol", "neb_salbutamol"),
        ("osce-c2", "back-to-back nebs", "neb_salbutamol"),
        ("osce-c2", "start an ics", "ics_controller"),
        ("osce-d3", "has she eaten anything new", "ask_exposure"),
        ("osce-d3", "what did she eat?", "ask_exposure"),
        ("osce-d4", "two abgs", "lactate"),
        ("osce-a", "lay him flat", "supine"),
        ("osce-b3", "on mum's lap", "keep_calm"),
    ] {
        assert_eq!(now(case, t), is, "{case}: {t}");
    }
}

/// Every keyword of every case, typed alone, whose reading moved — each one a trap closed.
/// (case, keyword, before, now.)
const KEYWORDS_MOVED: &[(&str, &str, &str, &str)] = &[];

#[test]
fn every_keyword_of_every_case_typed_alone_reads_as_before_but_for_the_traps() {
    // embla-cloud never had the twelve stations; their keyword lists go through the new rule here.
    let mut moved: Vec<(String, String, String, String)> = Vec::new();
    let mut n = 0;
    for case in cases() {
        for iv in &sce(&case).interventions {
            let m = &iv.matcher;
            for k in m.any_kw.iter().chain(m.all_groups.iter().flatten()) {
                n += 1;
                let (b, a) = (before(&case, k), now(&case, k));
                if b != a {
                    moved.push((case.clone(), k.clone(), b, a));
                }
            }
        }
    }
    assert!(n > 500, "only {n} keywords read");
    let want: Vec<(String, String, String, String)> = KEYWORDS_MOVED
        .iter()
        .map(|(c, k, b, a)| (c.to_string(), k.to_string(), b.to_string(), a.to_string()))
        .collect();
    assert_eq!(moved, want, "{n} keywords; moved: {moved:#?}");
}

// ── the bay, end to end ─────────────────────────────────────────────────────────────────────────

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
        let state = std::env::temp_dir().join(format!("vitals-recognition-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&state);
        let mut child = Command::new(env!("CARGO_BIN_EXE_vitals-web"))
            .env("VITALS_WEB_BIND", "127.0.0.1:0")
            .env("VITALS_STATE_DIR", &state)
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
        self.json(&format!("/api/new?ep={ep}"))["id"].as_str().unwrap_or_else(|| panic!("{ep}: no session id")).to_string()
    }

    fn order(&self, id: &str, text: &str) -> serde_json::Value {
        self.json(&format!("/api/step?id={id}&do={}", enc(text)))
    }

    fn tape(&self, id: &str) -> serde_json::Value {
        self.json(&format!("/api/tape?id={id}"))["tape"].clone()
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

/// What a step leaves on screen that an order could have changed.
fn bedside(v: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "hr": v["hr"], "sbp": v["sbp"], "dbp": v["dbp"], "rr": v["rr"], "status": v["status"],
        "harm": v["harm"], "rhythm": v["rhythm"],
    })
}

#[test]
fn an_order_nobody_recognised_does_nothing_live_and_on_replay_alike() {
    // Live, an order recognition declined used to be handed to `apply`, which read the words by
    // substring — the very trap — while the tape carried the empty id and replay did nothing. So
    // "ห้ามลุก" stood her up on screen, and the anchored run said she never stood.
    let s = Server::start();
    let idle = s.open("ep1");
    let quiet = s.order(&idle, "nothing anyone wrote a keyword for");
    for t in ["ห้ามลุก", "ประเมินทางเดินหายใจ", "ok i understand", "อย่าเพิ่งให้กลับบ้าน", "ห้ามให้อะดรีนาลีนเข้าเส้น"] {
        let id = s.open("ep1");
        let v = s.order(&id, t);
        assert_eq!(bedside(&v), bedside(&quiet), "{t}: as if nothing had been ordered");
        assert!(v["harm"].as_array().is_some_and(|h| h.is_empty()), "{t}: {}", v["harm"]);
        assert_eq!(s.tape(&id)[0], serde_json::json!({"do": t, "act": ""}), "{t}");
    }
}

#[test]
fn a_negated_shock_delivers_nothing_in_the_bay() {
    let s = Server::start();
    let id = s.open("ep2");
    let mut v = serde_json::Value::Null;
    for _ in 0..120 {
        v = s.json(&format!("/api/step?id={id}&tick=10"));
        if v["rhythm"] == "vf" || !v["outcome"].is_null() {
            break;
        }
    }
    assert_eq!(v["rhythm"], "vf", "EP2 never reached VF: {v}");
    let v = s.order(&id, "do not shock");
    assert_eq!(v["rhythm"], "vf", "a shock nobody asked for converted the rhythm: {v}");
    let tape = s.tape(&id);
    assert!(
        !tape.as_array().unwrap().iter().any(|st| st.get("shock").is_some()),
        "a negated shock went on the tape: {tape}"
    );
}
