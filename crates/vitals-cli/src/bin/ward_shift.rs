//! A whole shift on the ward, played the way a browser plays it.
//!
//! `ward_proof` proves the program. This proves the **server**: the four transactions a browser
//! signs, in the order a stranger meets them, against a running ward and a real cluster. It holds
//! its own key and never hands it over — every signature here is made locally and posted as bytes,
//! which is the same bargain the browser makes and the reason a shift belongs to the person who
//! played it rather than to whoever hosts the ward.
//!
//! What it walks: open an account · take the head · declare the shift · play a few orders · hand
//! over · anchor. Then it reports her chain length and her state from the ward's own board, so the
//! last line is a fact about the patient rather than about this program.
//!
//!   cargo run -p vitals-cli --bin ward_shift -- http://127.0.0.1:8479 <patient_id>
//!
//! Exit 0 only if the shift anchored.

use solana_sdk::signature::{Keypair, Signer};

fn get(url: &str) -> serde_json::Value {
    match ureq::get(url).call() {
        Ok(r) => r.into_json().unwrap_or(serde_json::Value::Null),
        // A refusal is an answer, not a failure: the ward says no with a body and a 409, and the
        // whole point of this program is to read what it said.
        Err(ureq::Error::Status(_, r)) => r.into_json().unwrap_or(serde_json::Value::Null),
        Err(e) => {
            eprintln!("  {url}: {e}");
            serde_json::Value::Null
        }
    }
}

/// Ask the ward for bytes to sign, sign them here, and post the signature back.
fn signed_step(base: &str, path: &str, key: &Keypair, what: &str) -> serde_json::Value {
    let who = key.pubkey();
    let asked = get(&format!("{base}{path}{}player={who}", if path.contains('?') { "&" } else { "?" }));
    let Some(msg) = asked["sign"].as_str() else {
        println!("  REFUSED  {what}: {}", terse(&asked));
        return asked;
    };
    let bytes: Vec<u8> = (0..msg.len() / 2)
        .filter_map(|i| u8::from_str_radix(&msg[i * 2..i * 2 + 2], 16).ok())
        .collect();
    let sig = key.sign_message(&bytes);
    let hex: String = sig.as_ref().iter().map(|b| format!("{b:02x}")).collect();
    let out = get(&format!("{base}/api/ward/submit?player={who}&sig={hex}"));
    match out.get("refused").and_then(|r| r.as_str()) {
        Some(said) => println!("  refused  {what} — {said}"),
        None if out.get("error").is_some() => println!("  ERROR    {what}: {}", terse(&out)),
        None => println!("  ok       {what}"),
    }
    out
}

fn terse(v: &serde_json::Value) -> String {
    v["error"].as_str().or_else(|| v["refused"].as_str()).unwrap_or("").to_string()
}

/// Play a shift end to end. Returns the anchor's answer.
fn whole_shift(base: &str, patient: u64, key: &Keypair) -> serde_json::Value {
    let me = key.pubkey();
    let opened = get(&format!("{base}/api/new?patient={patient}&player={me}"));
    let Some(session) = opened["id"].as_str() else {
        eprintln!("  could not open a shift: {}", terse(&opened));
        std::process::exit(1);
    };
    println!("  ok       her chart rebuilt — shift {} of her stay, head {}",
             opened["ward"]["shift"], &opened["ward"]["head"].as_str().unwrap_or("")[..8]);

    signed_step(base, "/api/ward/open", key, "opens an account");
    let took = signed_step(base, &format!("/api/ward/take?id={session}"), key, "takes the head");
    if took["took"] != serde_json::Value::Bool(true) {
        std::process::exit(1);
    }
    let declared = signed_step(base, &format!("/api/ward/declare?id={session}"), key,
                               "declares the shift before playing it");
    if declared["declared"] != serde_json::Value::Bool(true) {
        std::process::exit(1);
    }
    for order in ["oxygen", "adrenaline im", "normal saline bolus"] {
        let step = get(&format!("{base}/api/step?id={session}&player={me}&do={}",
                                order.replace(' ', "%20")));
        match step["hr"].as_f64() {
            Some(hr) => println!("  ok       {order} — hr {hr:.0} sbp {}", step["sbp"]),
            None => {
                eprintln!("  the order did not land: {}", terse(&step));
                std::process::exit(1);
            }
        }
    }
    let over = get(&format!("{base}/api/handover?id={session}&player={me}"));
    let Some(run_hash) = over["run_hash"].as_str() else {
        eprintln!("  the handover did not reduce: {}", terse(&over));
        std::process::exit(1);
    };
    println!("  ok       handed over — {} beats, tape {}", over["shift"]["beats"], &run_hash[..8]);
    signed_step(base, &format!("/api/ward/anchor?id={session}"), key,
                "anchors the shift onto her chain")
}

fn main() {
    let mut args = std::env::args().skip(1);
    let base = args.next().unwrap_or_else(|| "http://127.0.0.1:8479".into());
    let patient: u64 = args.next().and_then(|p| p.parse().ok()).unwrap_or_else(|| {
        eprintln!("usage: ward_shift <base-url> <patient_id> [--stale]");
        std::process::exit(2);
    });
    let stale = args.next().as_deref() == Some("--stale");

    // A key that has never existed before, which is what every stranger's browser holds.
    let key = Keypair::new();
    println!("── ward     {base}\n── patient  {patient}\n── key      {}", key.pubkey());
    let base = base.trim_end_matches('/').to_string();

    if stale {
        // The refusal that is the mechanic. B opens her chart and starts working; A arrives, plays
        // a whole shift and anchors, so the head moves; B then anchors against the head they were
        // told about and the program refuses them. B's work is not lost — it is on their tape, and
        // opening her again replays it onto the patient as she now is.
        let b = Keypair::new();
        let me = b.pubkey();
        let opened = get(&format!("{base}/api/new?patient={patient}&player={me}"));
        let Some(session) = opened["id"].as_str().map(str::to_string) else {
            eprintln!("  B could not open a shift: {}", terse(&opened));
            std::process::exit(1);
        };
        println!("  ok       B opens her chart at head {}",
                 &opened["ward"]["head"].as_str().unwrap_or("")[..8]);

        println!("── A arrives while B is still with her");
        let a_anchored = whole_shift(&base, patient, &key);
        if a_anchored["anchored"] != serde_json::Value::Bool(true) {
            std::process::exit(1);
        }
        println!("  ok       A anchored — the head is now {}",
                 &a_anchored["head"].as_str().unwrap_or("")[..8]);

        println!("── B tries to anchor the head they were told about");
        signed_step(&base, "/api/ward/open", &b, "B opens an account");
        signed_step(&base, &format!("/api/ward/take?id={session}"), &b, "B takes the head");
        signed_step(&base, &format!("/api/ward/declare?id={session}"), &b, "B declares");
        let _ = get(&format!("{base}/api/step?id={session}&player={me}&do=oxygen"));
        let _ = get(&format!("{base}/api/handover?id={session}&player={me}"));
        let refused = signed_step(&base, &format!("/api/ward/anchor?id={session}"), &b,
                                  "B anchors against the head that moved");
        if refused["refused"].is_string() {
            println!("\nthe refusal is the mechanic: the chain said no to B, in words, and A's \
                      shift stands");
            return;
        }
        eprintln!("\nB was not refused, and should have been");
        std::process::exit(1);
    }

    let anchored = whole_shift(&base, patient, &key);
    if anchored["anchored"] != serde_json::Value::Bool(true) {
        eprintln!("\nthe shift did not anchor");
        std::process::exit(1);
    }
    println!("  ok       her chain is {} shift(s) long, head {}, she is {}",
             anchored["shifts"], &anchored["head"].as_str().unwrap_or("")[..8], anchored["state"]);

    println!("\nthe loop closed: a stranger with a new key took a shift, played it, and the chain \
              says so");
}
