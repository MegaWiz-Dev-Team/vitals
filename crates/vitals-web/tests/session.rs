//! A case in progress belongs to whoever started it.
//!
//! It did not. Sessions were handed out as `s1`, `s2`, `s3` with no owner recorded, and every
//! route that touches one looked the session up by id and did as it was told. On a server two
//! people can reach, that means a stranger can type `s7` and give somebody else's patient an
//! order — and because the tape is what gets hashed and anchored, the harm they cause is
//! permanent and lands on the other person's record.
//!
//! No chain is needed for any of this: the server plays perfectly well without one.

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
        // Port 0: the OS picks one nobody else has, and the server reports back what it got.
        // Tests run in parallel, so a fixed port would make them collide with each other and
        // with whatever the developer happens to have running.
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let state = std::env::temp_dir().join(format!("vitals-test-{}-{n}", std::process::id()));
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

        // Owned before anything can fail. A panic between spawn and here leaks a live server,
        // which is how the first draft of this file left processes running.
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

    fn get(&self, path: &str) -> String {
        let url = format!("http://127.0.0.1:{}{path}", self.port);
        ureq::get(&url).call().map(|r| r.into_string().unwrap_or_default()).unwrap_or_else(|e| {
            match e {
                ureq::Error::Status(_, r) => r.into_string().unwrap_or_default(),
                other => panic!("{url}: {other}"),
            }
        })
    }

    fn json(&self, path: &str) -> serde_json::Value {
        serde_json::from_str(&self.get(path)).unwrap_or(serde_json::Value::Null)
    }

    fn new_case(&self, player: &str) -> String {
        let q = if player.is_empty() { String::new() } else { format!("&player={player}") };
        self.json(&format!("/api/new?ep=ep1{q}"))["id"].as_str().expect("a session id").to_string()
    }
}

const A: &str = "AStZxZ8XgH9nSKarLT4MzUrY8HM5LtExzDaN9SDoaKiq";
const B: &str = "3yHJZLgfBs2KWiRnJx65YejtFtkbSdJCezJEYGMcyYMZ";

/// The one that matters. A stranger must not be able to touch a case they did not open.
#[test]
fn a_stranger_cannot_drive_someone_elses_case() {
    let s = Server::start();
    let id = s.new_case(A);

    // Standing an anaphylaxis patient up is the scenario's own harm event, and it is permanent:
    // it goes on the tape, and the tape is what gets hashed.
    let hijack = s.json(&format!("/api/step?id={id}&player={B}&do=let%20her%20stand%20up"));
    assert!(hijack["error"].is_string(), "a stranger drove the case: {hijack}");

    let tape = s.json(&format!("/api/tape?id={id}&player={A}"));
    let steps = tape["tape"].as_array().map(|a| a.len()).unwrap_or(0);
    assert_eq!(steps, 0, "the stranger's order reached the tape: {tape}");
}

/// Every route that reaches into a session has to ask, not just the one.
#[test]
fn no_route_lets_a_stranger_in_by_the_side() {
    let s = Server::start();
    let id = s.new_case(A);
    for path in [
        format!("/api/step?id={id}&player={B}&tick=30"),
        format!("/api/kit?id={id}&player={B}&dev=o2&set=6"),
        format!("/api/tape?id={id}&player={B}"),
        format!("/api/say?id={id}&player={B}&q=hello"),
    ] {
        let r = s.json(&path);
        assert!(r["error"].is_string(), "{path} let a stranger in: {r}");
    }
}

/// The owner still plays, obviously.
#[test]
fn the_person_who_opened_it_can_drive_it() {
    let s = Server::start();
    let id = s.new_case(A);
    let v = s.json(&format!("/api/step?id={id}&player={A}&do=adrenaline%20im"));
    assert!(v["error"].is_null(), "the owner was refused: {v}");
    let tape = s.json(&format!("/api/tape?id={id}&player={A}"));
    assert_eq!(tape["tape"].as_array().map(|a| a.len()), Some(1));
}

/// A browser that cannot make a key — or a kiosk nobody signed into — still gets to play. The
/// session id itself has to be the secret in that case, so it cannot be a counter.
#[test]
fn an_anonymous_case_still_plays_but_its_id_is_not_guessable() {
    let s = Server::start();
    let id = s.new_case("");
    let v = s.json(&format!("/api/step?id={id}&do=adrenaline%20im"));
    assert!(v["error"].is_null(), "anonymous play was refused: {v}");

    assert!(id.len() >= 22, "id `{id}` is too short to be unguessable");
    assert!(!id.starts_with('s') || id.len() > 4, "`{id}` looks like a counter");
}

/// Two cases opened in a row must not have ids anybody can walk between.
#[test]
fn ids_are_not_a_sequence() {
    let s = Server::start();
    let ids: Vec<String> = (0..4).map(|_| s.new_case(A)).collect();
    for w in ids.windows(2) {
        assert_ne!(w[0], w[1], "ids repeat");
    }
    // A counter is the failure: s1, s2, s3 differ only in the last character.
    let sequential = ids.windows(2).all(|w| {
        w[0].len() == w[1].len() && w[0][..w[0].len() - 1] == w[1][..w[1].len() - 1]
    });
    assert!(!sequential, "ids are a counter: {ids:?}");
}

/// Guessing a session that does not exist must read the same as guessing one that does but is
/// not yours — otherwise the error message is an oracle for finding live cases.
#[test]
fn a_wrong_owner_and_a_missing_session_look_the_same() {
    let s = Server::start();
    let id = s.new_case(A);
    let theirs = s.json(&format!("/api/step?id={id}&player={B}&tick=1"));
    let absent = s.json(&format!("/api/step?id=nosuchsessionatall&player={B}&tick=1"));
    assert_eq!(theirs["error"], absent["error"], "the error tells a guesser which ids are live");
}

/// NEWS2 is an *early warning* score: it exists to decide whether somebody needs to come, and
/// how fast. Once a patient has died there is nothing left to warn about, and printing
/// "15 · emergency response" beside "Dead" is the same category of nonsense as a heart rate on a
/// corpse — which is the bug it appeared next to.
#[test]
fn a_dead_patient_is_not_given_an_early_warning_score() {
    let s = Server::start();
    let id = s.new_case(A);
    s.json(&format!("/api/step?id={id}&player={A}&do=let%20her%20stand%20up"));

    let mut view = serde_json::Value::Null;
    for _ in 0..30 {
        view = s.json(&format!("/api/step?id={id}&player={A}&tick=30"));
        if !view["outcome"].is_null() {
            break;
        }
    }
    let outcome = view["outcome"].as_str().unwrap_or("").to_string();
    assert!(outcome.starts_with("Death"), "the run did not end in a death: {outcome}");

    assert!(view["news"].is_null(), "a dead patient was given a NEWS2 of {}", view["news"]);
    // And the vitals it would have been computed from are gone too.
    assert_eq!(view["hr"], 0.0);
    assert_eq!(view["rr"], 0.0);
}

/// A patient who lives still gets one — the rule is about death, not about the run being over.
#[test]
fn a_patient_who_survives_still_has_a_score() {
    let s = Server::start();
    let id = s.new_case(A);
    for o in ["adrenaline im", "oxygen", "supine"] {
        s.json(&format!("/api/step?id={id}&player={A}&do={}", o.replace(' ', "%20")));
    }
    s.json(&format!("/api/step?id={id}&player={A}&tick=60"));
    s.json(&format!("/api/step?id={id}&player={A}&do=normal%20saline%20bolus"));
    s.json(&format!("/api/step?id={id}&player={A}&tick=300"));
    s.json(&format!("/api/step?id={id}&player={A}&do=admit%20for%20observation"));
    let mut view = serde_json::Value::Null;
    for _ in 0..10 {
        view = s.json(&format!("/api/step?id={id}&player={A}&tick=300"));
        if !view["outcome"].is_null() {
            break;
        }
    }
    assert!(view["outcome"].as_str().unwrap_or("").starts_with("Win"), "{}", view["outcome"]);
    assert!(!view["news"].is_null(), "a discharged patient should still have a score");
}
