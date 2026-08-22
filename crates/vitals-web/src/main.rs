//! Play EP1 in a browser.
//!
//! One process: the same `vitals-sce` automaton the verifier runs, a session per player, and a
//! tape that is recorded as you play. Reach a terminal state and the tape reduces to a leaf —
//! the same bytes `vitals-replay` would produce from the same tape, because it is the same code.
//!
//! Deliberately small. No framework, no database, no build step: tiny_http, a single HTML page,
//! and sessions in a map. The point is to make the automaton playable, not to ship a platform.

mod chain;
mod patient;
mod store;

use serde::Serialize;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use std::sync::{Arc, Mutex};
use tiny_http::{Header, Method, Response, Server};
use vitals_progress::record::AttemptRecord;
use vitals_progress::Difficulty;
use vitals_replay::{hex, leaf, record_for, replay, resume, sce_hash, Step};
use vitals_sce::{render_beat, Sce, SceState};

const PAGE: &str = include_str!("../static/index.html");
/// The real bedside monitor, vendored from Embla's device page.
///
/// Not reimplemented: it already draws ECG morphology in milliseconds (P 80ms, PR 160ms, a QRS
/// that stays 90ms at any rate), sweeps a cursor the way a monitor does instead of scrolling, and
/// knows VF from asystole from PEA. A hand-rolled trace reads as fake to a clinician instantly —
/// which is exactly what the first version of this app did.
const MONITOR: &str = include_str!("../static/device/monitor.html");
const VENT: &str = include_str!("../static/device/vent.html");
const PUMP: &str = include_str!("../static/device/pump.html");

/// Where the rendered EP1 clips live.
///
/// Served from disk rather than baked into the binary: 20 clips is 43MB, and the point of
/// reusing already-rendered film is that it does not need to be moved around again.
fn clips_dir() -> std::path::PathBuf {
    std::env::var("VITALS_CLIPS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::PathBuf::from("/Users/mimir/Developer/Embla/app-swift/Resources/cutscenes/ep1")
        })
}

/// The patient, keyed by the clinical status the automaton is reporting.
///
/// This is the Director's job in the real Story Mode, reduced to its smallest useful form: the
/// engine says how she is, and the screen shows it. The stills are EP1's, already rendered.
const STILLS: &[(&str, &[u8])] = &[
    ("stable", include_bytes!("../static/img/stable.jpg")),
    ("deteriorating", include_bytes!("../static/img/deteriorating.jpg")),
    ("critical", include_bytes!("../static/img/critical.jpg")),
    ("arrest", include_bytes!("../static/img/arrest.jpg")),
    ("improving", include_bytes!("../static/img/improving.jpg")),
    ("recovered", include_bytes!("../static/img/recovered.jpg")),
    ("dead", include_bytes!("../static/img/dead.jpg")),
];

struct Session {
    /// Which scenario, so a resumed run reloads the same automaton it was played against.
    ep: String,
    state: SceState,
    tape: Vec<Step>,
    beats: Vec<String>,
    sce_json: String,
    scenario: String,
    difficulty: Difficulty,
    anchored: bool,
    /// The conversation, kept only so she remembers what she already told you. It is never
    /// hashed, never anchored, and never leaves this process.
    said: Vec<(String, String)>,
    /// Last time this run was written to disk. Ticks arrive about once a second and are cheap to
    /// lose — the tape is the truth and a few seconds of it is a few seconds of sim — so they are
    /// throttled. Anything the player actually *did* is written immediately.
    saved_at: Option<std::time::Instant>,
}

/// A run as it sits on disk.
///
/// The state is not here, because the state is not a fact — it is what the tape computes. Storing
/// it would create a second copy that can disagree with the tape, which is the failure this repo
/// has already had once. On load the tape is replayed and the machine is rebuilt from it.
#[derive(serde::Serialize, serde::Deserialize)]
struct Saved {
    ep: String,
    /// The scenario this run was played against. A rewritten scenario must not silently resume a
    /// run into a different automaton — the outcome would be re-derived under rules the player
    /// never played.
    sce_hash: String,
    tape: Vec<Step>,
    said: Vec<(String, String)>,
    anchored: bool,
}

const SESSIONS: &str = "sessions";

impl Session {
    fn saved(&self) -> Saved {
        Saved {
            ep: self.ep.clone(),
            sce_hash: hex(&sce_hash(&self.sce_json)),
            tape: self.tape.clone(),
            said: self.said.clone(),
            anchored: self.anchored,
        }
    }

    /// Rebuild a run from disk by replaying its tape.
    fn restore(saved: Saved) -> Result<Session, String> {
        let sce_json = std::fs::read_to_string(scenario_path(&saved.ep)).map_err(|e| e.to_string())?;
        let want = hex(&sce_hash(&sce_json));
        if want != saved.sce_hash {
            return Err(format!("scenario {} changed under this run", saved.ep));
        }
        let (state, r) = resume(&sce_json, &saved.tape)?;
        Ok(Session {
            ep: saved.ep.clone(),
            state,
            beats: r.beats,
            tape: saved.tape,
            sce_json,
            scenario: title(&saved.ep).to_string(),
            difficulty: difficulty(&saved.ep),
            anchored: saved.anchored,
            said: saved.said,
            saved_at: Some(std::time::Instant::now()),
        })
    }
}

/// Write a run to disk. `urgent` is false only for a bare tick.
fn persist(store: &store::Store, id: &str, s: &mut Session, urgent: bool) {
    const THROTTLE: std::time::Duration = std::time::Duration::from_secs(3);
    if !urgent {
        if let Some(t) = s.saved_at {
            if t.elapsed() < THROTTLE {
                return;
            }
        }
    }
    if let Err(e) = store.put(SESSIONS, id, &s.saved()) {
        eprintln!("could not save session {id}: {e}");
    }
    s.saved_at = Some(std::time::Instant::now());
}

/// The anchoring tree this server is filling.
///
/// Shared by every player on the box, which is what the on-chain seeds say: the tree is keyed by
/// its id alone, and a Merkle proof needs every leaf in it. What is *not* shared is who a run
/// belongs to — the claim and progress accounts are seeded on the player, so two people on one
/// server have two separate records.
///
/// It has to outlive the process. The leaves are on chain either way, but the proof is built from
/// this list, so a server that forgets them can no longer prove anything it anchored.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Tree {
    tree_id: u64,
    leaves: Vec<[u8; 32]>,
}

const TREE: &str = "tree";

#[derive(Serialize)]
struct View {
    scenario: String,
    hr: f64,
    sbp: f64,
    dbp: f64,
    spo2: f64,
    rr: f64,
    temp: f64,
    gcs: u8,
    status: String,
    beats: Vec<String>,
    harm: Vec<String>,
    outcome: Option<String>,
    elapsed: f64,
    /// Only once the run is over — a run in progress has nothing to anchor yet.
    leaf: Option<String>,
    sce_hash: String,
    /// What is on the patient right now, in the order it went on.
    equipment: Vec<Kit>,
    /// Everything that happened, stamped with the scenario clock — the chart.
    chart: Vec<Note>,
    /// 0..100. Derived from the vitals against adult normal ranges, not a field the engine
    /// keeps: the automaton models a patient, not a health bar. Shown because a bar is
    /// legible at a glance and the numbers underneath it are the truth.
    stability: u32,
}

#[derive(Serialize, Clone)]
struct Kit {
    id: String,
    setting: Option<f64>,
    since: f64,
}

#[derive(Serialize, Clone)]
struct Note {
    t: f64,
    kind: String,
    text: String,
}

/// How far a value sits inside its normal band, 0 (way out) to 1 (fine).
fn band(v: f64, lo: f64, hi: f64, span: f64) -> f64 {
    let d = if v < lo { lo - v } else if v > hi { v - hi } else { 0.0 };
    (1.0 - (d / span)).clamp(0.0, 1.0)
}

impl Session {
    fn view(&self) -> View {
        let v = self.state.vitals;
        let elapsed: f64 = self
            .tape
            .iter()
            .map(|s| match s {
                Step::Tick(dt) => *dt,
                Step::Do(_) | Step::Ask(_) | Step::Set(..) | Step::Off(_) => 0.0,
            })
            .sum();
        let outcome = self.state.outcome().map(|o| format!("{o:?}"));
        // Derived by replaying the tape, not by reading the live state. Assembling a Replay by
        // hand here would show the player a leaf computed one way while the verifier computes it
        // another, and a leaf that depends on which side of the wire you stand on proves nothing.
        let leaf_hex = outcome.as_ref().and_then(|_| {
            let r = replay(&self.sce_json, &self.tape).ok()?;
            Some(hex(&leaf(&sce_hash(&self.sce_json), &self.tape, &r)))
        });
        let v2 = self.state.vitals;
        // Oxygenation and perfusion carry the most weight because they are what kills first.
        let stability = (band(v2.spo2, 94.0, 100.0, 14.0) * 0.34
            + band(v2.sbp, 100.0, 140.0, 45.0) * 0.28
            + band(v2.hr, 60.0, 100.0, 60.0) * 0.16
            + band(v2.rr, 12.0, 20.0, 18.0) * 0.12
            + band(v2.gcs as f64, 15.0, 15.0, 7.0) * 0.10)
            * 100.0;

        View {
            scenario: self.scenario.clone(),
            hr: v.hr.round(),
            sbp: v.sbp.round(),
            dbp: v.dbp.round(),
            spo2: v.spo2.round(),
            rr: v.rr.round(),
            temp: (v.temp * 10.0).round() / 10.0,
            gcs: v.gcs,
            status: format!("{:?}", self.state.status),
            beats: self.beats.clone(),
            harm: self.state.harm_events.clone(),
            outcome,
            elapsed,
            leaf: leaf_hex,
            sce_hash: hex(&sce_hash(&self.sce_json)),
            equipment: self
                .state
                .equipment()
                .iter()
                .map(|e| Kit { id: e.id.clone(), setting: e.setting, since: e.since_sec })
                .collect(),
            chart: self
                .state
                .events()
                .iter()
                .map(|e| Note { t: e.t_sec, kind: e.kind.clone(), text: e.text.clone() })
                .collect(),
            stability: if self.state.outcome().is_some() && stability < 3.0 { 0 } else { stability.round() as u32 },
        }
    }
}

fn scenario_path(id: &str) -> std::path::PathBuf {
    // CARGO_MANIFEST_DIR is baked in at build time and points at the machine that compiled this.
    // In a container that path does not exist, so the scenarios have to be findable at runtime.
    let root = match std::env::var("VITALS_SCENARIOS") {
        Ok(d) => std::path::PathBuf::from(d),
        Err(_) => std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
    };
    match id {
        "ep2" => root.join("demo/scenarios/ep2-stemi.json"),
        "ep3" => root.join("demo/scenarios/ep3-epiglottitis.json"),
        "ep4" => root.join("demo/scenarios/ep4-pulmonary-embolism.json"),
        "ep5" => root.join("demo/scenarios/ep5-the-night-the-stars-fell.json"),
        _ => root.join("conformance/sce-anaphylaxis-ep1.json"),
    }
}

fn title(id: &str) -> &'static str {
    match id {
        "ep2" => "EP2 · Time Is Muscle",
        "ep3" => "EP3 · Don't Make Him Cry",
        "ep4" => "EP4 · The Masquerader",
        "ep5" => "EP5 · The Night the Stars Fell",
        _ => "EP1 · The Last Bite",
    }
}

fn difficulty(ep: &str) -> Difficulty {
    match ep {
        "ep2" => Difficulty::Intern,
        "ep3" | "ep4" | "ep5" => Difficulty::Resident,
        _ => Difficulty::Student,
    }
}

fn new_session(ep: &str) -> Result<Session, String> {
    let sce_json = std::fs::read_to_string(scenario_path(ep)).map_err(|e| e.to_string())?;
    let sce = Sce::from_json(&sce_json).map_err(|e| e.to_string())?;
    Ok(Session {
        ep: ep.to_string(),
        state: SceState::new(sce),
        tape: Vec::new(),
        beats: Vec::new(),
        sce_json,
        scenario: title(ep).to_string(),
        difficulty: difficulty(ep),
        anchored: false,
        said: Vec::new(),
        saved_at: None,
    })
}

fn json(v: impl Serialize) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = serde_json::to_string(&v).unwrap_or_else(|_| "{}".into());
    Response::from_string(body)
        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap())
}

fn param(url: &str, key: &str) -> Option<String> {
    url.split_once('?')?.1.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then(|| percent_decode(v))
    })
}

/// Enough percent-decoding for a typed clinical order. No dependency for this.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => {
                out.push(' ');
                i += 1;
            }
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    Ok(c) => {
                        out.push(c as char);
                        i += 3;
                    }
                    Err(_) => {
                        out.push('%');
                        i += 1;
                    }
                }
            }
            c => {
                out.push(c as char);
                i += 1;
            }
        }
    }
    out
}

/// Endpoints that spend something — the server's signature, or the GPU.
///
/// Playing is open because a kiosk should just work. Signing a transaction on request is not,
/// and "whoever can reach the port" is not an authorisation model.
fn guarded(path: &str) -> bool {
    matches!(path, "/api/anchor" | "/api/claim" | "/api/say")
}

fn bearer_ok(req: &tiny_http::Request, token: &Option<String>) -> bool {
    let Some(want) = token else { return true };
    req.headers()
        .iter()
        .find(|h| h.field.equiv("authorization"))
        .map(|h| h.value.as_str().trim())
        .map(|v| v.strip_prefix("Bearer ").unwrap_or(v) == want)
        .unwrap_or(false)
}

fn main() {
    // Not 8090. On the machine this is developed on that port is already three things — a
    // syn-sentry web app, an eir-fhir container publishing on the host, and the service port the
    // hermodr fleet uses inside the cluster. A default that collides with the neighbours is a
    // default that fails on the one day nobody is watching.
    let addr = std::env::var("VITALS_WEB_BIND").unwrap_or_else(|_| "127.0.0.1:8474".into());
    let token = std::env::var("VITALS_TOKEN").ok().filter(|s| !s.is_empty());
    let loopback = addr.starts_with("127.") || addr.starts_with("localhost");
    if !loopback && token.is_none() {
        // Refusing to start is the only honest option. Bound to a public interface with no token,
        // anyone who finds the port can make this process sign transactions with its key.
        eprintln!("refusing to bind {addr} without VITALS_TOKEN — anyone reaching it could make \
                   this server sign with its key. Set VITALS_TOKEN, or bind to 127.0.0.1.");
        std::process::exit(2);
    }
    let server = match Server::http(&addr) {
        Ok(s) => s,
        // The raw panic said "Address already in use" and nothing about who has it.
        Err(e) => {
            eprintln!("cannot bind {addr}: {e}\n\
                       something else is on that port — try `lsof -nP -iTCP:{port} -sTCP:LISTEN`, \
                       or set VITALS_WEB_BIND to a free one.",
                      port = addr.rsplit(':').next().unwrap_or("?"));
            std::process::exit(2);
        }
    };

    let state_dir = std::env::var("VITALS_STATE_DIR").unwrap_or_else(|_| "state".into());
    let store = store::Store::open(std::path::PathBuf::from(&state_dir))
        .unwrap_or_else(|e| panic!("cannot open {state_dir}: {e}"));
    // A run nobody has touched in a day is a closed tab, not a patient.
    let swept = store.sweep(SESSIONS, std::time::Duration::from_secs(24 * 60 * 60));

    let mut restored = HashMap::new();
    let mut broken = 0usize;
    for (id, saved) in store.list::<Saved>(SESSIONS) {
        match Session::restore(saved) {
            Ok(s) => {
                restored.insert(id, s);
            }
            Err(e) => {
                // Loud, and dropped. A run that will not replay is exactly the thing this repo
                // must not paper over: it means the tape and the automaton disagree.
                eprintln!("dropping session {id}: {e}");
                store.del(SESSIONS, &id);
                broken += 1;
            }
        }
    }
    // Ids are handed out in order and must not be reissued over a restored run.
    let mut next_id = restored
        .keys()
        .filter_map(|k| k.strip_prefix('s').and_then(|n| n.parse::<u64>().ok()))
        .max()
        .unwrap_or(0);
    println!(
        "state      {} · {} run(s) resumed{}{}",
        store.root().display(),
        restored.len(),
        if swept > 0 { format!(" · {swept} expired") } else { String::new() },
        if broken > 0 { format!(" · {broken} unreplayable") } else { String::new() },
    );
    let sessions: Arc<Mutex<HashMap<String, Session>>> = Arc::new(Mutex::new(restored));

    let story = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../demo/ep1-en.json");
    let patient = patient::Patient::connect(&story);
    match &patient {
        Some(p) => println!("patient    {} — local model via Heimdall", p.name()),
        None => println!("patient    no gateway — set HEIMDALL_API_KEY to give her a voice"),
    }

    let chain = chain::Chain::connect();
    // Resume the tree this server was filling. Starting a new one every boot would strand every
    // leaf already anchored, because the proof is built from the leaf list and the old list is
    // what those leaves were anchored into.
    let tree = Arc::new(Mutex::new(store.get::<Tree>(TREE, "current").unwrap_or_default()));
    match &chain {
        Some(c) => {
            let mut t = tree.lock().unwrap();
            if t.tree_id == 0 {
                t.tree_id = c.slot();
            }
            println!("chain      connected · slot {} · tree #{} · {} leaf/leaves",
                     c.slot(), t.tree_id, t.leaves.len());
            println!("relay      {} — pays fees, holds no player key", c.relay_pubkey());
        }
        None => println!("chain      not connected — set VITALS_PROGRAM_ID and start a validator to anchor"),
    }
    // Signed halves waiting on the browser, keyed by player. Never persisted: a blockhash goes
    // stale in about a minute, so a pending transaction that outlives the process is worthless.
    let pendings: Arc<Mutex<HashMap<String, PendingWork>>> = Arc::new(Mutex::new(HashMap::new()));

    match (&token, loopback) {
        (Some(_), _) => println!("auth       bearer token required on anchor · claim · say"),
        (None, true) => println!("auth       none — loopback only, so the blast radius is this machine"),
        (None, false) => unreachable!("refused to start above"),
    }
    println!("Vitals — play at http://{addr}");

    // One slow local model, and /api/say holds a worker for as long as it takes. Without a
    // ceiling a single caller can occupy the GPU indefinitely.
    let mut said: Vec<Instant> = Vec::new();
    const SAY_PER_MIN: usize = 20;

    for req in server.incoming_requests() {
        let url = req.url().to_string();
        let path = url.split('?').next().unwrap_or("/").to_string();

        if guarded(&path) && !bearer_ok(&req, &token) {
            let _ = req.respond(
                Response::from_string(r#"{"error":"unauthorised"}"#)
                    .with_status_code(401)
                    .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap()),
            );
            continue;
        }
        if path == "/api/say" {
            said.retain(|t| t.elapsed() < Duration::from_secs(60));
            if said.len() >= SAY_PER_MIN {
                let _ = req.respond(json(serde_json::json!({ "error": "too many questions — give her a moment" })));
                continue;
            }
            said.push(Instant::now());
        }

        let resp = match (req.method(), path.as_str()) {
            (Method::Get, "/") => {
                // The page is served by the same process that holds the token, so handing it over
                // does not widen anything: reaching the page and reaching the API are one boundary.
                let page = match &token {
                    Some(tk) => PAGE.replace("__VITALS_TOKEN__", tk),
                    None => PAGE.replace("__VITALS_TOKEN__", ""),
                };
                let _ = req.respond(
                    Response::from_string(page).with_header(
                        Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..])
                            .unwrap(),
                    ),
                );
                continue;
            }
            // ── film ────────────────────────────────────────────────────────────
            // Story Mode does not show a still and call it a patient: it loops a per-state clip
            // and cuts to a full-frame cutscene on a beat. Both are already rendered.
            (Method::Get, p) if p.starts_with("/clip/") => {
                let name = p.trim_start_matches("/clip/");
                // Nothing but a bare clip name — no traversal into the rest of the disk.
                let safe = name
                    .strip_suffix(".mp4")
                    .filter(|n| n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
                match safe.and_then(|n| std::fs::read(clips_dir().join(format!("{n}.mp4"))).ok()) {
                    Some(bytes) => {
                        let _ = req.respond(
                            Response::from_data(bytes)
                                .with_header(Header::from_bytes(&b"Content-Type"[..], &b"video/mp4"[..]).unwrap())
                                .with_header(Header::from_bytes(&b"Cache-Control"[..], &b"public, max-age=86400"[..]).unwrap()),
                        );
                        continue;
                    }
                    None => Response::from_string("no such clip").with_status_code(404),
                }
            }
            (Method::Get, p) if p.starts_with("/img/") => {
                let key = p.trim_start_matches("/img/").trim_end_matches(".jpg");
                match STILLS.iter().find(|(k, _)| *k == key) {
                    Some((_, bytes)) => {
                        let _ = req.respond(
                            Response::from_data(*bytes)
                                .with_header(Header::from_bytes(&b"Content-Type"[..], &b"image/jpeg"[..]).unwrap())
                                .with_header(Header::from_bytes(&b"Cache-Control"[..], &b"public, max-age=86400"[..]).unwrap()),
                        );
                        continue;
                    }
                    None => Response::from_string("no such still").with_status_code(404),
                }
            }
            (Method::Get, p @ ("/device/monitor" | "/device/vent" | "/device/pump")) => {
                let page = match p {
                    "/device/vent" => VENT,
                    "/device/pump" => PUMP,
                    _ => MONITOR,
                };
                let _ = req.respond(Response::from_string(page).with_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap(),
                ));
                continue;
            }
            (Method::Get, "/device/vitals") => {
                // The monitor identifies the bed by header, the way the device page already does.
                let sid = req
                    .headers()
                    .iter()
                    .find(|h| h.field.equiv("x-embla-session"))
                    .map(|h| h.value.as_str().to_string())
                    .unwrap_or_default();
                let map = sessions.lock().unwrap();
                match map.get(&sid) {
                    None => json(serde_json::json!({})),
                    Some(s) => {
                        let v = s.state.vitals;
                        let r = v.rhythm;
                        json(serde_json::json!({
                            "hr": v.hr, "spo2": v.spo2, "sbp": v.sbp, "dbp": v.dbp,
                            "rr": v.rr, "temp": v.temp, "gcs": v.gcs,
                            "status": format!("{:?}", s.state.status),
                            "rhythm": r.as_str(),
                            // A monitor that invents a pulse is worse than one that misses an
                            // arrest, so this comes from the rhythm rather than from the numbers.
                            "pulse": r.perfusing(),
                            "shockable": r.shockable(),
                            "paused": false,
                        }))
                    }
                }
            }
            (Method::Get, "/api/new") => {
                let ep = param(&url, "ep").unwrap_or_else(|| "ep1".into());
                match new_session(&ep) {
                    Ok(s) => {
                        next_id += 1;
                        let id = format!("s{next_id}");
                        let view = s.view();
                        let mut map = sessions.lock().unwrap();
                        map.insert(id.clone(), s);
                        persist(&store, &id, map.get_mut(&id).expect("just inserted"), true);
                        drop(map);
                        json(serde_json::json!({ "id": id, "view": view }))
                    }
                    Err(e) => json(serde_json::json!({ "error": e })),
                }
            }
            (Method::Get, "/api/step") => {
                let id = param(&url, "id").unwrap_or_default();
                let mut map = sessions.lock().unwrap();
                match map.get_mut(&id) {
                    None => json(serde_json::json!({ "error": "no such session" })),
                    Some(s) => {
                        let acted = param(&url, "do").is_some();
                        if let Some(act) = param(&url, "do") {
                            let emitted = s.state.apply(&act);
                            s.beats.extend(emitted.iter().map(render_beat));
                            s.tape.push(Step::Do(act));
                        }
                        if let Some(dt) = param(&url, "tick").and_then(|v| v.parse::<f64>().ok()) {
                            let emitted = s.state.tick(dt);
                            s.beats.extend(emitted.iter().map(render_beat));
                            s.tape.push(Step::Tick(dt));
                        }
                        let v = s.view();
                        persist(&store, &id, s, acted);
                        json(v)
                    }
                }
            }
            // ── the kit ─────────────────────────────────────────────────────────
            // Attaching a device is not a free-text order. It is a pick from a catalogue with a
            // setting, and the flowmeter has to read what the learner actually chose — the same
            // shape Embla's device tray uses, so the chart and a debrief can quote the number.
            (Method::Get, "/api/kit") => {
                let id = param(&url, "id").unwrap_or_default();
                let dev = param(&url, "dev").unwrap_or_default();
                let set = param(&url, "set").and_then(|v| v.parse::<f64>().ok());
                let off = param(&url, "off").is_some();
                let mut map = sessions.lock().unwrap();
                match map.get_mut(&id) {
                    None => json(serde_json::json!({ "error": "no such session" })),
                    Some(s) => {
                        if off {
                            s.state.detach(&dev);
                            s.tape.push(Step::Off(dev.clone()));
                        } else if s.state.has_equipment(&dev)
                            && (set.is_none() || s.state.equipment_setting(&dev) == set)
                        {
                            // Already on, at that number. Re-picking it is not a second dose —
                            // and re-running the intervention would re-attach at the scenario's
                            // canonical setting, so the chart would log a change that never
                            // happened, then log changing it back.
                        } else if s.state.has_equipment(&dev) {
                            // On already, different number: turn the dial, do not re-dose.
                            if let Some(v) = set {
                                s.state.attach(&dev, Some(v));
                                s.tape.push(Step::Set(dev.clone(), v));
                            }
                        } else if let Some(phrase) = kit_phrase(&dev, set) {
                            // Go through the matcher, so the physiology moves exactly as it would
                            // for someone who typed it. The picker is a convenience, not a bypass.
                            let emitted = s.state.apply(&phrase);
                            s.beats.extend(emitted.iter().map(render_beat));
                            s.tape.push(Step::Do(phrase));
                            // Then correct the reading to what was actually dialled in. attach()
                            // records it too, so the chart quotes the learner's number rather
                            // than the scenario's canonical dose.
                            if let Some(v) = set {
                                if s.state.has_equipment(&dev) && s.state.equipment_setting(&dev) != Some(v) {
                                    s.state.attach(&dev, Some(v));
                                    // On the tape too. Without this the correction lived only in
                                    // this process: the player saw 6 L/min, the tape replayed to
                                    // the scenario's 10, and the leaf certified the wrong run.
                                    s.tape.push(Step::Set(dev.clone(), v));
                                }
                            }
                        }
                        let v = s.view();
                        persist(&store, &id, s, true);
                        json(v)
                    }
                }
            }
            (Method::Get, "/api/tape") => {
                let id = param(&url, "id").unwrap_or_default();
                let map = sessions.lock().unwrap();
                match map.get(&id) {
                    None => json(serde_json::json!({ "error": "no such session" })),
                    // The tape in the same shape vitals-replay takes, so a player can hand it to
                    // someone else and have the leaf re-derived off this machine entirely.
                    Some(s) => json(serde_json::json!({
                        "scenario": s.scenario,
                        "sce_hash": hex(&sce_hash(&s.sce_json)),
                        "tape": s.tape.iter().map(|st| match st {
                            Step::Tick(dt) => serde_json::json!({"tick": dt}),
                            Step::Do(t) => serde_json::json!({"do": t}),
                            Step::Ask(t) => serde_json::json!({"ask": t}),
                            Step::Set(id, v) => serde_json::json!({"set": id, "to": v}),
                            Step::Off(id) => serde_json::json!({"off": id}),
                        }).collect::<Vec<_>>()
                    })),
                }
            }
            (Method::Get, "/api/say") => {
                let id = param(&url, "id").unwrap_or_default();
                let q = param(&url, "q").unwrap_or_default();
                let Some(pt) = patient.as_ref() else {
                    let _ = req.respond(json(serde_json::json!({ "error": "no gateway — she has no voice here" })));
                    continue;
                };
                // Snapshot what the model needs, then release the lock: a local 26B reply takes
                // seconds and the tick loop must not block behind it.
                let (hist, status, spo2) = {
                    let mut map = sessions.lock().unwrap();
                    let Some(s) = map.get_mut(&id) else {
                        let _ = req.respond(json(serde_json::json!({ "error": "no such session" })));
                        continue;
                    };
                    // The question goes on the tape. The answer never will.
                    s.tape.push(Step::Ask(q.clone()));
                    (s.said.clone(), format!("{:?}", s.state.status), s.state.vitals.spo2)
                };
                match pt.say(&q, &hist, &status, spo2) {
                    Ok(reply) => {
                        let mut map = sessions.lock().unwrap();
                        if let Some(s) = map.get_mut(&id) {
                            s.said.push(("user".into(), q));
                            s.said.push(("assistant".into(), reply.clone()));
                            persist(&store, &id, s, true);
                        }
                        json(serde_json::json!({ "reply": reply, "who": pt.name() }))
                    }
                    Err(e) => json(serde_json::json!({ "error": e })),
                }
            }
            (Method::Get, "/api/chain") => {
                let t = tree.lock().unwrap();
                let who = param(&url, "player").and_then(|p| pubkey(&p));
                json(serde_json::json!({
                    "connected": chain.is_some(),
                    "voice": patient.is_some(),
                    "tree_id": t.tree_id,
                    "anchored": t.leaves.len(),
                    "relay": chain.as_ref().map(|c| c.relay_pubkey()),
                    // How many of the tree's leaves are *this* player's. Before the relay split
                    // there was one number here because there was one identity.
                    "proven": match (chain.as_ref(), who) {
                        (Some(c), Some(k)) => Some(c.proven_count(&k, t.tree_id)),
                        _ => None,
                    },
                }))
            }
            (Method::Get, "/api/anchor") => {
                let id = param(&url, "id").unwrap_or_default();
                let Some(c) = chain.as_ref() else {
                    let _ = req.respond(json(serde_json::json!({ "error": "no chain connected" })));
                    continue;
                };
                let mut map = sessions.lock().unwrap();
                let Some(s) = map.get_mut(&id) else {
                    let _ = req.respond(json(serde_json::json!({ "error": "no such session" })));
                    continue;
                };
                if s.state.outcome().is_none() {
                    let _ = req.respond(json(serde_json::json!({ "error": "the run has not finished" })));
                    continue;
                }
                if s.anchored {
                    let _ = req.respond(json(serde_json::json!({ "error": "already anchored" })));
                    continue;
                }
                // Rebuild the run from the tape through the shared reducer rather than from the
                // live session, so what gets anchored is exactly what a verifier would recompute.
                let r = match replay(&s.sce_json, &s.tape) {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = req.respond(json(serde_json::json!({ "error": e })));
                        continue;
                    }
                };
                // Whose run this is. It arrives from the browser and it is the key the browser
                // will sign with — the server has no way to produce that signature, which is what
                // makes the credential the player's rather than the server's.
                let Some(who) = param(&url, "player").and_then(|p| pubkey(&p)) else {
                    let _ = req.respond(json(serde_json::json!({ "error": "no player key" })));
                    continue;
                };
                let sce = sce_hash(&s.sce_json);
                let rec: AttemptRecord =
                    match record_for(who.to_bytes(), sce, sce, s.difficulty, false, &s.tape, &r) {
                        Ok(rec) => rec,
                        Err(e) => {
                            let _ = req.respond(json(serde_json::json!({ "error": e })));
                            continue;
                        }
                    };
                let mut t = tree.lock().unwrap();
                t.leaves.push(rec.leaf());
                let tree_id = t.tree_id;
                let leaves = t.leaves.clone();
                let index = leaves.len() as u64 - 1;
                drop(t);
                match c.prepare_anchor(&who, tree_id, &rec, &leaves) {
                    Ok(p) => {
                        let msg = hex_bytes(&p.message());
                        pendings.lock().unwrap().insert(
                            who.to_string(),
                            PendingWork { pending: p, session: id.clone(), index, score: rec.score(), level: None },
                        );
                        json(serde_json::json!({ "sign": msg }))
                    }
                    Err(e) => {
                        // Building it failed, so the leaf was never anchored. Take it back off the
                        // list or every later proof is built against a tree that does not exist.
                        tree.lock().unwrap().leaves.pop();
                        json(serde_json::json!({ "error": e }))
                    }
                }
            }
            (Method::Get, "/api/claim") => {
                let level: u8 = param(&url, "level").and_then(|v| v.parse().ok()).unwrap_or(2);
                let Some(c) = chain.as_ref() else {
                    let _ = req.respond(json(serde_json::json!({ "error": "no chain connected" })));
                    continue;
                };
                let Some(who) = param(&url, "player").and_then(|p| pubkey(&p)) else {
                    let _ = req.respond(json(serde_json::json!({ "error": "no player key" })));
                    continue;
                };
                let tree_id = tree.lock().unwrap().tree_id;
                match c.prepare_claim(&who, tree_id, level) {
                    Ok(p) => {
                        let msg = hex_bytes(&p.message());
                        pendings.lock().unwrap().insert(
                            who.to_string(),
                            PendingWork { pending: p, session: String::new(), index: 0, score: 0, level: Some(level) },
                        );
                        json(serde_json::json!({ "sign": msg }))
                    }
                    Err(e) => json(serde_json::json!({ "error": e })),
                }
            }
            // The other half. The browser signed the bytes we handed it; we drop the signature
            // into its slot and send. If it does not verify, nothing is sent.
            (Method::Get, "/api/submit") => {
                let Some(c) = chain.as_ref() else {
                    let _ = req.respond(json(serde_json::json!({ "error": "no chain connected" })));
                    continue;
                };
                let Some(who) = param(&url, "player").and_then(|p| pubkey(&p)) else {
                    let _ = req.respond(json(serde_json::json!({ "error": "no player key" })));
                    continue;
                };
                let Some(sig) = param(&url, "sig").and_then(|h| sig64(&h)) else {
                    let _ = req.respond(json(serde_json::json!({ "error": "no signature" })));
                    continue;
                };
                let Some(work) = pendings.lock().unwrap().remove(&who.to_string()) else {
                    let _ = req.respond(json(serde_json::json!({ "error": "nothing waiting to be signed" })));
                    continue;
                };
                let tx = match work.pending.signed(&sig) {
                    Ok(tx) => tx,
                    Err(e) => {
                        if work.level.is_none() {
                            tree.lock().unwrap().leaves.pop();
                        }
                        let _ = req.respond(json(serde_json::json!({ "error": e })));
                        continue;
                    }
                };
                match (c.submit(&tx), work.level) {
                    (Ok(()), Some(_)) => match c.claimed(&who) {
                        Ok(m) => json(serde_json::json!({ "granted": true, "message": m })),
                        Err(m) => json(serde_json::json!({ "granted": false, "message": m })),
                    },
                    (Ok(()), None) => {
                        // The tree really changed, so write it before anything else can fail.
                        let t = tree.lock().unwrap();
                        let _ = store.put(TREE, "current", &*t);
                        drop(t);
                        let mut map = sessions.lock().unwrap();
                        if let Some(s) = map.get_mut(&work.session) {
                            s.anchored = true;
                            persist(&store, &work.session, s, true);
                        }
                        drop(map);
                        // Read the id out and let go of the lock. A `tree.lock()` written inside
                        // the match scrutinee stays alive for the whole match, so taking it again
                        // in an arm deadlocks the one thread this server has — which is not a slow
                        // request, it is every request from then on.
                        let tree_id = tree.lock().unwrap().tree_id;
                        match c.anchored(&who, tree_id, work.index) {
                            Ok(a) => json(serde_json::json!({
                                "index": a.index, "root": a.root, "leaves": a.leaves,
                                "proven": a.proven, "score": work.score,
                                "counted": c.proven_count(&who, tree_id),
                            })),
                            Err(e) => json(serde_json::json!({ "error": e })),
                        }
                    }
                    (Err(e), Some(_)) => json(serde_json::json!({ "granted": false, "message": e })),
                    (Err(e), None) => {
                        // Nothing landed on chain, so the leaf is not in the tree.
                        tree.lock().unwrap().leaves.pop();
                        json(serde_json::json!({ "error": e }))
                    }
                }
            }
            _ => Response::from_string("not found").with_status_code(404),
        };
        let _ = req.respond(resp);
    }
}

/// What the browser is waiting to sign, and what to do once it has.
struct PendingWork {
    pending: chain::Pending,
    /// Which run this anchors. Empty for a claim.
    session: String,
    index: u64,
    score: u32,
    /// Set for a claim, `None` for an anchor.
    level: Option<u8>,
}

/// A player key as the browser sends it: base58, and it has to be a real curve point or the
/// transaction it is put into can never be signed.
fn pubkey(s: &str) -> Option<solana_sdk::pubkey::Pubkey> {
    let raw = bs58_to_32(s)?;
    let k = solana_sdk::pubkey::Pubkey::new_from_array(raw);
    (k.to_string() == s).then_some(k)
}

fn hex_bytes(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// A 64-byte signature as hex.
fn sig64(h: &str) -> Option<[u8; 64]> {
    if h.len() != 128 {
        return None;
    }
    let mut out = [0u8; 64];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(h.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// Decode a base58 pubkey into raw bytes.
fn bs58_to_32(s: &str) -> Option<[u8; 32]> {
    const A: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let mut out = vec![0u8; 32];
    for ch in s.bytes() {
        let mut carry = A.iter().position(|&c| c == ch)?;
        for b in out.iter_mut().rev() {
            carry += 58 * (*b as usize);
            *b = (carry & 0xff) as u8;
            carry >>= 8;
        }
        if carry != 0 {
            return None;
        }
    }
    out.try_into().ok()
}

/// The phrase a device pick sends through the intervention matcher.
///
/// One catalogue, and it carries the number: a learner who sets the flowmeter to 15 should see
/// 15 in the chart, not the scenario's canonical dose.
fn kit_phrase(dev: &str, set: Option<f64>) -> Option<String> {
    Some(match dev {
        "o2" => format!("oxygen face mask {} lpm", set.unwrap_or(10.0) as i64),
        "iv" => format!("iv access normal saline {} ml/hr", set.unwrap_or(999.0) as i64),
        "ett" => "intubate, secure the airway".to_string(),
        "supine" => "lay her flat, legs up".to_string(),
        "defib" => format!("defibrillate {} j", set.unwrap_or(200.0) as i64),
        _ => return None,
    })
}
