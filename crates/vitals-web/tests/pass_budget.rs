//! **A pass whose time is up asks the chain nothing more.**
//!
//! 27 Sep 2026, production 00055–00056: the ward's pass took a median 1,138 s (15 s on 25 Sep), with
//! `repair` alone at ~1,100 s. The pass has a ten-second budget, but `refresh` made its signature
//! listing *before* looking at it, so once the ten seconds were spent every remaining patient still
//! paid one listing — about 50 s each while devnet answered 429. Twenty-odd patients was twenty
//! minutes, and every arrival and closure on the ward waited behind it.
//!
//! The chain here is an address nothing listens on. A read that asks it fails with a connection
//! error; a read that honours a spent budget never finds out.

use vitals_web::store::Store;
use vitals_web::ward_chain::{Budget, Seen, WardChain};

#[test]
fn a_pass_whose_time_is_up_asks_the_chain_nothing_more() {
    // Its own process (one test binary), so these reach nobody else's test.
    std::env::set_var("VITALS_RPC", "http://127.0.0.1:1");
    std::env::set_var("VITALS_PROGRAM_ID", "4YpyZ2oM8jtxM9GwC61kUsnhMFvWkYatrWVZpiafqypz");
    std::env::set_var("VITALS_OPERATOR", "11111111111111111111111111111111");
    std::env::remove_var("VITALS_KEYPAIR");
    let chain = WardChain::connect().expect("a chain handle; nothing is asked of it yet");
    let dir = std::env::temp_dir().join(format!("vitals-pass-budget-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(dir).expect("a store");

    // The premise: with time left, the read does go to the chain — and finds nobody there.
    let mut seen = Seen::default();
    assert!(chain.refresh(7, &mut seen, &store, &Budget::pass()).is_err(),
            "the premise: a read with time left asks the chain, and this chain is not there");

    // Time up: nothing is asked, the read says it stopped, and says why.
    let spent = Budget {
        entries: 200,
        until: std::time::Instant::now().checked_sub(std::time::Duration::from_secs(1)),
    };
    let mut seen = Seen::default();
    let reading = chain
        .refresh(7, &mut seen, &store, &spent)
        .expect("a spent budget is a stop, not a failure — nothing was asked, so nothing failed");
    assert!(!reading.whole(), "a history nobody read must not be decided on");
    assert_eq!(reading.added, 0);
}
