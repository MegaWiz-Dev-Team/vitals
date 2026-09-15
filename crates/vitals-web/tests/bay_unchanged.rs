//! Eternal's bay, exactly as it was, when nothing asks for a ward.
//!
//! The ward's shift page is a **parameter to this bay, never a second play surface** (producer,
//! 16 ก.ย.): opened as `/ward/<id>` it will replay her chain and begin the run from the resumed
//! state, and everything else — the patient's voice, the orders, the monitor, the tape format, the
//! scoring — is the code we already maintain. One page, one engine, one tape.
//!
//! The risk that buys is precise and this file is the guard against it. vitals.academy is the
//! Eternal entry and a judge will open it during the sprint; a change made for the ward that
//! shifted the bay's default path by one second, one vital or one beat would be a regression in
//! the thing being judged, arriving through a door marked "new feature".
//!
//! So this pins the default path **before** the parameter exists. It is deliberately a
//! characterisation test: it asserts what the bay does today, not what anyone thinks it should do,
//! and it is meant to be read as "this is what must not change" rather than as a specification.

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
        let state = std::env::temp_dir().join(format!("vitals-bay-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&state);

        let mut child = Command::new(env!("CARGO_BIN_EXE_vitals-web"))
            .env("VITALS_WEB_BIND", "127.0.0.1:0")
            .env("VITALS_STATE_DIR", &state)
            .env_remove("VITALS_PROGRAM_ID")
            .env_remove("VITALS_TOKEN")
            .env_remove("HEIMDALL_API_KEY")
            // The Eternal entry, not the ward. This is the whole point of the file: what a judge
            // opens at vitals.academy, with nothing set that the ward would set.
            .env_remove("VITALS_WORLD")
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
        let body = ureq::get(&url)
            .call()
            .map(|r| r.into_string().unwrap_or_default())
            .unwrap_or_else(|e| match e {
                ureq::Error::Status(_, r) => r.into_string().unwrap_or_default(),
                other => panic!("{url}: {other}"),
            });
        serde_json::from_str(&body).unwrap_or(serde_json::Value::Null)
    }
}

/// A run with no ward in sight begins where it has always begun.
#[test]
fn a_run_opened_the_ordinary_way_starts_at_the_beginning() {
    let s = Server::start();
    let opened = s.json("/api/new?ep=ep1");
    let id = opened["id"].as_str().expect("a session id").to_string();
    let view = &opened["view"];

    assert!(view["elapsed"].as_f64().unwrap_or(-1.0).abs() < 0.001,
            "the clock starts at zero — a resumed run is the new thing, and it must not become \
             the default by accident: {}", view["elapsed"]);
    assert_eq!(view["beats"].as_array().map(|b| b.len()).unwrap_or(9), 0,
               "and nothing has happened to her yet");
    assert_eq!(view["chart"].as_array().map(|c| c.len()).unwrap_or(9), 0,
               "her chart is empty, which is what a stay that has not begun looks like");
    assert_eq!(view["harm"].as_array().map(|h| h.len()).unwrap_or(9), 0);
    assert!(view["equipment"].as_array().map(|e| e.is_empty()).unwrap_or(false),
            "and nothing is attached to her");
    assert_eq!(view["over"], false, "nor is she finished before anyone arrives");
    assert!(view["outcome"].is_null());
    assert!(view["leaf"].is_null(), "there is nothing to anchor about a run nobody has played");
    assert_eq!(view["scenario"], "EP1 · The Last Bite", "the default case is unchanged");

    // EP1's own opening numbers, from the scenario file rather than from memory: if the bay ever
    // starts a default run from a state that is not the scenario's start, these move.
    let sce: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../conformance/sce-anaphylaxis-ep1.json"),
        )
        .expect("ep1 is in the repository"),
    )
    .expect("ep1 parses");
    for vital in ["hr", "sbp", "spo2", "rr", "temp", "gcs"] {
        let want = sce["vitals0"][vital].as_f64().expect("the scenario says so");
        let got = view[vital].as_f64().unwrap_or(f64::NAN);
        assert!((got - want).abs() < 0.5,
                "{vital} opens at {got} and the scenario says {want} — a default run must begin at \
                 the scenario's own start, whatever the ward does");
    }

    // And the tape is empty, which is what makes the leaf at the end this run's own.
    let tape = s.json(&format!("/api/tape?id={id}"));
    let steps = tape["tape"].as_array().map(|t| t.len()).unwrap_or(99);
    assert_eq!(steps, 0, "a run that has not been played has nothing on its tape");
}

/// The ward's routes do not exist on the Eternal entry, and its page is untouched.
#[test]
fn the_eternal_entry_is_not_a_ward() {
    let s = Server::start();

    let ward = s.json("/api/ward");
    assert_eq!(ward["ward"], "not on this host",
               "vitals.academy answers for the Eternal entry and points at the ward rather than \
                publishing an empty census of a thing it does not run");
    assert!(ward["census"].is_null(), "and it publishes no numbers at all");

    // The two the ward host refuses are exactly the two vitals.academy must still answer.
    let usage = s.json("/api/usage");
    assert!(usage["ward"].is_null(), "usage here is this bay's own, not a ward's excuse");
    assert!(usage["days"].is_array(), "and it is the funnel a judge is invited to read");

    let chain = s.json("/api/chain");
    assert!(chain["ward"].is_null());
    assert!(chain["connected"].is_boolean(), "the chain answers about this bay, as it always has");
}
