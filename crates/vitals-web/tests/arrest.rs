//! A patient with no pulse must not be shown numbers that are measured off flowing blood.
//!
//! The bay's rail and the bedside device disagreed with each other for the whole of every arrest.
//! The device keyed on the rhythm and printed `--` for saturation and cuff pressure, which is
//! right: an oximeter has nothing pulsatile to detect and a cuff has nothing to occlude. The rail
//! printed whatever the automaton was still holding — `SpO₂ 80%`, `BP 54/54` — frozen at the
//! instant the rhythm changed. One screen, two answers to "does this patient have a pulse".
//!
//! Fixed in the one place that serves both, so this test checks the *wire* rather than either
//! renderer: what a reviewer sees with `curl` is what neither panel can contradict.

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
        let state = std::env::temp_dir().join(format!("vitals-arrest-{}-{n}", std::process::id()));
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

    /// What the bedside device is fed for this bed.
    fn device(&self, sid: &str) -> serde_json::Value {
        let url = format!("http://127.0.0.1:{}/device/vitals", self.port);
        let body = ureq::get(&url)
            .set("x-embla-session", sid)
            .call()
            .map(|r| r.into_string().unwrap_or_default())
            .expect("the device feed");
        serde_json::from_str(&body).unwrap_or(serde_json::Value::Null)
    }

    /// EP1 with nobody treating her: anaphylaxis untreated arrests, and its arrest state is PEA.
    /// The one path in the season that reaches a pulseless rhythm without a defibrillator.
    fn ep1_in_arrest(&self) -> (String, serde_json::Value) {
        let id = self.json("/api/new?ep=ep1")["id"].as_str().expect("a session id").to_string();
        let mut view = serde_json::Value::Null;
        for _ in 0..40 {
            view = self.json(&format!("/api/step?id={id}&tick=30"));
            if view["rhythm"] == "pea" {
                return (id, view);
            }
        }
        panic!("EP1 never arrested: last view {view}");
    }
}

/// The rail's numbers, on the wire.
#[test]
fn the_bay_stops_reporting_a_saturation_and_a_pressure_when_there_is_no_pulse() {
    let s = Server::start();
    let (_, v) = s.ep1_in_arrest();

    assert_eq!(v["status"], "Arrest", "the case is not where this test thinks it is");
    assert_eq!(v["pulse"], false, "a pulseless rhythm is reported as perfusing");
    assert!(v["spo2"].is_null(), "an arrest still reports a saturation: {}", v["spo2"]);
    assert!(v["sbp"].is_null(), "an arrest still reports a systolic: {}", v["sbp"]);
    assert!(v["dbp"].is_null(), "an arrest still reports a diastolic: {}", v["dbp"]);
    // Electrical, and the whole point of PEA. Blanking it would hide the trap.
    assert!(v["hr"].as_f64().unwrap_or(0.0) > 0.0, "PEA lost its countable rate");
    assert_eq!(v["shockable"], false, "PEA was offered to a defibrillator");
    // Not breathing. A steady respiratory rate over a cardiac arrest is the screen lying.
    assert_eq!(v["rr"].as_f64(), Some(0.0), "she is still breathing calmly through her arrest");
}

/// The device's numbers, on the wire — and they have to be the *same* numbers.
#[test]
fn both_panels_are_served_the_same_answer_about_the_pulse() {
    let s = Server::start();
    let (id, v) = s.ep1_in_arrest();
    let d = s.device(&id);

    for k in ["hr", "spo2", "sbp", "dbp", "rr", "pulse", "rhythm", "shockable"] {
        assert_eq!(d[k], v[k], "the rail and the bedside device disagree about {k}");
    }
    assert_eq!(d["rhythm"], "pea");
    assert!(d["spo2"].is_null() && d["sbp"].is_null() && d["dbp"].is_null());
}

/// And none of this fires on a patient who *has* a pulse — the guard has to be the rhythm, not
/// "anything that looks bad".
#[test]
fn a_perfusing_patient_still_reports_everything() {
    let s = Server::start();
    let id = s.json("/api/new?ep=ep1")["id"].as_str().expect("a session id").to_string();
    let v = s.json(&format!("/api/step?id={id}&tick=1"));
    assert_eq!(v["rhythm"], "sinus");
    assert_eq!(v["pulse"], true);
    for k in ["spo2", "sbp", "dbp"] {
        assert!(v[k].as_f64().unwrap_or(0.0) > 0.0, "a perfusing patient lost her {k}");
    }
    assert!(v["rr"].as_f64().unwrap_or(0.0) > 0.0, "a perfusing patient stopped breathing");
}

// ── the two renderers ───────────────────────────────────────────────────────
// Nothing type-checks either page, and both of them have to survive a `null` where a number used
// to be. `null + '%'` renders as "null%" on a screen a clinician is watching.

fn page(name: &str) -> String {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static").join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

#[test]
fn the_rail_prints_dashes_rather_than_the_word_null() {
    let html = page("index.html");
    assert!(
        !html.contains("setVital('#m-spo2',v.spo2+'%')"),
        "the rail concatenates the saturation again — an arrest renders as \"null%\""
    );
    assert!(
        !html.contains("setVital('#m-bp',v.sbp+'/'+v.dbp)"),
        "the rail concatenates the pressure again — an arrest renders as \"null/null\""
    );
    assert!(html.contains("v.spo2==null?'--'"), "the saturation has no missing-reading branch");
    assert!(html.contains("v.sbp==null||v.dbp==null?'--'"), "the pressure has no missing-reading branch");
}

#[test]
fn the_device_never_caches_a_cuff_reading_that_did_not_happen() {
    let html = page("device/monitor.html");
    assert!(
        !html.contains("if (!nibpAt || Date.now() - nibpAt > 180000){ nibpVal = [d.sbp, d.dbp]"),
        "the cuff caches nulls again — they print as \"0/0\" for three minutes after ROSC"
    );
    assert!(html.contains("const cuffable = d.sbp != null && d.dbp != null"), "the cuff guard is gone");
}
