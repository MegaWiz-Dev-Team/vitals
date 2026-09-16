//! Where the people on the ward came from, counted without knowing anything about them.
//!
//! Links we hand out carry `?src=<channel>` — superteam-th, medtwitter, reddit, embla, techsauce —
//! and the ward counts arrivals per channel the way the bay counts arrival, play and finish. What
//! makes this safe to publish is what it is not: a `src` is a string **we** chose and handed out,
//! never a fact about the person holding it. No cookie, no user agent, no address, nothing that
//! follows anybody anywhere. /privacy §6 stands unchanged.
//!
//! And the set is closed. A query string is a thing strangers can type, so anything outside the
//! allowlist is counted as nothing at all: without that, our own published statistics are a
//! writable surface, and a screenshot of them is whatever the last person to visit felt like
//! writing.

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
    fn ward() -> Server {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let state = std::env::temp_dir().join(format!("vitals-src-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&state);
        let mut child = Command::new(env!("CARGO_BIN_EXE_vitals-web"))
            .env("VITALS_WEB_BIND", "127.0.0.1:0")
            .env("VITALS_STATE_DIR", &state)
            .env("VITALS_WORLD", "1")
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

    fn hit(&self, path: &str) {
        let url = format!("http://127.0.0.1:{}{path}", self.port);
        let _ = ureq::get(&url).call();
    }

    fn usage(&self) -> serde_json::Value {
        let url = format!("http://127.0.0.1:{}/api/usage", self.port);
        let body = ureq::get(&url)
            .call()
            .map(|r| r.into_string().unwrap_or_default())
            .unwrap_or_default();
        serde_json::from_str(&body).unwrap_or(serde_json::Value::Null)
    }
}

/// Only a channel we handed out is a channel.
#[test]
fn the_accepted_channels_are_ours_and_the_rest_are_nothing() {
    use vitals_web::usage::channel;

    assert_eq!(channel("reddit"), Some("reddit"));
    assert_eq!(channel(" Reddit "), Some("reddit"), "a link pasted into a tweet keeps its count");

    for not_ours in ["", "nonsense", "reddit.com", "red dit", "../etc/passwd", "<script>",
                     "reddit&x=1", "superteam", &"a".repeat(200)] {
        assert_eq!(channel(not_ours), None,
                   "{not_ours:?} is not a channel we handed out — an open set makes our own \
                    published statistics a writable surface");
    }

    // The value stored is our own string, not the caller's bytes: that is what makes the map
    // un-writable rather than merely filtered.
    let ours: &'static str = channel("MEDTWITTER").expect("a channel of ours");
    assert_eq!(ours, "medtwitter");
}

/// The ward counts where people came from, and publishes nothing else about them.
#[test]
fn the_ward_counts_arrivals_per_channel_and_knows_nothing_more() {
    let s = Server::ward();

    s.hit("/?src=reddit");
    s.hit("/?src=reddit");
    s.hit("/?src=embla");
    s.hit("/?src=whatever-i-typed");
    s.hit("/");

    let u = s.usage();
    let by_src = &u["arrivals"]["by_src"];
    assert_eq!(by_src["reddit"], 2);
    assert_eq!(by_src["embla"], 1);
    assert!(by_src["whatever-i-typed"].is_null(),
            "a channel nobody handed out is counted as nothing at all: {by_src}");
    assert_eq!(by_src.as_object().map(|o| o.len()).unwrap_or(9), 2,
               "two channels arrived, and the map holds exactly those two");

    // What is published about an arrival is the channel and the count. Anything else here would
    // be a fact about a person, and this endpoint has never held one.
    let arrivals = u["arrivals"].as_object().expect("the arrivals block");
    for key in arrivals.keys() {
        assert!(["by_src", "total", "derivation"].contains(&key.as_str()),
                "{key} is not something the ward knows about an arrival");
    }
    assert_eq!(u["arrivals"]["total"], 5, "every arrival counts, even one with no channel on it");

    // And the ward still refuses to answer with the Eternal bay's numbers, which are not its own.
    assert_eq!(u["ward"], "not open yet");
    assert_eq!(u["usage_for_the_eternal_entry"], "https://vitals.academy/api/usage");
}
