//! The usage counter, end to end — and the sentences that have to travel with it.
//!
//! "How many people have played?" has no honest answer here and never will: *no signup* is on the
//! front door as a feature, so there are no accounts and nothing that is a person. What this
//! endpoint counts is runs and browsers, and the failure mode it is built against is not a wrong
//! number — it is a right number quoted as a headcount. So the assertions below are half
//! arithmetic and half vocabulary, and the vocabulary half is the one that matters in a pitch.

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
    /// A server on a state directory of its own, unless one is handed in — restarting onto the
    /// same directory is how "the count survives a deploy" is tested.
    fn start_on(state: std::path::PathBuf) -> Server {
        Server::start_on_with(state, &[])
    }

    fn start_on_with(state: std::path::PathBuf, extra: &[(&str, &str)]) -> Server {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_vitals-web"));
        for (k, v) in extra {
            cmd.env(k, v);
        }
        let mut child = cmd
            .env("VITALS_WEB_BIND", "127.0.0.1:0")
            .env("VITALS_STATE_DIR", &state)
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

    fn start() -> Server {
        Server::start_on(fresh_state("usage"))
    }

    /// A server whose per-address window is wide enough to open a dozen runs in a row.
    ///
    /// The window is six a minute, which is right for the bay and too small for a test that has
    /// to try several ids. Raised through the same env var the deploy uses rather than by
    /// sending a forged `X-Forwarded-For` per request — that would work today, and only because
    /// the address is read from the first entry of a header the caller controls.
    fn start_generous() -> Server {
        Server::start_on_with(fresh_state("usage"), &[("VITALS_TURNS_PER_MIN", "600")])
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

    fn stop(self) -> std::path::PathBuf {
        let dir = self.state.clone();
        // Drop kills the child and removes the directory, so take the path and forget the guard.
        let mut me = std::mem::ManuallyDrop::new(self);
        let _ = me.child.kill();
        let _ = me.child.wait();
        dir
    }

    fn open_run(&self, ep: &str, player: &str) -> String {
        let q = if player.is_empty() { String::new() } else { format!("&player={player}") };
        self.json(&format!("/api/new?ep={ep}{q}"))["id"].as_str().expect("a session id").to_string()
    }
}

fn fresh_state(tag: &str) -> std::path::PathBuf {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!("vitals-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    p
}

/// Two real player keys. `pubkey()` on the server rejects anything that is not one, and a
/// rejected key would be counted as a keyless run — which is a different row.
const KEY_A: &str = "9FJRwWnTNQXB9ff5SSmQKytCdVYqTQQPUz1b4zX9mt8y";
const KEY_B: &str = "SysvarC1ock11111111111111111111111111111111";

#[test]
fn opening_a_run_counts_a_run_and_the_browser_that_opened_it() {
    let s = Server::start();
    let before = s.json("/api/usage");
    assert_eq!(before["runs"]["started"], 0);
    assert_eq!(before["devices"]["distinct_browsers_seen"], 0);

    s.open_run("ep1", KEY_A);
    s.open_run("osce-a", KEY_A);
    s.open_run("ep1", KEY_B);

    let u = s.json("/api/usage");
    assert_eq!(u["runs"]["started"], 3, "runs are not being counted");
    assert_eq!(u["runs"]["by_case"]["ep1"], 2);
    assert_eq!(u["runs"]["by_case"]["osce-a"], 1);
    // Three runs, two browsers. This is the whole reason the two are different fields.
    assert_eq!(u["devices"]["distinct_browsers_seen"], 2, "one browser was counted twice");
}

#[test]
fn a_run_opened_without_a_key_is_a_run_and_not_a_device() {
    let s = Server::start();
    s.open_run("ep1", "");
    let u = s.json("/api/usage");
    assert_eq!(u["runs"]["started"], 1);
    assert_eq!(u["runs"]["started_without_a_device_key"], 1);
    assert_eq!(u["devices"]["distinct_browsers_seen"], 0, "a keyless run invented a device");
}

/// The end of a run is counted once, on the transition, and it is bucketed by the case's own
/// outcome id rather than by a label invented in the web layer.
#[test]
fn a_finished_run_is_counted_once_and_by_its_own_outcome() {
    let s = Server::start();
    let id = s.open_run("ep1", KEY_A);
    // EP1 with nobody treating her: anaphylaxis untreated arrests and then kills.
    let mut ended = false;
    for _ in 0..60 {
        // The owner has to identify itself: an owned case answers only to its owner, and a
        // request without the key gets the same "no such session" a stranger would.
        let v = s.json(&format!("/api/step?id={id}&player={KEY_A}&tick=30"));
        if !v["outcome"].is_null() {
            ended = true;
            break;
        }
    }
    assert!(ended, "EP1 never reached a terminal state");

    let u = s.json("/api/usage");
    assert_eq!(u["runs"]["finished"], 1, "the end of a run was not counted");
    assert_eq!(u["runs"]["died"], 1, "an untreated anaphylaxis was recorded as a survival");
    assert_eq!(u["runs"]["survived"], 0);
    assert!(
        u["runs"]["by_outcome"].as_object().is_some_and(|m| m.values().any(|v| v == 1)),
        "the outcome bucket is empty: {}", u["runs"]["by_outcome"]
    );

    // Every further request against a finished run must not count it again.
    for _ in 0..5 {
        s.json(&format!("/api/step?id={id}&player={KEY_A}&tick=30"));
    }
    assert_eq!(
        s.json("/api/usage")["runs"]["finished"], 1,
        "a finished run is counted again on every request that touches it"
    );
}

#[test]
fn the_count_survives_a_restart() {
    let a = Server::start();
    a.open_run("ep1", KEY_A);
    a.open_run("ep1", KEY_A);
    assert_eq!(a.json("/api/usage")["runs"]["started"], 2);
    let dir = a.stop();

    let b = Server::start_on(dir);
    let u = b.json("/api/usage");
    assert_eq!(u["runs"]["started"], 2, "a deploy reset the count");
    assert_eq!(u["devices"]["distinct_browsers_seen"], 1, "a restart re-counted a known browser");
}

// ── the vocabulary ──────────────────────────────────────────────────────────
// The dangerous failure here is not arithmetic. It is a number that reads as a headcount being
// quoted in a room where nobody can check it.

#[test]
fn the_numbers_never_travel_without_what_they_cannot_mean() {
    let s = Server::start();
    s.open_run("ep1", KEY_A);
    let u = s.json("/api/usage");

    let limits = u["limits"].as_array().expect("the numbers shipped with no limits");
    assert!(limits.len() >= 5, "the limits were trimmed down to {}", limits.len());
    let all: String = limits.iter().filter_map(|l| l.as_str()).collect::<Vec<_>>().join(" ");
    for must in [
        "no signup",                    // why people cannot be counted at all
        "One machine used by many",     // the undercount, and it is the one that matters
        "phone and a laptop",           // the overcount
        "anchored on chain",            // what an outsider can actually verify
    ] {
        assert!(all.contains(must), "the limits stopped saying {must:?}: {all}");
    }
    assert!(
        u["counts"].as_str().unwrap_or_default().contains("never people"),
        "the headline caveat is gone"
    );
    // The one externally checkable figure rides along, and says it is a floor.
    assert!(u["anchored_on_chain"].is_number(), "the chain count is missing");
    assert!(
        u["anchored_on_chain_note"].as_str().unwrap_or_default().contains("floor"),
        "the opt-in caveat on the anchored count is gone"
    );
}

#[test]
fn no_field_on_this_endpoint_can_be_read_as_a_headcount() {
    let s = Server::start();
    s.open_run("ep1", KEY_A);
    let raw = serde_json::to_string(&s.json("/api/usage")).unwrap();
    for banned in ["\"users\"", "\"players\"", "\"students\"", "user_count", "\"people\":"] {
        assert!(!raw.contains(banned), "{banned} appears in the usage payload");
    }
}

/// Public and read-only. A project that sells "check it yourself" and keeps its own usage behind
/// a token is arguing against itself — and the endpoint must never accept a write.
#[test]
fn it_is_public_and_it_only_reads() {
    let s = Server::start();
    let before = s.json("/api/usage")["runs"]["started"].clone();
    for _ in 0..3 {
        s.json("/api/usage");
    }
    assert_eq!(s.json("/api/usage")["runs"]["started"], before, "reading the counter moved it");
}

/// Enough percent-encoding to put a hostile case id in a query string.
fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// A case id this server cannot play must not open a run, and must not name a bucket.
///
/// `scenario_path` answers an unknown id with EP1's file, so `?ep=<anything>` used to open a
/// real playable run of EP1 filed under whatever the caller typed. `by_case` is durable and its
/// keys came straight off the query string of a public endpoint that needs no account — so a
/// stranger could name the shelf's own statistics, and grow the one document the day table and
/// the device counts also live in. The live tally still carries `no-such-ep-at-all` from
/// somebody trying it.
#[test]
fn a_case_this_server_cannot_play_is_refused_before_it_is_counted() {
    let s = Server::start_generous();

    for bogus in ["no-such-ep-at-all", "ep6", "ep1 ", "EP1", "../../etc/passwd", &"x".repeat(4000)] {
        let r = s.json(&format!("/api/new?ep={}", enc(bogus)));
        assert!(
            r["id"].is_null(),
            "{bogus:?} opened a run: {r}"
        );
        assert_eq!(r["error"], "no such case", "{bogus:?} was refused for the wrong reason: {r}");
    }

    let v = s.json("/api/usage");
    assert_eq!(v["runs"]["started"], 0, "a refused case was counted as a run: {v}");
    assert_eq!(
        v["runs"]["by_case"].as_object().map(|o| o.len()),
        Some(0),
        "a refused case named a bucket: {}",
        v["runs"]["by_case"]
    );

    // And the ids that are real still open.
    for good in ["ep1", "ep2", "osce-a"] {
        let r = s.json(&format!("/api/new?ep={good}"));
        assert!(r["id"].is_string(), "{good} stopped opening: {r}");
    }
}
