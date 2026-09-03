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
