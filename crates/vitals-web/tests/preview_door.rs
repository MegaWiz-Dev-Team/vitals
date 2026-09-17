//! **A third door: the ward is built, the patients are here, and nobody plays yet.**
//!
//! Founder's ruling, 18 ก.ย. Production has been sitting behind a closed door with an empty board
//! and a holding sentence, which means the week before the fair is spent proving nothing: the
//! factory cannot fill it, so nobody can see the patients it would fill it with, and the first
//! thing a judge would meet is a page about a ward rather than a ward.
//!
//! `preview` is the state in between. The factory's doors take packs, so production's queue fills
//! and the globe can show who is waiting and where they are from. The ticker admits nobody, and
//! nothing a stranger can press puts a hand on a patient: take, declare, anchor, release and the
//! leaving beacon all answer in one sentence, and it is the sentence a person standing at a bed
//! needs — "the ward opens soon — nobody plays yet".
//!
//! Everything about this file is about the door as a word and as a set of permissions. What the
//! *page* does with a waiting patient is in `globe_logic.mjs` and `waiting_page.rs`.

use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use vitals_web::ward_chain::{door_from, Door};

/// A ward with its door in a given state, and nothing else configured: no chain, no token beyond
/// the one below, a state directory of its own.
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
    fn preview() -> Server {
        let state = std::env::temp_dir().join(format!("vitals-preview-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&state);
        let mut child = Command::new(env!("CARGO_BIN_EXE_vitals-web"))
            .env("VITALS_WEB_BIND", "127.0.0.1:0")
            .env("VITALS_STATE_DIR", &state)
            .env("VITALS_WORLD", "1")
            .env("VITALS_WARD_DOOR", "preview")
            .env("VITALS_DOOR_TOKEN", "the-door-token")
            .env_remove("VITALS_PROGRAM_ID")
            .env_remove("VITALS_TOKEN")
            .env_remove("HEIMDALL_API_KEY")
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

    fn get(&self, path: &str) -> (u16, Value) {
        let url = format!("http://127.0.0.1:{}{path}", self.port);
        match ureq::get(&url).call() {
            Ok(r) => (r.status(), r.into_json().unwrap_or(Value::Null)),
            Err(ureq::Error::Status(c, r)) => (c, r.into_json().unwrap_or(Value::Null)),
            Err(e) => panic!("{url}: {e}"),
        }
    }

    fn post(&self, path: &str, body: &Value) -> (u16, Value) {
        let url = format!("http://127.0.0.1:{}{path}", self.port);
        let r = ureq::post(&url)
            .set("Authorization", "Bearer the-door-token")
            .set("Content-Type", "application/json")
            .send_string(&body.to_string());
        match r {
            Ok(res) => (res.status(), res.into_json().unwrap_or(Value::Null)),
            Err(ureq::Error::Status(c, res)) => (c, res.into_json().unwrap_or(Value::Null)),
            Err(e) => panic!("{url}: {e}"),
        }
    }

    /// A POST with no body at all — what `sendBeacon` sends.
    fn post_raw(&self, path: &str) -> (u16, Value) {
        let url = format!("http://127.0.0.1:{}{path}", self.port);
        match ureq::post(&url).send_string("") {
            Ok(r) => (r.status(), r.into_json().unwrap_or(Value::Null)),
            Err(ureq::Error::Status(c, r)) => (c, r.into_json().unwrap_or(Value::Null)),
            Err(e) => panic!("{url}: {e}"),
        }
    }
}

#[test]
fn the_door_has_three_words_and_everything_else_is_closed() {
    assert_eq!(door_from(Some("open")), Door::Open);
    assert_eq!(door_from(Some("preview")), Door::Preview);
    assert_eq!(door_from(Some("closed")), Door::Closed);

    // The spellings a deploy script or a person actually produces.
    assert_eq!(door_from(Some(" Preview ")), Door::Preview, "trimmed and case-blind");
    assert_eq!(door_from(Some("PREVIEW")), Door::Preview);
    assert_eq!(door_from(Some("Open")), Door::Open);

    // Every ambiguity resolves closed, and the asymmetry is the whole argument: a ward that stays
    // shut an hour too long costs an hour, and a ward that opens by accident is strangers treating
    // patients nobody chose to release.
    for wrong in ["", " ", "previewish", "preview mode", "yes", "true", "1", "opening", "open-ish"] {
        assert_eq!(door_from(Some(wrong)), Door::Closed, "{wrong:?} is not a door");
    }
    assert_eq!(door_from(None), Door::Closed, "unset is shut");
}

#[test]
fn what_each_door_allows() {
    // The factory: packs may arrive whenever the ward is being filled, which is both of the states
    // that are not shut. A closed ward refuses them at the door, as it always has.
    assert!(Door::Open.takes_packs());
    assert!(Door::Preview.takes_packs(), "the whole point: production's queue can be filled");
    assert!(!Door::Closed.takes_packs());

    // The ticker: only an open ward admits. In preview the queue grows and the beds stay empty.
    assert!(Door::Open.admits());
    assert!(!Door::Preview.admits(), "a bed filled in preview is a patient nobody may treat");
    assert!(!Door::Closed.admits());

    // A stranger's hands: only an open ward.
    assert!(Door::Open.plays());
    assert!(!Door::Preview.plays());
    assert!(!Door::Closed.plays());

    // The word on the payload, which is what every page branches on.
    assert_eq!(Door::Open.word(), "open");
    assert_eq!(Door::Preview.word(), "preview");
    assert_eq!(Door::Closed.word(), "closed");

    // And the sentence a stranger gets for pressing something in preview. One sentence, twelve
    // words or fewer, and it says what will change rather than what is forbidden.
    let said = Door::Preview.refusal();
    assert_eq!(said, "the ward opens soon — nobody plays yet");
    assert!(said.split_whitespace().count() <= 12);
}

/// **A preview ward takes packs, admits nobody, and refuses every hand in one sentence.**
///
/// The three permissions driven against the real binary rather than reasoned about: the factory's
/// door answers, the play routes refuse before they read a key or reach a chain, and the board says
/// which door it is behind.
#[test]
fn a_preview_ward_fills_its_queue_and_lets_nobody_play() {
    let s = Server::preview();

    // The factory's door: open enough to fill a queue.
    let (code, _) = s.post("/api/ward/queue", &serde_json::json!({ "packs": [] }));
    assert_eq!(code, 200, "a preview ward takes packs — that is what it is for");

    // The board says the word, and the count is where it always was.
    let (code, board) = s.get("/api/ward");
    assert_eq!(code, 200);
    assert_eq!(board["queue"]["door"], "preview");
    assert!(board["queue"]["waiting"].is_u64() || board["queue"]["waiting"].is_null());
    assert!(board["queue"]["waiting_patients"].is_array(),
            "the people, not only the number: {}", board["queue"]);

    // Every hand on a patient, refused in the sentence a person at a bed needs — and refused
    // before a key is read, which is why a request with no player key gets it too.
    for path in ["/api/ward/open", "/api/ward/take?id=x", "/api/ward/declare?id=x",
                 "/api/ward/anchor?id=x", "/api/ward/release?id=x"] {
        let (code, answer) = s.get(path);
        assert_eq!(code, 409, "{path} answered {code}: {answer}");
        assert_eq!(answer["refused"], "the ward opens soon — nobody plays yet", "{path}");
        assert_eq!(answer["door"], "preview", "{path}");
    }

    // The leaving beacon frees a head, so it is refused too; the heartbeat is a page saying it is
    // still there, which is true whatever the door says.
    let (code, answer) = s.post_raw("/api/ward/left?id=x");
    assert_eq!(code, 409, "{answer}");
    assert_eq!(answer["refused"], "the ward opens soon — nobody plays yet");
    let (code, _) = s.post_raw("/api/ward/beat?id=x");
    assert_eq!(code, 404, "a beat for a shift that does not exist is not about the door");
}
