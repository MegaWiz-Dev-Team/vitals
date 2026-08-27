//! The whole chain path, end to end, against a real validator.
//!
//! This is the part that was only ever proved by throwaway scripts pasted into a terminal — the
//! relay signing for fees while the player signs for identity, and a record that follows a person
//! across machines. Those scripts were deleted after each demonstration, so nothing stopped the
//! next edit to `chain.rs` from quietly breaking any of it.
//!
//! Marked `#[ignore]` because it needs a validator and a deployed program. Run it with:
//!
//! ```text
//! solana-test-validator                       # in another terminal
//! cd crates/vitals-program && cargo build-sbf --arch v3
//! solana program deploy target/deploy/vitals_program.so
//! VITALS_PROGRAM_ID=<the id> cargo test -p vitals-web --test chain_flow -- --ignored
//! ```

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

// ── a player, standing in for a browser ─────────────────────────────────────
//
// The browser holds an Ed25519 key and signs the bytes the server hands it. Here that is
// solana-sdk's keypair doing exactly the same thing, because the point is the server half.

struct Player {
    keypair: solana_sdk::signature::Keypair,
}

impl Player {
    fn new() -> Player {
        Player { keypair: solana_sdk::signature::Keypair::new() }
    }
    fn pubkey(&self) -> String {
        use solana_sdk::signature::Signer;
        self.keypair.pubkey().to_string()
    }
    fn sign(&self, msg_hex: &str) -> String {
        use solana_sdk::signature::Signer;
        let bytes: Vec<u8> = (0..msg_hex.len() / 2)
            .map(|i| u8::from_str_radix(&msg_hex[i * 2..i * 2 + 2], 16).unwrap())
            .collect();
        let sig = self.keypair.sign_message(&bytes);
        sig.as_ref().iter().map(|b| format!("{b:02x}")).collect()
    }
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

/// The address of a keypair file, via the CLI these tests already depend on.
fn pubkey_of(key: &std::path::Path) -> Option<String> {
    let out = Command::new("solana").arg("address").arg("-k").arg(key).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

impl Server {
    fn start() -> Option<Server> {
        let program = std::env::var("VITALS_PROGRAM_ID").ok()?;
        // Unique per server, not per process: these tests are threads in one binary, so
        // process::id() is the same for all of them. They shared one state directory, and each
        // start() wiped it — so a test could delete the sessions of a test still running. That is
        // why the suite passed one at a time and failed together.
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let state = std::env::temp_dir()
            .join(format!("vitals-chain-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&state);
        std::fs::create_dir_all(&state).ok()?;

        // Its own relay key, because the tree is scoped to whoever funds it. Sharing one key made
        // every server in this suite an *instance of one operator*, which is the configuration
        // production explicitly forbids — Cloud Run is pinned to a single instance for exactly
        // this reason — so they addressed the same tree and overwrote each other's leaves. Four
        // independent operators is what the suite is actually modelling, and independent
        // operators have their own keys.
        let key = state.join("relay.json");
        let ok = Command::new("solana-keygen")
            .args(["new", "--no-bip39-passphrase", "-s", "--force", "-o"])
            .arg(&key)
            .stdout(Stdio::null()).stderr(Stdio::null())
            .status().ok()?.success();
        if !ok { return None; }
        // Funded by transfer from whoever is running the suite, not by airdrop. A local validator
        // will airdrop all day; devnet's faucet refuses, and the tests then failed inside the
        // anchoring assertions — reporting that the chain path is broken when what is broken is
        // that the relay has no lamports. One path that works on any cluster where the operator
        // has funds is worth more than a faster one that lies on three of them.
        let rpc = std::env::var("VITALS_RPC").unwrap_or_else(|_| "http://127.0.0.1:8899".into());
        let funded = Command::new("solana")
            .args(["transfer", &pubkey_of(&key)?, "0.1", "--allow-unfunded-recipient", "-u", &rpc])
            .stdout(Stdio::null()).stderr(Stdio::null())
            .status().ok()?.success();
        if !funded { return None; }
        let mut child = Command::new(env!("CARGO_BIN_EXE_vitals-web"))
            .env("VITALS_WEB_BIND", "127.0.0.1:0")
            .env("VITALS_STATE_DIR", &state)
            .env("VITALS_PROGRAM_ID", program)
            .env("VITALS_KEYPAIR", &key)
            .env_remove("VITALS_TOKEN")
            .env_remove("HEIMDALL_API_KEY")
            .stdout(Stdio::piped())
            .spawn()
            .ok()?;
        let out = child.stdout.take()?;
        let mut me = Server { child, port: 0, state };
        let mut connected = false;
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if line.starts_with("chain      connected") {
                connected = true;
            }
            if let Some(a) = line.split("http://").nth(1) {
                me.port = a.trim().rsplit(':').next().and_then(|p| p.parse().ok()).unwrap_or(0);
                break;
            }
        }
        assert!(connected, "no validator — start solana-test-validator and deploy the program");
        assert!(me.port > 0);
        Some(me)
    }

    fn json(&self, path: &str) -> serde_json::Value {
        let url = format!("http://127.0.0.1:{}{path}", self.port);
        let body = match ureq::get(&url).call() {
            Ok(r) => r.into_string().unwrap_or_default(),
            Err(ureq::Error::Status(_, r)) => r.into_string().unwrap_or_default(),
            Err(e) => panic!("{url}: {e}"),
        };
        serde_json::from_str(&body).unwrap_or(serde_json::Value::Null)
    }

    /// prepare on the server, sign here, submit — the shape the browser uses. An anchor comes
    /// back as two messages (the record outgrew one packet) and both are signed together.
    fn signed(&self, p: &Player, path: &str) -> serde_json::Value {
        let r = self.json(path);
        let Some(msg) = r["sign"].as_str() else { return r };
        let mut url = format!("/api/submit?player={}&sig={}", p.pubkey(), p.sign(msg));
        if let Some(msg2) = r["sign2"].as_str() {
            url.push_str(&format!("&sig2={}", p.sign(msg2)));
        }
        self.json(&url)
    }

    /// Play EP1 well enough to win. `extra` makes an otherwise identical run distinct, because
    /// two identical runs produce the same leaf and the second is refused as a duplicate.
    fn win(&self, p: &Player, account: &str, extra: Option<&str>) -> serde_json::Value {
        let who = p.pubkey();
        let id = self.json(&format!("/api/new?ep=ep1&player={who}"))["id"]
            .as_str()
            .expect("session")
            .to_string();
        // Declare the run before playing it — the shape the browser now uses, and the thing the
        // program enforces: an undeclared run is refused at anchor time.
        let c = self.signed(p, &format!("/api/commit?id={id}&player={who}&account={account}"));
        assert_eq!(c["committed"], true, "commit failed: {c}");

        let mut orders = vec!["adrenaline im", "oxygen face mask 10 lpm", "lay her flat, legs up"];
        if let Some(e) = extra {
            orders.push(e);
        }
        for o in orders {
            self.json(&format!("/api/step?id={id}&player={who}&do={}", enc(o)));
        }
        self.json(&format!("/api/step?id={id}&player={who}&tick=60"));
        self.json(&format!("/api/step?id={id}&player={who}&do={}", enc("normal saline bolus")));
        self.json(&format!("/api/step?id={id}&player={who}&tick=300"));
        self.json(&format!("/api/step?id={id}&player={who}&do={}", enc("admit for observation")));
        self.json(&format!("/api/step?id={id}&player={who}&tick=600"));
        self.signed(p, &format!("/api/anchor?id={id}&player={who}&account={account}"))
    }
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The relay pays and the player owns: a key with no SOL finishes a case and holds the level.
#[test]
#[ignore = "needs a validator and VITALS_PROGRAM_ID"]
fn a_player_with_no_sol_anchors_claims_and_is_refused_what_it_did_not_earn() {
    let s = Server::start().expect("VITALS_PROGRAM_ID");
    let p = Player::new();
    let me = p.pubkey();

    let anchored = s.win(&p, &me, None);
    assert_eq!(anchored["proven"], true, "anchor failed: {anchored}");
    assert_eq!(anchored["score"], 100);

    // The program recomputes, so a claim above the evidence is refused by arithmetic.
    let over = s.signed(&p, &format!("/api/claim?level=4&player={me}&account={me}"));
    assert_eq!(over["granted"], false, "Expert was granted on one case: {over}");
    assert!(
        over["message"].as_str().unwrap_or("").contains("rejected"),
        "the refusal should quote the program: {over}"
    );

    let earned = s.signed(&p, &format!("/api/claim?level=1&player={me}&account={me}"));
    assert_eq!(earned["granted"], true, "{earned}");

    // And reading it back needs no key at all.
    let seen = s.json(&format!("/api/progress?account={me}"));
    assert_eq!(seen["attempts"], 1);
    assert_eq!(seen["level_name"], "Advanced beginner");
}

/// The record follows the person, not the machine.
#[test]
#[ignore = "needs a validator and VITALS_PROGRAM_ID"]
fn a_second_machine_watches_then_joins_then_adds_to_the_same_record() {
    let s = Server::start().expect("VITALS_PROGRAM_ID");
    let laptop = Player::new();
    let desktop = Player::new();
    let account = laptop.pubkey();

    assert_eq!(s.win(&laptop, &account, None)["proven"], true);
    assert_eq!(
        s.signed(&laptop, &format!("/api/claim?level=1&player={account}&account={account}"))["granted"],
        true
    );

    // The desktop has never been seen before and holds a different key entirely.
    let seen = s.json(&format!("/api/progress?account={account}"));
    assert_eq!(seen["attempts"], 1, "a stranger's machine must still be able to read: {seen}");

    let state = s.json(&format!("/api/account?device={}&account={account}", desktop.pubkey()));
    assert_eq!(state["linked"], false, "unlinked machines must say so: {state}");

    let refused = s.signed(&desktop, &format!("/api/claim?level=1&player={}&account={account}", desktop.pubkey()));
    assert_eq!(refused["granted"], false, "an unlinked machine claimed: {refused}");

    // Linked from a machine that already counts.
    let linked = s.signed(
        &laptop,
        &format!("/api/link?player={account}&account={account}&device={}", desktop.pubkey()),
    );
    assert_eq!(linked["devices"], 2, "{linked}");

    // A distinct run from the second machine lands on the same record.
    assert_eq!(s.win(&desktop, &account, Some("check her airway"))["proven"], true);
    let after = s.signed(&desktop, &format!("/api/claim?level=1&player={}&account={account}", desktop.pubkey()));
    assert_eq!(after["granted"], true, "{after}");

    let total = s.json(&format!("/api/progress?account={account}"));
    assert_eq!(total["attempts"], 2, "the second machine's run did not count: {total}");
}

/// A signature from the wrong key must be refused, and the speculative leaf taken back off the
/// tree — otherwise every later proof is built against a tree that does not exist.
#[test]
#[ignore = "needs a validator and VITALS_PROGRAM_ID"]
fn a_forged_signature_is_refused_and_the_tree_is_left_alone() {
    let s = Server::start().expect("VITALS_PROGRAM_ID");
    let victim = Player::new();
    let attacker = Player::new();
    let me = victim.pubkey();

    let before = s.json("/api/chain")["anchored"].as_u64().unwrap_or(0);

    let id = s.json(&format!("/api/new?ep=ep1&player={me}"))["id"].as_str().unwrap().to_string();
    // The victim commits honestly — the attack under test is at the signature, not the gate, and
    // without a commitment the anchor refuses before there is ever anything to sign.
    let c = s.signed(&victim, &format!("/api/commit?id={id}&player={me}&account={me}"));
    assert_eq!(c["committed"], true, "commit failed: {c}");
    for o in ["adrenaline im", "oxygen face mask 10 lpm", "lay her flat, legs up"] {
        s.json(&format!("/api/step?id={id}&player={me}&do={}", enc(o)));
    }
    s.json(&format!("/api/step?id={id}&player={me}&tick=600"));
    s.json(&format!("/api/step?id={id}&player={me}&do={}", enc("admit for observation")));
    s.json(&format!("/api/step?id={id}&player={me}&tick=600"));

    let prep = s.json(&format!("/api/anchor?id={id}&player={me}&account={me}"));
    let msg = prep["sign"].as_str().expect("something to sign");

    // Somebody else signs the bytes the victim was handed.
    let forged = s.json(&format!("/api/submit?player={me}&sig={}", attacker.sign(msg)));
    assert!(forged["error"].is_string(), "a forged signature was accepted: {forged}");

    let after = s.json("/api/chain")["anchored"].as_u64().unwrap_or(0);
    assert_eq!(before, after, "the refused leaf stayed on the tree");
}

/// Station Sets v2, end to end: an exam on OSCE-A anchored at full marks is a three-star case
/// (≥95% of the rubric is flawless), and /api/stars prices gate2's door from exactly that —
/// while every other set stays at zero, because a door is opened by its own set's stars and
/// nothing else's.
#[test]
#[ignore = "needs a validator and VITALS_PROGRAM_ID"]
fn a_perfect_station_exam_is_three_stars_and_prices_only_its_own_gate() {
    let s = Server::start().expect("VITALS_PROGRAM_ID");
    let p = Player::new();
    let me = p.pubkey();

    let id = s.json(&format!("/api/new?ep=osce-a&player={me}"))["id"]
        .as_str()
        .expect("session")
        .to_string();
    // Exam-ness is bound into the commitment before the first order — the star path.
    let c = s.signed(&p, &format!("/api/commit?id={id}&player={me}&account={me}&exam=1"));
    assert_eq!(c["committed"], true, "commit failed: {c}");
    // The canonical run: history, adrenaline inside the window, workup, the name, the
    // observation — every rubric item, including the asks, which are do-path actions
    // at a station (vitals-replay records no event for Step::Ask).
    for o in ["any allergies?", "what did you eat before this?", "adrenaline im",
              "oxygen face mask 10 lpm", "tryptase", "ecg", "anaphylaxis",
              "admit for observation"] {
        s.json(&format!("/api/step?id={id}&player={me}&do={}", enc(o)));
    }
    s.json(&format!("/api/step?id={id}&player={me}&tick=200"));

    let a = s.signed(&p, &format!("/api/anchor?id={id}&player={me}&account={me}"));
    assert_eq!(a["proven"], true, "anchor failed: {a}");
    assert_eq!(a["det"]["score"], a["det"]["max"], "the canonical run scores the whole rubric: {a}");

    let stars = s.json(&format!("/api/stars?account={me}"));
    assert_eq!(stars["stars"], 1, "the legacy count still says one distinct case: {stars}");
    assert_eq!(stars["pass_bps"], 7000);
    assert_eq!(stars["excellent_bps"], 8500);
    assert_eq!(stars["flawless_bps"], 9500, "the third bar rides in the reply: {stars}");
    assert_eq!(stars["tiers_max"], 3);
    let sets = stars["sets"].as_array().expect("sets");
    let gate2 = sets.iter().find(|g| g["gate"] == "gate2").expect("gate2");
    assert_eq!(gate2["tiers"]["osce-a"], 3, "full marks is a three-star case: {gate2}");
    assert_eq!(gate2["total"], 3);
    assert_eq!(gate2["need_now"], 3, "gate2 is priced at three of its six: {gate2}");
    assert_eq!(gate2["ceiling"], 6, "two playable members are worth six stars: {gate2}");
    assert!(
        gate2["total"].as_u64() >= gate2["need_now"].as_u64(),
        "gate2's door should stand open: {gate2}"
    );
    for g in sets.iter().filter(|g| g["gate"] != "gate2") {
        assert_eq!(g["total"], 0, "a star must never leak into another set's gate: {g}");
    }
}

/// The other side of the three-star repricing: excellent is no longer enough on its own.
///
/// The same station, run well but not perfectly — the two send-off items are skipped, which is
/// 36 of the rubric's 40 points, 90%: past the 85% excellence bar and short of the 95% flawless
/// one. Two stars against a door priced at three, so it stays shut, and the debrief's job is to
/// say how far short. This is the behaviour change the whole phase turns on, so it is pinned
/// against a real validator rather than argued about.
#[test]
#[ignore = "needs a validator and VITALS_PROGRAM_ID"]
fn an_excellent_but_not_flawless_run_is_two_stars_and_leaves_the_door_shut() {
    let s = Server::start().expect("VITALS_PROGRAM_ID");
    let p = Player::new();
    let me = p.pubkey();

    let id = s.json(&format!("/api/new?ep=osce-a&player={me}"))["id"]
        .as_str()
        .expect("session")
        .to_string();
    let c = s.signed(&p, &format!("/api/commit?id={id}&player={me}&account={me}&exam=1"));
    assert_eq!(c["committed"], true, "commit failed: {c}");
    // Everything the case asked for except the tryptase (2) and the supporting workup (2).
    for o in ["any allergies?", "what did you eat before this?", "adrenaline im",
              "oxygen face mask 10 lpm", "anaphylaxis", "admit for observation"] {
        s.json(&format!("/api/step?id={id}&player={me}&do={}", enc(o)));
    }
    s.json(&format!("/api/step?id={id}&player={me}&tick=200"));

    let a = s.signed(&p, &format!("/api/anchor?id={id}&player={me}&account={me}"));
    assert_eq!(a["proven"], true, "anchor failed: {a}");
    let (score, max) = (a["det"]["score"].as_u64().unwrap(), a["det"]["max"].as_u64().unwrap());
    let bps = score * 10_000 / max;
    assert!((8_500..9_500).contains(&bps), "this run must land between the bars, got {score}/{max}");

    let stars = s.json(&format!("/api/stars?account={me}"));
    let gate2 = stars["sets"].as_array().expect("sets").iter()
        .find(|g| g["gate"] == "gate2").expect("gate2").clone();
    assert_eq!(gate2["tiers"]["osce-a"], 2, "excellence is two stars, not three: {gate2}");
    assert_eq!(gate2["total"], 2);
    assert!(
        gate2["total"].as_u64() < gate2["need_now"].as_u64(),
        "two stars must no longer open a door priced at three: {gate2}"
    );
    // The legacy flat tally is unmoved by any of this: it still counts a cleared case as one.
    assert_eq!(stars["stars"], 1, "the pass bar means what it always meant: {stars}");
}

/// Linking a machine before ever finishing a case.
///
/// The account only exists once something has been anchored against it, so the first link has to
/// open it and add the machine in one go. It used to build the open and quietly drop the device
/// on the floor: the button said "Add it", a transaction went out, and nothing was linked.
#[test]
#[ignore = "needs a validator and VITALS_PROGRAM_ID"]
fn a_machine_that_has_never_played_can_still_link_another() {
    let s = Server::start().expect("VITALS_PROGRAM_ID");
    let first = Player::new();
    let second = Player::new();
    let account = first.pubkey();

    let before = s.json(&format!("/api/account?device={account}&account={account}"));
    assert_eq!(before["open"], false, "this test is pointless if the account already exists");

    let linked = s.signed(
        &first,
        &format!("/api/link?player={account}&account={account}&device={}", second.pubkey()),
    );
    assert!(linked["error"].is_null(), "linking failed: {linked}");
    assert_eq!(linked["devices"], 2, "the machine was not added: {linked}");

    let state = s.json(&format!("/api/account?device={}&account={account}", second.pubkey()));
    assert_eq!(state["open"], true);
    assert_eq!(state["linked"], true, "the second machine still cannot play: {state}");
}
