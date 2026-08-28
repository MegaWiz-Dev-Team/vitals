//! The transcript the bell hands back, asserted against the build that shipped before the seal.
//!
//! The encounter feed is the one surface no server test can reach. It is assembled in the
//! browser out of two things that never meet on the wire — the beats the server sends, and the
//! order lines the page writes the moment a candidate presses send — and it is what the
//! candidate reads while the case runs and what the examiner reads afterwards.
//!
//! A sealed run now carries no harm beat at all, so there is no line in the feed to rewrite when
//! the bell rings: `unsealHarm()` has to *write* the harm sentences in, at the place each of them
//! would have appeared. Whether it puts them back where they were is not a thing anybody should
//! establish by looking at a screenshot.
//!
//! So: `feed_replay.mjs` pulls the real functions out of `index.html` — by name, by brace
//! matching, never a paraphrase — runs them against a small DOM and replays a recorded run. The
//! fixtures under `tests/feed/` are the transcripts the *previous* build produced, captured off
//! the binary and the page as they were before the harm beat was sealed. Same script, same case,
//! and the answer has to come out the same line for line.
//!
//! Needs `node`. Without it the test says so and passes — CI has it, and a laptop that does not
//! should not be told its change is broken. Everything else about the seal is asserted over HTTP
//! in `exam_integrity`, which needs nothing but the server.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

struct Server {
    child: Child,
    port: u16,
    state: PathBuf,
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
        let state = std::env::temp_dir().join(format!("vitals-feed-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&state);
        let mut child = Command::new(env!("CARGO_BIN_EXE_vitals-web"))
            .env("VITALS_WEB_BIND", "127.0.0.1:0")
            .env("VITALS_STATE_DIR", &state)
            .env("VITALS_TURNS_PER_MIN", "600")
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
}

fn enc(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => c.to_string(),
            other => format!("%{:02X}", other as u32),
        })
        .collect()
}

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn have_node() -> bool {
    Command::new("node").arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Play a fixture's script and record what the page would have had to work with: the order line
/// the candidate wrote, if any, and the reply that came back — one entry per paint.
fn record(s: &Server, ep: &str, script: &serde_json::Value) -> serde_json::Value {
    let id = s.json(&format!("/api/new?ep={ep}"))["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{ep}: no session id"))
        .to_string();
    let mut steps: Vec<serde_json::Value> = Vec::new();
    let mut ended = false;

    for act in script.as_array().expect("a script") {
        let (kind, arg) = (act[0].as_str().unwrap_or_default(), &act[1]);
        let v = match kind {
            // An order does not advance the clock — the page sends `&do=` on its own, which is
            // what makes placing a harm hard: the tick before it reads the very same second.
            "order" => s.json(&format!("/api/step?id={id}&do={}", enc(arg.as_str().unwrap_or_default()))),
            _ => s.json(&format!("/api/step?id={id}&tick={}", arg.as_f64().unwrap_or(0.0))),
        };
        let order = if kind == "order" { arg.clone() } else { serde_json::Value::Null };
        ended = !v["outcome"].is_null();
        steps.push(serde_json::json!({ "order": order, "v": v }));
        if ended {
            break;
        }
    }
    if !ended {
        for _ in 0..300 {
            let v = s.json(&format!("/api/step?id={id}&tick=15"));
            let done = !v["outcome"].is_null();
            steps.push(serde_json::json!({ "order": serde_json::Value::Null, "v": v }));
            if done {
                ended = true;
                break;
            }
        }
    }
    assert!(ended, "{ep}: the run never reached the bell, so there is no transcript to compare");
    serde_json::Value::Array(steps)
}

/// Run the page's own feed code over a recorded run and return the transcript it builds.
fn transcript(run: &serde_json::Value, mode: &str, tag: &str) -> serde_json::Value {
    let path = std::env::temp_dir().join(format!("vitals-run-{}-{tag}.json", std::process::id()));
    std::fs::write(&path, run.to_string()).expect("write the run");
    let out = Command::new("node")
        .arg(repo().join("crates/vitals-web/tests/feed_replay.mjs"))
        .arg(repo().join("crates/vitals-web/static/index.html"))
        .arg(&path)
        .arg(mode)
        .output()
        .expect("run node");
    let _ = std::fs::remove_file(&path);
    assert!(
        out.status.success(),
        "the feed replay would not run — the page's own functions no longer load:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("the replay printed something that is not JSON")
}

fn fixtures() -> Vec<(String, serde_json::Value)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/feed");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("tests/feed")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    assert!(files.len() >= 5, "the transcript fixtures went missing: {files:?}");
    files
        .into_iter()
        .map(|p| {
            let v: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&p).expect("a fixture"))
                    .unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            (p.file_stem().unwrap().to_string_lossy().to_string(), v)
        })
        .collect()
}

/// **The assertion the seal is judged on.** After the bell, the feed reads exactly as it read
/// before any of this — same sentences, same order, same positions, same tooltips.
///
/// The two shapes that make this hard are both in the fixtures. A harm whose order produced no
/// other beat has nothing to sit between, and the tick before it carries the same clock, so
/// there is nothing in the reply that separates the two paints; the chart row the order wrote is
/// what settles it. A harm the clock fired at the bell arrives in the same paint as the terminal
/// beat and has to land in front of it, not after.
#[test]
fn the_bell_puts_the_harm_lines_back_exactly_where_they_were() {
    if !have_node() {
        println!("skip: node is not installed — CI runs this");
        return;
    }
    let s = Server::start();
    for (ep, fix) in fixtures() {
        let want = &fix["transcript"];
        let mode = fix["mode"].as_str().unwrap_or("exam");
        let run = record(&s, &ep, &fix["script"]);
        let got = transcript(&run, mode, &ep);

        assert_eq!(
            got.as_array().map(Vec::len),
            want.as_array().map(Vec::len),
            "{ep}: the feed is a different length after the bell\n got  {got:#}\n want {want:#}"
        );
        for (i, (a, b)) in got
            .as_array()
            .unwrap()
            .iter()
            .zip(want.as_array().unwrap())
            .enumerate()
        {
            assert_eq!(a, b, "{ep}: line {i} of the transcript moved or changed\n got  {a}\n want {b}");
        }
        // The fixtures are only worth something if they contain the thing under test.
        let harms = want.as_array().unwrap().iter().filter(|l| l["kind"] == "harm").count();
        assert!(harms > 0, "{ep}: this fixture has no harm line in it at all");
    }
}

/// While the clock runs, nothing in the feed says a harm happened — on any station, at any tick.
/// The same property `exam_integrity` asserts on the wire, asserted here on what is drawn.
#[test]
fn a_sealed_run_draws_no_harm_line_while_it_plays() {
    if !have_node() {
        println!("skip: node is not installed — CI runs this");
        return;
    }
    let s = Server::start();
    for (ep, fix) in fixtures() {
        if fix["mode"].as_str() != Some("exam") {
            continue;
        }
        let run = record(&s, &ep, &fix["script"]);
        let steps = run.as_array().expect("steps");
        // Every paint but the last one is a paint with the clock still running.
        for (i, step) in steps.iter().take(steps.len() - 1).enumerate() {
            let beats = step["v"]["beats"].as_array().expect("beats");
            let harm: Vec<&serde_json::Value> =
                beats.iter().filter(|b| b.as_str().is_some_and(|b| b.starts_with("harm"))).collect();
            assert!(
                harm.is_empty(),
                "{ep}: paint {i} carried {harm:?} — the feed would print a harm marker the \
                 instant the candidate acted, which is the verdict the seal exists to withhold"
            );
        }
        // …and the last one, the bell, hands every one of them back.
        let end = &steps[steps.len() - 1]["v"];
        assert!(
            end["beats"].as_array().is_some_and(|a| a
                .iter()
                .any(|b| b.as_str().is_some_and(|b| b.starts_with("harm:") && b != "harm:sealed"))),
            "{ep}: the bell never gave the harm beats back: {end}"
        );
    }
}

/// A practice episode is not sealed at either end. The server sends the harm beat when it
/// happens, the feed prints it there, and the bell has nothing to put back — the filter that
/// makes an exam work must not reach into a lesson.
#[test]
fn a_practice_episode_prints_its_harm_where_it_happens() {
    if !have_node() {
        println!("skip: node is not installed — CI runs this");
        return;
    }
    let s = Server::start();
    let (ep, fix) = fixtures()
        .into_iter()
        .find(|(_, f)| f["mode"].as_str() == Some("practice"))
        .expect("a practice fixture");
    let run = record(&s, &ep, &fix["script"]);
    let steps = run.as_array().expect("steps");
    assert!(
        steps.iter().take(steps.len() - 1).any(|st| st["v"]["beats"]
            .as_array()
            .is_some_and(|a| a.iter().any(|b| b.as_str().is_some_and(|b| b.starts_with("harm:"))))),
        "{ep}: a practice run stopped saying what went wrong while it was still teachable"
    );
    // And the transcript is the one the previous build drew, so no line was doubled by the bell
    // writing back a sentence the feed had already printed.
    assert_eq!(transcript(&run, "practice", &ep), fix["transcript"], "{ep}: the practice feed moved");
}
