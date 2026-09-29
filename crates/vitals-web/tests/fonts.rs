//! **The ward's typefaces come from the ward.**
//!
//! Every page used to load its fonts from fonts.googleapis.com and fonts.gstatic.com, so each
//! visitor's browser told Google which page it was on, and the stylesheet could not carry an
//! integrity hash because Google tailors it per browser (ZAP 90003, 29 Sep 2026). The four
//! typefaces are SIL OFL and now ship inside the binary; a content-security policy can then say
//! `font-src 'self'`.

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
        let state = std::env::temp_dir().join(format!("vitals-fonts-{}", std::process::id()));
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
    /// Status, content-type and body bytes of a GET.
    fn get(&self, path: &str) -> (u16, String, Vec<u8>) {
        let mut s = std::net::TcpStream::connect(("127.0.0.1", self.port)).expect("connect");
        s.set_read_timeout(Some(std::time::Duration::from_secs(20))).ok();
        // HTTP/1.0 so a large body comes back whole rather than chunked, and the bytes read here
        // are the file's own.
        write!(s, "GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n").expect("send");
        let mut raw = Vec::new();
        let _ = s.read_to_end(&mut raw);
        let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or(raw.len());
        let head = String::from_utf8_lossy(&raw[..split]).to_string();
        let body = raw.get(split + 4..).unwrap_or_default().to_vec();
        let status = head.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
        let ctype = head
            .lines()
            .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-type:").map(|v| v.trim().to_string()))
            .unwrap_or_default();
        (status, ctype, body)
    }
}

#[test]
fn the_wards_typefaces_come_from_the_ward() {
    let s = Server::start();
    let (status, ctype, css) = s.get("/fonts/fonts.css");
    assert_eq!(status, 200, "the stylesheet is served");
    assert!(ctype.starts_with("text/css"), "as css: {ctype}");
    let css = String::from_utf8(css).expect("utf-8");
    assert!(!css.contains("gstatic.com") && !css.contains("googleapis.com"),
            "the stylesheet sends a browser nowhere else");

    // Every face it names is served, as woff2, with bytes that look like one.
    let names: Vec<&str> = css.split("url(/fonts/").skip(1).filter_map(|r| r.split(')').next()).collect();
    assert!(names.len() >= 40, "the four families are all there: {}", names.len());
    for n in names {
        let (status, ctype, body) = s.get(&format!("/fonts/{n}"));
        assert_eq!(status, 200, "{n} is served");
        assert_eq!(ctype, "font/woff2", "{n} as woff2");
        assert!(body.starts_with(b"wOF2"), "{n} is a woff2 file");
    }
    assert_eq!(s.get("/fonts/not-a-font.woff2").0, 404, "a name it does not hold is a miss");

    // And no page sends a visitor to Google for them.
    for page in ["/", "/ward/1", "/privacy"] {
        let (_, _, body) = s.get(page);
        let body = String::from_utf8_lossy(&body);
        assert!(!body.contains("fonts.googleapis.com") && !body.contains("fonts.gstatic.com"),
                "{page} still loads fonts from Google");
    }
}
