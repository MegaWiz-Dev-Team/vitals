//! How much this bay is actually used — counted honestly, and shipped with what the count cannot
//! mean.
//!
//! The question asked was "how many people have played?". The honest answer is that this bay
//! cannot tell you and never will: *no signup* is on the front door as a feature, so there are no
//! accounts, and without accounts there is nothing that is a person. What can be counted is
//! **runs** and **devices**, and both are wrong in opposite directions:
//!
//!   * one person on a phone and a laptop is two devices — **over**counts
//!   * one Embla Box in a faculty, fifty students through it, is one device — **under**counts,
//!     and it undercounts hardest in exactly the place we care about most
//!
//! So the policy is to stop trying to count people and count runs precisely instead. A run is the
//! unit the deck already uses and the unit that can be defended.
//!
//! **Every number this module produces travels with its limits attached.** [`Usage::view`] cannot
//! emit the counts without the sentences, because they are built in the same call — a bare
//! integer from this endpoint would be quoted back as a headcount within a week, and "N students"
//! is a claim nobody here can substantiate.
//!
//! ## Why no consent banner
//!
//! This is a server counting its own work: an increment when a run opens, an increment when one
//! ends. No IP, no user-agent, no cross-site identifier, nothing that follows a reader anywhere.
//! That is a different act from Google Analytics, which is why GA sits behind a consent gate here
//! and this does not — and it is also why this count is *better*: GA loses everyone who declines,
//! and this loses nobody.
//!
//! ## Devices, and what a device fingerprint is allowed to be
//!
//! A device is a browser keypair, which is minted freely and cleared with the site data. To avoid
//! counting one browser twice across a restart the month's fingerprints are stored — but a stored
//! player key would be a durable identifier for a person's pseudonym, which is more than counting
//! needs. So what is stored is `sha256(month || pubkey)` truncated to 32 bits: enough to
//! de-duplicate within a month, not enough to be an identity, and salted by the month so the same
//! browser is a different fingerprint in September. Collisions only ever lose a device, never
//! invent one.

use crate::store::Store;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const KIND: &str = "usage";
const DOC: &str = "all";
/// Daily rows kept. Ninety days is what the spec asked for and what a term is.
const DAYS: usize = 90;
/// Months of device fingerprints kept. Two: the current one, and the one before it so a rollover
/// at midnight on the 1st does not read as everybody leaving.
const MONTHS: usize = 2;
/// Fingerprints stored per month. Past this the device count is a floor and says so, rather than
/// the record growing without a bound.
const MAX_DEVICES: usize = 20_000;

#[derive(Default, Serialize, Deserialize, Clone, Copy)]
struct Day {
    started: u64,
    finished: u64,
}

/// The tally as it sits in the store. One document: every counted event is one small write, and
/// there is nothing here two writers could interleave badly because one process owns it.
#[derive(Default, Serialize, Deserialize)]
struct Rec {
    /// The first day anything was counted. A total with no window is not a number.
    since: Option<String>,
    runs_started: u64,
    runs_finished: u64,
    /// Runs opened by a browser that minted no player key — a kiosk, or a browser that could
    /// not. They are runs and they are not devices, and the gap is published rather than implied.
    runs_without_a_key: u64,
    /// Started, by case id.
    by_case: BTreeMap<String, u64>,
    /// Finished, by the case's own outcome id — the vocabulary the scenario author wrote, not a
    /// bucket invented here.
    by_outcome: BTreeMap<String, u64>,
    /// Finished, split the one way the engine itself is willing to split it.
    died: u64,
    survived: u64,
    days: BTreeMap<String, Day>,
    /// `YYYY-MM` → the month's device fingerprints.
    devices: BTreeMap<String, BTreeSet<String>>,
}

pub struct Usage {
    rec: Rec,
}

impl Usage {
    /// Counts resumed from the store, so a deploy is not a reset.
    pub fn open(store: &Store) -> Usage {
        Usage { rec: store.get(KIND, DOC).unwrap_or_default() }
    }

    /// A run was opened. `player` is the browser's public key when it has one.
    pub fn started(&mut self, case: &str, player: Option<&str>, store: &Store) {
        let (day, month) = now();
        self.rec.since.get_or_insert_with(|| day.clone());
        self.rec.runs_started += 1;
        *self.rec.by_case.entry(case.to_string()).or_default() += 1;
        self.rec.days.entry(day).or_default().started += 1;
        match player {
            Some(k) => {
                // Salted with the same month the bucket is keyed on — one clock reading, so the
                // fingerprint and the month it is filed under cannot come from different days.
                let fp = fingerprint(&month, k);
                let set = self.rec.devices.entry(month).or_default();
                if set.len() < MAX_DEVICES {
                    set.insert(fp);
                }
            }
            None => self.rec.runs_without_a_key += 1,
        }
        self.prune();
        self.persist(store);
    }

    /// A run reached a terminal state. `outcome` is the case's own outcome id.
    pub fn finished(&mut self, outcome: &str, died: bool, store: &Store) {
        let (day, _) = now();
        self.rec.since.get_or_insert_with(|| day.clone());
        self.rec.runs_finished += 1;
        *self.rec.by_outcome.entry(outcome.to_string()).or_default() += 1;
        if died {
            self.rec.died += 1;
        } else {
            self.rec.survived += 1;
        }
        self.rec.days.entry(day).or_default().finished += 1;
        self.prune();
        self.persist(store);
    }

    fn prune(&mut self) {
        while self.rec.days.len() > DAYS {
            let Some(oldest) = self.rec.days.keys().next().cloned() else { break };
            self.rec.days.remove(&oldest);
        }
        while self.rec.devices.len() > MONTHS {
            let Some(oldest) = self.rec.devices.keys().next().cloned() else { break };
            self.rec.devices.remove(&oldest);
        }
    }

    fn persist(&self, store: &Store) {
        if let Err(e) = store.put(KIND, DOC, &self.rec) {
            eprintln!("could not save usage: {e}");
        }
    }

    /// What `/api/usage` serves.
    ///
    /// The counts and [`LIMITS`] are built together on purpose. There is no method on this type
    /// that returns the numbers alone, because the failure mode of a usage endpoint is not a
    /// wrong number — it is a right number quoted as something it is not.
    pub fn view(&self) -> serde_json::Value {
        let (today, month) = now();
        let devices = self.rec.devices.get(&month).map(|s| s.len()).unwrap_or(0);
        serde_json::json!({
            // First field on purpose: it is the sentence that has to survive being skimmed.
            "counts": "runs and devices — never people. This bay has no signup, so it cannot count people.",
            "since": self.rec.since,
            "today": today,
            "runs": {
                "started": self.rec.runs_started,
                "finished": self.rec.runs_finished,
                "survived": self.rec.survived,
                "died": self.rec.died,
                "started_without_a_device_key": self.rec.runs_without_a_key,
                "by_case": self.rec.by_case,
                "by_outcome": self.rec.by_outcome,
            },
            "devices": {
                "month": month,
                // Named for what it is at every level. There is no field on this endpoint whose
                // name could be mistaken for a headcount.
                "distinct_browsers_seen": devices,
                "capped_at": MAX_DEVICES,
                "is_a_floor": devices >= MAX_DEVICES,
            },
            "days": self.rec.days.iter().map(|(d, v)| serde_json::json!({
                "day": d, "started": v.started, "finished": v.finished,
            })).collect::<Vec<_>>(),
            "days_kept": DAYS,
            "limits": LIMITS,
        })
    }

    /// The startup line, next to the meter's.
    pub fn describe(&self) -> String {
        format!(
            "{} run(s) started · {} finished · counted as runs and devices, never people",
            self.rec.runs_started, self.rec.runs_finished
        )
    }
}

/// What these numbers cannot mean. Shipped with every reply.
///
/// Written out rather than summarised because the endpoint is public and the numbers are small:
/// a small number quoted without its limits is how "two anchored runs" becomes "two users" and
/// then, in a room, "we have no traction". The honest framing of a small number is that it is
/// small *and* that the instrument cannot see most of what happened.
pub const LIMITS: [&str; 7] = [
    "There is no signup here, by design. Nothing on this endpoint is a count of people.",
    "One machine used by many is one device: a shared box in a faculty with fifty students \
     through it counts once. This undercounts hardest where the use matters most.",
    "One person on a phone and a laptop is two devices. This overcounts.",
    "A device is a browser keypair. Clearing site data mints a new one; two profiles on one \
     machine are two.",
    "A run started without a player key is counted as a run and as no device — see \
     runs.started_without_a_device_key.",
    "Only runs anchored on chain can be checked by anyone but us; see /api/chain. Everything \
     else here is this server's own tally of its own work.",
    "These are this bay's numbers only. Figures from the production engine are a different \
     system and must never be added to them.",
];

/// A month-salted, truncated fingerprint. See the module note for why it is truncated.
fn fingerprint(month: &str, pubkey: &str) -> String {
    let d = Sha256::digest(format!("{month}\u{0}{pubkey}").as_bytes());
    d[..4].iter().map(|b| format!("{b:02x}")).collect()
}

/// `(YYYY-MM-DD, YYYY-MM)` right now.
fn now() -> (String, String) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    (day_key(secs), crate::meter::month_key(secs))
}

/// `YYYY-MM-DD` from a Unix timestamp — the same civil-from-days arithmetic `month_key` uses, so
/// the two cannot disagree about which day a month starts on.
pub fn day_key(epoch_secs: u64) -> String {
    let days = (epoch_secs / 86_400) as i64;
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(name: &str) -> Store {
        // Per-process: a fixed path let a second `cargo test` run against this checkout
        // delete this one's directory mid-write. See `tests/durability.rs`.
        let p = std::env::temp_dir().join(format!("vitals-usage-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        Store::with(crate::store::Backend::Disk { root: p }).unwrap()
    }

    #[test]
    fn day_key_is_the_civil_calendar() {
        assert_eq!(day_key(0), "1970-01-01");
        assert_eq!(day_key(1_756_684_800), "2025-09-01");
        assert_eq!(day_key(1_756_684_799), "2025-08-31");
        assert_eq!(day_key(1_767_225_599), "2025-12-31");
        assert_eq!(day_key(1_767_225_600), "2026-01-01");
        // A leap day, which is where naive date arithmetic goes wrong.
        assert_eq!(day_key(1_709_164_800), "2024-02-29");
    }

    #[test]
    fn a_run_counts_once_when_it_opens_and_once_when_it_ends() {
        let s = store("count");
        let mut u = Usage::open(&s);
        u.started("osce-a", Some("KEY1"), &s);
        u.started("osce-a", Some("KEY1"), &s);
        u.started("ep1", None, &s);
        u.finished("win_discharge", false, &s);
        u.finished("death_arrest", true, &s);

        let v = u.view();
        assert_eq!(v["runs"]["started"], 3);
        assert_eq!(v["runs"]["finished"], 2);
        assert_eq!(v["runs"]["survived"], 1);
        assert_eq!(v["runs"]["died"], 1);
        assert_eq!(v["runs"]["by_case"]["osce-a"], 2);
        assert_eq!(v["runs"]["by_outcome"]["death_arrest"], 1);
        // Two runs from one browser is one device, and the keyless run is neither.
        assert_eq!(v["devices"]["distinct_browsers_seen"], 1);
        assert_eq!(v["runs"]["started_without_a_device_key"], 1);
    }

    #[test]
    fn two_browsers_are_two_devices_and_one_browser_is_one() {
        let s = store("devices");
        let mut u = Usage::open(&s);
        for _ in 0..5 {
            u.started("ep1", Some("PHONE"), &s);
        }
        assert_eq!(u.view()["devices"]["distinct_browsers_seen"], 1);
        u.started("ep1", Some("LAPTOP"), &s);
        assert_eq!(u.view()["devices"]["distinct_browsers_seen"], 2);
    }

    #[test]
    fn the_count_survives_a_restart_through_the_store() {
        let s = store("persist");
        let mut u = Usage::open(&s);
        u.started("ep1", Some("KEY1"), &s);
        u.finished("win_discharge", false, &s);
        let again = Usage::open(&s);
        let v = again.view();
        assert_eq!(v["runs"]["started"], 1);
        assert_eq!(v["runs"]["finished"], 1);
        assert_eq!(v["devices"]["distinct_browsers_seen"], 1, "a restart re-counted a known browser");
    }

    /// The one property this file exists for.
    #[test]
    fn the_numbers_cannot_be_served_without_what_they_cannot_mean() {
        let s = store("limits");
        let mut u = Usage::open(&s);
        u.started("ep1", Some("KEY1"), &s);
        let v = u.view();
        let limits = v["limits"].as_array().expect("no limits shipped with the numbers");
        assert_eq!(limits.len(), LIMITS.len());
        let all = limits.iter().filter_map(|l| l.as_str()).collect::<Vec<_>>().join(" ");
        for must in ["no signup", "One machine used by many", "phone and a laptop", "anchored on chain"] {
            assert!(all.contains(must), "the limits stopped saying {must:?}");
        }
        assert!(
            v["counts"].as_str().unwrap_or_default().contains("never people"),
            "the headline caveat is gone"
        );
    }

    /// No field on this endpoint may be readable as a headcount — not a key, not a value.
    #[test]
    fn nothing_here_is_called_a_user_a_player_or_a_student() {
        let s = store("naming");
        let mut u = Usage::open(&s);
        u.started("ep1", Some("KEY1"), &s);
        u.finished("win_discharge", false, &s);
        let text = serde_json::to_string(&u.view()).unwrap();
        // Checked over the whole payload rather than over the keys, because a value is quoted
        // just as often as a key is.
        for banned in ["\"users\"", "\"players\"", "\"students\"", "user_count", "\"people\":"] {
            assert!(!text.contains(banned), "{banned} appears in the usage payload: {text}");
        }
        // The limits are allowed to say "people" — in the sentence that says there are none to
        // count — and that sentence is the reason the check above is on field-shaped strings.
        assert!(text.contains("never people"));
    }

    #[test]
    fn the_daily_rows_are_bounded() {
        let s = store("days");
        let mut u = Usage::open(&s);
        for i in 0..(DAYS + 40) {
            u.rec.days.insert(format!("2020-01-{:02}+{i}", i % 28), Day::default());
        }
        u.prune();
        assert!(u.rec.days.len() <= DAYS, "the daily history grows without a bound");
    }

    #[test]
    fn only_the_current_month_and_the_one_before_it_keep_fingerprints() {
        let s = store("months");
        let mut u = Usage::open(&s);
        for m in ["2026-05", "2026-06", "2026-07", "2026-08"] {
            u.rec.devices.insert(m.to_string(), BTreeSet::from(["aa".to_string()]));
        }
        u.prune();
        assert_eq!(u.rec.devices.len(), MONTHS);
        assert!(u.rec.devices.contains_key("2026-08"), "the newest month was pruned");
        assert!(!u.rec.devices.contains_key("2026-05"), "a stale month was kept");
    }

    /// A fingerprint may not be the key, and it may not survive the month.
    #[test]
    fn a_fingerprint_is_short_salted_and_not_the_key_itself() {
        let a = fingerprint("2026-08", "SOMEPLAYERPUBKEY");
        let b = fingerprint("2026-09", "SOMEPLAYERPUBKEY");
        assert_eq!(a.len(), 8, "the fingerprint is not truncated");
        assert_ne!(a, b, "the same browser is linkable across months");
        assert!(!a.contains("SOMEPLAYER"), "the key is stored in the clear");
        assert_eq!(a, fingerprint("2026-08", "SOMEPLAYERPUBKEY"), "the fingerprint is not stable");
    }
}
