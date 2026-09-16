//! What the ward host will and will not answer.
//!
//! Two of the sweep's findings on 16 ก.ย., and they are the same finding twice: a rule the page
//! keeps and the server does not. The page refuses to play a patient before the head is taken, and
//! the page is the only thing that refuses; the page opens no episodes, and the server will open
//! one to anybody who asks for a patient whose id is not a number.
//!
//! A rule only the client holds is a rule a scripted client does not have.

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
    fn start(ward: bool) -> Server {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let state = std::env::temp_dir().join(format!("vitals-host-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&state);
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_vitals-web"));
        cmd.env("VITALS_WEB_BIND", "127.0.0.1:0")
            .env("VITALS_STATE_DIR", &state)
            .env_remove("VITALS_PROGRAM_ID")
            .env_remove("VITALS_TOKEN")
            .env_remove("HEIMDALL_API_KEY")
            .stdout(Stdio::piped());
        if ward {
            cmd.env("VITALS_WORLD", "1");
        } else {
            cmd.env_remove("VITALS_WORLD");
        }
        let mut child = cmd.spawn().expect("start vitals-web");
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

    fn get(&self, path: &str) -> (u16, String) {
        let url = format!("http://127.0.0.1:{}{path}", self.port);
        match ureq::get(&url).call() {
            Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
            Err(ureq::Error::Status(c, r)) => (c, r.into_string().unwrap_or_default()),
            Err(e) => panic!("{url}: {e}"),
        }
    }
}

/// **Nothing of the season lives on the ward host** (CWF_PLAN.md ruling 13).
///
/// `/api/new?patient=abc` fell through: `patient` did not parse as a number, so the handler went
/// on to read `ep`, found none, defaulted to `ep1`, and opened a practice run of the season's
/// first episode on the host whose front page is a globe. A stranger who mistypes a patient id
/// gets a story episode; a crawler gets one per request.
#[test]
fn a_patient_id_that_is_not_a_number_is_not_an_episode() {
    let s = Server::start(true);
    for bad in ["abc", "1789528326x", "-1", "", "0x2a", "9999999999999999999999"] {
        let (code, body) = s.get(&format!("/api/new?patient={bad}"));
        assert_eq!(code, 404, "/api/new?patient={bad} answered {code}: {body}");
        assert!(!body.contains("\"view\""),
                "and it opened a run rather than refusing: {body}");
    }

    // The ward host still plays the ward. A well-formed id that nobody admitted is a different
    // answer — it reached the ward and the ward said she is not here.
    let (code, body) = s.get("/api/new?patient=1789528326");
    assert_ne!(code, 404, "a real patient id is the ward's business, not a 404: {body}");

    // And the Eternal entry is untouched: `patient` means nothing there, and the season is its
    // whole purpose.
    let e = Server::start(false);
    let (code, body) = e.get("/api/new?patient=abc");
    assert_eq!(code, 200, "vitals.academy opens a run, as it always has: {body}");
    assert!(body.contains("\"view\""), "with a view in it: {body}");
}

/// **A shift is not playable until the head is hers, and the server is what says so.**
///
/// `/api/step` advanced any session it was handed. On a ward session that means a scripted client
/// could play a patient nobody had taken — the chain refuses the anchor at the end, but the work
/// is done, the tape exists, and the board says nothing. The page's own refusal is a courtesy to
/// the person reading it; this is the rule.
///
/// The gate is the declaration, because that is the thing the chain stamps: a ward session may not
/// be stepped until its commitment has landed, and the commitment is only prepared for a player
/// the chain says is holding her head.
#[test]
fn a_ward_session_may_not_be_played_before_it_is_declared() {
    use vitals_web::ward::may_step;
    assert!(may_step(false, true, false).is_ok(), "the Eternal bay has no head to take");
    assert!(may_step(false, false, false).is_ok(), "nor does a practice run");
    assert!(may_step(true, true, false).is_ok(), "a declared shift plays");
    let refused = may_step(true, false, false).expect_err("an undeclared shift must not");
    assert!(refused.contains("take"),
            "and the refusal is a sentence the person at her bed can act on: {refused}");
}

/// **A rebuild must not be behind a shift this server just wrote.**
///
/// Reopening a patient in the seconds after an anchor reads the chain before the transaction is
/// finalized: the count comes back one short and the page says "shift 1 · 0 anchored shifts" over
/// a patient who has just been treated. Measured at ten to twenty seconds on staging.
///
/// This server knows better, because it is the one that wrote it. It remembers the shift count it
/// last anchored per patient for a minute, and a read that comes back behind that is refused with
/// a sentence rather than served as a chart — a stale count on a page whose whole claim is that
/// the chart is the chain is the worst possible thing to show.
#[test]
fn a_read_behind_what_this_server_just_wrote_is_refused() {
    use std::time::Duration;
    use vitals_web::ward::behind_the_head;

    // Nothing remembered: whatever the chain says is the truth.
    assert!(behind_the_head(3, None).is_none());

    // The chain has caught up, or is ahead because somebody else anchored.
    assert!(behind_the_head(3, Some((3, Duration::from_secs(2)))).is_none());
    assert!(behind_the_head(4, Some((3, Duration::from_secs(2)))).is_none());

    // Behind, and recently written: this server wrote shift 3 two seconds ago and the chain still
    // says 2.
    let wait = behind_the_head(2, Some((3, Duration::from_secs(2)))).expect("refused");
    assert!(wait.contains("landing"), "the sentence says what is happening, not what failed: {wait}");

    // Behind, and long ago: the memory has expired and the chain is the authority again. A server
    // that argued with the chain for ever would be a server with its own opinion of the record.
    assert!(behind_the_head(2, Some((3, Duration::from_secs(61)))).is_none(),
            "after a minute the chain is what there is, whatever we think we wrote");
}
