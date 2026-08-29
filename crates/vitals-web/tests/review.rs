//! The reviewer's form, driven the way the two reviewers will drive it.
//!
//! `/api/review` is the first route in this server that reads a request body. Everything else is
//! a GET with query parameters, including the player's own free text — and a reviewer's answers
//! do not fit in a URL: eight thousand Thai characters in a box, nine bytes a character once
//! percent-encoded, roughly 72KB in one address. So this endpoint is a different shape from every
//! other one, and the things a different shape can get wrong are all here:
//!
//!   * a body larger than the server will hold — refused whole, never quietly cut down
//!   * a body that is not UTF-8 — refused, never `from_utf8_lossy`'d into mojibake
//!   * Thai that has to arrive as the exact characters that were typed, English medical terms and
//!     all, through JSON, through the store, and back off disk
//!   * the same review sent twice, which is one record, and two different ones, which are two
//!
//! And the property that has nothing to do with the shape and everything to do with what this
//! product claims: **a review is data recorded alongside a run and never an input to one.** A
//! leaf is the hash of a tape; a mark sheet is a function of that tape and a rubric. An opinion
//! about the product cannot be allowed to move either, or the anchor stops meaning what the
//! landing page says it means. That is the last test in this file, and it is the one to read
//! first if any of this is ever rewritten.

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
        Server::start_with(None)
    }

    /// `token` set is the shipped shape of a public deployment: a bearer token on everything that
    /// signs or spends. The form and its route must open anyway — see the test that pins it.
    fn start_with(token: Option<&str>) -> Server {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let state = std::env::temp_dir().join(format!("vitals-review-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&state);

        let mut cmd = Command::new(env!("CARGO_BIN_EXE_vitals-web"));
        cmd.env("VITALS_WEB_BIND", "127.0.0.1:0")
            .env("VITALS_STATE_DIR", &state)
            // The shipped window is six a minute per address, which is right for a public
            // endpoint and wrong for a test that submits a dozen reviews from one loopback
            // address in half a second. The window itself is `meter`'s to test.
            .env("VITALS_TURNS_PER_MIN", "600")
            .env_remove("VITALS_PROGRAM_ID")
            .env_remove("VITALS_TOKEN")
            .env_remove("HEIMDALL_API_KEY")
            .stdout(Stdio::piped());
        if let Some(t) = token {
            cmd.env("VITALS_TOKEN", t);
        }
        let mut child = cmd.spawn().expect("start vitals-web");
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

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    fn get(&self, path: &str) -> (u16, String) {
        match ureq::get(&self.url(path)).call() {
            Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
            Err(ureq::Error::Status(c, r)) => (c, r.into_string().unwrap_or_default()),
            Err(e) => panic!("{}: {e}", self.url(path)),
        }
    }

    fn json(&self, path: &str) -> serde_json::Value {
        serde_json::from_str(&self.get(path).1).unwrap_or(serde_json::Value::Null)
    }

    /// Bytes, not a string: one of the cases below is deliberately not UTF-8, and a helper that
    /// could only send text could not ask the question.
    fn post(&self, path: &str, body: &[u8]) -> (u16, serde_json::Value) {
        let r = ureq::post(&self.url(path))
            .set("Content-Type", "application/json")
            .send_bytes(body);
        let (code, text) = match r {
            Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
            Err(ureq::Error::Status(c, r)) => (c, r.into_string().unwrap_or_default()),
            Err(e) => panic!("{}: {e}", self.url(path)),
        };
        (code, serde_json::from_str(&text).unwrap_or(serde_json::Value::Null))
    }

    /// Every review on disk, keyed the way the store keys it. The reader is the test, because
    /// there is deliberately no route that reads these back — a reviewer's opinion of the
    /// product is not public, and a page for it would need an identity this endpoint does not
    /// have.
    fn filed(&self) -> Vec<(String, serde_json::Value)> {
        let dir = self.state.join("review");
        let Ok(rd) = std::fs::read_dir(&dir) else { return Vec::new() };
        let mut out: Vec<(String, serde_json::Value)> = rd
            .flatten()
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
            .map(|e| {
                let key = e.path().file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
                // Read as bytes and decode strictly. `from_utf8_lossy` here would hide exactly
                // the failure these tests exist to catch.
                let bytes = std::fs::read(e.path()).expect("read record");
                let text = String::from_utf8(bytes).expect("record is not UTF-8");
                (key, serde_json::from_str(&text).expect("record is not JSON"))
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// The raw bytes of the single record on disk, for the tests that care what is *in* the file
    /// rather than what parses out of it.
    fn only_record_bytes(&self) -> Vec<u8> {
        let f = self.filed();
        assert_eq!(f.len(), 1, "expected exactly one record, found {}", f.len());
        std::fs::read(self.state.join("review").join(format!("{}.json", f[0].0))).expect("read")
    }
}

// ── real Thai, lifted from the documents these two people were sent ─────────────────────────
//
// Not invented for the test. `REVIEW_REQUEST_SIRAWIT_CLINICAL_RULINGS.md` asks about the ECG
// film in `osce-b`, and this is the shape the answer comes back in: Thai prose with the medical
// terms left in English, because that is how the answer would actually be written. If the
// encoding path is wrong anywhere — the body read, the JSON parse, the character clamp, the
// store, the file — one of these strings comes back different, and the assertion says which.
const SAID_ECG: &str = "ยืนยันครับ ไม่แสดงฟิล์มดีกว่า — เอา old anterior infarct จาก PTB-XL มาใช้แทน \
acute STEMI ไม่ได้ เพราะสถานีนี้สอนเรื่อง door-to-balloon ภายใน 10 นาที ผู้เรียนที่เห็นฟิล์มเก่า \
แล้วถูกบอกว่านี่คือ STEMI เฉียบพลัน จะจำ pattern ที่ผิดไปใช้ข้างเตียง";
const SAID_ADRENALINE: &str = "0.2 mg IM ถูกต้องครับ (0.01 mg/kg ในเด็ก 20 กก.) \
ส่วนการหักคะแนนเมื่อให้ขนาดผู้ใหญ่ 0.5 mg ผมเห็นด้วยว่าควรหัก แม้เด็กจะรอด — \
เพราะ anaphylaxis ในเด็กคือเคสที่ dose error เกิดบ่อยที่สุด และ OSCE คือที่ที่ควรเจอครั้งแรก \
ไม่ใช่ที่ ER";
const NOTES_THAI: &str = "หมวด 1 ผมเห็นด้วยกับ asystole เป็นจุดจบร่วมของ ep2 ครับ\n\
ส่วน ep3 — PEA ที่ 148 ยังอ่านแปลก ๆ อยู่ ถ้าเขียนให้ชีพจรลงจาก 148 → 60 ใน 90 วินาที \
ก่อนเข้า arrest จะเป็น bradyasystolic ที่ถูกต้องกว่า";

fn student_body(notes: &str) -> String {
    serde_json::json!({
        "role": "student",
        "name": "มุก",
        "contact": "LINE: mook",
        "answers": [
            { "id": "th-krap-a",
              "asked": "7 · คำลงท้าย — สมชาย (osce-a, 71 ปี) ไม่มี “ครับ” เลยสักคำ",
              "chose": "some", "chose_label": "ควรมีบ้าง — บอกว่าตรงไหนข้างล่าง",
              "said": "ควรมีตรงประโยคแรกกับตอนที่ยอมให้ข้อมูลค่ะ ที่เหลือห้วนได้" },
        ],
        "notes": notes,
        "revision": "vitals 0.5.1",
    })
    .to_string()
}

// ── the form ────────────────────────────────────────────────────────────────────────────────

/// One URL, and it opens. That is the whole requirement: the two people this was built for are a
/// final-year medical student and a physician, and any step between the link and the first
/// question is a step at which the review does not happen.
#[test]
fn the_form_is_served_at_one_url() {
    let s = Server::start();
    let (code, html) = s.get("/review");
    assert_eq!(code, 200, "the form did not open");
    // The page carries both lists and picks between them in the browser. These are the items each
    // document opens with — the physician's most dangerous section, and the student's first
    // flagged line — plus the two things the documents said the old form could not carry: the
    // context that lets a reviewer answer without the document open, and a way to agree.
    assert!(html.contains("ep2 — STEMI, จุดจบที่หัวใจหยุดเต้น"), "หมวด 1 is not on the page");
    assert!(html.contains("leather creaking on leather"), "the student's flagged lines are not on the page");
    assert!(html.contains("ทำไมเราคิดว่าอาจผิด"), "the items carry no context to answer from");
    assert!(html.contains("ยืนยัน — PEA"), "there is no way to agree with what we already do");
    assert!(html.contains("อัดเสียงส่งกลับมาทางเดิม"), "the invitation to answer by voice is gone");
    assert!(!html.contains("type=\"file\""), "the form grew an upload it cannot honour");
    assert!(html.contains("/api/review"), "the form does not know where to post");
}

/// The build stamp is substituted on the way out, and it is load-bearing twice.
///
/// It records which build a reviewer's answers were written about — "the timing felt wrong" means
/// one thing against 0.5.1 and another against whatever ships after the physician's rulings land.
/// And its *absence* is how a copy of this page that was mailed, or opened off disk, knows it has
/// no server to post to and hands the answers back to be sent by hand instead. A served copy that
/// still carried the placeholder would take an hour of typing and post it nowhere.
#[test]
fn the_served_copy_is_stamped_and_the_standalone_copy_is_not() {
    let s = Server::start();
    let (_, html) = s.get("/review");
    assert!(!html.contains("__VITALS_BUILD__"), "the served page still carries the placeholder");
    assert!(
        html.contains(&format!("content=\"vitals {}\"", env!("CARGO_PKG_VERSION"))),
        "the served page is not stamped with the build"
    );
    let raw = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static/review.html"),
    )
    .expect("static/review.html");
    assert!(raw.contains("__VITALS_BUILD__"), "the standalone copy lost its placeholder");
}

/// Neither the form nor the route may need a token, on a server that has one.
///
/// A bearer token is right for the endpoints that make this process sign or spend. It is wrong
/// here: the people being asked to review are not engineers, have no account and no wallet, and
/// were handed a link. A 401 on this route is a review that does not arrive, and we would never
/// learn that it did not.
#[test]
fn a_reviewer_needs_no_token_on_a_server_that_has_one() {
    let s = Server::start_with(Some("a-secret-nobody-gave-the-reviewers"));
    assert_eq!(s.get("/review").0, 200, "the form asked a reviewer for a token");
    let (code, body) = s.post("/api/review", student_body("ไม่มีอะไรเพิ่มครับ").as_bytes());
    assert_eq!(code, 200, "the route asked a reviewer for a token: {body}");
}

// ── what is stored is what was typed ────────────────────────────────────────────────────────

/// The one that matters most. Thai in, Thai out, character for character.
///
/// This repo has been bitten on the encoding path before — the query-string decoder read
/// percent-escaped Thai as Latin-1 and put mojibake on the tape — and every layer here is a
/// chance to do it again: the body read, the UTF-8 decode, `serde_json`, the character clamp in
/// `review.rs`, `serde_json` again on the way to disk. So the assertion is made twice: once on
/// the parsed record, and once on the raw bytes of the file, where a re-encoded or escaped
/// string would show up as bytes that no longer contain the sentence.
#[test]
fn thai_arrives_as_the_characters_that_were_typed() {
    let s = Server::start();
    let body = serde_json::json!({
        "role": "physician",
        "name": "นพ.ศิรวิทย์ ตันศิริ",
        "contact": "sirawit@example.ac.th",
        "answers": [
            { "id": "p-ecg-stemi", "asked": "ECG ของ STEMI เฉียบพลัน", "said": SAID_ECG },
            { "id": "p2", "asked": "เด็กหญิง 6 ขวบ 20 กก.", "said": SAID_ADRENALINE },
        ],
        "notes": NOTES_THAI,
    })
    .to_string();

    let (code, reply) = s.post("/api/review", body.as_bytes());
    assert_eq!(code, 200, "{reply}");
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["answers"], 2);

    let filed = s.filed();
    assert_eq!(filed.len(), 1, "one submission, one record");
    let rec = &filed[0].1;
    assert_eq!(rec["role"], "physician");
    assert_eq!(rec["name"], "นพ.ศิรวิทย์ ตันศิริ");
    assert_eq!(rec["answers"][0]["said"], SAID_ECG);
    assert_eq!(rec["answers"][0]["asked"], "ECG ของ STEMI เฉียบพลัน");
    assert_eq!(rec["answers"][1]["said"], SAID_ADRENALINE);
    assert_eq!(rec["notes"], NOTES_THAI);
    // The medical terms the physician left in English are part of the answer, not decoration:
    // "old anterior infarct", "PTB-XL", "door-to-balloon", "0.01 mg/kg".
    let said = rec["answers"][0]["said"].as_str().unwrap();
    for term in ["old anterior infarct", "PTB-XL", "door-to-balloon", "acute STEMI"] {
        assert!(said.contains(term), "the English term {term:?} did not survive");
    }
    assert!(rec["answers"][1]["said"].as_str().unwrap().contains("0.01 mg/kg"));

    // And on disk, as bytes. A record whose Thai was escaped to `\uXXXX` or re-encoded would
    // parse identically above and be unreadable to anyone opening the file.
    let raw = s.only_record_bytes();
    let text = String::from_utf8(raw).expect("the record on disk is not UTF-8");
    assert!(text.contains(SAID_ECG), "the file does not contain the sentence that was typed");
    assert!(text.contains("นพ.ศิรวิทย์ ตันศิริ"));

    // The server stamps the time, not the client. Nothing in the body above said when this was.
    assert!(rec["at"].as_u64().unwrap_or(0) > 1_700_000_000, "no server-side timestamp");
    assert!(filed[0].0.starts_with(&format!("{:010}-", rec["at"].as_u64().unwrap())));
}

/// A full review at the form's own ceiling, which is what the cap has to fit.
///
/// The physician's list is the twenty-eight rulings his document asks for, each carrying that
/// document's four lines of context so he can answer from a phone with nothing open beside him.
/// Every box filled to the four thousand characters `review.rs` keeps, an option picked on every
/// item, the eight-thousand-character notes box, all in Thai at three bytes a character: the page
/// itself produces 381 KiB at that ceiling, measured. It must be *taken*, not refused — a
/// reviewer who answered everything is the one we most want to hear from, and he is also the one
/// a cap left at the old 256 KiB would have turned away.
#[test]
fn the_largest_review_the_form_can_produce_is_taken_whole() {
    let s = Server::start();
    let answers: Vec<serde_json::Value> = (1..=28)
        .map(|i| {
            serde_json::json!({
                "id": format!("r-{i}"),
                // The four lines, at the length the longest real item reaches.
                "asked": "ตอนนี้ระบบทำอะไร · ทำไมเราคิดว่าอาจผิด · เราจะแก้เป็น · คำถาม ".repeat(19),
                "chose": "asys",
                "chose_label": "ยืนยัน — asystole เป็นจุดจบร่วมของทั้งสองทาง",
                "said": "ก".repeat(4000),
            })
        })
        .collect();
    let body = serde_json::json!({
        "role": "physician", "name": "นพ.ศิรวิทย์ ตันศิริ",
        "answers": answers, "notes": "ข".repeat(8000),
    })
    .to_string();
    assert!(
        body.len() > 380 * 1024,
        "the ceiling case is smaller than the page can actually produce: {} bytes",
        body.len()
    );

    let (code, reply) = s.post("/api/review", body.as_bytes());
    assert_eq!(code, 200, "a full review was refused at {} bytes: {reply}", body.len());

    let filed = s.filed();
    let rec = &filed[0].1;
    assert_eq!(rec["answers"].as_array().unwrap().len(), 28, "answers were dropped");
    for a in rec["answers"].as_array().unwrap() {
        assert_eq!(a["said"].as_str().unwrap().chars().count(), 4000, "an answer was cut short");
        assert!(a["asked"].as_str().unwrap().chars().count() > 400, "the item's context was cut");
        assert_eq!(a["chose"], "asys", "the chosen branch was dropped");
    }
    assert_eq!(rec["notes"].as_str().unwrap().chars().count(), 8000);
}

/// A valid submission padded to exactly the cap, and one byte past it.
///
/// The pair is the point: the limit is a line, not a suggestion, and the answer on the far side
/// of it is a refusal with a number in it — never a shorter record that looks whole.
#[test]
fn the_cap_is_a_line_and_the_far_side_of_it_is_refused() {
    const CAP: usize = 1024 * 1024;
    // Padding in ASCII so the arithmetic is exact; the Thai path is proven above.
    let body = |n: usize| {
        let (head, tail) = (r#"{"role":"physician","notes":""#, r#""}"#);
        format!("{head}{}{tail}", "x".repeat(n - head.len() - tail.len()))
    };

    let s = Server::start();
    let at = body(CAP);
    assert_eq!(at.len(), CAP);
    let (code, reply) = s.post("/api/review", at.as_bytes());
    assert_eq!(code, 200, "a body of exactly the cap was refused: {reply}");
    assert_eq!(s.filed().len(), 1);

    let over = body(CAP + 1);
    let (code, reply) = s.post("/api/review", over.as_bytes());
    assert_eq!(code, 413, "a body past the cap was not refused: {reply}");
    assert_eq!(reply["limit"], CAP, "the refusal does not say what the limit is");
    assert_eq!(s.filed().len(), 1, "the oversized body left something on disk");

    // Comfortably past, the way a paste of the wrong thing would be.
    let (code, _) = s.post("/api/review", body(CAP * 4).as_bytes());
    assert_eq!(code, 413);
    assert_eq!(s.filed().len(), 1);
}

/// Not UTF-8 is refused, not repaired.
///
/// `from_utf8_lossy` is right one screen away, on query parameters, where a mangled order is a
/// word that matches nothing and sits visibly on the tape. It is wrong here: it would put ``
/// where half a Thai character used to be, in the middle of a physician's ruling, and store it as
/// though it were what they wrote. Nobody would ever find out.
#[test]
fn a_body_that_is_not_utf8_is_refused_rather_than_repaired() {
    let s = Server::start();
    // A valid submission with one Thai character cut in half — exactly what a byte-wise truncation
    // upstream would hand us.
    let good = r#"{"role":"student","notes":"ตอบแล้ว"}"#.as_bytes().to_vec();
    let mut broken = good.clone();
    broken.truncate(broken.len() - 4);
    broken.extend_from_slice(b"\"}");

    let (code, reply) = s.post("/api/review", &broken);
    assert_eq!(code, 400, "a body that was not UTF-8 was accepted: {reply}");
    assert!(s.filed().is_empty(), "mojibake reached the store");

    // The same bytes, whole, are taken — so the refusal above is about the encoding and not
    // about the shape.
    assert_eq!(s.post("/api/review", &good).0, 200);
}

/// Nothing typed is not a review, and the refusal says so.
#[test]
fn an_empty_submission_is_refused_and_says_which_way_it_was_empty() {
    let s = Server::start();
    for (body, want) in [
        (r#"{"role":"physician"}"#, "empty"),
        (r#"{"role":"physician","notes":"   ","answers":[]}"#, "empty"),
        (r#"{"role":"physician","answers":[{"id":"p1","asked":"a","said":"  "}]}"#, "empty"),
        // A role the store does not know. Not silently filed under one of the two, because a
        // student's answer and a physician's carry different weight and must never be conflated.
        (r#"{"role":"dean","notes":"สวัสดีครับ"}"#, "role"),
        (r#"{"notes":"สวัสดีครับ"}"#, "role"),
    ] {
        let (code, reply) = s.post("/api/review", body.as_bytes());
        assert_eq!(code, 400, "{body} was accepted");
        assert_eq!(reply["error"], want, "{body} was refused for the wrong reason");
    }
    // Not JSON at all — the usual cause is a chat app that wrapped the paste.
    let (code, reply) = s.post("/api/review", b"{not json");
    assert_eq!(code, 400);
    assert_eq!(reply["error"], "not json");
    assert!(s.filed().is_empty(), "a refused submission left something on disk");
}

/// **Agreeing arrives, and it arrives as agreement.**
///
/// The physician's document ranks หมวด 1 as the most dangerous thing in it and asks him to
/// *confirm* four arrest rhythms we have already chosen. Confirming is the likeliest answer to
/// all four — and before the form carried a branch it was the one answer that could not be sent:
/// an empty box, dropped as unanswered, indistinguishable from four rulings he never reached. We
/// would have gone back and asked him again for something he had already told us.
#[test]
fn agreement_arrives_and_is_not_mistaken_for_silence() {
    let s = Server::start();
    let body = serde_json::json!({
        "role": "physician",
        "name": "นพ.ศิรวิทย์ ตันศิริ",
        "answers": [
            // Three confirmed with nothing typed, one confirmed with a note, one left alone.
            { "id": "r-ep2", "asked": "1.1 · ep2 — STEMI, จุดจบที่หัวใจหยุดเต้น",
              "chose": "asys", "chose_label": "ยืนยัน — asystole เป็นจุดจบร่วมของทั้งสองทาง", "said": "" },
            { "id": "r-ep5", "asked": "1.3 · ep5 — บาดเจ็บ เสียเลือดจนหมด",
              "chose": "pea", "chose_label": "ยืนยัน — PEA", "said": "" },
            { "id": "r-ep4", "asked": "1.4 · ep4 — pulmonary embolism",
              "chose": "pea", "chose_label": "ยืนยัน — PEA", "said": "" },
            { "id": "r-ep3", "asked": "1.2 · ep3 — เด็ก 5 ขวบ, epiglottitis",
              "chose": "asys", "chose_label": "asystole — และให้เคสพาชีพจรลงก่อน",
              "said": "ให้ลงจาก 148 → 60 ใน 90 วินาทีก่อนเข้า arrest ครับ" },
            { "id": "news2-under16", "asked": "3.1 · NEWS2 กับผู้ป่วยอายุต่ำกว่า 16 ปี", "said": "" }
        ],
    })
    .to_string();
    let (code, reply) = s.post("/api/review", body.as_bytes());
    assert_eq!(code, 200, "{reply}");

    let rec = &s.filed()[0].1;
    let answers = rec["answers"].as_array().unwrap();
    assert_eq!(answers.len(), 4, "an item answered only by picking an option was dropped");
    let ids: Vec<&str> = answers.iter().map(|a| a["id"].as_str().unwrap()).collect();
    assert!(!ids.contains(&"news2-under16"), "an item the reviewer never touched was stored");

    // The branch is a value, not prose somebody has to re-read to find out which way it went.
    let ep2 = answers.iter().find(|a| a["id"] == "r-ep2").unwrap();
    assert_eq!(ep2["chose"], "asys");
    assert_eq!(ep2["chose_label"], "ยืนยัน — asystole เป็นจุดจบร่วมของทั้งสองทาง");
    assert_eq!(ep2["said"], "", "agreement was recorded as prose it never was");
    // And a branch with prose beside it keeps both.
    let ep3 = answers.iter().find(|a| a["id"] == "r-ep3").unwrap();
    assert_eq!(ep3["chose"], "asys");
    assert!(ep3["said"].as_str().unwrap().contains("148 → 60"));
}

/// Two physicians ruling opposite ways, neither writing a word, are two records — not one
/// overwriting the other because their empty boxes hashed the same.
#[test]
fn opposite_rulings_with_no_prose_are_two_records() {
    let s = Server::start();
    let one = |chose: &str, name: &str| {
        serde_json::json!({
            "role": "physician", "name": name,
            "answers": [{ "id": "win-means", "asked": "2.1 · “ชนะ” ในเกมนี้ ควรแปลว่าอะไร",
                          "chose": chose, "chose_label": "…", "said": "" }],
        })
        .to_string()
    };
    let (a, ra) = s.post("/api/review", one("a", "อาจารย์ ก").as_bytes());
    let (b, rb) = s.post("/api/review", one("b", "อาจารย์ ข").as_bytes());
    assert_eq!((a, b), (200, 200));
    assert_ne!(ra["id"], rb["id"]);
    assert_eq!(s.filed().len(), 2, "one ruling overwrote the other");
}

// ── the same reviewer, twice ────────────────────────────────────────────────────────────────

/// Send tapped twice on a bad connection is one review, not two.
///
/// And the same reviewer coming back tomorrow with more to say is two, not an overwrite of
/// yesterday.
///
/// **The second send is deliberately made to land in a later second.** Over loopback both posts
/// finish inside the same second, the key's timestamp prefix matches by luck, and a test that did
/// not wait would pass while the shipped server filed two records — which is exactly what it did
/// when this was driven by hand: eighteen seconds apart, one review, two files, and the keys
/// themselves saying so with a shared content hash.
#[test]
fn the_same_reviewer_twice_is_one_record_or_two_by_what_they_said() {
    let s = Server::start();

    let again = student_body("ครั้งแรกครับ");
    let (a, ra) = s.post("/api/review", again.as_bytes());
    std::thread::sleep(std::time::Duration::from_millis(1_100));
    let (b, rb) = s.post("/api/review", again.as_bytes());
    assert_eq!((a, b), (200, 200));
    assert_eq!(ra["id"], rb["id"], "a resend was filed under a different key");
    assert_eq!(s.filed().len(), 1, "tapping Send twice counted the reviewer twice");
    // The record keeps the second it first arrived in, not the second it was resent in.
    assert!(s.filed()[0].0.starts_with(ra["id"].as_str().unwrap().split('-').next().unwrap()));

    // Same person, more to say. That is a second review and must not overwrite the first.
    let (c, rc) = s.post("/api/review", student_body("นึกได้อีกข้อครับ").as_bytes());
    assert_eq!(c, 200);
    assert_ne!(rc["id"], ra["id"]);
    let filed = s.filed();
    assert_eq!(filed.len(), 2, "the second review replaced the first");
    let notes: Vec<&str> = filed.iter().map(|(_, r)| r["notes"].as_str().unwrap_or("")).collect();
    assert!(notes.contains(&"ครั้งแรกครับ") && notes.contains(&"นึกได้อีกข้อครับ"));
}

// ── the property the product depends on ─────────────────────────────────────────────────────

/// **A reviewer's opinion is data beside a run and never an input to one.**
///
/// The leaf is the hash of a tape and the mark sheet is a function of that tape and a rubric.
/// Neither may move because somebody filed an opinion about the product — not the run the review
/// is *about*, and not an identical run played afterwards. If either could move, the anchor stops
/// meaning what the landing page says it means, and the mark sheet stops being a function of what
/// the candidate did.
///
/// Asserted twice, because there are two ways to break it: on the same session across a
/// submission, and on a second identical run of the same case after one has been filed.
#[test]
fn a_review_never_reaches_the_leaf_or_the_mark_sheet() {
    let s = Server::start();

    // A run with a mark sheet behind it, driven to the bell by the clock rather than by an
    // ending — deterministic, and it exercises the scorer either way.
    let play = |ep: &str| -> (String, serde_json::Value, serde_json::Value) {
        let id = s.json(&format!("/api/new?ep={ep}"))["id"]
            .as_str()
            .expect("a session id")
            .to_string();
        for order in ["adrenaline im", "oxygen", "normal saline bolus"] {
            s.json(&format!("/api/step?id={id}&do={}", order.replace(' ', "%20")));
        }
        for _ in 0..12 {
            let v = s.json(&format!("/api/step?id={id}&tick=60"));
            if v["over"] == serde_json::Value::Bool(true) {
                break;
            }
        }
        let view = s.json(&format!("/api/finish?id={id}"));
        let marks = s.json(&format!("/api/marks?id={id}"));
        (id, view, marks)
    };

    let (first_id, before, marks_before) = play("osce-a");
    let leaf = before["leaf"].as_str().expect("a finished run has a leaf").to_string();
    assert!(!leaf.is_empty());
    let score = marks_before["score"].clone();
    assert!(score.is_number(), "no mark sheet to compare: {marks_before}");
    // The tape is what the leaf hashes, so it is the thing that must not gain a step.
    let tape_before = s.json(&format!("/api/tape?id={first_id}"));

    // A review that is *about this very case*, naming it, filed while the run sits finished.
    let body = serde_json::json!({
        "role": "physician",
        "name": "นพ.ศิรวิทย์ ตันศิริ",
        "answers": [{
            "id": "p1", "asked": "เกณฑ์เวลา adrenaline",
            "said": "osce-a ควรเป็น 5 นาทีครับ ส่วน ep1 ที่ 60 วินาทีเข้มไป",
        }],
        "notes": NOTES_THAI,
    })
    .to_string();
    let (code, _) = s.post("/api/review", body.as_bytes());
    assert_eq!(code, 200);
    assert_eq!(s.filed().len(), 1, "the review was not stored — the test below would be vacuous");

    // 1. The very run the review was filed against, read again across the submission.
    let after = s.json(&format!("/api/finish?id={first_id}"));
    let marks_after = s.json(&format!("/api/marks?id={first_id}"));
    assert_eq!(after["leaf"].as_str(), Some(leaf.as_str()), "a review moved this run's leaf");
    assert_eq!(after, before, "a review changed the run it was filed against");
    assert_eq!(marks_after, marks_before, "a review changed this run's mark sheet");
    assert_eq!(
        s.json(&format!("/api/tape?id={first_id}")),
        tape_before,
        "a review put something on the tape"
    );

    // 2. An identical run, played from scratch after the review was filed.
    let (_, again, marks_again) = play("osce-a");
    assert_eq!(
        again["leaf"].as_str(),
        Some(leaf.as_str()),
        "a review moved the leaf of an identical run"
    );
    assert_eq!(marks_again["score"], score, "a review moved the score of an identical run");
    assert_eq!(again["sce_hash"], before["sce_hash"]);
    assert_eq!(again["outcome"], before["outcome"]);
    assert_eq!(again["elapsed"], before["elapsed"]);
    // The whole mark sheet, not only its total.
    assert_eq!(marks_again, marks_before, "a review moved the mark sheet");

    // And the review is where it is supposed to be, and nowhere near the run's own kinds.
    assert!(s.state.join("review").is_dir(), "reviews are not under their own kind");
    assert!(
        std::fs::read_dir(s.state.join("review")).unwrap().count() == 1,
        "more than the one review was written"
    );
}

/// A review outlives the sweep that clears abandoned runs.
///
/// `store::class_of` defaults an unrecognised kind to durable, which is the right default and the
/// reason nothing had to be added for this — but it is a default, and a default nobody asserts is
/// one edit away from being changed for a reason that has nothing to do with reviews. Losing a
/// physician's written rulings to a twenty-four-hour sweep would be discovered the day we went
/// looking for them.
#[test]
fn a_review_is_never_swept() {
    assert_eq!(vitals_web::store::class_of(vitals_web::review::KIND), vitals_web::store::Class::Durable);
    assert_eq!(vitals_web::review::KIND, "review");
}
