//! The payout policy, decided without a clock, a network or a key in the room.
//!
//! Everything money-shaped that can be wrong is wrong in `decide` and `split`, so it is checked
//! here where a wrong answer costs nothing and a validator is not needed to see it.

use std::collections::BTreeSet;
use vitals_web::payout::{self, Ask, Split, Verdict, DEVNET_GENESIS, MEMO_PREFIX};

const AUTHOR: &str = "3RJV5VrzqPLXbvRurWbMVB5WTXyvK1R4FwZ39hZRYLyR";
const LEAF: &str = "b211ee54dd700a993d80ce933333d2b26dbac66c18ad93ed93a4fadd353960b7";

fn allow(keys: &[&str]) -> BTreeSet<String> {
    keys.iter().map(|k| k.to_string()).collect()
}

fn ask<'a>(
    allowlist: &'a BTreeSet<String>,
    paid: &'a BTreeSet<String>,
    author: Option<&'a str>,
) -> Ask<'a> {
    Ask {
        rate: 1_000_000,
        platform_bps: 1500,
        allowlist,
        author,
        leaf: LEAF,
        paid,
        spent_today: 0,
        daily_cap: 100_000_000,
        balance: 5_000_000_000,
    }
}

// ── the split ───────────────────────────────────────────────────────────────

#[test]
fn the_split_is_integer_and_the_halves_always_sum_to_the_rate() {
    assert_eq!(payout::split(1_000_000, 1500), Split { author: 850_000, platform: 150_000 });
    // Every rate and every share: the two parts must reconstruct the whole, exactly. A lamport
    // that goes missing here is a lamport nobody can account for later.
    for rate in [0u64, 1, 2, 3, 7, 999, 1_000_000, 123_456_789, u64::MAX / 4] {
        for bps in [0u32, 1, 999, 1500, 2000, 3333, 9999, 10_000] {
            let s = payout::split(rate, bps);
            assert_eq!(s.author + s.platform, rate, "rate {rate} bps {bps} lost a lamport");
        }
    }
}

/// Rounding must never come out of the author's share.
#[test]
fn a_rounded_lamport_is_taken_from_the_platform_not_the_author() {
    // 3 * 1500 / 10000 = 0.45 → 0 to the platform, all three to the author.
    assert_eq!(payout::split(3, 1500), Split { author: 3, platform: 0 });
    assert_eq!(payout::split(1, 9999), Split { author: 1, platform: 0 });
}

#[test]
fn the_whole_rate_can_go_either_way_when_told_to() {
    assert_eq!(payout::split(1000, 0), Split { author: 1000, platform: 0 });
    assert_eq!(payout::split(1000, 10_000), Split { author: 0, platform: 1000 });
}

// ── the memo, which is the only record that a payout happened ───────────────

#[test]
fn a_memo_names_its_leaf_and_survives_the_round_trip() {
    let m = payout::memo(LEAF);
    assert!(m.starts_with(MEMO_PREFIX), "the version prefix is what makes this readable later");
    assert_eq!(payout::leaf_from_memo(&m).as_deref(), Some(LEAF));
}

/// The shape a real RPC hands back, which is not the shape we sent.
///
/// `getSignaturesForAddress` frames a memo with its instruction length and joins several with
/// "; ". A parser written against the string we wrote passed every test here and then found
/// nothing on devnet — this is that live failure, kept.
#[test]
fn the_framing_a_real_rpc_adds_is_read_through() {
    let framed = format!("[81] {}", payout::memo(LEAF));
    assert_eq!(payout::leaf_from_memo(&framed).as_deref(), Some(LEAF));
    let other = "c".repeat(64);
    let two = format!("[12] something else; [81] {}", payout::memo(&other));
    assert_eq!(payout::leaf_from_memo(&two).as_deref(), Some(other.as_str()));
}

#[test]
fn a_memo_that_is_not_ours_names_nothing() {
    for not_ours in [
        "",
        "hello",
        "vitals.payout.v2 ",
        "vitals.payout.v1 ",
        "vitals.payout.v1 short",
        "vitals.payout.v1 zzzz",
        "prefixed vitals.payout.v1 b211ee54dd700a993d80ce933333d2b26dbac66c18ad93ed93a4fadd353960b7",
        // A bracket that is not the transport's length marker is not a licence to skip text.
        "[not-a-length] vitals.payout.v1 b211ee54dd700a993d80ce933333d2b26dbac66c18ad93ed93a4fadd353960b7",
    ] {
        assert_eq!(payout::leaf_from_memo(not_ours), None, "{not_ours:?} was read as a payout");
    }
}

/// The paid set is re-derived from the chain, so it must survive junk beside it.
#[test]
fn the_paid_set_is_rebuilt_from_memos_and_ignores_everything_else() {
    let other = "a".repeat(64);
    let paid = payout::paid_from_memos(vec![
        payout::memo(LEAF).as_str(),
        "someone else's memo",
        payout::memo(&other).as_str(),
        "vitals.payout.v1 not-a-hash",
    ]);
    assert_eq!(paid.len(), 2);
    assert!(paid.contains(LEAF) && paid.contains(&other));
}

// ── the policy ──────────────────────────────────────────────────────────────

#[test]
fn an_attributed_allowlisted_author_is_paid() {
    let (a, p) = (allow(&[AUTHOR]), BTreeSet::new());
    assert_eq!(
        payout::decide(&ask(&a, &p, Some(AUTHOR))),
        Verdict::Pay(Split { author: 850_000, platform: 150_000 })
    );
}

/// **The guard that stops a commit being a payment instruction.**
#[test]
fn a_signed_attribution_alone_does_not_get_paid() {
    let (a, p) = (allow(&["SomebodyElsesKey1111111111111111111111111"]), BTreeSet::new());
    match payout::decide(&ask(&a, &p, Some(AUTHOR))) {
        Verdict::Skip(why) => assert!(why.contains("not payable"), "{why}"),
        v => panic!("an unlisted key was paid: {v:?}"),
    }
}

/// Empty allowlist pays nobody. It is the default, and the default must be the safe one.
#[test]
fn an_empty_allowlist_pays_nobody() {
    let (a, p) = (BTreeSet::new(), BTreeSet::new());
    assert!(matches!(payout::decide(&ask(&a, &p, Some(AUTHOR))), Verdict::Skip(_)));
}

#[test]
fn a_case_nobody_has_claimed_is_not_paid_for() {
    let (a, p) = (allow(&[AUTHOR]), BTreeSet::new());
    match payout::decide(&ask(&a, &p, None)) {
        Verdict::Skip(why) => assert!(why.contains("no attribution"), "{why}"),
        v => panic!("a payout went out with no author: {v:?}"),
    }
}

/// The same leaf twice is one payment, and the chain is what says so.
#[test]
fn a_leaf_the_chain_has_already_paid_is_never_paid_again() {
    let a = allow(&[AUTHOR]);
    let paid = payout::paid_from_memos(vec![payout::memo(LEAF).as_str()]);
    match payout::decide(&ask(&a, &paid, Some(AUTHOR))) {
        Verdict::Skip(why) => assert!(why.contains("already paid"), "{why}"),
        v => panic!("a leaf was paid twice: {v:?}"),
    }
}

#[test]
fn rate_zero_makes_the_whole_thing_inert() {
    let (a, p) = (allow(&[AUTHOR]), BTreeSet::new());
    let mut k = ask(&a, &p, Some(AUTHOR));
    k.rate = 0;
    match payout::decide(&k) {
        Verdict::Skip(why) => assert!(why.contains("payouts are off"), "{why}"),
        v => panic!("something was paid with the rate at zero: {v:?}"),
    }
}

#[test]
fn the_daily_cap_skips_rather_than_queues() {
    let (a, p) = (allow(&[AUTHOR]), BTreeSet::new());
    let mut k = ask(&a, &p, Some(AUTHOR));
    k.spent_today = k.daily_cap;
    match payout::decide(&k) {
        Verdict::Skip(why) => assert!(why.contains("cap reached"), "{why}"),
        v => panic!("the cap was crossed: {v:?}"),
    }
    // And the payout that would cross it is refused whole, not part-paid.
    k.spent_today = k.daily_cap - 1;
    assert!(matches!(payout::decide(&k), Verdict::Skip(_)), "a payout straddled the cap");
}

/// Stopping at twice the rate means running out is something you see coming.
#[test]
fn a_wallet_is_not_paid_down_to_empty() {
    let (a, p) = (allow(&[AUTHOR]), BTreeSet::new());
    let mut k = ask(&a, &p, Some(AUTHOR));
    k.balance = k.rate * 2 - 1;
    match payout::decide(&k) {
        Verdict::Skip(why) => assert!(why.contains("below twice the rate"), "{why}"),
        v => panic!("the wallet was drained: {v:?}"),
    }
    k.balance = k.rate * 2;
    assert!(matches!(payout::decide(&k), Verdict::Pay(_)), "an affordable payout was refused");
}

/// The cluster this may run against is pinned in code, and it is not mainnet.
#[test]
fn the_cluster_guard_names_devnet() {
    assert_eq!(DEVNET_GENESIS, "EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG");
}

// ── against a real cluster ──────────────────────────────────────────────────

/// The cluster guard is the line that keeps this off mainnet, so it is checked against a real
/// RPC rather than trusted. A local validator has its own genesis, which is not devnet's — so
/// pointing the payer at one must refuse, exactly as mainnet would.
#[test]
#[ignore = "needs a local validator at VITALS_RPC"]
fn a_cluster_that_is_not_devnet_is_refused() {
    let rpc = std::env::var("VITALS_RPC").unwrap_or_else(|_| "http://127.0.0.1:8899".into());
    // SAFETY: single-threaded test; the values are read back inside from_env immediately.
    unsafe {
        std::env::set_var("VITALS_PAYOUT_LAMPORTS", "1000000");
        std::env::set_var("VITALS_PAYOUT_KEY", std::env::var("PAYOUT_KEY").unwrap_or_default());
    }
    let got = vitals_web::payout::Payer::from_env(&rpc);
    unsafe {
        std::env::remove_var("VITALS_PAYOUT_LAMPORTS");
        std::env::remove_var("VITALS_PAYOUT_KEY");
    }
    match got {
        Err(why) => assert!(
            why.contains("devnet-only") || why.contains("genesis"),
            "refused, but not for being the wrong cluster: {why}"
        ),
        Ok(_) => panic!("a local validator was accepted as devnet — the mainnet guard is open"),
    }
}

/// Rate 0 needs no key, no cluster and no wallet, and must not even look for them.
#[test]
fn rate_zero_builds_no_payer_and_asks_for_nothing() {
    // SAFETY: single-threaded test.
    unsafe {
        std::env::set_var("VITALS_PAYOUT_LAMPORTS", "0");
        std::env::remove_var("VITALS_PAYOUT_KEY");
    }
    let got = vitals_web::payout::Payer::from_env("http://127.0.0.1:1");
    unsafe { std::env::remove_var("VITALS_PAYOUT_LAMPORTS") };
    assert!(matches!(got, Ok(None)), "rate 0 did something: {:?}", got.err());
}

/// A rate with no key is a configuration that asks to pay people and cannot. It stops the deploy.
#[test]
fn a_rate_without_a_key_refuses_to_start() {
    // SAFETY: single-threaded test.
    unsafe {
        std::env::set_var("VITALS_PAYOUT_LAMPORTS", "1000000");
        std::env::remove_var("VITALS_PAYOUT_KEY");
    }
    let got = vitals_web::payout::Payer::from_env("http://127.0.0.1:1");
    unsafe { std::env::remove_var("VITALS_PAYOUT_LAMPORTS") };
    match got {
        Err(why) => assert!(why.contains("VITALS_PAYOUT_KEY"), "{why}"),
        Ok(_) => panic!("a payout rate was accepted with no wallet behind it"),
    }
}

/// The whole mechanism against devnet: pay once, see it in the memos, refuse to pay it twice.
///
/// Needs a funded payout key. `PAYOUT_KEY` points at one; the author paid is whatever
/// `PAYOUT_AUTHOR` names, so this never has to hard-code somebody's wallet.
#[test]
#[ignore = "needs a funded devnet payout key in PAYOUT_KEY"]
fn a_payout_lands_on_devnet_and_is_never_made_twice() {
    let key = std::env::var("PAYOUT_KEY").expect("PAYOUT_KEY");
    let author = std::env::var("PAYOUT_AUTHOR").expect("PAYOUT_AUTHOR");
    // SAFETY: single-threaded test.
    unsafe {
        std::env::set_var("VITALS_PAYOUT_LAMPORTS", "1000000");
        std::env::set_var("VITALS_PAYOUT_KEY", &key);
        std::env::set_var("VITALS_PAYOUT_ALLOWLIST", &author);
    }
    let payer = vitals_web::payout::Payer::from_env("https://api.devnet.solana.com")
        .expect("devnet should be accepted")
        .expect("a rate was set, so there should be a payer");

    let before = payer.paid_leaves().expect("read the paid set");
    // A leaf of this run's own, so the test proves the refusal rather than tripping over the
    // last run's success. The first time this ran it paid LEAF for real, and the second time it
    // stopped on the chain's memo — which is the mechanism working, but not a test that can be
    // run twice.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("a clock")
        .as_nanos();
    let leaf = format!("{stamp:064x}");
    assert!(!before.contains(&leaf), "a leaf from this instant was somehow already paid");

    let allowlist: BTreeSet<String> = [author.clone()].into_iter().collect();
    let k = Ask {
        rate: payer.rate,
        platform_bps: payer.platform_bps,
        allowlist: &allowlist,
        author: Some(&author),
        leaf: &leaf,
        paid: &before,
        spent_today: 0,
        daily_cap: payer.daily_cap,
        balance: payer.balance(),
    };
    let Verdict::Pay(split) = payout::decide(&k) else {
        panic!("the first payout was refused: {:?}", payout::decide(&k));
    };
    let paid = payer.pay(&leaf, &author, split).expect("the payout should land");
    println!("  paid {} lamports to {author}", paid.author_lamports);
    println!("  https://explorer.solana.com/tx/{}?cluster=devnet", paid.signature);

    // Known immediately, because the payer remembers what it just did — the RPC's index lags,
    // and this is the window a naive implementation would pay twice in.
    let straight_away = payer.paid_leaves().expect("re-read the paid set");
    assert!(straight_away.contains(&leaf), "the payer did not remember its own payment");

    // And on the chain, once the index catches up. Polled rather than asserted at once: how long
    // that takes is devnet's business, and pretending it is instant is how the window got missed.
    let mut on_chain = false;
    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_secs(2));
        if payout::paid_from_memos(
            fresh_memos(&payer).iter().map(String::as_str),
        )
        .contains(&leaf)
        {
            on_chain = true;
            break;
        }
    }
    assert!(on_chain, "the memo never appeared in the wallet's history on chain");
    let after = straight_away;

    // And the second attempt is refused by the chain's own record, not by anything remembered.
    let k2 = Ask { paid: &after, ..k };
    match payout::decide(&k2) {
        Verdict::Skip(why) => assert!(why.contains("already paid"), "{why}"),
        v => panic!("the same leaf was about to be paid twice: {v:?}"),
    }
    unsafe {
        std::env::remove_var("VITALS_PAYOUT_LAMPORTS");
        std::env::remove_var("VITALS_PAYOUT_KEY");
        std::env::remove_var("VITALS_PAYOUT_ALLOWLIST");
    }
}

/// The wallet's memos straight from the RPC, with nothing this process remembers mixed in — so
/// the test can tell "the chain knows" apart from "the payer knows".
fn fresh_memos(payer: &vitals_web::payout::Payer) -> Vec<String> {
    use std::process::Command;
    let out = Command::new("curl")
        .args(["-s", "https://api.devnet.solana.com", "-X", "POST", "-H",
               "Content-Type: application/json", "-d",
               &format!(r#"{{"jsonrpc":"2.0","id":1,"method":"getSignaturesForAddress","params":["{}",{{"limit":20}}]}}"#,
                        payer.address())])
        .output()
        .expect("curl");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
    v["result"].as_array().map(|a| a.iter()
        .filter(|s| s["err"].is_null())
        .filter_map(|s| s["memo"].as_str().map(str::to_string))
        .collect()).unwrap_or_default()
}
