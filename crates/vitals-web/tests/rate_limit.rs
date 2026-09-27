//! **One person asking a lot does not silence everybody else.**
//!
//! `/api/say` was capped at twenty questions a minute for the whole ward — a single shared bucket, so
//! two strangers at two beds competed for the same twenty, and anybody who asked fast enough took the
//! ward's voice away from everyone (security audit L1, 25 Sep 2026). The cap is now per caller, keyed
//! by the address the platform observed (`client_addr` — proven on Cloud Run not to buy a fresh
//! allowance from an invented `X-Forwarded-For`), with an instance-wide ceiling kept above it.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};

struct Server {
    child: Child,
    port: u16,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}
impl Server {
    fn start() -> Server {
        let state = std::env::temp_dir().join(format!("vitals-ratelimit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&state);
        let mut child = Command::new(env!("CARGO_BIN_EXE_vitals-web"))
            .env("VITALS_WEB_BIND", "127.0.0.1:0")
            .env("VITALS_STATE_DIR", &state)
            .env("VITALS_WORLD", "1")
            .env("VITALS_WARD_DOOR", "open")
            .env("VITALS_DOOR_TOKEN", "the-door-token")
            .env_remove("VITALS_PROGRAM_ID")
            .env_remove("VITALS_TOKEN")
            .env_remove("HEIMDALL_API_KEY")
            .stdout(Stdio::piped())
            .spawn()
            .expect("start vitals-web");
        let out = child.stdout.take().expect("stdout");
        let mut me = Server { child, port: 0 };
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if let Some(a) = line.split("http://").nth(1) {
                me.port = a.trim().rsplit(':').next().and_then(|p| p.parse().ok()).unwrap_or(0);
                break;
            }
        }
        assert!(me.port > 0, "server never said what port it took");
        me
    }

    /// A question from `who`, as the platform would have observed their address.
    fn say_as(&self, who: &str) -> String {
        let mut s = std::net::TcpStream::connect(("127.0.0.1", self.port)).expect("connect");
        s.set_read_timeout(Some(std::time::Duration::from_secs(20))).ok();
        write!(
            s,
            "GET /api/say?q=hello HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Forwarded-For: {who}\r\n\
             Connection: close\r\n\r\n"
        )
        .expect("send");
        let mut raw = Vec::new();
        let _ = s.read_to_end(&mut raw);
        String::from_utf8_lossy(&raw).to_string()
    }
}

const REFUSED: &str = "too many questions";

#[test]
fn one_person_asking_a_lot_does_not_silence_everybody_else() {
    let s = Server::start();

    // Twenty questions from one stranger are answered (whatever the answer is), and the twenty-first
    // is refused — the per-person limit still holds.
    for i in 1..=20 {
        assert!(!s.say_as("203.0.113.1").contains(REFUSED), "question {i} from the first stranger was refused");
    }
    assert!(s.say_as("203.0.113.1").contains(REFUSED), "the first stranger's twenty-first question was not refused");

    // **A different stranger still gets to ask.** With one shared bucket this is the line that
    // failed: the first person had spent the whole ward's twenty.
    assert!(!s.say_as("198.51.100.7").contains(REFUSED),
            "a second stranger was refused because the first one had asked twenty questions");
}
