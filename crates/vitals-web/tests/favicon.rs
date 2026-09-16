//! The ward host's own mark, and the tab it must not change.
//!
//! The founder asked for a favicon on 16 ก.ย. Two things make it worth a test rather than a line
//! of HTML. The first is that a favicon is a file the page names and the server has to answer for:
//! a `<link>` pointing at a route nobody wrote is a broken tab icon that no test and no reviewer
//! ever sees, because a browser fails it silently. The second is the Eternal entry — a judge may
//! have vitals.academy open during the sprint, and its 🫀 is what that tab says. So this file
//! asserts the mark is served and linked on the ward's pages *and* that the season's pages are
//! untouched, which is the half that is easy to break while meaning well.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

const LINK: &str = "<link rel=\"icon\" type=\"image/svg+xml\" href=\"/world/favicon.svg\">";

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn statics(name: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static").join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

struct Server {
    child: Child,
    port: u16,
    _state: PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self._state);
    }
}

impl Server {
    /// The ward host: `VITALS_WORLD=1` is what makes the root the globe rather than the landing.
    fn start() -> Server {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let state = std::env::temp_dir().join(format!("vitals-favicon-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&state);
        let mut child = Command::new(env!("CARGO_BIN_EXE_vitals-web"))
            .env("VITALS_WEB_BIND", "127.0.0.1:0")
            .env("VITALS_STATE_DIR", &state)
            .env("VITALS_WORLD", "1")
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

    /// Status, headers and body, because a favicon is all three: the wrong content type is served
    /// with a perfectly good 200 and drawn as nothing.
    fn get(&self, path: &str) -> (u16, Vec<(String, String)>, Vec<u8>) {
        let url = format!("http://127.0.0.1:{}{path}", self.port);
        let r = match ureq::get(&url).call() {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(other) => panic!("{url}: {other}"),
        };
        let code = r.status();
        let heads = r
            .headers_names()
            .iter()
            .filter_map(|h| r.header(h).map(|v| (h.to_ascii_lowercase(), v.to_string())))
            .collect();
        let mut body = Vec::new();
        std::io::Read::read_to_end(&mut r.into_reader(), &mut body).expect("body");
        (code, heads, body)
    }
}

fn header<'a>(heads: &'a [(String, String)], name: &str) -> Option<&'a str> {
    heads.iter().find(|(h, _)| h == name).map(|(_, v)| v.as_str())
}

/// The mark is a file in the repository, not a string in a Rust source file: the designer edits an
/// SVG, and the server bakes in whatever that file says.
#[test]
fn the_mark_is_a_file_the_server_can_bake_in() {
    let p = repo().join("pitch/logo/favicon-world.svg");
    let svg = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    assert!(svg.contains("<svg"), "the mark is an SVG");
    assert!(svg.contains("viewBox=\"0 0 64 64\""),
            "a favicon with no viewBox does not scale down to 16 px: {svg}");
}

#[test]
fn the_ward_host_serves_its_mark() {
    let s = Server::start();
    let (code, heads, body) = s.get("/world/favicon.svg");
    assert_eq!(code, 200, "the page names this file, so the server has to answer for it");
    assert_eq!(header(&heads, "content-type"), Some("image/svg+xml"),
               "an SVG served as anything else is drawn as nothing, with a 200");
    let on_disk = std::fs::read(repo().join("pitch/logo/favicon-world.svg")).expect("the mark");
    assert_eq!(body, on_disk, "the bytes served are the designer's file, unedited");
    let cache = header(&heads, "cache-control").unwrap_or_default().to_string();
    assert!(cache.contains("max-age="), "a mark that never changes is worth caching: {cache:?}");
    let secs: u64 = cache
        .split("max-age=")
        .nth(1)
        .and_then(|t| t.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|d| d.parse().ok())
        .unwrap_or(0);
    assert!(secs >= 86_400, "long, not a five-minute cache: {cache:?}");
}

/// The globe, as the browser gets it — not as the file on disk reads.
#[test]
fn the_globe_links_the_mark() {
    let s = Server::start();
    let (code, _, body) = s.get("/");
    assert_eq!(code, 200);
    let page = String::from_utf8_lossy(&body);
    assert!(page.contains(LINK), "the ward's front door carries the icon link");
}

/// The shift page and both receipt pages. The receipt is reached by a hash nobody holds, so what
/// comes back is its "no such shift" page — which is still a page with a tab, and still the ward's.
#[test]
fn the_ward_s_other_pages_carry_it_too() {
    assert!(statics("world/shift.html").contains(LINK), "the shift page");
    let s = Server::start();
    let (_, _, body) = s.get("/shift/0000000000000000000000000000000000000000000000000000000000000000");
    let page = String::from_utf8_lossy(&body);
    assert!(page.contains("/world/favicon.svg"), "the receipt page: {page}");
}

/// The half that is easy to break while meaning well.
#[test]
fn the_season_s_tab_is_not_touched() {
    for name in ["index.html", "landing.html"] {
        let page = statics(name);
        assert!(page.contains("🫀"), "{name} keeps the heart a judge may have open in a tab");
        assert!(!page.contains("/world/favicon.svg"),
                "{name} is the Eternal entry, and the ward's mark has no business on it");
    }
}

/// iOS takes a PNG for a home-screen bookmark and nothing else — an SVG there is no icon at all.
/// Rendered from the same SVG rather than drawn again, so there is one mark and one file to edit.
#[test]
fn a_home_screen_bookmark_gets_a_square_png() {
    let s = Server::start();
    let (code, heads, body) = s.get("/world/apple-touch-icon.png");
    assert_eq!(code, 200);
    assert_eq!(header(&heads, "content-type"), Some("image/png"));
    assert_eq!(&body[1..4], b"PNG", "a PNG, whatever the route is called");
    // The IHDR dimensions, straight out of the file: 180x180 is what iOS asks for.
    let w = u32::from_be_bytes([body[16], body[17], body[18], body[19]]);
    let h = u32::from_be_bytes([body[20], body[21], body[22], body[23]]);
    assert_eq!((w, h), (180, 180), "180 px square");
    for page in ["world/index.html", "world/shift.html"] {
        assert!(statics(page).contains("<link rel=\"apple-touch-icon\" href=\"/world/apple-touch-icon.png\">"),
                "{page} offers the home-screen icon");
    }
}
