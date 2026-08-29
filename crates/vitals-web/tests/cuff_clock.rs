//! The bedside cuff and the bay have to be telling the same story about the same patient.
//!
//! At the instant the bay's own strip read `58/38`, the bedside pane read `92/60` — the pressure
//! the patient walked in with, never re-measured — and under it, "· 24 s ago", counting a
//! different clock from the `0:56` at the top of the same screen. The cause was one expression:
//! the cuff re-cycled on `Date.now() - nibpAt > 180000`, three minutes of **wall** time, while
//! the case runs several times faster than the wall (the bay ticks 2–3 scenario seconds every
//! 700 ms). An eight-minute station therefore almost never re-cycled the cuff at all.
//!
//! A NIBP cuff genuinely is a periodic snapshot and stays one here. What changed is the clock it
//! is dated on: `pump.html` already reads the scenario clock off this same feed and says why —
//! *the pump's clock is the scenario's clock, not the browser's* — and the monitor now reads the
//! same field, so the two panes cannot drift apart from each other or from the bay.
//!
//! Nothing type-checks these pages, so the checks are on the text of the file, the way
//! `arrest.rs` already checks the renderers.

use std::path::PathBuf;

fn page(name: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static").join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// The expression that caused it, by name. A cuff on the wall clock in a case that does not run
/// on the wall clock is the whole defect.
#[test]
fn the_cuff_no_longer_dates_its_reading_on_the_browsers_clock() {
    let html = page("device/monitor.html");
    let code: String = html
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !code.contains("Date.now() - nibpAt"),
        "the cuff re-cycles on wall time again — an eight-minute station never re-measures"
    );
    assert!(
        !code.contains("nibpAt = Date.now()") && !code.contains("nibpAt=Date.now()"),
        "a cuff reading is stamped with wall time again — its age will not match the bay's clock"
    );
}

/// …and it counts the one the rest of the station counts.
#[test]
fn the_cuff_counts_the_scenario_clock_the_feed_carries() {
    let html = page("device/monitor.html");
    assert!(html.contains("V.t_sec"), "the monitor reads no scenario clock off the feed");
    assert!(
        html.contains("const CUFF_CYCLE_SEC = 180"),
        "the cuff cycle is no longer a named number of scenario seconds"
    );
    assert!(
        html.contains("now - nibpAt >= CUFF_CYCLE_SEC"),
        "the cuff no longer cycles on elapsed scenario seconds"
    );
}

/// The two panes that date anything read the same field off the same feed. If one of them is ever
/// renamed, this is where it is caught rather than on a screen.
#[test]
fn the_monitor_and_the_pump_read_the_same_scenario_clock() {
    for name in ["device/monitor.html", "device/pump.html"] {
        assert!(page(name).contains("t_sec"), "{name} does not read the scenario clock");
    }
}

/// A cuff that cannot tell the time does not invent an age.
///
/// The alternative — silently falling back to `Date.now()` — is the bug this replaced, and it is
/// worse than saying nothing: an undated snapshot is honest, a snapshot dated against a clock
/// nobody else on the screen is watching is not.
#[test]
fn a_reading_with_no_clock_behind_it_is_printed_without_an_age() {
    let html = page("device/monitor.html");
    assert!(
        html.contains("const ago = now == null || nibpAt == null ? null : Math.max(0, Math.round(now - nibpAt));"),
        "the age is no longer allowed to be absent"
    );
    assert!(
        html.contains("ago == null\n      ? 'mmHg'"),
        "an undated reading prints something other than a bare unit"
    );
}

/// The one line this pane is waiting on, kept as a test rather than as a sentence in a commit
/// message, so it is not waiting silently.
///
/// `/device/vitals` does not carry the scenario clock today. `pump.html` has read `t_sec` off it
/// since it was written and gets `undefined`, which is why its infused volume sits at 0 mL for
/// every run; `vent.html` reads `equipment` off it and gets `undefined` too, which is why it
/// never sees an ETT. The monitor now needs the same field for the same reason. The fix is one
/// line in the `/device/vitals` arm of `crates/vitals-web/src/main.rs`:
///
/// ```text
///     "t_sec": s.state.t_sec(),
/// ```
///
/// The field has landed, so this runs with everything else. An ignored test nobody turns on is
/// a comment with extra steps.
#[test]
fn the_device_feed_carries_the_scenario_clock() {
    let s = server::Server::start();
    let id = s.json("/api/new?ep=ep1")["id"].as_str().expect("a session id").to_string();
    s.json(&format!("/api/step?id={id}&tick=120"));
    let d = s.device(&id);
    assert_eq!(
        d["t_sec"].as_f64(),
        Some(120.0),
        "the bedside pane is given no scenario clock, so it cannot date a cuff reading: {d}"
    );
}

/// The same throwaway server harness `arrest.rs` uses, kept local so neither file owns the other.
mod server {
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, Stdio};

    pub struct Server {
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
        pub fn start() -> Server {
            static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let state = std::env::temp_dir().join(format!("vitals-cuff-{}-{n}", std::process::id()));
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

        pub fn json(&self, path: &str) -> serde_json::Value {
            let url = format!("http://127.0.0.1:{}{path}", self.port);
            let body = ureq::get(&url).call().map(|r| r.into_string().unwrap_or_default()).unwrap_or_default();
            serde_json::from_str(&body).unwrap_or(serde_json::Value::Null)
        }

        pub fn device(&self, sid: &str) -> serde_json::Value {
            let url = format!("http://127.0.0.1:{}/device/vitals", self.port);
            let body = ureq::get(&url)
                .set("x-embla-session", sid)
                .call()
                .map(|r| r.into_string().unwrap_or_default())
                .expect("the device feed");
            serde_json::from_str(&body).unwrap_or(serde_json::Value::Null)
        }
    }
}
