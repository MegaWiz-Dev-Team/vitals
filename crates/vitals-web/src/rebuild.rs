//! Sessions rebuilt when somebody asks for them, rather than all of them at boot.
//!
//! Every saved session used to be replayed from the chain before the server would listen: 22 of
//! them cost 79.7 s on staging, 3.6 s each, while the first real reader waited 62 s for a door that
//! was busy rebuilding runs nobody had asked about.
//!
//! Nothing outside a request reads a session. The map is locked only inside the request loop, and
//! the background threads — the refill, the beat sweeper, the board refresh — never touch it. The
//! sweeper's own comment says what it relies on instead: "After a restart the leases are the net
//! until the pages beat again."
//!
//! So the work moves to the moment somebody actually holds an id, which is the only moment it is
//! worth anything.

use std::collections::HashSet;
use std::sync::{Condvar, Mutex};

/// One rebuild per id at a time.
///
/// A rebuild is a chain replay. Two for the same session is two walks of one history to reach one
/// answer, and a browser that retries while the first is still running would start the second.
/// A second caller waits for the first and then takes what it produced; a *different* id is not
/// made to queue behind it.
#[derive(Default)]
pub struct Rebuilds {
    in_flight: Mutex<HashSet<String>>,
    finished: Condvar,
}

impl Rebuilds {
    /// Build this id unless it is already there, or somebody else is already building it.
    ///
    /// `already` is asked twice on purpose: once before waiting, and once after — because the
    /// thread we waited for has almost certainly just put the answer where `already` can see it,
    /// and rebuilding it again would be the exact waste this exists to prevent.
    pub fn once(&self, id: &str, already: impl Fn() -> bool, build: impl FnOnce()) {
        if already() {
            return;
        }
        let mut flight = self.in_flight.lock().unwrap_or_else(|e| e.into_inner());
        while flight.contains(id) {
            flight = self.finished.wait(flight).unwrap_or_else(|e| e.into_inner());
        }
        if already() {
            return;
        }
        flight.insert(id.to_string());
        drop(flight);
        // The gate is given up by a guard rather than by the line after `build()`: a rebuild that
        // panics must not leave an id locked for the life of the process, with every later request
        // for it waiting on a thread that is gone.
        let _leaving = Leaving { gate: self, id: id.to_string() };
        build();
    }
}

struct Leaving<'a> {
    gate: &'a Rebuilds,
    id: String,
}

impl Drop for Leaving<'_> {
    fn drop(&mut self) {
        self.gate
            .in_flight
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
        self.gate.finished.notify_all();
    }
}

/// What a saved session is, as far as the record itself can say.
///
/// Not "in a bed": a bed is derived from the board, and asking the chain for one at boot is the
/// work being removed. What a record knows is whether anybody could still play it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A ward shift that is neither handed over nor anchored — somebody could still be at that
    /// bedside.
    WardLive,
    /// A ward shift that is finished: handed over, anchored, or both.
    WardFinished,
    /// A review run. No patient, no head, nothing on the chain.
    Review,
    /// A season run, or anything else this server kept.
    Other,
}

/// One line at boot saying what is in the store, counted from the records and nothing else.
pub fn census(kinds: &[Kind]) -> String {
    let n = |k: Kind| kinds.iter().filter(|x| **x == k).count();
    format!(
        "{} saved · {} mid-shift · {} finished · {} review · {} other",
        kinds.len(),
        n(Kind::WardLive),
        n(Kind::WardFinished),
        n(Kind::Review),
        n(Kind::Other),
    )
}

/// The answer for an id whose session is in the store and cannot be brought back.
///
/// **It must free the bed.** The head is held by a lease the chain keeps for about ten minutes and
/// by a heartbeat this server frees after 75 s of silence. A page told only "no such session" keeps
/// beating, so the bed stays taken for the whole lease by somebody who cannot play on it. Telling
/// the page to stop beating turns ten minutes into seventy-five seconds, and offering the exit lets
/// the person put the head back themselves rather than wait.
pub fn cannot_rebuild(why: &str) -> serde_json::Value {
    serde_json::json!({
        "error": why,
        "stop_beating": true,
        "leave": true,
    })
}
