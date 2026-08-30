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

/// The deck is served from the same place the app is, so anyone who can reach the bay can be
/// shown the pitch without a file being emailed around.
#[test]
fn the_deck_is_served() {
    let s = Server::start();
    let html = s.get("/slides");
    assert!(html.contains("<title>"), "no page came back");
    assert!(html.len() > 100_000, "only {} bytes — that is not the deck", html.len());
    assert!(html.contains("VITALS") || html.contains("Vitals"), "this is not the Vitals deck");
}

/// The speaking script is **not** served, and this test exists so it cannot come back by
/// accident. `pitch/script.html` is the presenter's notes — what to say and what not to say in
/// front of whoever is listening — and it was compiled into the binary and answered at
/// `/slides/script` with no token in front of it. The deck is the public artefact; the notes
/// behind it are not.
#[test]
fn the_speaking_script_is_not_served() {
    let s = Server::start();
    assert_eq!(s.get("/slides/script"), "not found", "the speaking script is being served again");
    assert!(
        !s.get("/slides").contains("60-second cut"),
        "the script's own section came back on the deck route"
    );
}

/// The deck gets arrow keys when it is served, and the file on disk keeps none of it — that file
/// is regenerated by scripts, and build-pdf.sh renders it off disk where a printed deck has no
/// use for a next button.
#[test]
fn the_served_deck_presents_but_the_file_stays_clean() {
    let s = Server::start();
    let served = s.get("/slides");
    assert!(served.contains("deck-nav"), "the served deck has no navigation");
    assert!(served.contains("scroll-snap-type"), "one slide at a time is the point");

    let file = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../pitch/deck.html"),
    )
    .expect("deck on disk");
    assert!(!file.contains("deck-nav"), "presentation mode leaked into the file");
}

/// Baked into the binary rather than read from disk. Twice today a path that existed on the build
/// machine did not exist in the container — the patient could not speak and the film would not
/// play — and both looked like something else. A deck that cannot go missing cannot go missing
/// during a pitch.
#[test]
fn the_deck_does_not_depend_on_a_file_being_there() {
    let s = Server::start();
    // The test server runs with no repo around it beyond the binary itself.
    assert!(s.get("/slides").len() > 100_000);
}

// ── the policy and the terms ────────────────────────────────────────────────
//
// Both are `include_str!`'d beside the deck and the donate page, for a reason the deck already
// established the hard way: a page that can go missing in a container goes missing on the day it
// matters. For these two the day it matters is an OAuth consent screen fetching the privacy URL,
// and a physician who has just been asked for her contact details wanting to know what happens to
// them. A 404 at either is worse than a plain page.

/// Both routes answer, unguarded, with the page that belongs at them.
#[test]
fn the_policy_and_the_terms_are_served() {
    let s = Server::start();
    let privacy = s.get("/privacy");
    assert!(privacy.contains("<title>Privacy"), "no policy came back: {}", &privacy[..privacy.len().min(120)]);
    let terms = s.get("/terms");
    assert!(terms.contains("<title>Terms"), "no terms came back: {}", &terms[..terms.len().min(120)]);
}

/// The stamp is replaced on the way out, exactly as it is for the reviewer's form. A policy is a
/// claim about one build's behaviour, and a reader who cannot tell which build cannot check it.
#[test]
fn the_policy_and_the_terms_say_which_build_they_describe() {
    let s = Server::start();
    for p in ["/privacy", "/terms"] {
        let html = s.get(p);
        assert!(!html.contains("__VITALS_BUILD__"), "{p} went out with the placeholder still in it");
        assert!(html.contains("vitals 0."), "{p} does not name the build it describes");
    }
}

/// The sentences these pages exist for. Each one is the load-bearing disclosure of its page: the
/// irreversibility of an anchor, and what a score is not. A redesign that loses either has lost
/// the reason the page was written, and it must fail here rather than in front of a reader.
#[test]
fn the_pages_still_say_the_thing_they_were_written_to_say() {
    let s = Server::start();
    let privacy = s.get("/privacy");
    for must in [
        "Anchoring a run is permanent",
        "cannot delete it",
        // The anonymity option covers credit, not storage — the one thing a reviewer could
        // reasonably misread, and the reason a physician's contact details are on our disk.
        "anonymise the record",
        // What leaves the machine, named rather than implied.
        "Vertex AI",
    ] {
        assert!(privacy.contains(must), "the policy stopped saying {must:?}");
    }
    let terms = s.get("/terms");
    for must in [
        "not a clinical qualification",
        "nothing in it is advice",
        "without warranty of any kind",
        "AGPL",
    ] {
        assert!(terms.contains(must), "the terms stopped saying {must:?}");
    }
}

/// Neither page runs any script at all. Every other page here loads GA4 behind a consent gate;
/// the two that describe what we collect do not collect while they are being read. Checked as
/// "no `<script>`" rather than "no tag id", because the policy *names* googletagmanager in the
/// sentence explaining what the tag does — the substring is prose here, and the property worth
/// pinning is that nothing on these pages executes.
#[test]
fn the_policy_and_the_terms_run_no_script() {
    let s = Server::start();
    for p in ["/privacy", "/terms"] {
        let html = s.get(p);
        assert!(!html.contains("<script"), "{p} runs a script");
        assert!(!html.contains("gtag("), "{p} calls gtag");
    }
}

/// Compiled in rather than read from disk, like the deck and the donate page. The test server
/// runs with no repository around it beyond the binary itself.
#[test]
fn the_policy_does_not_depend_on_a_file_being_there() {
    let s = Server::start();
    assert!(s.get("/privacy").len() > 10_000);
    assert!(s.get("/terms").len() > 5_000);
}
