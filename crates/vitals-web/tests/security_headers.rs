//! **Every response carries the security headers — a page, an API answer, a refusal and a miss.**
//!
//! On 27 Sep 2026 the ward sent none: no HSTS, no framing rule, no nosniff, no referrer policy. The
//! ward's page is where a stranger signs a transaction, so a site that can frame it invisibly can
//! steer that signature (clickjacking). The headers are added at the one place every response
//! leaves through, and this test reads one of each kind of response so a route cannot quietly
//! bypass it.
//!
//! What is deliberately *not* enforced yet: a script/style Content-Security-Policy. The page carries
//! inline script and embeds YouTube-nocookie and Google Fonts, so a wrong policy would break the ward;
//! it needs its own audit first. Framing is enforced now because it is the part that protects the
//! signature and it restricts nothing the page itself does.

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
        let state = std::env::temp_dir().join(format!("vitals-headers-{}", std::process::id()));
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

    /// The status and the body of a plain GET.
    fn body(&self, path: &str) -> (u16, String) {
        let mut s = std::net::TcpStream::connect(("127.0.0.1", self.port)).expect("connect");
        s.set_read_timeout(Some(std::time::Duration::from_secs(20))).ok();
        write!(s, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").expect("send");
        let mut raw = Vec::new();
        let _ = s.read_to_end(&mut raw);
        let text = String::from_utf8_lossy(&raw).to_string();
        let status = text.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
        let body = text.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default();
        (status, body)
    }

    /// The status and the headers (names lowercased) of a plain GET, read off the wire.
    fn head(&self, path: &str) -> (u16, Vec<(String, String)>) {
        let mut s = std::net::TcpStream::connect(("127.0.0.1", self.port)).expect("connect");
        s.set_read_timeout(Some(std::time::Duration::from_secs(20))).ok();
        write!(s, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").expect("send");
        let mut raw = Vec::new();
        let _ = s.read_to_end(&mut raw);
        let text = String::from_utf8_lossy(&raw);
        let head = text.split("\r\n\r\n").next().unwrap_or_default();
        let mut lines = head.lines();
        let status = lines
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|c| c.parse().ok())
            .unwrap_or(0);
        let headers = lines
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
            .collect();
        (status, headers)
    }
}

fn header<'a>(h: &'a [(String, String)], name: &str) -> Option<&'a str> {
    h.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

#[test]
fn every_kind_of_response_carries_the_security_headers() {
    let s = Server::start();
    // One of each: the page a stranger signs on, an API answer, a door refusal, and a miss.
    for (path, what) in [
        ("/", "the ward's page"),
        ("/api/ward/cases", "an API answer"),
        ("/api/ward/bytes", "a door refusal (no token)"),
        ("/no-such-page-here", "a miss"),
    ] {
        let (status, h) = s.head(path);
        assert!(status > 0, "{what} ({path}) got no response at all");
        let need = |name: &str, must: &str| {
            let v = header(&h, name).unwrap_or_else(|| {
                panic!("{what} ({path}, {status}) carries no {name} header")
            });
            assert!(v.contains(must), "{what} ({path}): {name} is {v:?}, expected it to contain {must:?}");
        };
        need("x-content-type-options", "nosniff");
        need("x-frame-options", "DENY");
        need("content-security-policy", "frame-ancestors 'none'");
        need("referrer-policy", "strict-origin-when-cross-origin");
        need("strict-transport-security", "max-age=");
        // The bedside's 🎤 button uses SpeechRecognition: the microphone stays allowed for this
        // origin, and what the ward never uses is switched off.
        need("permissions-policy", "microphone=(self)");
        need("permissions-policy", "camera=()");
        need("permissions-policy", "geolocation=()");
    }
}

/// **A refusal tells a stranger "no" and nothing else.** On 27 Sep 2026 the door's 401 named the
/// environment variable holding its secret to anybody who knocked without it. The name is not the
/// secret, but it is a map of where the secret lives and what it guards, and the operator who needs
/// that detail reads it in the server's own log, not in a reply to whoever asked.
#[test]
fn a_door_refusal_names_no_variable_and_no_mechanism() {
    let s = Server::start();
    let (status, body) = s.body("/api/ward/bytes");
    assert_eq!(status, 401, "the door still refuses a caller with no token: {body}");
    assert!(!body.contains("VITALS_"), "the refusal names an environment variable: {body}");
    assert!(!body.contains("bay.js"), "the refusal describes where another secret is kept: {body}");
}
