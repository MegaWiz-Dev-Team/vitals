//! **What a browser is allowed to keep, and for how long.**
//!
//! The founder sent a screenshot of the ward on 17 ก.ย. with three things in it that had been
//! fixed hours earlier — the old ring counting, a sentence that had been rewritten, "take a shift"
//! on a patient nobody can take. His tab was serving him a page out of its own cache, and nothing
//! in the answer had told it not to.
//!
//! Two rules, and they are opposite on purpose:
//!
//!   * **a page is never kept.** `no-cache` is not "do not store" — it is "ask me before you use
//!     it again", which is exactly right for a document whose whole content is the state of a ward
//!     that changes every minute. The revalidation is one conditional request against an ETag the
//!     server already sends.
//!   * **a stamped asset is kept for a year.** `bay.js?v=<build>` cannot mean two different files,
//!     so a browser that never asks again is never wrong — and the page that references it is
//!     never stale, because the page is never kept.

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
    fn start() -> Server {
        let state = std::env::temp_dir().join(format!("vitals-headers-{}", std::process::id()));
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

    /// The header this answer carries, lowercased, or "" — read off a real response.
    fn cache_header(&self, path: &str) -> String {
        let url = format!("http://127.0.0.1:{}{path}", self.port);
        let res = match ureq::get(&url).call() {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(e) => panic!("{url}: {e}"),
        };
        res.header("Cache-Control").unwrap_or_default().to_ascii_lowercase()
    }
}

#[test]
fn a_page_is_revalidated_and_a_stamped_asset_is_kept_for_a_year() {
    let s = Server::start();

    for page in ["/", "/ward/1789528325", "/ward/abc", "/shift/zzz", "/review"] {
        let got = s.cache_header(page);
        assert!(got.contains("no-cache"),
                "{page} may be kept and reused without asking: Cache-Control {got:?} — this is how \
                 the founder read a ward three hours out of date in his own tab");
        assert!(!got.contains("immutable"), "{page}: {got:?}");
    }

    let asset = s.cache_header("/bay.js?v=vitals-test");
    assert!(asset.contains("immutable") && asset.contains("max-age=31536000"),
            "a stamped asset is the one thing a browser should never ask about twice: {asset:?}");
}
