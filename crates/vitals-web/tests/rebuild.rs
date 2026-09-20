//! Sessions rebuilt when somebody asks for them, rather than all of them at boot.
//!
//! Boot restored every saved session: 22 of them, 79.7 s, 3.6 s each — a chain rebuild apiece, done
//! for requests that may never arrive, while the first real reader was held 62 s waiting for the
//! container to listen. Nothing outside a request ever reads a session (`Arc::clone(&sessions)`
//! appears nowhere; all five spawned threads mention it zero times), so none of that work had to
//! happen before the door opened.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use vitals_web::rebuild::{census, cannot_rebuild, Kind, Rebuilds};

/// **Two requests for the same cold id share one rebuild.**
///
/// A rebuild is a chain replay. Two of them for one session is two walks of the same history to
/// reach the same answer, and — if they interleave — two writes of what should be one.
#[test]
fn two_requests_for_a_cold_id_produce_one_rebuild() {
    let rebuilds = Arc::new(Rebuilds::default());
    let built = Arc::new(AtomicUsize::new(0));
    let ready = Arc::new(AtomicBool::new(false));

    let mut hands = Vec::new();
    for _ in 0..2 {
        let (rebuilds, built, ready) = (rebuilds.clone(), built.clone(), ready.clone());
        hands.push(std::thread::spawn(move || {
            rebuilds.once(
                "one-id",
                || ready.load(Ordering::SeqCst),
                || {
                    // Long enough that the second thread is certainly inside `once` while this runs.
                    std::thread::sleep(std::time::Duration::from_millis(120));
                    built.fetch_add(1, Ordering::SeqCst);
                    ready.store(true, Ordering::SeqCst);
                },
            );
        }));
    }
    for h in hands {
        h.join().expect("both callers return");
    }
    assert_eq!(built.load(Ordering::SeqCst), 1,
               "one rebuild for one id: the second caller waited for the first and took its answer");

    // A different id is a different rebuild — the gate is per session, not a queue for all of them.
    rebuilds.once("another", || false, || { built.fetch_add(1, Ordering::SeqCst); });
    assert_eq!(built.load(Ordering::SeqCst), 2);

    // And an id already in hand costs nothing at all.
    rebuilds.once("one-id", || true, || { built.fetch_add(1, Ordering::SeqCst); });
    assert_eq!(built.load(Ordering::SeqCst), 2, "nothing is rebuilt that is already there");
}

/// **What boot says it found, without touching a chain to say it.**
///
/// The counts come off the saved records themselves. "With a bed" is not among them on purpose: a
/// bed is derived from the board, not carried in a session, and asking the chain for it at boot is
/// the thing being removed. What a record does know is whether it is still live.
#[test]
fn the_census_says_what_is_in_the_store_without_reading_the_chain() {
    let kinds = [
        Kind::WardLive, Kind::WardLive, Kind::WardFinished, Kind::Review, Kind::Other, Kind::Review,
    ];
    let line = census(&kinds);
    assert!(line.contains("6"), "the total, first: {line}");
    assert!(line.contains("2 mid-shift"), "the two that could still be played: {line}");
    assert!(line.contains("1 finished"), "{line}");
    assert!(line.contains("2 review"), "{line}");
    assert!(line.contains("1 other"), "{line}");

    assert!(census(&[]).contains('0'), "an empty store says so rather than saying nothing");
}

/// **A session that cannot be rebuilt does not leave its holder stuck on a bed.**
///
/// The head is held by a lease the chain will not release for ten minutes, and by a heartbeat this
/// server frees after 75 s of silence. A page told only "no such session" keeps beating — so the
/// bed stays taken for the whole lease by somebody who cannot play. The answer has to tell the page
/// to stop, and offer the exit that puts the head back.
#[test]
fn a_session_that_cannot_be_rebuilt_frees_its_bed() {
    let v = cannot_rebuild("the tape for a3f… is not here, so this shift cannot be replayed");

    assert_eq!(v["stop_beating"], true,
               "or the head is held for the whole lease by a page that cannot use it");
    assert_eq!(v["leave"], true, "and the exit is offered rather than left to be guessed at");
    let says = v["error"].as_str().expect("words a person at a bedside can act on");
    assert!(says.contains("not here") || says.contains("rebuilt"),
            "which say what happened rather than naming a status code: {says}");
    assert!(!says.contains("panic") && !says.contains("unwrap"), "{says}");
}

/// **A one-at-a-time gate is given back however the worker leaves, panics included.**
///
/// `refresh_behind` took `BOARD_READING` with a `swap(true)` and gave it back with a
/// `store(false)` on the last line of the spawned thread. Every line above it could panic — a
/// chain read, or the `view.lock().unwrap()` that panics on a poisoned mutex, which is exactly why
/// the rest of this file uses `unwrap_or_else(|e| e.into_inner())` instead. One panic there and the
/// flag stays set for the life of the process: nothing refreshes the board again, and the symptom
/// is the worst kind there is — no error, no log, just a ward that quietly gets older while every
/// request is told it is being refreshed behind the answer.
///
/// `Rebuilds` already learned this one level up and gives its gate back from a `Drop`. This is that
/// lesson for a flag, so the two are the same shape and neither can be the exception.
#[test]
fn a_gate_is_given_back_even_when_the_worker_panics() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use vitals_web::rebuild::take;

    static FLAG: AtomicBool = AtomicBool::new(false);

    let held = take(&FLAG).expect("a free gate is taken");
    assert!(take(&FLAG).is_none(), "a second taker is refused outright, never queued behind it");

    // The worker dies the way a chain read dies: mid-flight, with the gate in hand.
    let died = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _gate = held;
        panic!("the chain read blew up");
    }));
    assert!(died.is_err(), "the panic is real, not swallowed by the test");

    let next = take(&FLAG).expect(
        "the next worker gets in — a process that panicked once is not a process that stops \
         refreshing its board until somebody notices a stale ward");
    assert!(FLAG.load(Ordering::SeqCst), "and while it works, the gate reads as taken");
    drop(next);
    assert!(!FLAG.load(Ordering::SeqCst), "and it gives the gate back on the ordinary way out too");
}
