//! **The factory's doors take their own secret, and never the page's.**
//!
//! Found by the scenario sweep on 16 ก.ย.: `/bay.js` is served with `__VITALS_TOKEN__` replaced by
//! the value of `VITALS_TOKEN`, and `VITALS_TOKEN` is what guarded the two doors that write to the
//! ward. So the token was on a public page. Anyone who opened a shift could read it out of the
//! script and queue patients of their own, or put a picture of their choosing on somebody else's
//! patient — which are exactly the two things those doors exist to prevent.
//!
//! The rule this file holds is the root-cause one, not the instance: a secret that is printed into
//! a page cannot also be a secret that lets somebody write. The player's own routes keep the page
//! token, because the page has to call them and they spend nothing but this server's own key on
//! this server's own sessions. The doors take `VITALS_DOOR_TOKEN`, and if there is not one they do
//! not open at all — falling back to the page token is how this happened in the first place.

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
    /// A ward host with a page token, and a door token only if one is given.
    fn start(door: Option<&str>) -> Server {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let state = std::env::temp_dir().join(format!("vitals-doors-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&state);
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_vitals-web"));
        cmd.env("VITALS_WEB_BIND", "127.0.0.1:0")
            .env("VITALS_STATE_DIR", &state)
            .env("VITALS_WORLD", "1")
            .env("VITALS_TOKEN", "the-page-token")
            .env("VITALS_WARD_DOOR", "open")
            .env_remove("VITALS_PROGRAM_ID")
            .env_remove("HEIMDALL_API_KEY")
            .stdout(Stdio::piped());
        match door {
            Some(d) => cmd.env("VITALS_DOOR_TOKEN", d),
            None => cmd.env_remove("VITALS_DOOR_TOKEN"),
        };
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

    fn post(&self, path: &str, bearer: Option<&str>, body: &str) -> (u16, String) {
        let url = format!("http://127.0.0.1:{}{path}", self.port);
        let mut r = ureq::post(&url).set("Content-Type", "application/json");
        if let Some(b) = bearer {
            r = r.set("Authorization", &format!("Bearer {b}"));
        }
        match r.send_string(body) {
            Ok(res) => (res.status(), res.into_string().unwrap_or_default()),
            Err(ureq::Error::Status(c, res)) => (c, res.into_string().unwrap_or_default()),
            Err(e) => panic!("{url}: {e}"),
        }
    }

    fn get(&self, path: &str) -> (u16, String) {
        let url = format!("http://127.0.0.1:{}{path}", self.port);
        match ureq::get(&url).call() {
            Ok(res) => (res.status(), res.into_string().unwrap_or_default()),
            Err(ureq::Error::Status(c, res)) => (c, res.into_string().unwrap_or_default()),
            Err(e) => panic!("{url}: {e}"),
        }
    }
}

const A_PACK: &str = r#"{"packs":[]}"#;

/// The page's token is public, so it opens nothing that writes.
#[test]
fn the_page_token_does_not_open_the_factorys_doors() {
    let s = Server::start(Some("the-door-token"));

    // It really is public: this is the script every visitor loads.
    let (_, js) = s.get("/bay.js");
    assert!(js.contains("the-page-token"),
            "the page token is printed into bay.js — if that ever stops being true this test is \
             about a danger that no longer exists and should be re-read, not deleted");

    for door in ["/api/ward/queue", "/api/ward/pack/42"] {
        let (code, body) = s.post(door, Some("the-page-token"), A_PACK);
        assert_eq!(code, 401,
                   "{door} accepted the token that is printed into a public page: {body}");
        let (code, _) = s.post(door, None, A_PACK);
        assert_eq!(code, 401, "{door} with no token at all");
        let (code, body) = s.post(door, Some("the-door-token"), A_PACK);
        assert_ne!(code, 401, "{door} refused the door's own token: {body}");
        assert_ne!(code, 503, "{door} has a door token and must open: {body}");
    }
}

/// The player's own routes keep the page token: the page has to call them.
#[test]
fn the_players_own_routes_still_answer_to_the_page() {
    let s = Server::start(Some("the-door-token"));
    let (code, body) = s.post("/api/say?id=nosuch&q=hello", Some("the-page-token"), "");
    assert_ne!(code, 401, "the page must be able to speak for the player it is showing: {body}");
    let (code, _) = s.post("/api/say?id=nosuch&q=hello", Some("the-door-token"), "");
    assert_eq!(code, 401,
               "and the door's secret opens the doors and nothing else — two keys for two doors, \
                not a rank where one outranks the other");
}

/// **No door token, no doors.** The failure mode that made this necessary was a fallback.
#[test]
fn a_ward_with_no_door_token_does_not_open_the_doors_at_all() {
    let s = Server::start(None);
    for door in ["/api/ward/queue", "/api/ward/pack/42"] {
        let (code, body) = s.post(door, Some("the-page-token"), A_PACK);
        assert_eq!(code, 503,
                   "{door} must say it has no secret to check against, and never fall back to the \
                    page's: {body}");
        assert!(body.contains("VITALS_DOOR_TOKEN"),
                "and the sentence has to name what is missing, because the person reading it is \
                 the one who can set it: {body}");
    }
}
