//! One tick of the factory: read the ward, top the queue up, complete one patient's faces.
//!
//! The order is the order of what can go wrong. The ward is read first, and a ward that cannot be
//! read ends the tick with nothing built — a factory that pushed against a board it had not seen
//! would be guessing who is in the beds. The ledger is reconciled with the board second, so every
//! later decision about who is busy is about *this* board. The unseen packs are resent third,
//! one by one, which is both the recovery from a queue the door lost and the probe that tells us
//! the real depth. Only then is anything new built, and only as much as the queue is short.
//!
//! **Every write lands as soon as it is true.** A face is recorded in the manifest the moment it
//! is uploaded and a pack in the ledger the moment the door says queued, so a crash between two
//! steps costs a minute of work and never a second copy of anything: the door is content-addressed
//! and the ledger is keyed by the door's own id.
//!
//! **Dry run** reads and plans and stops before the token: no secret is fetched, no request is
//! sent, no file is written. What it prints is what the real run would do from the same state.

use crate::door::{Door, FillReply, Outbound, Pushed, Token, WardCase, WardView};
use crate::ledger::{Ledger, Sent};
use crate::manifest::Manifest;
use crate::need::{fmt as fmt_weight, weights, Weights};
use crate::plan::{bed_cap, plan, Base, Inputs, MIN_COUNTRIES, MIN_REGIONS, NEAR_FACE, QUEUE_CAP, QUEUE_WINDOW};
use crate::region::{Region, ALL};
use crate::pool::{person_for, read_pool, Person};
use crate::prompts;
use crate::tools::{sha256_hex, Tools};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use vitals_web::ward::Pack;
use vitals_web::ward_chain::{pack_id, PORTRAITS};

/// webp quality, as the batch used.
pub const WEBP_QUALITY: u8 = 86;

/// The 256 px sibling every portrait gets: `<sha>-256.webp`, the same sha as the full one so the
/// pair is addressable, at this quality (~7–12 KB). The board can draw a wall of faces from these.
pub const VARIANT_PX: u32 = 256;
pub const VARIANT_QUALITY: u8 = 80;

/// List price of one image edit (gemini-2.5-flash-image, image out) and of one judge call
/// (gemini-2.5-flash, a short text answer with two small images in) — for the estimate in the
/// tick line and the ledger. Estimated from list price, never measured.
pub const EDIT_USD: f64 = 0.039;
pub const JUDGE_USD: f64 = 0.0005;

/// The estimate, in USD.
pub fn estimate_usd(edits: usize, judge_calls: usize) -> f64 {
    edits as f64 * EDIT_USD + judge_calls as f64 * JUDGE_USD
}

/// How many faces the painter may try for one person before the factory gives up on her this
/// tick. Each try is a new seed and each is judged; a pack is never built on a rejected face.
pub const FACE_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone)]
pub struct Config {
    /// The ward's origin, e.g. `https://vitals-world-….run.app`.
    pub ward: String,
    /// Keep the queue at least this deep.
    pub queue_depth: usize,
    /// Faces painted with mflux per tick, so a tick stays under the interval: a person is started
    /// only while fewer than this have been painted, and then gets her [`FACE_ATTEMPTS`] tries.
    /// Packs past this wait for the next tick.
    pub bases_per_tick: usize,
    /// The checkout: the scenarios, the persona files, the pool and the endemic list.
    pub repo: PathBuf,
    /// `~/.vitals/world`: the manifest, the ledger, the faces.
    pub world_dir: PathBuf,
    /// The ward's own GCP project: where its `vitals-door-token` secret lives. Staging is
    /// `vitals-academy-dev`, production `vitals-academy`, and a token read from the wrong one is
    /// a door that says `unauthorised` — which is how this became two fields.
    pub secret_project: String,
    /// The project the image editor runs in (`vitals-academy`; the dev project has no Vertex).
    pub vertex_project: String,
    pub bucket: String,
    /// The image editor (states from a base).
    pub model: String,
    /// The text model that judges whether a face is a photograph of a person.
    pub judge_model: String,
    /// Image edits allowed per UTC day, counted from the ledger across ticks. Bases are painted
    /// locally and are not counted; when the budget is spent, states wait for tomorrow.
    pub edits_per_day: usize,
    pub dry_run: bool,
    pub seed: u64,
    /// Unix seconds, for the ledger.
    pub now: u64,
}

impl Config {
    pub fn manifest_path(&self) -> PathBuf {
        self.world_dir.join("portraits.json")
    }
    pub fn ledger_path(&self) -> PathBuf {
        self.world_dir.join("factory-ledger.json")
    }
    /// Where a face's bytes are kept locally, under the same name as in the bucket.
    pub fn face_path(&self, sha: &str) -> PathBuf {
        self.world_dir.join("portraits").join(format!("{sha}.webp"))
    }
}

/// What a tick did, in words and in numbers.
#[derive(Debug, Default)]
pub struct Report {
    pub lines: Vec<String>,
    /// Anything a person should look at. A rejected pack is one; a closed door is not.
    pub errors: Vec<String>,
    pub queued: usize,
    pub duplicates: usize,
    /// States the judge refused twice and left out.
    pub rejected: usize,
    /// Packs the door refused (an ERROR line each).
    pub door_rejected: usize,
    /// The queue's depth as the door last reported it.
    pub depth: Option<usize>,
    pub faces_made: usize,
    /// Faces painted, passed or refused — what the per-tick cap counts.
    pub faces_tried: usize,
    pub states_made: usize,
    /// Image edits made this tick.
    pub edits: usize,
    /// Edits already made today by earlier ticks, from the ledger, so the budget spans ticks.
    pub edits_before: usize,
    /// Judge calls made this tick (every question, every gate).
    pub judge_calls: usize,
    /// States left for tomorrow because the edit budget was spent.
    pub deferred_budget: usize,
}

impl Report {
    /// Edits left in today's budget.
    fn edits_left(&self, cfg: &Config) -> usize {
        cfg.edits_per_day.saturating_sub(self.edits_before + self.edits)
    }
}

impl Report {
    fn say(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
    }
    fn fail(&mut self, line: impl Into<String>) {
        let line = line.into();
        self.lines.push(format!("ERROR {line}"));
        self.errors.push(line);
    }
}

/// The url a face has once it is in the bucket.
fn face_url(sha: &str) -> String {
    format!("{PORTRAITS}/{sha}.webp")
}

/// Write a face locally and put it in the bucket, with its 256 px sibling; the url is
/// content-addressed either way and the sibling's is the same sha with `-256`.
fn publish(cfg: &Config, tools: &dyn Tools, webp: &[u8]) -> Result<String, String> {
    let sha = sha256_hex(webp);
    let path = cfg.face_path(&sha);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    if !path.exists() {
        std::fs::write(&path, webp).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    tools.upload(&path, &format!("{sha}.webp"))?;
    publish_sibling(cfg, tools, &sha, webp)?;
    Ok(face_url(&sha))
}

/// The 256 px sibling of a full portrait, by the full one's sha.
fn publish_sibling(cfg: &Config, tools: &dyn Tools, sha: &str, full: &[u8]) -> Result<String, String> {
    let small = tools.webp_resized(full, VARIANT_QUALITY, VARIANT_PX)?;
    let path = cfg.face_path(&format!("{sha}-256"));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    if !path.exists() {
        std::fs::write(&path, &small).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    tools.upload(&path, &format!("{sha}-256.webp"))?;
    Ok(sibling_url(&face_url(sha)))
}

/// A region's name for the log, or the word for a code the table does not place.
fn region_name(r: Option<Region>) -> &'static str {
    r.map_or("region unplaced", Region::name)
}

/// `student` and the like, for a line of the log.
fn level_words(level: &str) -> String {
    level.to_string()
}

/// `…/<sha>.webp` → `…/<sha>-256.webp`.
pub fn sibling_url(url: &str) -> String {
    match url.strip_suffix(".webp") {
        Some(stem) => format!("{stem}-256.webp"),
        None => url.to_string(),
    }
}

/// The sha a portrait url names.
fn sha_of(url: &str) -> Option<&str> {
    url.rsplit('/').next().and_then(|n| n.strip_suffix(".webp"))
}

pub fn tick(cfg: &Config, door: &dyn Door, tools: &dyn Tools) -> Report {
    let mut r = Report::default();
    if cfg.dry_run {
        r.say("dry run: nothing is fetched, sent or written");
    }

    // ── what the factory holds ──
    let pool = match std::fs::read_to_string(cfg.repo.join("crates/vitals-web/data/personas.json")).map_err(|e| e.to_string()).and_then(|s| read_pool(&s)) {
        Ok(p) => p,
        Err(e) => {
            r.fail(format!("the pool could not be read: {e}"));
            return r;
        }
    };
    let need: Weights = match std::fs::read_to_string(cfg.repo.join("crates/vitals-web/data/physicians.json")).map_err(|e| e.to_string()).and_then(|s| weights(&s, &pool)) {
        Ok(w) => w,
        Err(e) => {
            r.fail(format!("the physicians series could not be read, so nothing is weighted and nothing is built: {e}"));
            return r;
        }
    };
    let mut manifest = match Manifest::load(&cfg.manifest_path()) {
        Ok(m) => m,
        Err(e) => {
            r.fail(e);
            return r;
        }
    };
    let mut ledger = match Ledger::load(&cfg.ledger_path()) {
        Ok(l) => l,
        Err(e) => {
            r.fail(e);
            return r;
        }
    };
    r.edits_before = ledger.spend.get(&crate::ledger::utc_day(cfg.now)).map_or(0, |s| s.edits);
    if let Some(other) = ledger.sent.values().map(|s| &s.ward).find(|w| *w != &cfg.ward) {
        r.fail(format!(
            "the ledger at {} belongs to {other}, and this run targets {} — one world directory per ward, \
             or the same face ends up on two boards",
            cfg.ledger_path().display(),
            cfg.ward
        ));
        return r;
    }
    r.say(format!("holding {} people, {} faces on file, {} packs in the ledger", pool.len(), manifest.entries.len(), ledger.sent.len()));

    // ── the ward ──
    let ward = match door.read_ward() {
        Ok(w) => w,
        Err(e) => {
            r.fail(format!("the ward could not be read, so nothing was built: {e}"));
            return r;
        }
    };
    if !ward.readable {
        r.say(format!("the ward is not readable right now ({}), so nothing was built", ward.why.as_deref().unwrap_or("no reason given")));
        return r;
    }
    let open = ward.open().count();
    match &ward.queue {
        Some(q) => r.say(format!("ward {}: {open} in beds of {}, {} waiting, door {}", ward.source, q.beds, q.waiting, q.door)),
        None => r.say(format!("ward {}: {open} in beds of {}; this build publishes no queue block, the door's answer will say the depth", ward.source, ward.beds)),
    }
    for note in ledger.reconcile(&ward) {
        r.say(note);
    }

    // ── the case door: the ward's list is the catalogue ──
    let cases: Vec<WardCase> = match door.read_cases() {
        Ok(c) => c,
        Err(e) => {
            r.fail(format!("the case door could not be read ({e}); a pack names a case from the ward's own list and nothing else, so nothing is built"));
            return r;
        }
    };
    if cases.is_empty() {
        r.fail("the ward lists no cases (no case door on this build, or an empty one); a pack names a case from the ward's own list and nothing else, so nothing is built");
        return r;
    }
    r.say(format!(
        "case door: {} cases listed — {} with a stated patient, {} written for a country ({} endemic), {} provisional; levels {}",
        cases.len(),
        cases.iter().filter(|w| w.patient.is_some()).count(),
        cases.iter().filter(|w| w.country.is_some()).count(),
        cases.iter().filter(|w| w.endemic).count(),
        cases.iter().filter(|w| w.provisional).count(),
        crate::plan::LEVELS.iter().map(|l| format!("{l} {}", cases.iter().filter(|w| w.difficulty == *l).count())).collect::<Vec<_>>().join(" · ")
    ));
    if cases.iter().all(|w| w.patient.is_none()) {
        r.say("no case states its patient, so every case fits any adult of either sex this tick — a man may be drawn for a case written about a woman; the ward's 92b4181 adds `patient` to each row and the fit rule reads it");
    }
    if ward.queue.as_ref().is_some_and(|q| q.door != "open") {
        r.say("the door is closed — the ward opens when the founder says so; nothing to do until then");
        if !cfg.dry_run {
            if let Err(e) = ledger.save(&cfg.ledger_path()) {
                r.fail(e);
            }
        }
        return r;
    }

    // Waiting packs that name a case the ward no longer lists — a season id from before 0543ed7 —
    // are re-sent as they were: the door refuses them in its own words and the ledger drops them,
    // which frees the face.
    let stale: Vec<String> = ledger.unseen().into_iter().filter(|(_, s)| !cases.iter().any(|w| w.case_id == s.case)).map(|(_, s)| format!("{} ({})", s.name, s.case)).collect();
    if !stale.is_empty() {
        r.say(format!("{} waiting pack(s) name a case the ward does not list and will be refused at the re-send and dropped: {}", stale.len(), stale.join(", ")));
    }

    // ── the plan, before anything is touched ──
    let resend: Vec<(String, Outbound)> = ledger.unseen().into_iter().map(|(id, s)| (id.clone(), s.to_outbound())).collect();
    let known_depth = ward.queue.as_ref().map(|q| q.waiting).unwrap_or(resend.len());
    let want_guess = cfg.queue_depth.saturating_sub(known_depth.max(resend.len()));
    r.say(need.table());
    r.say(format!(
        "spread: no country in more than {} of {} beds at once; among the last {QUEUE_WINDOW} packs no country more than {QUEUE_CAP} times, at least {MIN_COUNTRIES} countries and {MIN_REGIONS} of {} regions; every region within {} draws",
        bed_cap(ward.beds),
        ward.beds,
        ALL.len(),
        crate::plan::WORLD_WINDOW
    ));
    let inputs = Inputs { cases: &cases, pool: &pool, manifest: &manifest, ward: &ward, ledger: &ledger, weights: &need, beds: ward.beds, want: want_guess, seed: cfg.seed };
    let planned = plan(&inputs);
    for n in &planned.notes {
        r.say(n.clone());
    }

    if cfg.dry_run {
        r.say(format!("would resend {} unseen pack(s) first, one by one, and read the depth off the last answer", resend.len()));
        r.say(format!("would then build {} pack(s) to bring the queue to {} (assuming depth {known_depth}):", planned.packs.len(), cfg.queue_depth));
        let mut to_make = 0;
        for pl in &planned.packs {
            let face = match &pl.base {
                Base::Have { age, .. } => format!("face on file (made at {age})"),
                Base::Make { key, age } => {
                    to_make += 1;
                    if to_make <= cfg.bases_per_tick { format!("would make a face for {key} at {age} with mflux") } else { format!("face for {key} at {age} deferred (cap {} per tick)", cfg.bases_per_tick) }
                }
            };
            r.say(format!(
                "  {} ({}) — {} {} {} from {} · {} (drawn: {}, weight {} people/doctor){} · {} · {}",
                pl.pack.case, level_words(&pl.level), pl.pack.persona.name, pl.sex.letter().to_uppercase(), pl.pack.persona.age, pl.pack.persona.country,
                region_name(pl.region), pl.pack.persona.country, fmt_weight(pl.weight),
                if pl.pack.endemic { " (endemic)" } else { "" }, pl.case_why, face
            ));
        }
        // The next twenty draws whatever the shortfall, for the founder: the queue as it is about
        // to look, country by country and region by region.
        let next = plan(&Inputs { want: QUEUE_WINDOW, ..inputs });
        r.say(format!("next {QUEUE_WINDOW} draws from this state, by need under the spread rules (country · region · person · case):"));
        let mut countries: BTreeSet<&str> = BTreeSet::new();
        let mut regions: BTreeSet<Region> = BTreeSet::new();
        for (n, pl) in next.packs.iter().enumerate() {
            countries.insert(&pl.pack.persona.country);
            regions.extend(pl.region);
            r.say(format!("  {}. {} · {} · {} · case_id {} ({})", n + 1, pl.pack.persona.country, region_name(pl.region), pl.pack.persona.name, pl.pack.case, level_words(&pl.level)));
        }
        r.say(format!("spread: {} countries, {} regions of {} in these {} draws", countries.len(), regions.len(), ALL.len(), next.packs.len()));
        for n in next.notes.iter().filter(|n| n.starts_with("redrawn")) {
            r.say(format!("over the {}: {n}", QUEUE_WINDOW));
        }
        dry_run_faces(cfg, &mut r, &ward, &pool, &manifest);
        return r;
    }

    // ── the token, once ──
    let token = match tools.secret_token(&cfg.secret_project) {
        Ok(t) => t,
        Err(e) => {
            r.fail(e);
            return r;
        }
    };

    // ── a face remade since her pack was sent reaches the pack while it waits ──
    replace_remade_faces(cfg, tools, door, &token, &pool, &mut manifest, &mut ledger, &mut r);
    carry_siblings_to_waiting_packs(door, &token, &manifest, &mut ledger, &mut r);
    save_ledger(cfg, &ledger, &mut r);

    // ── resend what the board has not shown yet: recovery and probe in one ──
    let mut depth: Option<usize> = None;
    let mut lost = 0;
    for (id, out) in &resend {
        match door.push(&token, std::slice::from_ref(out)) {
            Ok(Pushed::Queued(q)) => {
                depth = Some(q.depth);
                r.queued += q.queued;
                r.duplicates += q.duplicates;
                lost += q.queued;
                if let Some(why) = q.rejected.first() {
                    r.door_rejected += 1;
                    r.fail(format!("the door now refuses {} ({}, sent earlier): {why} — dropped from the ledger", out.pack.persona.name, out.pack.case));
                    ledger.sent.remove(id);
                }
            }
            Ok(Pushed::Closed { why }) => {
                r.say(format!("door closed: {why}"));
                save_ledger(cfg, &ledger, &mut r);
                return r;
            }
            Ok(Pushed::Refused { error }) => {
                r.fail(format!("the door refused the page: {error}"));
                save_ledger(cfg, &ledger, &mut r);
                return r;
            }
            Err(e) => {
                r.fail(format!("push failed: {e}"));
                save_ledger(cfg, &ledger, &mut r);
                return r;
            }
        }
    }
    if !resend.is_empty() {
        r.say(format!("resent {} unseen pack(s): {} still queued, {} put back, depth {}", resend.len(), r.duplicates, lost, depth.map_or("?".to_string(), |d| d.to_string())));
    }
    // With nothing to resend the depth is the ward's word, or unknown; an empty page asks the door.
    if depth.is_none() {
        match door.push(&token, &[]) {
            Ok(Pushed::Queued(q)) => depth = Some(q.depth),
            Ok(Pushed::Closed { why }) => {
                r.say(format!("door closed: {why}"));
                save_ledger(cfg, &ledger, &mut r);
                return r;
            }
            Ok(Pushed::Refused { error }) => {
                r.fail(format!("the door refused an empty page: {error}"));
                save_ledger(cfg, &ledger, &mut r);
                return r;
            }
            Err(e) => {
                r.fail(format!("push failed: {e}"));
                save_ledger(cfg, &ledger, &mut r);
                return r;
            }
        }
    }
    let depth_now = depth.unwrap_or(0);
    r.depth = Some(depth_now);

    // ── build what the queue is short ──
    let want = cfg.queue_depth.saturating_sub(depth_now);
    let planned = if want == want_guess {
        planned
    } else {
        let again = plan(&Inputs { cases: &cases, pool: &pool, manifest: &manifest, ward: &ward, ledger: &ledger, weights: &need, beds: ward.beds, want, seed: cfg.seed });
        for n in &again.notes {
            r.say(n.clone());
        }
        again
    };
    r.say(format!("queue depth {depth_now}, want {}: building {}", cfg.queue_depth, planned.packs.len()));
    let mut deferred = 0;
    let mut door_takes_256 = true;
    for pl in planned.packs {
        let mut pack = pl.pack;
        let who = pool.iter().find(|p| p.key == pl.person).expect("a planned person is in the pool");
        // The entry her face is filed under: on file, or painted now.
        let slot = match &pl.base {
            Base::Have { key, .. } => key.clone(),
            Base::Make { key, age } => {
                if r.faces_tried >= cfg.bases_per_tick {
                    deferred += 1;
                    continue;
                }
                match make_face(cfg, tools, who, *age, pl.place.as_str(), &mut r) {
                    Ok(url) => {
                        manifest.record_base(key, *age, &url, who);
                        if let Err(e) = manifest.save(&cfg.manifest_path()) {
                            r.fail(e);
                            return r;
                        }
                        r.faces_made += 1;
                        r.say(format!("made a face for {key} at {age}"));
                        manifest.entry_with_stable(&url).map(|(k, _)| k.clone()).unwrap_or_else(|| format!("{key}@{age}"))
                    }
                    Err(e) => {
                        r.fail(format!("{e} — no pack for {} this tick", who.name));
                        continue;
                    }
                }
            }
        };
        // Her stable: made from the base, judged, never the base itself. A pack whose stable was
        // refused twice goes out with no picture rather than with the wrong one.
        pack.portrait.clear();
        match ensure_stable(cfg, tools, &mut manifest, &slot, who, pack.persona.age, &mut r) {
            Ok(Some(stable)) => {
                pack.portrait.insert("stable_256".into(), sibling_url(&stable));
                pack.portrait.insert("stable".into(), stable);
            }
            Ok(None) => r.say(format!("{} goes out without a picture: {} stable was refused twice, or waits for tomorrow's edit budget", who.name, who.sex.possessive())),
            Err(e) => {
                r.fail(format!("{}'s stable could not be made: {e} — no pack for {} this tick", who.name, who.sex.object()));
                continue;
            }
        }
        let id = pack_id(&pack);
        if !door_takes_256 {
            pack.portrait.retain(|k, _| !k.ends_with("_256"));
        }
        // The pack names its case; the wire says it again as case_id, with the level, for the
        // door that reads them.
        let outbound = |pack: &Pack| Outbound { pack: pack.clone(), case_id: Some(pack.case.clone()), difficulty: Some(pl.level.clone()) };
        let mut reply = door.push(&token, std::slice::from_ref(&outbound(&pack)));
        if let Ok(Pushed::Queued(q)) = &reply {
            if door_takes_256 && q.rejected.iter().any(|why| refuses_256(why)) {
                // 7b's door does not know the sibling keys yet: say so once, send without, and
                // remember for the rest of the tick. The siblings are in the bucket for the door
                // that takes them.
                door_takes_256 = false;
                r.say("the door does not take 256 px keys yet; sending packs without their siblings this tick");
                pack.portrait.retain(|k, _| !k.ends_with("_256"));
                reply = door.push(&token, std::slice::from_ref(&outbound(&pack)));
            }
        }
        match reply {
            Ok(Pushed::Queued(q)) => {
                r.queued += q.queued;
                r.duplicates += q.duplicates;
                r.depth = Some(q.depth);
                if let Some(why) = q.rejected.first() {
                    r.door_rejected += 1;
                    r.fail(format!("the door rejected {} ({} {} {}): {why}", pack.case, pack.persona.name, pack.persona.age, pack.persona.country));
                    continue;
                }
                let mut sent = Sent::new(&pack.case, who, pack.persona.age, pack.endemic, pack.portrait.get("stable").cloned(), cfg.now, &cfg.ward);
                sent.sex = pack.persona.sex.clone();
                sent.variants_sent = pack.portrait.contains_key("stable_256");
                sent.case_id = Some(pack.case.clone());
                sent.difficulty = Some(pl.level.clone());
                ledger.sent.insert(id.clone(), sent);
                save_ledger(cfg, &ledger, &mut r);
                r.say(format!(
                    "{} {} — {} {} {} from {} · {} (drawn: {}, weight {} people/doctor){} · {} · {} · id {} · depth {}",
                    if q.queued == 1 { "queued" } else { "already queued" },
                    pack.case, pack.persona.name, pack.persona.sex.to_uppercase(), pack.persona.age, pack.persona.country,
                    region_name(pl.region), pack.persona.country, fmt_weight(pl.weight),
                    if pack.endemic { " (endemic)" } else { "" },
                    level_words(&pl.level), pl.case_why,
                    &id[..12], q.depth
                ));
            }
            Ok(Pushed::Closed { why }) => {
                r.say(format!("door closed mid-tick: {why}"));
                break;
            }
            Ok(Pushed::Refused { error }) => {
                r.fail(format!("the door refused the page: {error}"));
                break;
            }
            Err(e) => {
                r.fail(format!("push failed: {e}"));
                break;
            }
        }
    }
    if deferred > 0 {
        r.say(format!("{deferred} pack(s) deferred to the next tick: their faces are past the cap of {} painted per tick", cfg.bases_per_tick));
    }
    r.say(format!("pushed: {} queued, {} duplicates, {} refused by the door, depth {} · {} face(s) painted, {} passed", r.queued, r.duplicates, r.door_rejected, r.depth.map_or("?".into(), |d| d.to_string()), r.faces_tried, r.faces_made));

    // ── the rest of one patient's faces ──
    complete_faces(cfg, door, tools, &token, &ward, &pool, &mut manifest, &mut ledger, &mut r);
    record_spend(cfg, &mut ledger, &mut r);
    save_ledger(cfg, &ledger, &mut r);
    r
}

/// Today's spend into the ledger, and the cost line — estimated from list price, never measured.
fn record_spend(cfg: &Config, ledger: &mut Ledger, r: &mut Report) {
    let day = crate::ledger::utc_day(cfg.now);
    let s = ledger.spend.entry(day.clone()).or_default();
    s.edits += r.edits;
    s.judge_calls += r.judge_calls;
    s.deferred += r.deferred_budget;
    s.usd = estimate_usd(s.edits, s.judge_calls);
    r.say(format!(
        "cost, estimated from list price (edits × {EDIT_USD} USD + judge calls × {JUDGE_USD} USD): this tick {} edit(s) + {} judge call(s) ≈ {:.3} USD · today {day}: {} of {} edits used, {} judge call(s), {} state(s) deferred: budget ≈ {:.3} USD",
        r.edits, r.judge_calls, estimate_usd(r.edits, r.judge_calls),
        s.edits, cfg.edits_per_day, s.judge_calls, s.deferred, s.usd
    ));
}

/// For every pack still waiting: if the manifest's face for her key and age is no longer the one
/// she was sent with — a face remade after a person refused it — replace it through the pack door
/// (e56946b) and record the new address, so this happens once. A door that refuses because she is
/// in a bed already is logged and nothing is recorded: an admitted patient's faces are added
/// through her own door and never replaced.
#[allow(clippy::too_many_arguments)]
fn replace_remade_faces(cfg: &Config, tools: &dyn Tools, door: &dyn Door, token: &Token, pool: &[Person], manifest: &mut Manifest, ledger: &mut Ledger, r: &mut Report) {
    // Every waiting pack: the entry her face is filed under (by the address she was sent with,
    // which is her stable or, before 16 Sep, her base), and the made stable it should carry.
    let waiting: Vec<(String, String, String, u16)> = ledger
        .sent
        .iter()
        .filter(|(_, s)| s.patient_id.is_none())
        .filter_map(|(id, s)| {
            let slot = match s.stable.as_deref().and_then(|u| manifest.entry_with_stable(u)) {
                Some((k, _)) => k.clone(),
                None => manifest.base_for(&s.key, &(s.age..=s.age))?.key,
            };
            Some((id.clone(), slot, s.key.clone(), s.age))
        })
        .collect();
    for (id, slot, key, age) in waiting {
        let Some(who) = pool.iter().find(|p| p.key == key) else { continue };
        let made = match ensure_stable(cfg, tools, manifest, &slot, who, age, r) {
            Ok(Some(url)) => url,
            Ok(None) => continue,
            Err(e) => {
                r.fail(format!("{}'s stable could not be made for {} waiting pack: {e}", who.name, who.sex.possessive()));
                continue;
            }
        };
        if ledger.sent[&id].stable.as_deref() == Some(made.as_str()) {
            continue;
        }
        let url = made;
        let set = BTreeMap::from([("stable".to_string(), url.clone()), ("stable_256".to_string(), sibling_url(&url))]);
        let name = ledger.sent[&id].name.clone();
        match door.replace(token, &id, &set) {
            Ok(FillReply::Filled(f)) if f.added > 0 && f.rejected.is_empty() => {
                if let Some(s) = ledger.sent.get_mut(&id) {
                    s.stable = Some(url.clone());
                    s.variants_sent = true;
                }
                r.say(format!("replaced the face of {name} on waiting pack {} with {url}", &id[..12]));
            }
            Ok(FillReply::Filled(f)) if f.added > 0 && f.rejected.iter().all(|w| refuses_256(w)) => {
                if let Some(s) = ledger.sent.get_mut(&id) {
                    s.stable = Some(url.clone());
                }
                r.say(format!("replaced the face of {name} on waiting pack {} with {url}; the door does not take 256 px keys yet", &id[..12]));
            }
            Ok(FillReply::Filled(f)) => {
                for why in f.rejected {
                    r.say(format!("the face of {name} on pack {} was not replaced: {why}", &id[..12]));
                }
            }
            Ok(FillReply::Closed { why }) => {
                r.say(format!("door closed: {why}"));
                return;
            }
            Ok(FillReply::Refused { error }) => r.fail(format!("replacing the face of {name} on pack {}: {error}", &id[..12])),
            Err(e) => r.fail(format!("replacing the face of {name} on pack {}: {e}", &id[..12])),
        }
    }
}

/// Is this the door saying it does not know 256 px keys or addresses?
fn refuses_256(why: &str) -> bool {
    why.contains("_256") || why.contains("-256.webp")
}

/// Waiting packs that carry no siblings yet get them through the replace door, when the door takes
/// them; the first refusal in a tick stops the rest until the next tick.
fn carry_siblings_to_waiting_packs(door: &dyn Door, token: &Token, manifest: &Manifest, ledger: &mut Ledger, r: &mut Report) {
    let due: Vec<(String, String)> = ledger
        .sent
        .iter()
        .filter(|(_, s)| s.patient_id.is_none() && !s.variants_sent)
        .filter_map(|(id, s)| {
            let stable = s.stable.as_deref()?;
            let (_, e) = manifest.entry_with_stable(stable)?;
            e.portrait_256.get("stable").map(|v| (id.clone(), v.clone()))
        })
        .collect();
    for (id, url) in due {
        let set = BTreeMap::from([("stable_256".to_string(), url)]);
        let name = ledger.sent[&id].name.clone();
        match door.replace(token, &id, &set) {
            Ok(FillReply::Filled(f)) if f.added > 0 && f.rejected.is_empty() => {
                if let Some(s) = ledger.sent.get_mut(&id) {
                    s.variants_sent = true;
                }
                r.say(format!("carried the 256 px sibling to {name}'s waiting pack {}", &id[..12]));
            }
            Ok(FillReply::Filled(f)) => {
                if f.rejected.iter().any(|w| refuses_256(w)) {
                    r.say("the door does not take 256 px keys yet; the waiting packs keep theirs for a later tick");
                    return;
                }
                for why in f.rejected {
                    r.say(format!("{name}'s waiting pack {} did not take the sibling: {why}", &id[..12]));
                }
            }
            Ok(FillReply::Closed { why }) => {
                r.say(format!("door closed: {why}"));
                return;
            }
            Ok(FillReply::Refused { error }) => r.fail(format!("siblings for {name}'s pack {}: {error}", &id[..12])),
            Err(e) => r.fail(format!("siblings for {name}'s pack {}: {e}", &id[..12])),
        }
    }
}

fn save_ledger(cfg: &Config, ledger: &Ledger, r: &mut Report) {
    if let Err(e) = ledger.save(&cfg.ledger_path()) {
        r.fail(e);
    }
}

/// A base face: mflux, webp, the gate, sha, bucket. The url is the face's address.
///
/// The gate is the text model on Vertex, asked the brief's one question with the face inline. A
/// "no" is a new seed; after [`FACE_ATTEMPTS`] the person is given up on for this tick with a
/// sentence naming her, and nothing of the refused faces is recorded or uploaded — a doll that
/// reached the bucket would have an address, and an address is something a pack can carry.
fn make_face(cfg: &Config, tools: &dyn Tools, who: &Person, age: u16, place: &str, r: &mut Report) -> Result<String, String> {
    let work = cfg.world_dir.join("work");
    std::fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    let png = work.join(format!("{}@{age}.png", who.key));
    // The face's seed is the person, her age and this run's seed — so a tick (seeded by the clock
    // unless FACTORY_SEED says otherwise) never repeats a refused face, and a `--face` rerun with
    // another FACTORY_SEED tries three new ones rather than the same three.
    let base_seed = sha256_hex(format!("{}@{age}/{}", who.key, cfg.seed).as_bytes())[..8].chars().fold(0u64, |a, c| a * 16 + c.to_digit(16).unwrap_or(0) as u64);
    let prompt = prompts::base(age, who.sex, place);
    for attempt in 0..FACE_ATTEMPTS {
        let seed = base_seed.wrapping_add(u64::from(attempt) * 7919);
        tools.paint(&prompt, seed, &png)?;
        r.faces_tried += 1;
        let bytes = std::fs::read(&png).map_err(|e| format!("{}: {e}", png.display()))?;
        let _ = std::fs::remove_file(&png);
        let webp = tools.webp(&bytes, WEBP_QUALITY)?;
        let (ok, why) = tools.judge(&cfg.vertex_project, &cfg.judge_model, &webp, "image/webp", prompts::PHOTOREAL)?;
        r.judge_calls += 1;
        // A child's face is also asked how old it looks; only an answer inside the door's band
        // for the drawn age passes, because the painter renders "eight" as four unless told
        // otherwise and the door's band at eight is 6–10.
        let looks = if ok && age < prompts::CHILD_UNDER {
            let answer = tools.ask(&cfg.vertex_project, &cfg.judge_model, &webp, "image/webp", prompts::AGE)?;
            r.judge_calls += 1;
            let n = first_number(&answer);
            let band = vitals_web::ward::age_band(age);
            let fits = n.is_some_and(|n| band.contains(&n));
            Some((n, band, fits))
        } else {
            None
        };
        let fits = looks.as_ref().is_none_or(|l| l.2);
        r.say(format!(
            "face for {} at {age}, seed {seed}: photorealistic: {}{}{}",
            who.key,
            if ok { "yes" } else { "no — a new seed" },
            if why.is_empty() { String::new() } else { format!(" ({why})") },
            match &looks {
                Some((n, band, fits)) => format!(
                    " · looks {} (band {}\u{2013}{}){}",
                    n.map_or("?".to_string(), |n| n.to_string()),
                    band.start(),
                    band.end(),
                    if *fits { "" } else { " — a new seed" }
                ),
                None => String::new(),
            }
        ));
        if ok && fits {
            return publish(cfg, tools, &webp);
        }
        // A refused face is kept locally, never uploaded, so a person can see what was refused
        // and why the gate is right or wrong. `work/refused` is safe to delete.
        let refused = work.join("refused");
        if std::fs::create_dir_all(&refused).is_ok() {
            let _ = std::fs::write(refused.join(format!("{}@{age}-{seed}.webp", who.key)), &webp);
        }
    }
    Err(format!(
        "{} ({} at {age}): three faces in a row were not photographs of a person, and no pack is built on a rejected face",
        who.name, who.key
    ))
}

/// The first whole number in a sentence — "7", "She looks about 3." — or none.
fn first_number(text: &str) -> Option<u16> {
    let digits: String = text.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// Remake one face on request — `KEY@AGE`, e.g. `KOR-0@8` — through the same gate, for a face a
/// person looked at and refused. The old picture and every state edited from it leave the
/// manifest; the new url is returned and recorded. The ward is not touched: a queued pack that
/// carries the old address keeps it until the door lets a queued pack's portraits be replaced.
///
/// The report comes back with the error too, so the verdicts on the refused faces are not lost.
pub fn remake_face(cfg: &Config, tools: &dyn Tools, spec: &str) -> Result<(String, Report), Box<(String, Report)>> {
    let mut r = Report::default();
    match remake(cfg, tools, spec, &mut r) {
        Ok(url) => Ok((url, r)),
        Err(e) => Err(Box::new((e, r))),
    }
}

fn remake(cfg: &Config, tools: &dyn Tools, spec: &str, r: &mut Report) -> Result<String, String> {
    let (key, age) = spec
        .split_once('@')
        .and_then(|(k, a)| a.parse::<u16>().ok().map(|a| (k.to_string(), a)))
        .ok_or_else(|| format!("{spec} is not KEY@AGE (e.g. KOR-0@8): a face has an age"))?;
    let pool_text = std::fs::read_to_string(cfg.repo.join("crates/vitals-web/data/personas.json")).map_err(|e| e.to_string())?;
    let pool = read_pool(&pool_text)?;
    let who = pool.iter().find(|p| p.key == key).ok_or_else(|| format!("nobody in the pool is {key}"))?;
    let mut manifest = Manifest::load(&cfg.manifest_path())?;
    let url = make_face(cfg, tools, who, age, &who.place, r)?;
    let slot = format!("{key}@{age}");
    manifest.entries.remove(&slot);
    manifest.record_base(&key, age, &url, who);
    // record_base files a batch-age face under the bare key; a remake is always its own entry.
    if !manifest.entries.contains_key(&slot) {
        if let Some(mut e) = manifest.entries.remove(&key) {
            e.portrait.retain(|k, _| k == "base");
            e.portrait_256.clear();
            manifest.entries.insert(slot.clone(), e);
        }
    }
    manifest.save(&cfg.manifest_path())?;
    r.say(format!("{slot}: {url} recorded; the old face and its states are off the file"));
    r.faces_made = 1;
    Ok(url)
}

/// Which manifest entry a patient on the board was given her face from.
/// Is this a full-size portrait address (not a 256 px sibling)?
fn is_full_url(url: &str) -> bool {
    !url.ends_with("-256.webp")
}

/// The full-size address of a portrait the board shows: a 256 px sibling names the full picture
/// it was made from (same sha, `-256` off), and anything else is already full-size.
fn full_of(url: &str) -> String {
    match url.strip_suffix("-256.webp") {
        Some(stem) => format!("{stem}.webp"),
        None => url.to_string(),
    }
}

/// The full-size face the board shows for her, whichever field carries it and whichever size:
/// `portraits.stable`, else `portrait` — as the full address, never the thumbnail.
fn board_stable(p: &crate::door::BoardPatient) -> Option<String> {
    p.portraits.get("stable").or(p.portrait.as_ref()).map(|s| full_of(s))
}

fn entry_for(manifest: &Manifest, who: &Person, p: &crate::door::BoardPatient) -> Option<String> {
    if let Some(url) = board_stable(p) {
        if let Some((k, _)) = manifest.entry_with_stable(&url) {
            return Some(k.clone());
        }
    }
    let age = p.age?;
    manifest.base_for(&who.key, &(age.saturating_sub(NEAR_FACE)..=age.saturating_add(NEAR_FACE))).map(|b| b.key)
}

/// The states a patient on the board still lacks, and where each would come from.
struct Gap {
    patient_id: u64,
    who: Person,
    key: String,
    /// The full-size face the states are edited from and judged against: the entry's base when
    /// the board shows a face from this entry, else the face the board shows.
    reference: String,
    /// On file already: push these.
    from_manifest: BTreeMap<String, String>,
    /// Not on file: make these.
    to_make: Vec<&'static str>,
    /// Pictures the board shows (full size) that have no 256 px sibling on the board or on file:
    /// made from the board's own picture, under its sha. No model, no filing.
    siblings_from_board: Vec<(String, String)>,
    /// Her age on the board, for the choice of wording: a child's states are asked for gently.
    age: Option<u16>,
    /// Whether the manifest entry under `key` holds the very face the board shows. When a face was
    /// remade after she was admitted the board keeps the old one (add only), the states are edited
    /// from the board's face, and recording them under the new face's entry would file one
    /// woman's expressions under another's picture. So they are pushed and not recorded.
    record: bool,
}

fn gaps(ward: &WardView, pool: &[Person], manifest: &Manifest, r: &mut Report) -> Vec<Gap> {
    let mut out = Vec::new();
    for p in ward.open() {
        let (Some(name), Some(country)) = (&p.name, &p.country) else {
            r.say(format!("patient {} has no pack on the board, so there is nobody to put a face on", p.patient_id));
            continue;
        };
        let Some(who) = person_for(pool, name, country) else {
            r.say(format!("patient {} ({name}, {country}) is nobody in the pool; not this factory's", p.patient_id));
            continue;
        };
        let Some(key) = entry_for(manifest, who, p) else {
            r.say(format!("patient {} ({name}) has no face on file to make the others from", p.patient_id));
            continue;
        };
        let entry = &manifest.entries[&key];
        // The reference the other states are edited from has to be the full-size face: the
        // board's `portrait` is the 256 px sibling when one exists (4920a43), and a face locked
        // against a thumbnail would make every state after it softer than the first.
        let Some(stable) = board_stable(p).or_else(|| entry.portrait.get("stable").cloned()) else {
            continue;
        };
        let has = |st: &str| p.portraits.contains_key(st);
        let record = entry.portrait.get("stable") == Some(&stable) || entry.portrait.get("base") == Some(&stable);
        // The reference is the painted base when the board's face is this entry's; a patient
        // admitted before the rule shows her base as her stable, and that is her reference too.
        let reference = if record { entry.base().cloned().unwrap_or_else(|| stable.clone()) } else { stable.clone() };
        let mut from_manifest = BTreeMap::new();
        let mut to_make = Vec::new();
        for st in prompts::STATES {
            if has(st) {
                continue;
            }
            match entry.portrait.get(st).filter(|_| record) {
                Some(url) => {
                    from_manifest.insert(st.to_string(), url.clone());
                }
                None => to_make.push(st),
            }
        }
        if p.portraits.is_empty() && p.portrait.is_none() {
            // A build that publishes no set at all: the stable is pushed too, so the board can show her.
            from_manifest.insert("stable".into(), stable.clone());
        }
        // The 256 px siblings of every state she has on file that the board does not show yet.
        if record {
            for (st, v) in &entry.portrait_256 {
                let k = format!("{st}_256");
                if !p.portraits.contains_key(&k) {
                    from_manifest.insert(k, v.clone());
                }
            }
        }
        // And of every full-size picture the board shows that neither the board nor the file has
        // a sibling for — states edited from a face the board kept, pushed and never filed.
        let mut siblings_from_board = Vec::new();
        for (st, url) in &p.portraits {
            if st.ends_with("_256") || !is_full_url(url) {
                continue;
            }
            let k = format!("{st}_256");
            if p.portraits.contains_key(&k) || from_manifest.contains_key(&k) {
                continue;
            }
            siblings_from_board.push((st.clone(), url.clone()));
        }
        if from_manifest.is_empty() && to_make.is_empty() && siblings_from_board.is_empty() {
            continue;
        }
        out.push(Gap { patient_id: p.patient_id, who: who.clone(), key, reference, from_manifest, to_make, siblings_from_board, age: p.age, record });
    }
    out
}

fn dry_run_faces(cfg: &Config, r: &mut Report, ward: &WardView, pool: &[Person], manifest: &Manifest) {
    let mut made_one = false;
    for g in gaps(ward, pool, manifest, r) {
        if !g.from_manifest.is_empty() {
            r.say(format!("would push {} state(s) on file for patient {} ({}): {:?}", g.from_manifest.len(), g.patient_id, g.who.name, g.from_manifest.keys().collect::<Vec<_>>()));
        }
        if !g.to_make.is_empty() {
            if made_one {
                r.say(format!("patient {} ({}) also lacks {:?}; would wait for a later tick", g.patient_id, g.who.name, g.to_make));
            } else {
                r.say(format!("would make {:?} for patient {} ({}) from {} with {} and push them", g.to_make, g.patient_id, g.who.name, g.reference, cfg.model));
                made_one = true;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn complete_faces(cfg: &Config, door: &dyn Door, tools: &dyn Tools, token: &Token, ward: &WardView, pool: &[Person], manifest: &mut Manifest, ledger: &mut Ledger, r: &mut Report) {
    let mut made_one = false;
    let found = gaps(ward, pool, manifest, r);
    for g in found {
        let mut set = g.from_manifest.clone();
        // Siblings of the board's own pictures: fetched, resized, uploaded under their sha. No
        // model is called, so this is not the one-patient-per-tick step.
        for (st, url) in &g.siblings_from_board {
            let full = match sha_of(url).filter(|sha| cfg.face_path(sha).exists()) {
                Some(sha) => std::fs::read(cfg.face_path(sha)).map_err(|e| e.to_string()),
                None => tools.fetch(url),
            };
            match full.and_then(|bytes| sha_of(url).ok_or_else(|| format!("{url} is not a portrait address")).and_then(|sha| publish_sibling(cfg, tools, sha, &bytes))) {
                Ok(small) => {
                    set.insert(format!("{st}_256"), small);
                }
                Err(e) => r.fail(format!("patient {} ({}): the sibling of {st} could not be made: {e}", g.patient_id, g.who.name)),
            }
        }
        if !g.to_make.is_empty() {
            if made_one {
                r.say(format!("patient {} ({}) still lacks {:?}; next tick", g.patient_id, g.who.name, g.to_make));
            } else {
                made_one = true;
                let made = make_states(cfg, tools, &g, manifest, r);
                set.extend(made.set);
                for (st, e) in made.failed {
                    r.fail(format!("patient {} ({}): {st} could not be made: {e}", g.patient_id, g.who.name));
                }
                if !made.refused.is_empty() {
                    if let Some(sent) = ledger.sent.values_mut().find(|s| s.patient_id == Some(g.patient_id)) {
                        sent.refused.extend(made.refused);
                    }
                }
            }
        }
        if set.is_empty() {
            continue;
        }
        match door.fill(token, g.patient_id, &set) {
            Ok(FillReply::Filled(f)) => {
                r.say(format!("patient {} ({}): {} added, {} kept, now {:?}", g.patient_id, g.who.name, f.added, f.kept, f.states));
                let mut said_256 = false;
                for why in f.rejected {
                    if refuses_256(&why) {
                        if !said_256 {
                            r.say("the door does not take 256 px keys yet; the siblings wait for a later tick");
                            said_256 = true;
                        }
                        continue;
                    }
                    r.fail(format!("patient {}: {why}", g.patient_id));
                }
            }
            Ok(FillReply::Closed { why }) => {
                r.say(format!("door closed: {why}"));
                return;
            }
            Ok(FillReply::Refused { error }) => r.fail(format!("patient {}: {error}", g.patient_id)),
            Err(e) => r.fail(format!("patient {}: {e}", g.patient_id)),
        }
    }
}

/// The five other states from her base, each recorded the moment it is in the bucket.
///
/// One state failing does not lose the others: the editor refuses some pictures outright (Vertex
/// filtered a child's "deteriorating" as prohibited content, 16 Sep), and what was made is still
/// pushed — the board shows the nearest milder state for the rest, which is its own rule.
/// What making a patient's states came to: the pictures to push, the states that could not be
/// made (with the error), and the states the judge refused twice (with why).
#[derive(Default)]
struct Made {
    set: BTreeMap<String, String>,
    failed: Vec<(&'static str, String)>,
    refused: Vec<String>,
}

fn make_states(cfg: &Config, tools: &dyn Tools, g: &Gap, manifest: &mut Manifest, r: &mut Report) -> Made {
    let mut made = Made::default();
    let reference = match read_face(cfg, tools, &g.reference) {
        Ok(b) => b,
        Err(e) => {
            made.failed.push(("stable", format!("the reference face could not be read: {e}")));
            return made;
        }
    };
    let child = g.age.is_some_and(|a| a < prompts::CHILD_UNDER);
    for st in &g.to_make {
        let prompt = match prompts::state_for(st, g.who.sex, child) {
            Some(p) => p,
            None => {
                made.failed.push((st, format!("no prompt for {st}")));
                continue;
            }
        };
        match gated_edit(cfg, tools, &reference, &prompt, st, &g.who.name, r) {
            Ok(Gate::Budget) => continue,
            Ok(Gate::Made(webp)) => match publish(cfg, tools, &webp) {
                Ok(url) => {
                    if g.record {
                        manifest.record_state(&g.key, st, &url);
                        manifest.record_variant(&g.key, st, &sibling_url(&url));
                        if let Err(e) = manifest.save(&cfg.manifest_path()) {
                            r.fail(e);
                        }
                    }
                    r.states_made += 1;
                    r.say(format!("made {st} for {} ({}){}", g.who.name, g.key, if g.record { "" } else { " — from the face the board shows, which is not the one on file; pushed, not recorded" }));
                    made.set.insert(format!("{st}_256"), sibling_url(&url));
                    made.set.insert(st.to_string(), url);
                }
                Err(e) => made.failed.push((st, e)),
            },
            Ok(Gate::Refused) => {
                r.rejected += 1;
                let why = format!("{st}: refused twice by the judge; left out, the board falls back to the nearest milder picture");
                r.say(format!("{} ({}): {why}", g.who.name, g.key));
                made.refused.push(why);
            }
            Err(e) => made.failed.push((st, e)),
        }
    }
    made
}

/// A face's bytes: the local copy when this machine made it, else the bucket.
fn read_face(cfg: &Config, tools: &dyn Tools, url: &str) -> Result<Vec<u8>, String> {
    match sha_of(url) {
        Some(sha) if cfg.face_path(sha).exists() => std::fs::read(cfg.face_path(sha)).map_err(|e| e.to_string()),
        _ => tools.fetch(url),
    }
}

fn mime_of(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"RIFF") { "image/webp" } else { "image/png" }
}

/// What one gated edit came to.
enum Gate {
    /// The picture, judged the same person and showing the state.
    Made(Vec<u8>),
    /// Refused twice: left out, never filed, never sent; the ladder shows the nearest milder one.
    Refused,
    /// Today's edit budget is spent: not tried; waits for tomorrow.
    Budget,
}

/// One state, edited from the reference and judged twice — the same person as the reference,
/// and showing the state's own feature — with one re-edit on a no, inside today's edit budget.
fn gated_edit(cfg: &Config, tools: &dyn Tools, reference: &[u8], prompt: &str, state: &str, name: &str, r: &mut Report) -> Result<Gate, String> {
    let ref_mime = mime_of(reference);
    let shows = prompts::shows(state).ok_or_else(|| format!("no sentence for {state}"))?;
    for attempt in 0..2 {
        if r.edits_left(cfg) == 0 {
            r.deferred_budget += 1;
            r.say(format!("{name} {state}: deferred: budget ({} of {} edits used today)", r.edits_before + r.edits, cfg.edits_per_day));
            return Ok(Gate::Budget);
        }
        let png = tools.edit(&cfg.vertex_project, &cfg.model, reference, ref_mime, prompt)?;
        r.edits += 1;
        let webp = tools.webp(&png, WEBP_QUALITY)?;
        let (same, why) = tools.judge_pair(&cfg.vertex_project, &cfg.judge_model, reference, ref_mime, &webp, "image/webp", prompts::SAME_PERSON)?;
        r.judge_calls += 1;
        if !same {
            r.say(format!("{name} {state}{}: not the same person ({why}){}", if attempt == 0 { "" } else { ", re-edit" }, if attempt == 0 { " — one re-edit" } else { "" }));
            continue;
        }
        let (ok, why) = tools.judge(&cfg.vertex_project, &cfg.judge_model, &webp, "image/webp", &shows)?;
        r.judge_calls += 1;
        if ok {
            r.say(format!("{name} {state}{}: same person, shows the state ({why})", if attempt == 0 { "" } else { ", re-edit" }));
            return Ok(Gate::Made(webp));
        }
        r.say(format!("{name} {state}{}: same person but does not show the state ({why}){}", if attempt == 0 { "" } else { ", re-edit" }, if attempt == 0 { " — one re-edit" } else { "" }));
    }
    Ok(Gate::Refused)
}

/// Her stable, made from the base and judged, on file under `stable` with the base under `base`.
/// `Ok(Some(url))` is the made stable (already on file, or made now); `Ok(None)` is a stable
/// refused twice — she goes out without a picture rather than with the wrong one.
fn ensure_stable(cfg: &Config, tools: &dyn Tools, manifest: &mut Manifest, slot: &str, who: &Person, age: u16, r: &mut Report) -> Result<Option<String>, String> {
    let entry = manifest.entries.get(slot).ok_or_else(|| format!("{slot} is not on file"))?;
    if let Some(made) = entry.made_stable() {
        return Ok(Some(made.clone()));
    }
    let base_url = entry.base().cloned().ok_or_else(|| format!("{slot} has no face on file"))?;
    let reference = read_face(cfg, tools, &base_url)?;
    let prompt = prompts::state_for(prompts::STABLE, who.sex, age < prompts::CHILD_UNDER).ok_or_else(|| "no stable prompt".to_string())?;
    match gated_edit(cfg, tools, &reference, &prompt, prompts::STABLE, &who.name, r)? {
        Gate::Budget => Ok(None),
        Gate::Made(webp) => {
            let url = publish(cfg, tools, &webp)?;
            manifest.record_stable(slot, &url);
            manifest.record_variant(slot, "stable", &sibling_url(&url));
            manifest.save(&cfg.manifest_path())?;
            r.say(format!("made stable for {} ({slot}) from the base", who.name));
            Ok(Some(url))
        }
        Gate::Refused => {
            r.rejected += 1;
            Ok(None)
        }
    }
}

/// The 256 px siblings of every portrait already on file that has none: fetched from the bucket
/// (or read locally), resized, uploaded under the full one's sha with `-256`, and recorded. Runs
/// until nothing is missing; a second run makes nothing. The ward is not touched here — the next
/// tick carries the siblings to the patients and packs whose doors take them.
pub fn backfill_variants(cfg: &Config, tools: &dyn Tools) -> Report {
    let mut r = Report::default();
    let mut manifest = match Manifest::load(&cfg.manifest_path()) {
        Ok(m) => m,
        Err(e) => {
            r.fail(e);
            return r;
        }
    };
    let due: Vec<(String, String, String)> = manifest
        .entries
        .iter()
        .flat_map(|(k, e)| {
            e.portrait
                .iter()
                .filter(|(st, _)| !e.portrait_256.contains_key(*st))
                .map(|(st, url)| (k.clone(), st.clone(), url.clone()))
                .collect::<Vec<_>>()
        })
        .collect();
    r.say(format!("{} portrait(s) on file without a 256 px sibling", due.len()));
    let mut made = 0;
    for (key, st, url) in due {
        let Some(sha) = sha_of(&url) else {
            r.fail(format!("{key} {st}: {url} is not a portrait address"));
            continue;
        };
        let full = if cfg.face_path(sha).exists() { std::fs::read(cfg.face_path(sha)).map_err(|e| e.to_string()) } else { tools.fetch(&url) };
        let full = match full {
            Ok(b) => b,
            Err(e) => {
                r.fail(format!("{key} {st}: {e}"));
                continue;
            }
        };
        match publish_sibling(cfg, tools, sha, &full) {
            Ok(small) => {
                manifest.record_variant(&key, &st, &small);
                if let Err(e) = manifest.save(&cfg.manifest_path()) {
                    r.fail(e);
                    return r;
                }
                made += 1;
            }
            Err(e) => r.fail(format!("{key} {st}: {e}")),
        }
    }
    r.say(format!("{made} sibling(s) made and recorded"));
    r
}

/// For the binary: the checkout this binary was built from, when run from anywhere else.
pub fn default_repo() -> PathBuf {
    std::env::var_os("VITALS_REPO").map(PathBuf::from).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").to_path_buf())
}
