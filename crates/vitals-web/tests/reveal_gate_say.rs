//! `/api/say` must not hand a learner a fact they have not earned.
//!
//! A case marks some of its patient's facts `on_direct_ask`: she says them only when asked about
//! that exact thing. The model volunteers them anyway, about one reply in fifty, and a learner who
//! never asked gets the history for free. The window gate in `vitals-sce` catches every such leak
//! in the internal eval; these tests prove the served path actually runs it — regenerate with the
//! gate's hint, then fall back to the case's own safe line — and that an honest reply costs
//! exactly one model call, as it did before the gate existed.
//!
//! No real model is called. A stand-in `/chat/completions` server, built the way `rpc_bound.rs`
//! builds its stand-in RPC, plays back scripted replies and counts the calls. The leaking reply is
//! read from the case file at run time, so no case text is compiled into this test, and no
//! assertion message prints it.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// A stand-in model gateway. Each patient turn (a request carrying a system brief) pops the next
/// scripted reply; the last one repeats once the script runs out. The reachability probe the
/// server sends at boot carries no brief and is answered but not counted.
struct FakeModel {
    port: u16,
    turns: Arc<AtomicUsize>,
    script: Arc<Mutex<VecDeque<String>>>,
    /// Every counted request body, in order, so a test can read what was sent.
    seen: Arc<Mutex<Vec<serde_json::Value>>>,
}

impl FakeModel {
    fn start(script: &[&str]) -> FakeModel {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let turns = Arc::new(AtomicUsize::new(0));
        let script: Arc<Mutex<VecDeque<String>>> =
            Arc::new(Mutex::new(script.iter().map(|s| s.to_string()).collect()));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (t, sc, se) = (turns.clone(), script.clone(), seen.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (t, sc, se) = (t.clone(), sc.clone(), se.clone());
                std::thread::spawn(move || serve(stream, t, sc, se));
            }
        });
        FakeModel { port, turns, script, seen }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.port)
    }

    fn turns(&self) -> usize {
        self.turns.load(Ordering::SeqCst)
    }

    fn then(&self, replies: &[&str]) {
        let mut s = self.script.lock().unwrap();
        s.clear();
        s.extend(replies.iter().map(|r| r.to_string()));
    }

    fn system_prompts(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|b| b["messages"][0]["content"].as_str().unwrap_or_default().to_string())
            .collect()
    }
}

/// Read one HTTP request, honouring Content-Length — a patient brief is several kilobytes, so a
/// single `read` would cut it short.
fn read_request(stream: &mut std::net::TcpStream) -> Option<String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        let text = String::from_utf8_lossy(&buf);
        if let Some(end) = text.find("\r\n\r\n") {
            let len = text[..end]
                .lines()
                .find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.eq_ignore_ascii_case("content-length").then(|| v.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            if buf.len() >= end + 4 + len {
                return Some(String::from_utf8_lossy(&buf[end + 4..end + 4 + len]).to_string());
            }
        }
    }
}

fn serve(
    mut stream: std::net::TcpStream,
    turns: Arc<AtomicUsize>,
    script: Arc<Mutex<VecDeque<String>>>,
    seen: Arc<Mutex<Vec<serde_json::Value>>>,
) {
    let Some(body) = read_request(&mut stream) else { return };
    let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
    let is_turn = v["messages"][0]["role"] == "system";
    let content = if is_turn {
        turns.fetch_add(1, Ordering::SeqCst);
        seen.lock().unwrap().push(v);
        let mut s = script.lock().unwrap();
        if s.len() > 1 { s.pop_front().unwrap_or_default() } else { s.front().cloned().unwrap_or_default() }
    } else {
        "ok".to_string()
    };
    let payload = serde_json::json!({"choices": [{"message": {"role": "assistant", "content": content}}]})
        .to_string();
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
}

struct Server {
    child: Child,
    port: u16,
    state: std::path::PathBuf,
    log: Arc<Mutex<Vec<String>>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.state);
    }
}

impl Server {
    fn start(model: &FakeModel) -> Server {
        static N: AtomicUsize = AtomicUsize::new(0);
        let state = std::env::temp_dir().join(format!(
            "vitals-gate-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&state);
        let mut child = Command::new(env!("CARGO_BIN_EXE_vitals-web"))
            .env("VITALS_WEB_BIND", "127.0.0.1:0")
            .env("VITALS_STATE_DIR", &state)
            .env("HEIMDALL_API_KEY", "test-only")
            .env("HEIMDALL_API_URL", model.url())
            .env("VITALS_TURNS_PER_MIN", "100")
            .env_remove("VITALS_VERTEX_URL")
            .env_remove("VITALS_PROGRAM_ID")
            .env_remove("VITALS_TOKEN")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start vitals-web");
        let out = child.stdout.take().expect("stdout");
        let err = child.stderr.take().expect("stderr");
        let log = Arc::new(Mutex::new(Vec::new()));
        let sink = log.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                sink.lock().unwrap().push(line);
            }
        });
        let mut me = Server { child, port: 0, state, log };
        let mut lines = BufReader::new(out).lines();
        for line in lines.by_ref().map_while(Result::ok) {
            if let Some(a) = line.split("http://").nth(1) {
                me.port = a.trim().rsplit(':').next().and_then(|p| p.parse().ok()).unwrap_or(0);
                break;
            }
        }
        // Keep draining stdout so the server never blocks on a full pipe.
        std::thread::spawn(move || for _ in lines.map_while(Result::ok) {});
        assert!(me.port > 0, "server never said what port it took");
        me
    }

    fn json(&self, path: &str) -> serde_json::Value {
        let url = format!("http://127.0.0.1:{}{path}", self.port);
        let body = ureq::get(&url).call().map(|r| r.into_string().unwrap_or_default()).unwrap_or_else(
            |e| match e {
                ureq::Error::Status(_, r) => r.into_string().unwrap_or_default(),
                other => panic!("{path}: {other}"),
            },
        );
        serde_json::from_str(&body).unwrap_or(serde_json::Value::Null)
    }

    fn new_case(&self) -> String {
        self.json("/api/new?ep=ep1")["id"].as_str().expect("a session id").to_string()
    }

    fn say(&self, id: &str, q: &str) -> serde_json::Value {
        self.json(&format!("/api/say?id={id}&q={}", enc(q)))
    }

    /// The gate's log lines, waiting briefly for stderr to catch up with the reply.
    fn gate_log(&self, want: usize) -> Vec<String> {
        for _ in 0..40 {
            let l: Vec<String> =
                self.log.lock().unwrap().iter().filter(|l| l.starts_with("reveal-gate ")).cloned().collect();
            if l.len() >= want {
                return l;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        self.log.lock().unwrap().iter().filter(|l| l.starts_with("reveal-gate ")).cloned().collect()
    }

    fn whole_log(&self) -> String {
        self.log.lock().unwrap().join("\n")
    }
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn ep1() -> serde_json::Value {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo/ep1-en.json");
    serde_json::from_str(&std::fs::read_to_string(p).expect("ep1 persona")).expect("ep1 json")
}

/// EP1's first held-back fact: its scripted line (what a leak looks like — the model reusing
/// the script's own words) and one of its authored keywords (what asking for it looks like).
fn held_back() -> (String, String) {
    let p = ep1();
    let n = p["dialogue"]
        .as_array()
        .and_then(|d| d.iter().find(|n| n["reveal"] == "on_direct_ask"))
        .expect("ep1 holds something back")
        .clone();
    let line = n["patient"].as_str().expect("a line").to_string();
    let kw = n["keywords"][0].as_str().expect("a keyword").to_string();
    (line, kw)
}

fn fallback() -> String {
    ep1()["fallback"].as_str().expect("ep1 has a fallback").to_string()
}

/// A reply that is plainly hers and states nothing she holds back.
const ORDINARY: &str = "It hurts. I'm frightened.";
/// What the hint says to the model, and only that: it names node ids, never a line.
const HINT_MARK: &str = "Answer again, in character, without it.";

#[test]
fn an_ordinary_reply_passes_untouched_with_exactly_one_model_call() {
    let m = FakeModel::start(&[ORDINARY]);
    let s = Server::start(&m);
    let id = s.new_case();
    let r = s.say(&id, "how are you feeling");
    assert!(r["reply"] == ORDINARY, "an honest reply was changed");
    assert_eq!(m.turns(), 1, "an honest reply cost more than one model call");
    assert!(!m.system_prompts()[0].contains(HINT_MARK), "a first attempt carried a retry hint");
    let log = s.gate_log(1);
    assert_eq!(log.len(), 1, "expected one gate line per reply");
    assert!(log[0].contains("action=checked"), "a clean reply was not logged as checked");
}

#[test]
fn an_unearned_reveal_never_reaches_the_learner() {
    let (leak, _) = held_back();
    let m = FakeModel::start(&[&leak]);
    let s = Server::start(&m);
    let id = s.new_case();
    let r = s.say(&id, "hello");
    let got = r["reply"].as_str().unwrap_or_default();
    assert!(got != leak, "the leaking reply reached the learner");
    assert!(got == fallback(), "after the regenerate cap the learner did not get the case's fallback line");
    // One first attempt plus REGEN_CAP regenerations, each carrying the gate's hint.
    assert_eq!(m.turns(), 3, "the gate did not regenerate up to its cap");
    let sys = m.system_prompts();
    assert!(!sys[0].contains(HINT_MARK) && sys[1].contains(HINT_MARK) && sys[2].contains(HINT_MARK),
        "regenerations did not carry the gate's hint");
    // The fallback renders like any reply.
    assert!(r["who"].is_string(), "a fallback reply lost its speaker");
    assert!(r["off_language"].is_boolean(), "a fallback reply lost its language flag");

    // The log says what the gate did and nothing of what was said.
    let log = s.gate_log(1);
    assert!(log.iter().any(|l| l.contains("action=fell_back") && l.contains("regenerations=2")),
        "the fallback was not logged");
    let whole = s.whole_log();
    assert!(!whole.contains(&leak), "gated content reached the log");
    assert!(!whole.contains(&fallback()), "reply text reached the log");

    // What she is remembered as having said is what the learner saw, not the leak — otherwise
    // the next turn's history would hand the model its own leak back.
    m.then(&[ORDINARY]);
    let _ = s.say(&id, "what happened");
    // The brief itself lists every scripted line, so only the conversation after it is read.
    let last = m.seen.lock().unwrap().last().cloned().unwrap_or_default();
    let history: Vec<String> = last["messages"]
        .as_array()
        .map(|a| a.iter().skip(1).map(|x| x["content"].to_string()).collect())
        .unwrap_or_default();
    assert!(history.len() >= 2, "the second turn carried no history");
    assert!(!history.iter().any(|c| c.contains(&leak)), "the leaked reply was kept in her history");
}

#[test]
fn a_leak_that_regenerates_clean_returns_the_clean_reply() {
    let (leak, _) = held_back();
    let m = FakeModel::start(&[&leak, ORDINARY]);
    let s = Server::start(&m);
    let id = s.new_case();
    let r = s.say(&id, "hello");
    assert!(r["reply"] == ORDINARY, "the clean regeneration was not what the learner got");
    assert_eq!(m.turns(), 2, "a clean regeneration should stop the loop");
    let log = s.gate_log(1);
    assert!(log.iter().any(|l| l.contains("action=regenerated") && l.contains("regenerations=1")),
        "the regeneration was not logged");
    assert!(!s.whole_log().contains(&leak), "gated content reached the log");
}

/// The question being answered counts. A learner who asks about the held-back thing has earned
/// it in this very reply.
#[test]
fn a_fact_asked_for_in_this_question_is_earned() {
    let (leak, kw) = held_back();
    let m = FakeModel::start(&[&leak]);
    let s = Server::start(&m);
    let id = s.new_case();
    let r = s.say(&id, &format!("tell me about {kw}"));
    assert!(r["reply"].as_str() == Some(leak.as_str()), "an earned answer was withheld");
    assert_eq!(m.turns(), 1, "an earned answer was regenerated");
}

/// And it stays earned: asked once, it may come up again later in the conversation.
#[test]
fn a_fact_asked_for_earlier_stays_earned() {
    let (leak, kw) = held_back();
    let m = FakeModel::start(&[ORDINARY]);
    let s = Server::start(&m);
    let id = s.new_case();
    let _ = s.say(&id, &format!("tell me about {kw}"));
    m.then(&[&leak]);
    let r = s.say(&id, "anything else");
    assert!(r["reply"].as_str() == Some(leak.as_str()), "an earlier question's answer was withheld");
    assert_eq!(m.turns(), 2, "an earned answer was regenerated");
}
