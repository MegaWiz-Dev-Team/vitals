//! What a free public bay may spend, and what happens when it has spent it.
//!
//! Free to everyone plus paid inference means a stranger can spend this project's money. The
//! answer is a per-address rate limit and a monthly ceiling — and the ceiling is visible: when
//! it is reached the page says what this month's compute funded and shows the donation counter,
//! rather than a bare 429. The constraint is part of the product, the same way the treasury is
//! public on chain (decision recorded 2026-08-24).
//!
//! The windows are keyed on the caller's network address, not on the player key or the session
//! id — both of those are minted freely by any browser, so a limit keyed on them is a limit on
//! honesty only. The address is the one thing a stranger cannot rotate for free. The monthly
//! ceiling is the backstop that actually caps spend, whatever the addresses do.
//!
//! The month's count survives a restart the same way a session does: through the [`Store`],
//! which is Firestore on Cloud Run. A ceiling that resets on every deploy is not a ceiling.

use crate::store::Store;
use std::collections::HashMap;
use std::time::{Duration, Instant};

const KIND: &str = "meter";
const MINUTE: Duration = Duration::from_secs(60);
const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// The month's spend as it sits in the store, keyed `m<YYYY-MM>`.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct MonthRec {
    /// Patient turns answered this month — the unit of spend a visitor can see and check.
    used: u64,
    /// Times someone followed the donation link this month. Clicks, not money — the money is
    /// counted where it lands, but the click is the conversion this page can measure honestly.
    clicks: u64,
}

/// The answer to "may this address ask the patient something right now?".
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Ok,
    /// This address is over its own window; everyone else is unaffected.
    SlowDown { retry_secs: u64 },
    /// The whole bay has spent its month. This is the visible ceiling, not an error.
    Ceiling,
}

pub struct Meter {
    /// Questions one address may ask per minute / per day. The patient cannot answer faster
    /// than the minute rate anyway; the day rate is what stops a quiet overnight drain.
    per_min: usize,
    per_day: usize,
    /// Patient turns the whole bay may spend per calendar month. 0 disables the ceiling —
    /// explicitly, because the default is capped: a deploy that forgets the env var must be
    /// bounded, not unlimited.
    cap: u64,
    donate: Option<String>,
    windows: HashMap<String, Vec<Instant>>,
    month: String,
    used: u64,
    clicks: u64,
}

impl Meter {
    /// Configured from the environment, counts resumed from the store.
    pub fn open(store: &Store) -> Meter {
        let num = |k: &str, d: u64| {
            std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d)
        };
        Meter::with(
            num("VITALS_TURNS_PER_MIN", 6) as usize,
            num("VITALS_TURNS_PER_DAY", 200) as usize,
            num("VITALS_MONTHLY_TURNS", 20_000),
            std::env::var("VITALS_DONATE_URL").ok().filter(|s| !s.is_empty()),
            store,
        )
    }

    pub fn with(per_min: usize, per_day: usize, cap: u64, donate: Option<String>, store: &Store) -> Meter {
        let month = current_month();
        let rec: MonthRec = store.get(KIND, &doc_key(&month)).unwrap_or_default();
        Meter { per_min, per_day, cap, donate, windows: HashMap::new(), month, used: rec.used, clicks: rec.clicks }
    }

    /// A new month is a new budget. Checked wherever a count is read or written, so a server
    /// that runs across midnight on the 31st rolls over without a restart.
    fn roll(&mut self, store: &Store) {
        let now = current_month();
        if now != self.month {
            self.month = now;
            let rec: MonthRec = store.get(KIND, &doc_key(&self.month)).unwrap_or_default();
            self.used = rec.used;
            self.clicks = rec.clicks;
            self.windows.clear();
        }
    }

    pub fn allow(&mut self, addr: &str, store: &Store) -> Verdict {
        self.roll(store);
        if self.cap > 0 && self.used >= self.cap {
            return Verdict::Ceiling;
        }
        // Bounded memory before a new entry, not after: the map only grows one address at a
        // time, so pruning at a threshold keeps it at the threshold.
        if self.windows.len() >= 4096 && !self.windows.contains_key(addr) {
            self.windows.retain(|_, w| {
                w.retain(|t| t.elapsed() < DAY);
                !w.is_empty()
            });
        }
        let w = self.windows.entry(addr.to_string()).or_default();
        w.retain(|t| t.elapsed() < DAY);
        let in_minute: Vec<&Instant> = w.iter().filter(|t| t.elapsed() < MINUTE).collect();
        if in_minute.len() >= self.per_min {
            // When the oldest question inside the window ages out is when the next one fits.
            let oldest = in_minute.iter().map(|t| t.elapsed()).max().unwrap_or_default();
            return Verdict::SlowDown { retry_secs: (MINUTE.saturating_sub(oldest)).as_secs().max(1) };
        }
        if w.len() >= self.per_day {
            let oldest = w.first().map(|t| t.elapsed()).unwrap_or_default();
            return Verdict::SlowDown { retry_secs: (DAY.saturating_sub(oldest)).as_secs().max(1) };
        }
        w.push(Instant::now());
        Verdict::Ok
    }

    /// One answered turn. Counted only when the model actually replied — a failed call cost
    /// nothing worth metering, and billing the visitor's allowance for our error would be wrong
    /// in the annoying direction.
    pub fn spend(&mut self, store: &Store) {
        self.roll(store);
        self.used += 1;
        self.persist(store);
    }

    /// Someone followed the donation link.
    pub fn click(&mut self, store: &Store) {
        self.roll(store);
        self.clicks += 1;
        self.persist(store);
    }

    fn persist(&self, store: &Store) {
        let rec = MonthRec { used: self.used, clicks: self.clicks };
        if let Err(e) = store.put(KIND, &doc_key(&self.month), &rec) {
            eprintln!("could not save meter {}: {e}", self.month);
        }
    }

    pub fn donate_url(&self) -> Option<&str> {
        self.donate.as_deref()
    }

    /// What `/api/meter` serves, and what rides inside a ceiling reply: enough for the page to
    /// say what the month funded and show the counter, without a second request.
    pub fn view(&self) -> serde_json::Value {
        serde_json::json!({
            "month": self.month,
            "used": self.used,
            "cap": if self.cap > 0 { Some(self.cap) } else { None },
            "remaining": if self.cap > 0 { Some(self.cap.saturating_sub(self.used)) } else { None },
            "clicks": self.clicks,
            "donate": self.donate,
        })
    }

    /// The startup line, next to the others.
    pub fn describe(&self) -> String {
        let ceiling = match self.cap {
            0 => "no monthly ceiling — explicitly disabled".to_string(),
            c => format!("{}/{} turns this month ({})", self.used, c, self.month),
        };
        format!(
            "{ceiling} · {}/min · {}/day per address · donations {}",
            self.per_min,
            self.per_day,
            if self.donate.is_some() { "linked" } else { "not linked" },
        )
    }
}

fn doc_key(month: &str) -> String {
    format!("m{month}")
}

fn current_month() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    month_key(secs)
}

/// `YYYY-MM` from a Unix timestamp — the civil-from-days arithmetic, no calendar dependency.
pub fn month_key(epoch_secs: u64) -> String {
    let days = (epoch_secs / 86_400) as i64;
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(name: &str) -> Store {
        let p = std::env::temp_dir().join(format!("vitals-meter-{name}"));
        let _ = std::fs::remove_dir_all(&p);
        Store::with(crate::store::Backend::Disk { root: p }).unwrap()
    }

    #[test]
    fn month_key_is_the_civil_calendar() {
        assert_eq!(month_key(0), "1970-01");
        assert_eq!(month_key(1_756_684_800), "2025-09"); // 2025-09-01T00:00:00Z
        assert_eq!(month_key(1_756_684_799), "2025-08"); // one second earlier
        assert_eq!(month_key(1_767_225_599), "2025-12"); // 2025-12-31T23:59:59Z
        assert_eq!(month_key(1_767_225_600), "2026-01"); // the next second
    }

    #[test]
    fn the_minute_window_holds_one_address_and_only_that_address() {
        let s = store("min");
        let mut m = Meter::with(2, 100, 0, None, &s);
        assert_eq!(m.allow("a", &s), Verdict::Ok);
        assert_eq!(m.allow("a", &s), Verdict::Ok);
        assert!(matches!(m.allow("a", &s), Verdict::SlowDown { .. }));
        // A different address is not paying for a's enthusiasm.
        assert_eq!(m.allow("b", &s), Verdict::Ok);
    }

    #[test]
    fn slow_down_says_when_to_come_back() {
        let s = store("retry");
        let mut m = Meter::with(1, 100, 0, None, &s);
        assert_eq!(m.allow("a", &s), Verdict::Ok);
        match m.allow("a", &s) {
            Verdict::SlowDown { retry_secs } => assert!((1..=60).contains(&retry_secs)),
            v => panic!("expected SlowDown, got {v:?}"),
        }
    }

    #[test]
    fn the_ceiling_stops_everyone_and_zero_disables_it() {
        let s = store("cap");
        let mut m = Meter::with(100, 100, 2, None, &s);
        assert_eq!(m.allow("a", &s), Verdict::Ok);
        m.spend(&s);
        assert_eq!(m.allow("b", &s), Verdict::Ok);
        m.spend(&s);
        // Two turns spent, cap 2 — every address is refused, including a fresh one.
        assert_eq!(m.allow("c", &s), Verdict::Ceiling);

        let mut open = Meter::with(100, 100, 0, None, &s);
        open.used = 1_000_000;
        assert_eq!(open.allow("c", &s), Verdict::Ok);
    }

    #[test]
    fn the_count_survives_a_restart_through_the_store() {
        let s = store("persist");
        let mut m = Meter::with(100, 100, 10, None, &s);
        m.spend(&s);
        m.spend(&s);
        m.click(&s);
        let again = Meter::with(100, 100, 10, None, &s);
        assert_eq!(again.used, 2);
        assert_eq!(again.clicks, 1);
    }

    #[test]
    fn the_view_carries_what_the_ceiling_page_needs() {
        let s = store("view");
        let mut m = Meter::with(100, 100, 5, Some("https://give.example".into()), &s);
        m.spend(&s);
        let v = m.view();
        assert_eq!(v["used"], 1);
        assert_eq!(v["cap"], 5);
        assert_eq!(v["remaining"], 4);
        assert_eq!(v["donate"], "https://give.example");
        assert_eq!(v["month"].as_str().unwrap().len(), 7);
    }

    #[test]
    fn an_uncapped_meter_reports_no_cap_rather_than_zero() {
        let s = store("nocap");
        let m = Meter::with(100, 100, 0, None, &s);
        assert!(m.view()["cap"].is_null());
        assert!(m.view()["remaining"].is_null());
    }
}
