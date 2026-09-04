//! How many times these two endpoints reach for the chain.
//!
//! `/api/authors` and `/api/payout` display things the chain knows. Both once did a fresh read on
//! every request — `getSignaturesForAddress` with a lookup per memo, and a `get_program_accounts`
//! — behind endpoints anyone can curl, on a server whose request loop is one thread. The page
//! polls `/api/payout` twelve times per finished run, so a class finishing together was hundreds
//! of fan-outs in half a minute, competing for the RPC budget with the anchoring those same
//! learners were waiting on.
//!
//! Counting them is the only way to know. A test that reads the source and asserts a call "is
//! inside the TTL block" passes the day somebody moves it back out and leaves the comment.
//!
//! So: an RPC that counts what it is asked, and the endpoints pointed at it.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// A stand-in Solana RPC that counts calls and answers just enough to be believed.
struct CountingRpc {
    port: u16,
    heavy: Arc<AtomicUsize>,
}

impl CountingRpc {
    fn start() -> CountingRpc {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let heavy = Arc::new(AtomicUsize::new(0));
        let counter = heavy.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let counter = counter.clone();
                std::thread::spawn(move || serve(stream, counter));
            }
        });
        CountingRpc { port, heavy }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Calls that fan out over the chain: the account sweep and the signature scan.
    fn heavy_calls(&self) -> usize {
        self.heavy.load(Ordering::Relaxed)
    }
}

fn serve(mut stream: std::net::TcpStream, heavy: Arc<AtomicUsize>) {
    let mut buf = [0u8; 8192];
    let Ok(n) = stream.read(&mut buf) else { return };
    let body = String::from_utf8_lossy(&buf[..n]).to_string();
    let method = ["getProgramAccounts", "getSignaturesForAddress", "getGenesisHash",
                  "getAccountInfo", "getBalance", "getLatestBlockhash", "getSlot",
                  "getTransaction", "getVersion"]
        .into_iter()
        .find(|m| body.contains(m))
        .unwrap_or("");
    if matches!(method, "getProgramAccounts" | "getSignaturesForAddress") {
        heavy.fetch_add(1, Ordering::Relaxed);
    }
    // Devnet's genesis, so the payout guard is satisfied; empty results for everything else.
    let result = match method {
        "getGenesisHash" => "\"EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG\"".to_string(),
        "getProgramAccounts" | "getSignaturesForAddress" => "[]".to_string(),
        "getBalance" => r#"{"context":{"slot":1},"value":5000000000}"#.to_string(),
        "getSlot" => "1".to_string(),
        "getVersion" => r#"{"solana-core":"2.3.1"}"#.to_string(),
        _ => r#"{"context":{"slot":1},"value":null}"#.to_string(),
    };
    let payload = format!(r#"{{"jsonrpc":"2.0","id":1,"result":{result}}}"#);
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{payload}",
        payload.len()
    );
}

struct Server {
    child: Child,
    port: u16,
    state: std::path::PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.state);
    }
}

impl Server {
    /// `relay` is the chain's fee payer: without a program id and a relay there is no chain at
    /// all, `chain_view` returns nothing, and the endpoint would read zero because there is
    /// nothing to read — which would make the bound look satisfied for the wrong reason.
    fn start(rpc: &str, key: &std::path::Path, relay: &std::path::Path) -> Server {
        // Unique per server: these tests run in parallel and shared a directory, so one
        // wiped the other's on start.
        static N: AtomicUsize = AtomicUsize::new(0);
        let state = std::env::temp_dir().join(format!(
            "vitals-bound-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&state);
        std::fs::create_dir_all(&state).expect("state dir");
        let mut child = Command::new(env!("CARGO_BIN_EXE_vitals-web"))
            .env("VITALS_WEB_BIND", "127.0.0.1:0")
            .env("VITALS_STATE_DIR", &state)
            .env("VITALS_RPC", rpc)
            .env("VITALS_PAYOUT_LAMPORTS", "1000000")
            .env("VITALS_PAYOUT_KEY", key)
            .env("VITALS_KEYPAIR", relay)
            .env("VITALS_PROGRAM_ID", "535FMHHZ4rp5hNmvSmdNFoaatLX82cCXHfRg3hpyBTSG")
            .env_remove("VITALS_TOKEN")
            .env_remove("HEIMDALL_API_KEY")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
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
        if me.port == 0 {
            // Say why. A payout-enabled server exits at boot on bad config, and "never said what
            // port it took" is a useless way to be told that.
            let mut why = String::new();
            if let Some(mut err) = me.child.stderr.take() {
                let _ = err.read_to_string(&mut why);
            }
            panic!("the server did not start: {}", why.trim());
        }
        me
    }

    fn get(&self, path: &str) {
        let url = format!("http://127.0.0.1:{}{path}", self.port);
        let _ = ureq::get(&url).call().map(|r| r.into_string());
    }
}

fn throwaway_key() -> std::path::PathBuf {
    use solana_sdk::signature::Keypair;
    static N: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "vitals-bound-key-{}-{}.json",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    // A real keypair: `read_keypair_file` checks that the public half derives from the private
    // one, so sixty-four arbitrary bytes are rejected and the server exits at boot. It signs
    // nothing here — the fake RPC accepts no transactions.
    let bytes = Keypair::new().to_bytes().to_vec();
    std::fs::write(&path, serde_json::to_string(&bytes).expect("json")).expect("write key");
    path
}

/// **The bound the endpoint claims.** Twenty requests, one fan-out.
#[test]
fn the_authors_endpoint_reaches_for_the_chain_once_a_minute_however_often_it_is_asked() {
    let rpc = CountingRpc::start();
    let key = throwaway_key();
    let relay = throwaway_key();
    let s = Server::start(&rpc.url(), &key, &relay);

    s.get("/api/authors");
    let after_first = rpc.heavy_calls();
    for _ in 0..19 {
        s.get("/api/authors");
    }
    let after_twenty = rpc.heavy_calls();
    let _ = std::fs::remove_file(&key);
    let _ = std::fs::remove_file(&relay);

    assert!(after_first > 0, "the first call read nothing at all — the test is not wired up");
    assert_eq!(
        after_twenty, after_first,
        "twenty requests cost {after_twenty} chain calls, not {after_first}. The endpoint says \
         'at most one chain read a minute' and would be lying."
    );
}

/// **No chain read at all.** The page polls this a dozen times per finished run.
#[test]
fn the_payout_endpoint_never_reaches_for_the_chain() {
    let rpc = CountingRpc::start();
    let key = throwaway_key();
    let relay = throwaway_key();
    let s = Server::start(&rpc.url(), &key, &relay);

    // Let boot settle, then count only what the endpoint itself costs.
    s.get("/api/fuel");
    let before = rpc.heavy_calls();
    for _ in 0..24 {
        s.get(&format!("/api/payout?leaf={}", "a".repeat(64)));
    }
    let after = rpc.heavy_calls();
    let _ = std::fs::remove_file(&key);
    let _ = std::fs::remove_file(&relay);

    assert_eq!(
        after, before,
        "twenty-four polls cost {} chain call(s). This path is a map lookup: the page polls it \
         twelve times per finished run, on the one thread that is also anchoring.",
        after - before
    );
}
