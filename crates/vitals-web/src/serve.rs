//! Which requests may be answered off the loop that writes.
//!
//! **Why this exists.** Cloud Run is told this container takes eight requests at once
//! (`concurrency 8`) and `main` has always answered them one at a time —
//! `for mut req in server.incoming_requests()`. During a ticker pass, which is eleven to fifteen
//! seconds at the census the ward now runs at and about eighty on a cold start, Cloud Run counts
//! eight in flight while seven of them sit unserved, and refuses the ninth with *no available
//! instance*. `max-instances` is 1 and must stay 1: the anchoring tree is in memory, and a second
//! instance would hold a different one. So the platform refuses exactly as configured and the
//! container is the thing that is wrong.
//!
//! Three bursts were measured on production on 22 ก.ย. — 01:41, 02:32 and 04:35 — on `/api/ward`,
//! `/api/usage`, `/review` and `/favicon.ico`. Every one of them is a read.
//!
//! **Why it is a list and not a guess.** The question that decides whether a route may leave the
//! single loop is *does answering it change anything* — a session, a tape, the tree, a counter on
//! disk. That question has one right answer per route and it is not inferable from the method:
//! `GET /api/new` starts a run. So the paths are named here, one at a time, and the test beside
//! this file is what stops the list growing by assumption. A route added to the server and not to
//! this list keeps its old behaviour, which is the safe direction.
//!
//! Everything absent from the list stays exactly where it was, in the order it was always served.
//! A take or an anchor arriving mid-pass should wait for the pass rather than race it; a stranger
//! reading the board should not, and until now they were refused.

/// May this request be answered away from the loop that writes?
///
/// `method` is the HTTP method as the server spells it (`GET`, `POST`); `path` is the URL with any
/// query string already removed.
pub fn is_read_only(method: &str, path: &str) -> bool {
    if !method.eq_ignore_ascii_case("GET") {
        return false;
    }
    // The board and the counters a dashboard polls, the pages a stranger reads, and the files
    // those pages pull. Nothing here opens a session, writes a tape, or touches the chain.
    const READS: &[&str] = &[
        "/api/ward",
        "/api/usage",
        "/review",
        "/privacy",
        "/stats",
        "/start",
        "/start/",
        "/bay.css",
        "/bay.js",
        "/favicon.ico",
        "/world/favicon.svg",
        "/world/apple-touch-icon.png",
    ];
    if READS.contains(&path) {
        return true;
    }
    // The guide's own pictures: a closed set inside the server, so this prefix cannot reach
    // anything the server does not already refuse.
    path.starts_with("/start/img/")
}
