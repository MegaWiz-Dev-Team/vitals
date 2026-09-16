//! What the ward host puts on the wire.
//!
//! Measured against staging on 16 ก.ย.: the globe 274 KB, `bay.js` 247 KB, `bay.css` 99 KB,
//! `/api/ward` 14 KB — none of it compressed, and asking for gzip changed nothing. A first visit
//! to the ward is 620 KB of text that gzips to roughly a tenth of that, and the ward pays for
//! every byte of it on a service that scales to zero.
//!
//! Two rules here, and they are different rules. **Compression** is about the bytes: a client that
//! says it takes gzip gets gzip, and the thing it decodes to is byte-for-byte what a client that
//! did not ask for it receives. **Caching** is about asking again: the build-stamped files never
//! change under one URL, so they are immutable for a year; the board changes every minute, so it
//! is fifteen seconds and an ETag — long enough to absorb a room full of people opening it at
//! once, short enough that a death is on screen before anybody has stopped looking.

use std::io::{BufRead, BufReader, Read};
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
    fn start() -> Server {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let state = std::env::temp_dir().join(format!("vitals-egress-{}-{n}", std::process::id()));
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

    /// A raw request, so the bytes on the wire are the bytes this test reads — a client library
    /// that decompresses for you cannot tell you what was sent.
    fn raw(&self, path: &str, accept_gzip: bool) -> (String, Vec<u8>) {
        use std::io::Write;
        let mut s = std::net::TcpStream::connect(("127.0.0.1", self.port)).expect("connect");
        let enc = if accept_gzip { "Accept-Encoding: gzip\r\n" } else { "" };
        write!(s, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n{enc}\r\n")
            .expect("write");
        let mut all = Vec::new();
        s.read_to_end(&mut all).expect("read");
        let split = all.windows(4).position(|w| w == b"\r\n\r\n").expect("headers end");
        let head = String::from_utf8_lossy(&all[..split]).to_ascii_lowercase();
        let body = all[split + 4..].to_vec();
        // A big body comes back chunked, which is HTTP/1.1 doing its job and not the server doing
        // anything unusual — but a reader that skips the framing is reading the framing as data.
        let body = if head.contains("transfer-encoding: chunked") { dechunk(&body) } else { body };
        (head, body)
    }
}

/// Chunked transfer, undone: `<hex length>\r\n<bytes>\r\n`, ending at a zero-length chunk.
fn dechunk(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(nl) = rest.windows(2).position(|w| w == b"\r\n") {
        let len = usize::from_str_radix(
            String::from_utf8_lossy(&rest[..nl]).split(';').next().unwrap_or("0").trim(),
            16,
        )
        .unwrap_or(0);
        if len == 0 {
            break;
        }
        let start = nl + 2;
        out.extend_from_slice(&rest[start..start + len]);
        rest = &rest[start + len + 2..];
    }
    out
}

fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines()
        .find(|l| l.starts_with(&format!("{name}:")))
        .map(|l| l[name.len() + 1..].trim())
}

fn gunzip(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(body).read_to_end(&mut out).expect("valid gzip");
    out
}

/// **A client that takes gzip gets gzip, and it decodes to exactly what everyone else gets.**
#[test]
fn the_wards_text_is_compressed_for_a_client_that_asks() {
    let s = Server::start();
    for path in ["/", "/bay.js", "/bay.css", "/api/ward"] {
        let (plain_head, plain) = s.raw(path, false);
        assert!(header(&plain_head, "content-encoding").is_none(),
                "{path} compressed a response for a client that never asked: {plain_head}");

        let (head, body) = s.raw(path, true);
        assert_eq!(header(&head, "content-encoding"), Some("gzip"),
                   "{path} is {} bytes of text and went out uncompressed: {head}", plain.len());
        assert_eq!(gunzip(&body), plain, "{path}: the gzip decodes to something else");
        // The three big files are markup, script and stylesheet — repetitive text that gzip halves
        // several times over. An empty board is mostly its own derivations, which are prose said
        // once, so it saves less; on staging with three patients in it the payload is 14 KB and
        // compresses like the rest. A third off is the floor worth a round trip.
        let saved = 1.0 - body.len() as f64 / plain.len() as f64;
        let floor = if path == "/api/ward" { 0.33 } else { 0.5 };
        assert!(saved > floor,
                "{path} gzipped to {} of {} bytes — {:.0}% saved, under the {:.0}% that makes it \
                 worth compressing at all",
                body.len(), plain.len(), saved * 100.0, floor * 100.0);
    }
}

/// Small answers are not worth a compressor, and a picture is already compressed.
#[test]
fn what_is_not_compressed_and_why() {
    let s = Server::start();
    let (head, _) = s.raw("/world/favicon.svg", true);
    // An SVG is text and it is 533 bytes: gzip would save a couple of hundred and cost a header.
    assert!(header(&head, "content-encoding").is_none(), "a 533-byte mark is not worth it: {head}");
    let (head, _) = s.raw("/world/apple-touch-icon.png", true);
    assert!(header(&head, "content-encoding").is_none(), "a PNG is already compressed: {head}");
}

/// **What never changes is immutable; what changes every minute is fifteen seconds and an ETag.**
#[test]
fn the_wards_caching_says_which_kind_of_thing_each_answer_is() {
    let s = Server::start();
    for path in ["/bay.js?v=abc123", "/bay.css?v=abc123", "/world/favicon.svg"] {
        let (head, _) = s.raw(path, false);
        let cache = header(&head, "cache-control").unwrap_or_default();
        assert!(cache.contains("immutable"),
                "{path} is addressed by a build stamp and cannot change under that URL: {cache:?}");
        assert!(cache.contains("max-age=31536000"), "{path}: a year — {cache:?}");
    }

    let (head, _) = s.raw("/api/ward", false);
    let cache = header(&head, "cache-control").unwrap_or_default();
    assert!(cache.contains("max-age=15"),
            "the board is a live answer: long enough to absorb a room opening it at once, short \
             enough that a death is on screen before anybody has stopped looking — {cache:?}");
    let tag = header(&head, "etag").expect("the board carries an ETag").to_string();
    assert!(tag.len() > 2, "and it is a real one: {tag}");

    // Asked again with the tag, the ward says "still that" rather than sending it twice.
    let mut s2 = std::net::TcpStream::connect(("127.0.0.1", s.port)).expect("connect");
    use std::io::Write;
    write!(s2, "GET /api/ward HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nIf-None-Match: {tag}\r\n\r\n")
        .expect("write");
    let mut all = Vec::new();
    s2.read_to_end(&mut all).expect("read");
    let text = String::from_utf8_lossy(&all);
    assert!(text.starts_with("HTTP/1.1 304"), "an unchanged board is a 304: {}", &text[..40.min(text.len())]);
}
