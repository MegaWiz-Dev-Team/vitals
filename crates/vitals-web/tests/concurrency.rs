//! **A stranger reading the board is answered while the ward is mid-pass.**
//!
//! Measured on production, 22 ก.ย.: Cloud Run answering 429 *no available instance* on
//! `/api/ward`, `/api/usage`, `/review` and `/favicon.ico`, in three bursts, each one inside a
//! ticker pass. The service is configured `concurrency 8` — we have told Cloud Run this container
//! can have eight requests in flight — and `main` has always answered one at a time. During a pass
//! the platform counts eight in hand while seven sit unserved, and refuses the ninth. `max-instances`
//! is 1 and stays 1, because the anchoring tree is in memory.
//!
//! So the platform did what it was told and the container was the thing that lied.
//!
//! **Why no test saw it.** Every harness in this repository issues one request at a time against a
//! server that serves one at a time. That is a perfectly consistent world in which this bug cannot
//! exist. The instrument had to be built before the fault could be looked at — which is the same
//! order everything else this week has gone in.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The door's own secret, never the page's.
const DOOR: &str = "concurrency-door-secret";

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
    /// A ward whose pass takes `pass_ms`, so the loop can be held long enough to ask what happens
    /// to everybody else while it is.
    fn with_slow_pass(pass_ms: u64) -> Server {
        Server::slow(pass_ms, 0)
    }

    /// A ward whose pass takes `pass_ms` and whose board read takes `board_ms`.
    fn slow(pass_ms: u64, board_ms: u64) -> Server {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let state = std::env::temp_dir().join(format!("vitals-conc-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&state);
        let mut child = Command::new(env!("CARGO_BIN_EXE_vitals-web"))
            .env("VITALS_WEB_BIND", "127.0.0.1:0")
            .env("VITALS_STATE_DIR", &state)
            .env("VITALS_WORLD", "1")
            .env("VITALS_WARD_DOOR", "open")
            .env("VITALS_TICK_SLEEP_MS", pass_ms.to_string())
            .env("VITALS_BOARD_SLEEP_MS", board_ms.to_string())
            // **The door has to be open for the pass to be askable for at all.** Removing
            // `VITALS_DOOR_TOKEN` does not open the factory's doors, it shuts them: this test
            // first passed in 1.88 s having never occupied the loop, because the tick it fired
            // was refused before reaching the ward. A test written to catch a harness answering
            // for work it did not do, doing exactly that.
            .env("VITALS_DOOR_TOKEN", DOOR)
            .env_remove("VITALS_PROGRAM_ID")
            .env_remove("VITALS_TOKEN")
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

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }
}

/// **A pass holds the writing loop; it does not hold the ward.**
#[test]
fn reads_are_answered_while_a_pass_holds_the_loop() {
    let s = Server::with_slow_pass(4_000);

    // Warm the board once, so the first reader in the burst is not also paying for a cold read.
    let _ = ureq::get(&s.url("/api/ward")).call();

    // The pass, in its own thread, holding the loop for four seconds.
    let tick = s.url("/api/ward/tick");
    let pass = std::thread::spawn(move || {
        ureq::post(&tick)
            .set("Authorization", &format!("Bearer {DOOR}"))
            .call()
            .map(|r| r.status())
            .unwrap_or(0)
    });
    // Long enough that the pass is certainly inside its hold, short enough that most of the hold
    // is still ahead of the readers.
    std::thread::sleep(Duration::from_millis(400));

    // Four strangers, the way a dashboard poll and a visitor arrive together.
    let reads: Vec<_> = ["/api/ward", "/api/usage", "/review", "/stats"]
        .iter()
        .map(|p| {
            let url = s.url(p);
            let path = p.to_string();
            std::thread::spawn(move || {
                let began = Instant::now();
                let code = ureq::get(&url).call().map(|r| r.status()).unwrap_or(0);
                (path, code, began.elapsed())
            })
        })
        .collect();

    let mut slow = Vec::new();
    for h in reads {
        let (path, code, took) = h.join().expect("a reader thread");
        assert_eq!(code, 200, "{path} was not answered while the ward was mid-pass");
        if took > Duration::from_millis(1_500) {
            slow.push(format!("{path} took {}ms", took.as_millis()));
        }
    }
    assert!(
        slow.is_empty(),
        "reads waited for the pass instead of being answered beside it: {}",
        slow.join(", ")
    );

    // And the pass really did run: a tick that was refused would have held nothing, which is how
    // this test passed before it could fail.
    let code = pass.join().expect("the pass thread");
    assert_eq!(code, 200, "the pass was refused, so the loop was never held and this test proved \
                           nothing — check the door secret");
}


/// **A slow read holds nobody but its own reader.**
///
/// The pass leaving the loop is what the test above proves. This is the other half, and the half
/// the reader pool is for: the board's first read on a cold instance goes to the chain inline and
/// took sixteen to twenty-one seconds on staging. On one thread that is twenty seconds in which
/// the ward answers nobody — the same refusal as a pass, arriving through a different door.
///
/// Without the pool this fails: `/api/usage`, `/review` and `/stats` would queue behind the board
/// read and be answered after it. With it they are answered beside it.
#[test]
fn a_slow_board_read_does_not_hold_the_other_readers() {
    let s = Server::slow(0, 4_000);

    let board = s.url("/api/ward");
    let slow = std::thread::spawn(move || {
        let began = Instant::now();
        let code = ureq::get(&board).call().map(|r| r.status()).unwrap_or(0);
        (code, began.elapsed())
    });
    std::thread::sleep(Duration::from_millis(400));

    let others: Vec<_> = ["/api/usage", "/review", "/stats"]
        .iter()
        .map(|p| {
            let url = s.url(p);
            let path = p.to_string();
            std::thread::spawn(move || {
                let began = Instant::now();
                let code = ureq::get(&url).call().map(|r| r.status()).unwrap_or(0);
                (path, code, began.elapsed())
            })
        })
        .collect();

    for h in others {
        let (path, code, took) = h.join().expect("a reader thread");
        assert_eq!(code, 200, "{path} was not answered while the board was being read");
        assert!(
            took < Duration::from_millis(1_500),
            "{path} waited {}ms for a slow board read instead of being answered beside it",
            took.as_millis()
        );
    }

    // And the slow read really was slow: otherwise nothing was being waited on and this proved
    // nothing, which is the failure mode the test above already fell into once.
    let (code, took) = slow.join().expect("the board thread");
    assert_eq!(code, 200, "the board read itself failed");
    assert!(
        took >= Duration::from_millis(3_500),
        "the board read took only {}ms, so it never held anything and this test proved nothing",
        took.as_millis()
    );
}
