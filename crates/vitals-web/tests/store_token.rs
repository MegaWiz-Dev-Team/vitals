//! The access token the store uses, and how often it is asked for.
//!
//! Every Firestore operation fetched a fresh token from `metadata.google.internal` before its own
//! request: five call sites, no cache, nothing reading `expires_in`. So each store read on Cloud Run
//! cost two round trips, one of them to the metadata server — and a boot that restores N sessions
//! paid 2N of them in series. Measured on staging, 18 September: 137 s of a 137.8 s boot, with a
//! reader held for 118 s of it.
//!
//! A token is good for an hour. Asking once an hour instead of once a call is the whole fix.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use vitals_web::store::{refresh_after, Tokens};

/// **A token is fetched once for the window it is good for.**
#[test]
fn a_token_is_fetched_once_for_the_window_it_is_good_for() {
    // The margin: 80% of the life, or five minutes before the end, whichever comes first. A token
    // that expires mid-request is a request that fails for no reason anybody can act on.
    assert_eq!(refresh_after(3600), Duration::from_secs(2880), "an hour is refreshed at 48 minutes");
    assert_eq!(refresh_after(600), Duration::from_secs(300),
               "ten minutes: five before the end is earlier than 80%, so five wins");
    assert_eq!(refresh_after(300), Duration::from_secs(0), "a token this short is refreshed on sight");
    assert_eq!(refresh_after(0), Duration::from_secs(0), "and one with no life at all is never held");

    let calls = AtomicUsize::new(0);
    let fetch = |v: &'static str| {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok((v.to_string(), 3600u64))
    };

    let tokens = Tokens::default();
    assert_eq!(tokens.get(|| fetch("t1")).expect("a token"), "t1");
    assert_eq!(tokens.get(|| fetch("t2")).expect("the same token"), "t1",
               "the second call is answered from the first: this is the whole point");
    assert_eq!(calls.load(Ordering::SeqCst), 1,
               "two store calls inside the window fetch the token once");

    // A 401 is the one thing that can make a live token wrong. It forces exactly one refresh.
    tokens.invalidate();
    assert_eq!(tokens.get(|| fetch("t3")).expect("a fresh one"), "t3");
    assert_eq!(calls.load(Ordering::SeqCst), 2, "one refresh, not a storm of them");

    // A metadata server that will not answer is not cached as a success, or one bad second would
    // become an hour of a store that cannot read.
    let sad = Tokens::default();
    assert!(sad.get(|| Err("metadata server said no".to_string())).is_err());
    let after = sad.get(|| Ok(("recovered".to_string(), 3600)));
    assert_eq!(after.expect("it tries again"), "recovered",
               "a failure is not remembered as an answer");
}
