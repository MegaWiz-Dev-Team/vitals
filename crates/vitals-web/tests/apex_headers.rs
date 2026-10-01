//! **The front door answers with the security headers, and its typefaces come from itself.**
//!
//! Ported from the ward on 30 Sep 2026. The ward had shipped the headers on the 27th; this build
//! (vitals.academy, and the Eternal game behind it) had none. Framing is same-origin only, because
//! the bay frames its own /device/ pages. DENY had turned that panel into a black box on the ward.

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
        let state = std::env::temp_dir().join(format!("vitals-apex-headers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&state);
        let mut child = Command::new(env!("CARGO_BIN_EXE_vitals-web"))
            .env("VITALS_WEB_BIND", "127.0.0.1:0")
            .env("VITALS_STATE_DIR", &state)
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
    /// Status, headers (names lowercased) and body of a GET, read whole (HTTP/1.0: never chunked).
    fn get(&self, path: &str) -> (u16, Vec<(String, String)>, Vec<u8>) {
        self.get_as("127.0.0.1", path)
    }
    /// The same GET, as it arrives for a named host — the front door answers `vitals.academy`
    /// differently from the game's host.
    fn get_as(&self, host: &str, path: &str) -> (u16, Vec<(String, String)>, Vec<u8>) {
        let mut s = std::net::TcpStream::connect(("127.0.0.1", self.port)).expect("connect");
        s.set_read_timeout(Some(std::time::Duration::from_secs(20))).ok();
        write!(s, "GET {path} HTTP/1.0\r\nHost: {host}\r\n\r\n").expect("send");
        let mut raw = Vec::new();
        let _ = s.read_to_end(&mut raw);
        let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or(raw.len());
        let head = String::from_utf8_lossy(&raw[..split]).to_string();
        let body = raw.get(split + 4..).unwrap_or_default().to_vec();
        let mut lines = head.lines();
        let status = lines.next().and_then(|l| l.split_whitespace().nth(1)).and_then(|c| c.parse().ok()).unwrap_or(0);
        let headers = lines
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
            .collect();
        (status, headers, body)
    }
}

fn header<'a>(h: &'a [(String, String)], name: &str) -> Option<&'a str> {
    h.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

#[test]
fn the_front_door_answers_with_the_security_headers() {
    let s = Server::start();
    for path in ["/", "/privacy", "/play", "/no-such-page-here", "/fonts/fonts.css"] {
        let (status, h, _) = s.get(path);
        assert!(status > 0, "{path} got no response");
        let need = |name: &str, must: &str| {
            let v = header(&h, name).unwrap_or_else(|| panic!("{path} ({status}) carries no {name}"));
            assert!(v.contains(must), "{path}: {name} is {v:?}, expected {must:?}");
        };
        need("x-content-type-options", "nosniff");
        need("x-frame-options", "SAMEORIGIN");
        need("content-security-policy", "frame-ancestors 'self'");
        need("content-security-policy", "default-src 'self'");
        need("content-security-policy", "object-src 'none'");
        need("referrer-policy", "strict-origin-when-cross-origin");
        need("strict-transport-security", "max-age=");
        need("permissions-policy", "microphone=(self)");
        need("cross-origin-opener-policy", "same-origin");
    }
}

#[test]
fn the_front_doors_typefaces_come_from_itself() {
    let s = Server::start();
    let (status, h, css) = s.get("/fonts/fonts.css");
    assert_eq!(status, 200);
    assert!(header(&h, "content-type").is_some_and(|c| c.starts_with("text/css")));
    let css = String::from_utf8(css).expect("utf-8");
    let names: Vec<&str> = css.split("url(/fonts/").skip(1).filter_map(|r| r.split(')').next()).collect();
    assert!(names.len() >= 40, "the four families: {}", names.len());
    for n in names {
        let (status, h, body) = s.get(&format!("/fonts/{n}"));
        assert_eq!(status, 200, "{n}");
        assert_eq!(header(&h, "content-type"), Some("font/woff2"), "{n}");
        assert!(body.starts_with(b"wOF2"), "{n} is a woff2 file");
    }
    for page in ["/", "/privacy", "/terms"] {
        let (_, _, body) = s.get(page);
        let body = String::from_utf8_lossy(&body);
        assert!(!body.contains("fonts.googleapis.com") && !body.contains("fonts.gstatic.com"),
                "{page} still loads fonts from Google");
    }
}

/// **The front door serves its fonts as fonts** (30 Sep 2026). v0.10.1 kept /fonts/ on the apex
/// rather than redirecting it, and then answered it with the landing page — `text/html` — so every
/// page on vitals.academy fell back to system type. The tests above asked the game's host.
#[test]
fn the_front_door_serves_its_fonts_as_fonts() {
    let s = Server::start();
    let (status, h, css) = s.get_as("vitals.academy", "/fonts/fonts.css");
    assert_eq!(status, 200);
    assert!(header(&h, "content-type").is_some_and(|c| c.starts_with("text/css")),
            "the apex answered its stylesheet as {:?}", header(&h, "content-type"));
    let css = String::from_utf8(css).expect("utf-8");
    let first = css.split("url(/fonts/").nth(1).and_then(|r| r.split(')').next()).expect("a face");
    let (status, h, body) = s.get_as("vitals.academy", &format!("/fonts/{first}"));
    assert_eq!(status, 200);
    assert_eq!(header(&h, "content-type"), Some("font/woff2"));
    assert!(body.starts_with(b"wOF2"));
}

/// The value of one directive in a policy, as its list of sources.
fn directive<'a>(csp: &'a str, name: &str) -> Vec<&'a str> {
    csp.split(';')
        .map(str::trim)
        .find(|d| d.split_whitespace().next() == Some(name))
        .map(|d| d.split_whitespace().skip(1).collect())
        .unwrap_or_default()
}

/// **The landing's films, art and count are allowed where they actually come from** (1 Oct 2026).
/// The front door keeps only its own pages and sends everything else to the game's host with a
/// 301, so the landing's `/clip/`, `/img/` and `/api/usage` arrive from that origin. The policy
/// asked here is the one the apex sends; the origin is the one its own redirect names, so the two
/// cannot drift apart without this failing.
#[test]
fn the_front_doors_policy_allows_the_origin_it_redirects_to() {
    let s = Server::start();
    let (status, h, _) = s.get_as("vitals.academy", "/");
    assert_eq!(status, 200);
    let csp = header(&h, "content-security-policy").expect("a policy").to_string();
    for (path, dir) in [("/clip/ep1_teaser.mp4", "media-src"), ("/img/stable.jpg", "img-src"), ("/api/usage", "connect-src")] {
        let (status, h, _) = s.get_as("vitals.academy", path);
        assert_eq!(status, 301, "{path} is sent to the game's host");
        let to = header(&h, "location").expect("a Location");
        let origin = to.splitn(4, '/').take(3).collect::<Vec<_>>().join("/");
        assert!(directive(&csp, dir).contains(&origin.as_str()),
                "{path} is redirected to {origin}, which the front door's {dir} refuses: {csp}");
    }
}

/// **Nothing the pages load comes from an origin the policy does not name.** Every script the
/// pages add by hand is checked against `script-src`, the one directive an absolute URL in this
/// build reaches.
#[test]
fn every_script_origin_in_the_pages_is_in_the_policy() {
    let s = Server::start();
    let (_, h, _) = s.get("/");
    let csp = header(&h, "content-security-policy").expect("a policy").to_string();
    let scripts = directive(&csp, "script-src");
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/static");
    let mut pages = vec![];
    for e in std::fs::read_dir(dir).expect("static").flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "html") { pages.push(p) }
    }
    assert!(pages.len() >= 5);
    for p in pages {
        let body = std::fs::read_to_string(&p).expect("page");
        for hit in body.split(".src='https://").skip(1).chain(body.split("<script src=\"https://").skip(1)) {
            let host = hit.split(['/', '\'', '"']).next().unwrap_or("");
            let origin = format!("https://{host}");
            assert!(scripts.contains(&origin.as_str()) || scripts.contains(&"'self'") && host.is_empty(),
                    "{} loads a script from {origin}, which script-src refuses", p.display());
        }
    }
}
