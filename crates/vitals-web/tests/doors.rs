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

    for door in ["/api/ward/queue", "/api/ward/pack/42", "/api/ward/case", "/api/ward/tick"] {
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

/// **The catalogue is a read, and reads on this ward are public.**
///
/// `/api/ward/case` writes what strangers will be asked to treat and takes the factory's secret.
/// `/api/ward/cases` says what the ward is holding, which is the same kind of fact as the census:
/// a list only the people we hand a token to can read is not a catalogue, it is a claim.
#[test]
fn the_catalogue_is_readable_without_a_token() {
    let s = Server::start(Some("the-door-token"));
    let (code, _) = s.get("/api/ward/cases");
    assert_eq!(code, 200, "the ward says what it is holding to anybody who asks");
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
    for door in ["/api/ward/queue", "/api/ward/pack/42", "/api/ward/case", "/api/ward/tick"] {
        let (code, body) = s.post(door, Some("the-page-token"), A_PACK);
        assert_eq!(code, 503,
                   "{door} must say it has no secret to check against, and never fall back to the \
                    page's: {body}");
        assert!(body.contains("VITALS_DOOR_TOKEN"),
                "and the sentence has to name what is missing, because the person reading it is \
                 the one who can set it: {body}");
    }
}

/// **The ward's pass can be asked for by a scheduler, and answers with the pass itself.**
///
/// min-instances 0 plus Cloud Run's CPU-only-during-a-request means an in-process ticker on a
/// quiet ward has no CPU to tick with: a bed freed at 03:00 refills when somebody knocks, not on
/// the minute. Ruling 11 says the refill is an event, not a person noticing. So the pass is also a
/// route — `POST /api/ward/tick`, behind the factory's door token like the other operator routes,
/// called by Cloud Scheduler every minute — and the pass runs *inside* the request, because that
/// is the only time the container is given CPU. The in-process ticker stays as the fallback and
/// takes the same gate, so the two can never run one pass twice.
///
/// The body is the instrument: the same facts the slow-pass line prints, so the Scheduler's own
/// response log shows a pass's shape without anybody reading container logs.
#[test]
fn a_scheduler_can_ask_for_the_pass_and_gets_the_pass_back() {
    let s = Server::start(Some("door-secret"));

    let (code, body) = s.post("/api/ward/tick", Some("door-secret"), "");
    assert_eq!(code, 200, "the door's own token runs the pass: {body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_else(|e| panic!("{e}: {body}"));
    for field in ["took_ms", "patients", "listed", "pace", "spans", "notes"] {
        assert!(v.get(field).is_some(),
                "the answer carries `{field}` — the pass's own instrument, in the Scheduler's log");
    }
    // No chain on this host, so the pass says so rather than pretending: that is a pass that ran,
    // not a failure of the route, and the caller gets 200 with the sentence in `notes`.
    assert!(v["notes"].as_array().is_some_and(|n| !n.is_empty()),
            "a ward with no chain says why nobody was admitted: {body}");

    // Idempotent for a scheduler that retries: a second ask after the first finished is another
    // pass, not an error.
    let (again, _) = s.post("/api/ward/tick", Some("door-secret"), "");
    assert_eq!(again, 200);
}

/// **The team's dashboard is a page the ward serves, public, and it reads the ward same-origin.**
///
/// The founder, 20 ก.ย.: "ทีมผมต้องมี dashboard ให้ดู". One static page reading `/api/ward` and
/// `/api/usage` every thirty seconds — arrivals, keys, shifts, beds, went home, died, the door, the
/// revision, the board's provenance, the catalogue's counts. The ward sends no CORS headers, so a
/// page that reads it has to be served by it: `GET /stats`, no-cache HTML like the other pages,
/// not behind the door — there is nothing on it a stranger cannot already read from the API.
#[test]
fn the_dashboard_is_served_by_the_ward_and_reads_it_same_origin() {
    let s = Server::start(Some("door-secret"));
    let (code, body) = s.get("/stats");
    assert_eq!(code, 200, "the dashboard is a page the ward serves: {}", &body[..body.len().min(200)]);
    assert!(body.contains("api/usage") && body.contains("api/ward"),
            "and it reads the ward and the usage counters from this origin, not another");
    assert!(body.contains(r#"name="robots" content="noindex""#),
            "a team page, not a landing: search engines are asked to leave it alone");
    assert!(!body.contains("http://") && !body.contains("https://fonts."),
            "no request leaves this origin from the dashboard");
    // **English, every label.** The founder, 22 Sep: "ทำเป็นภาษาอังกฤษให้หมด". The page is read by
    // the team, the director, and whoever the founder shows it to, and not all of them read Thai —
    // a label only half the room can read is a number only half the room can check. Written as a
    // rule rather than a note, because a note cannot fail and this one has to.
    let thai: String = body.chars().filter(|c| ('\u{0E00}'..='\u{0E7F}').contains(c)).collect();
    assert!(thai.is_empty(), "every label on the dashboard is English, and these are not: {thai}");
}

/// **The two counts the founder reads are on one line, with the day they are counted from.**
///
/// The director, 23 Sep: the funnel's two steps belong side by side on the dashboard, "since that
/// is the number the founder will look at on Wednesday". They can only be put side by side because
/// the window exists: until f0633a9, `bedsides_opened` had counted since the build that introduced
/// it and `arrivals` since the ward's first visitor, and this page divided one by the other anyway
/// and printed "2% of those who arrived". Two counts on two clocks are not a ratio.
///
/// So the rule has two halves and both must hold. The page names both steps on one line, and it
/// names the day they start from. The date is not decoration on that line — it is the thing that
/// makes the pair comparable, and a pair printed without it is exactly the number that meant
/// nothing for three days.
#[test]
fn the_dashboard_puts_the_two_funnel_counts_on_one_line_with_the_day_they_start() {
    let s = Server::start(Some("door-secret"));
    let (code, body) = s.get("/stats");
    assert_eq!(code, 200);
    assert!(
        body.contains(r#"<div id="lead">"#),
        "the line has a place of its own on the page, above the grid of tiles"
    );
    let line = body
        .split_once("function twoCounts(")
        .expect("one function writes that line, so what a reader reads can be read here")
        .1;
    let line = &line[..line.find("\n}").unwrap_or(line.len())];
    assert!(
        line.contains("bedsides_opened") && line.contains("shifts_taken"),
        "both steps are on the line — the second is the one the founder is asking about, and it \
         means nothing without the first beside it"
    );
    assert!(
        line.contains("f.since"),
        "and the day they are both counted from is on the line with them"
    );
    assert!(
        !line.contains("ar.total") && !line.contains("arrivals_all_time"),
        "and the lifetime total is not: it has counted since the ward's first visitor and is \
         nobody's denominator here"
    );
}

/// **Every link the bedside offers a stranger goes somewhere this ward serves.**
///
/// The guide link shipped to production at 09:04 on 22 ก.ย. and `/start` was not a route. It sits
/// on the bedside before the take, addressed to the one person on the ward who has said out loud
/// that they do not know what they are doing — "First time? two-minute guide" — and pressing it
/// would have given them a 404. A dead link is worse than no link at that exact moment, because it
/// spends the trust of somebody who had already decided to ask for help.
///
/// The rule is not "remember to add the route". It is that the page and the server are checked
/// against each other: whatever path `guideLink()` names, this ward answers. An empty link is the
/// honest state while there is no page, and it passes here; a link to nothing does not.
#[test]
fn the_guide_link_on_the_bedside_goes_somewhere_this_ward_serves() {
    let bay = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("static/bay.js"),
    )
    .expect("bay.js");
    let body = bay
        .split_once("function guideLink()")
        .expect("guideLink is still the one place the bedside names the guide")
        .1;
    let body = &body[..body.find("\n}").unwrap_or(body.len())];

    let Some(rest) = body.split_once("href=\"") else {
        // No link at all: nothing is promised, so nothing can be broken.
        return;
    };
    let path = rest.1.split('"').next().unwrap_or_default().to_string();
    assert!(path.starts_with('/'), "the guide link leaves this origin: {path}");

    let s = Server::start(None);
    let (code, _) = s.get(&path);
    assert_eq!(
        code, 200,
        "the bedside offers a stranger {path} and this ward answers {code} — a dead link on the \
         one control that exists for somebody who has already said they need help"
    );
}

/// **Every picture the guide shows is a picture this ward serves.**
///
/// The same rule as the guide link, one level in. The page walks a stranger through eight
/// screenshots; a caption pointing at a file the server does not have is the 404 of this morning
/// again, in a smaller place and harder to notice — the text would still read, and the reader
/// would be looking at a broken image while being told what to see in it.
///
/// The set is closed on purpose, so this also checks the other direction: a name nobody put in the
/// list is answered 404 rather than reaching for anything.
#[test]
fn every_picture_the_guide_shows_is_one_this_ward_serves() {
    let s = Server::start(None);
    let (code, page) = s.get("/start");
    assert_eq!(code, 200, "the guide does not open: {}", &page[..page.len().min(200)]);

    let mut seen = 0;
    let mut rest = page.as_str();
    while let Some(a) = rest.find("src=\"") {
        rest = &rest[a + 5..];
        let Some(end) = rest.find('"') else { break };
        let src = &rest[..end];
        rest = &rest[end..];
        if !src.starts_with("/start/img/") {
            assert!(!src.starts_with("http"), "the guide loads a picture off this origin: {src}");
            continue;
        }
        seen += 1;
        let (code, _) = s.get(src);
        assert_eq!(code, 200, "the guide shows {src} and this ward answers {code}");
    }
    assert!(seen >= 6, "the guide showed {seen} pictures — the scanner has stopped working");

    // And the set is closed: a name that is not in it reaches nothing.
    let (code, _) = s.get("/start/img/../../../etc/passwd");
    assert_ne!(code, 200, "a path outside the list was served");
    let (code, _) = s.get("/start/img/09-not-a-picture.jpg");
    assert_eq!(code, 404, "a name nobody put in the list is answered 404");
}

/// **The catalogue endpoint carries the status sentence, so the catalogue page never types a date.**
#[test]
fn the_catalogue_endpoint_carries_the_status_sentence() {
    let s = Server::start(None);
    let (code, body) = s.get("/api/ward/cases");
    assert_eq!(code, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_else(|e| panic!("{e}: {body}"));
    assert!(v["status"].as_str().is_some_and(|t| t.contains("open for play since") && t.contains("provisional")),
            "the catalogue says both facts in one sentence, from the one constant: {}", v["status"]);
}
