//! The chart is a record of orders, not a running mark sheet.
//!
//! The commit titled *"the chart says what was ordered, not what the rubric calls it"* stopped
//! the chart printing intervention ids — `exam_throat`, `dx_anaphylaxis`,
//! `adrenaline_undosed` — because an id is the rubric's own needle and names what is being
//! marked. It printed the case author's label instead, and the labels are the author's working
//! notes. Nineteen of them, across twelve stations, end in the author's verdict:
//!
//! ```text
//! 0:12 | ORDER | IV-push adrenaline (HARM)
//! 0:12 | HARM  | ⚠ harm recorded
//! ```
//!
//! The harm line is sealed until the bell. The order line above it was not, so the surface that
//! has to stay neutral was telling a candidate mid-exam that they had just got it wrong — worst
//! on `osce-d3`, where the two adrenaline doses *are* the question the station asks: pick one and
//! the chart named it as the trap, so picking the other took no clinical reasoning at all.
//!
//! `main.rs` has the unit tests for the stripping rule against every label on disk. These are the
//! same property asserted where a candidate would actually meet it: over HTTP, off a running
//! server, on the JSON the browser receives.

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
        let state = std::env::temp_dir().join(format!("vitals-chart-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&state);

        let mut child = Command::new(env!("CARGO_BIN_EXE_vitals-web"))
            .env("VITALS_WEB_BIND", "127.0.0.1:0")
            .env("VITALS_STATE_DIR", &state)
            // Thirteen cases, opened back to back. The shipped window is six new runs a minute
            // per address, which is right for a public bay and wrong for a test that has to
            // walk the whole shelf; the limit itself has its own tests in `meter`.
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

    fn open(&self, ep: &str) -> String {
        self.json(&format!("/api/new?ep={ep}"))["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{ep}: no session id"))
            .to_string()
    }

    fn order(&self, id: &str, text: &str) -> serde_json::Value {
        self.json(&format!("/api/step?id={id}&do={}", enc(text)))
    }

    fn tick(&self, id: &str, dt: f64) -> serde_json::Value {
        self.json(&format!("/api/step?id={id}&tick={dt}"))
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

/// Every case a candidate can sit, and the file behind it.
fn cases() -> Vec<(String, PathBuf)> {
    let r = repo();
    let mut v: Vec<(String, PathBuf)> =
        vec![("ep1".to_string(), r.join("conformance/sce-anaphylaxis-ep1.json"))];
    let mut stations: Vec<PathBuf> = std::fs::read_dir(r.join("demo/stations"))
        .expect("demo/stations")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().ends_with(".sce.json"))
        .collect();
    stations.sort();
    for p in stations {
        let id = p.file_name().unwrap().to_string_lossy().replace(".sce.json", "");
        v.push((id, p));
    }
    assert!(v.len() >= 13, "the shelf stopped being found: {v:?}");
    v
}

fn strings(v: &serde_json::Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| x.as_str())
        .map(|s| s.to_lowercase())
        .collect()
}

/// Free text that reaches this intervention, built from the case's own matcher.
///
/// The engine matches on substrings of the lower-cased order, first declaration wins. Taking the
/// keywords the author wrote is the only way to be sure the order lands somewhere real without
/// this test carrying its own guesses about what a station will accept.
fn query_for(iv: &serde_json::Value) -> Option<String> {
    let m = &iv["match"];
    let not = strings(&m["not_kw"]);
    let clean = |k: &String| !not.iter().any(|n| k.to_lowercase().contains(n.as_str()));

    let mut parts: Vec<String> = Vec::new();
    for g in m["all_groups"].as_array().into_iter().flatten() {
        parts.push(strings(g).into_iter().find(clean)?);
    }
    let any = strings(&m["any_kw"]);
    if !any.is_empty() {
        parts.push(any.into_iter().find(clean)?);
    }
    if parts.is_empty() {
        return None;
    }
    let text = parts.join(" ");
    // The join can manufacture an excluded keyword across a boundary (" iv"). Better to fire no
    // order than one the author excluded.
    (!not.iter().any(|n| text.contains(n.as_str()))).then_some(text)
}

fn chart_lines(v: &serde_json::Value) -> Vec<String> {
    v["chart"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["text"].as_str())
        .map(str::to_string)
        .collect()
}

/// **The regression, over the wire.** Order everything each case can be ordered, and read the
/// chart the browser reads. Nothing on it grades the order.
#[test]
fn no_chart_line_grades_the_order_that_produced_it() {
    let s = Server::start();
    let mut charted = 0usize;
    let mut hit: Vec<String> = Vec::new();

    for (ep, path) in cases() {
        let sce: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("a case")).expect("json");
        let id = s.open(&ep);
        let mut expected: Vec<String> = Vec::new();

        for iv in sce["interventions"].as_array().into_iter().flatten() {
            let label = iv["label"].as_str().unwrap_or_default();
            if label.to_ascii_uppercase().contains("HARM") {
                // What the chart must show instead: the order, with the grade taken off.
                expected.push(
                    label
                        .rsplit_once('(')
                        .map(|(head, _)| head.trim_end().to_string())
                        .unwrap_or_else(|| label.to_string()),
                );
            }
            let Some(q) = query_for(iv) else { continue };
            s.order(&id, &q);
            s.tick(&id, 2.0);
        }

        let v = s.tick(&id, 2.0);
        let lines = chart_lines(&v);
        assert!(!lines.is_empty(), "{ep}: nothing reached the chart at all");
        charted += lines.len();

        for line in &lines {
            // No exemption for `harm:sealed` any more: a sealed chart has no harm row to
            // exempt. See `a_sealed_chart_carries_no_harm_row` in `exam_integrity`.
            assert!(
                !line.to_ascii_uppercase().contains("HARM"),
                "{ep}: the chart printed {line:?} — the candidate has just been told, mid-run, \
                 that the order they gave was the mistake"
            );
        }
        // …and the order is still *named*, so this is a neutral chart rather than a blank one.
        for want in expected {
            if lines.iter().any(|l| l == &want) {
                hit.push(format!("{ep}: {want}"));
            }
        }
    }

    assert!(charted > 100, "only {charted} chart lines were produced; the drive did not drive");
    assert!(
        hit.len() >= 12,
        "only {} of the annotated orders actually reached a chart, so this test is not seeing \
         the lines it is asserting about: {hit:?}",
        hit.len()
    );
}

/// The report's own reproduction, kept as its own case because it is the one anybody will retype.
#[test]
fn ordering_iv_push_adrenaline_on_osce_a_charts_the_order_and_seals_the_verdict() {
    let s = Server::start();
    let id = s.open("osce-a");
    s.order(&id, "adrenaline iv push");
    let v = s.tick(&id, 12.0);

    assert!(v["outcome"].is_null(), "the run ended before the assertion: {v}");
    let lines = chart_lines(&v);
    assert!(
        lines.iter().any(|l| l == "IV-push adrenaline"),
        "the order is missing from the chart entirely: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l.contains("(HARM")),
        "the order line still carries the author's verdict: {lines:?}"
    );
    // The chart carries no harm row at all now — not the sentence, not a redacted stand-in.
    // A row stamped `HARM` on the same second as the order is the verdict again, in the one
    // column that cannot say it, and the seal that only took the words off left the shape.
    assert!(
        !v["chart"].as_array().expect("chart").iter().any(|c| c["kind"] == "harm"),
        "a harm row is back on a sealed chart: {v}"
    );
    // Nor on the feed's channel, which used to carry `harm:sealed` and printed it as
    // "⚠ harm recorded" the second the candidate acted.
    assert!(
        !v["beats"].as_array().expect("beats").iter().any(|b| b.as_str().is_some_and(|b| b.starts_with("harm"))),
        "a harm beat is back on a sealed reply: {v}"
    );
    // The classification itself is untouched — the bell hands all of it over.
    let end = {
        let mut last = v.clone();
        for _ in 0..40 {
            last = s.tick(&id, 30.0);
            if !last["outcome"].is_null() {
                break;
            }
        }
        last
    };
    assert!(
        end["harm"].as_array().is_some_and(|a| a.iter().any(|h| h
            .as_str()
            .is_some_and(|h| h.contains("iv push adrenaline")))),
        "the harm stopped being recorded at all: {end}"
    );
}

/// OSCE D3 is the station this mattered most on: two adrenaline doses, one for a twenty-kilo
/// child and one for an adult, and choosing between them is the whole assessment. The dose stays
/// on the chart — it is what was ordered — and the grade does not, so the chart cannot be read to
/// find out which one was the trap.
#[test]
fn the_paediatric_dose_station_charts_both_doses_the_same_way() {
    let s = Server::start();

    let wrong = s.open("osce-d3");
    s.order(&wrong, "adrenaline 0.5 mg im");
    let a = chart_lines(&s.tick(&wrong, 4.0));
    assert!(
        a.iter().any(|l| l == "Adrenaline 0.5 mg IM — adult dose"),
        "the adult dose is not charted as what was ordered: {a:?}"
    );

    let right = s.open("osce-d3");
    s.order(&right, "adrenaline 0.2 mg im");
    let b = chart_lines(&s.tick(&right, 4.0));
    assert!(
        b.iter().any(|l| l == "Adrenaline 0.2 mg IM — 0.01/kg"),
        "the paediatric dose is not charted as what was ordered: {b:?}"
    );

    // The two charts differ in the dose the candidate chose and in nothing that grades it —
    // and that now includes the row itself, which is gone rather than redacted.
    for line in a.iter().chain(b.iter()) {
        assert!(!line.contains("HARM"), "{line:?} grades the choice");
    }
    assert_eq!(a.len(), b.len(), "the wrong dose charts a different number of rows: {a:?} / {b:?}");
}
