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

use crate::catalogue::read_catalogue;
use crate::door::{Door, FillReply, Pushed, Token, WardView};
use crate::ledger::{Ledger, Sent};
use crate::manifest::Manifest;
use crate::need::{fmt as fmt_weight, weights, Weights};
use crate::plan::{bed_cap, plan, Base, Inputs, NEAR_FACE};
use crate::pool::{person_for, read_endemic, read_pool, Person};
use crate::prompts;
use crate::tools::{sha256_hex, Tools};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use vitals_web::ward::Pack;
use vitals_web::ward_chain::{pack_id, PORTRAITS};

/// webp quality, as the batch used.
pub const WEBP_QUALITY: u8 = 86;

/// The 256 px sibling every portrait gets: `<sha>-256.webp`, the same sha as the full one so the
/// pair is addressable, at this quality (~7–12 KB). The board can draw a wall of faces from these.
pub const VARIANT_PX: u32 = 256;
pub const VARIANT_QUALITY: u8 = 80;

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
    /// The ward's own GCP project: where its `vitals-token` secret lives. Staging is
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
    pub rejected: usize,
    /// The queue's depth as the door last reported it.
    pub depth: Option<usize>,
    pub faces_made: usize,
    /// Faces painted, passed or refused — what the per-tick cap counts.
    pub faces_tried: usize,
    pub states_made: usize,
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
    let catalogue = read_catalogue(&cfg.repo);
    for u in &catalogue.unbuildable {
        r.say(format!("not built: {} — {}", u.id, u.why));
    }
    if catalogue.cases.is_empty() {
        r.fail(format!("no case can be built from {}", cfg.repo.display()));
        return r;
    }
    let pool = match std::fs::read_to_string(cfg.repo.join("crates/vitals-web/data/personas.json")).map_err(|e| e.to_string()).and_then(|s| read_pool(&s)) {
        Ok(p) => p,
        Err(e) => {
            r.fail(format!("the pool could not be read: {e}"));
            return r;
        }
    };
    let endemic = match std::fs::read_to_string(cfg.repo.join("crates/vitals-web/data/endemic.json")).map_err(|e| e.to_string()).and_then(|s| read_endemic(&s)) {
        Ok(e) => e,
        Err(e) => {
            r.fail(format!("the endemic list could not be read: {e}"));
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
    if let Some(other) = ledger.sent.values().map(|s| &s.ward).find(|w| *w != &cfg.ward) {
        r.fail(format!(
            "the ledger at {} belongs to {other}, and this run targets {} — one world directory per ward, \
             or the same face ends up on two boards",
            cfg.ledger_path().display(),
            cfg.ward
        ));
        return r;
    }
    r.say(format!(
        "holding {} buildable cases, {} people, {} faces on file, {} packs in the ledger",
        catalogue.cases.len(),
        pool.len(),
        manifest.entries.len(),
        ledger.sent.len()
    ));

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
    if ward.queue.as_ref().is_some_and(|q| q.door != "open") {
        r.say("the door is closed — the ward opens when the founder says so; nothing to do until then");
        if !cfg.dry_run {
            if let Err(e) = ledger.save(&cfg.ledger_path()) {
                r.fail(e);
            }
        }
        return r;
    }

    // ── the plan, before anything is touched ──
    let resend: Vec<(String, Pack)> = ledger.unseen().into_iter().map(|(id, s)| (id.clone(), s.to_pack())).collect();
    let known_depth = ward.queue.as_ref().map(|q| q.waiting).unwrap_or(resend.len());
    let want_guess = cfg.queue_depth.saturating_sub(known_depth.max(resend.len()));
    r.say(need.table());
    r.say(format!("bed cap: no country in more than {} of {} beds at once", bed_cap(ward.beds), ward.beds));
    let planned = plan(&Inputs {
        catalogue: &catalogue, pool: &pool, endemic: &endemic, manifest: &manifest, ward: &ward, ledger: &ledger,
        weights: &need, beds: ward.beds, want: want_guess, seed: cfg.seed,
    });
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
                "  {} — {} {} {} from {} (drawn: {}, weight {} people/doctor){} · {}",
                pl.pack.case, pl.pack.persona.name, pl.sex.letter().to_uppercase(), pl.pack.persona.age, pl.pack.persona.country,
                pl.pack.persona.country, fmt_weight(pl.weight),
                if pl.pack.endemic { " (endemic)" } else { "" }, face
            ));
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
    replace_remade_faces(door, &token, &manifest, &mut ledger, &mut r);
    carry_siblings_to_waiting_packs(door, &token, &manifest, &mut ledger, &mut r);
    save_ledger(cfg, &ledger, &mut r);

    // ── resend what the board has not shown yet: recovery and probe in one ──
    let mut depth: Option<usize> = None;
    let mut lost = 0;
    for (id, pack) in &resend {
        match door.push(&token, std::slice::from_ref(pack)) {
            Ok(Pushed::Queued(q)) => {
                depth = Some(q.depth);
                r.queued += q.queued;
                r.duplicates += q.duplicates;
                lost += q.queued;
                if let Some(why) = q.rejected.first() {
                    r.rejected += 1;
                    r.fail(format!("the door now refuses {} ({}, sent earlier): {why} — dropped from the ledger", pack.persona.name, pack.case));
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
        plan(&Inputs { catalogue: &catalogue, pool: &pool, endemic: &endemic, manifest: &manifest, ward: &ward, ledger: &ledger, weights: &need, beds: ward.beds, want, seed: cfg.seed })
    };
    r.say(format!("queue depth {depth_now}, want {}: building {}", cfg.queue_depth, planned.packs.len()));
    let mut deferred = 0;
    let mut door_takes_256 = true;
    for pl in planned.packs {
        let mut pack = pl.pack;
        if let Base::Have { key, .. } = &pl.base {
            if let Some(v) = manifest.entries.get(key).and_then(|e| e.portrait_256.get("stable")) {
                pack.portrait.insert("stable_256".into(), v.clone());
            }
        }
        if let Base::Make { key, age } = &pl.base {
            if r.faces_tried >= cfg.bases_per_tick {
                deferred += 1;
                continue;
            }
            let who = pool.iter().find(|p| &p.key == key).expect("a planned person is in the pool");
            match make_face(cfg, tools, who, *age, pl.place.as_str(), &mut r) {
                Ok(url) => {
                    manifest.record_base(key, *age, &url, who);
                    let slot = manifest.entry_with_stable(&url).map(|(k, _)| k.clone()).unwrap_or_else(|| format!("{key}@{age}"));
                    manifest.record_variant(&slot, "stable", &sibling_url(&url));
                    if let Err(e) = manifest.save(&cfg.manifest_path()) {
                        r.fail(e);
                        return r;
                    }
                    pack.portrait.insert("stable_256".into(), sibling_url(&url));
                    pack.portrait.insert("stable".into(), url);
                    r.faces_made += 1;
                    r.say(format!("made a face for {key} at {age}"));
                }
                Err(e) => {
                    r.fail(format!("{e} — no pack for {} this tick", who.name));
                    continue;
                }
            }
        }
        let who = pool.iter().find(|p| p.key == pl.person).expect("a planned person is in the pool");
        let id = pack_id(&pack);
        if !door_takes_256 {
            pack.portrait.retain(|k, _| !k.ends_with("_256"));
        }
        let mut reply = door.push(&token, std::slice::from_ref(&pack));
        if let Ok(Pushed::Queued(q)) = &reply {
            if door_takes_256 && q.rejected.iter().any(|why| refuses_256(why)) {
                // 7b's door does not know the sibling keys yet: say so once, send without, and
                // remember for the rest of the tick. The siblings are in the bucket for the door
                // that takes them.
                door_takes_256 = false;
                r.say("the door does not take 256 px keys yet; sending packs without their siblings this tick");
                pack.portrait.retain(|k, _| !k.ends_with("_256"));
                reply = door.push(&token, std::slice::from_ref(&pack));
            }
        }
        match reply {
            Ok(Pushed::Queued(q)) => {
                r.queued += q.queued;
                r.duplicates += q.duplicates;
                r.depth = Some(q.depth);
                if let Some(why) = q.rejected.first() {
                    r.rejected += 1;
                    r.fail(format!("rejected {} ({} {} {}): {why}", pack.case, pack.persona.name, pack.persona.age, pack.persona.country));
                    continue;
                }
                let mut sent = Sent::new(&pack.case, who, pack.persona.age, pack.endemic, pack.portrait.get("stable").cloned(), cfg.now, &cfg.ward);
                sent.sex = pack.persona.sex.clone();
                sent.variants_sent = pack.portrait.contains_key("stable_256");
                ledger.sent.insert(id.clone(), sent);
                save_ledger(cfg, &ledger, &mut r);
                r.say(format!(
                    "{} {} — {} {} {} from {} (drawn: {}, weight {} people/doctor){} · id {} · depth {}",
                    if q.queued == 1 { "queued" } else { "already queued" },
                    pack.case, pack.persona.name, pack.persona.sex.to_uppercase(), pack.persona.age, pack.persona.country,
                    pack.persona.country, fmt_weight(pl.weight),
                    if pack.endemic { " (endemic)" } else { "" },
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
    r.say(format!("pushed: {} queued, {} duplicates, {} rejected, depth {} · {} face(s) painted, {} passed", r.queued, r.duplicates, r.rejected, r.depth.map_or("?".into(), |d| d.to_string()), r.faces_tried, r.faces_made));

    // ── the rest of one patient's faces ──
    complete_faces(cfg, door, tools, &token, &ward, &pool, &mut manifest, &mut r);
    save_ledger(cfg, &ledger, &mut r);
    r
}

/// For every pack still waiting: if the manifest's face for her key and age is no longer the one
/// she was sent with — a face remade after a person refused it — replace it through the pack door
/// (e56946b) and record the new address, so this happens once. A door that refuses because she is
/// in a bed already is logged and nothing is recorded: an admitted patient's faces are added
/// through her own door and never replaced.
fn replace_remade_faces(door: &dyn Door, token: &Token, manifest: &Manifest, ledger: &mut Ledger, r: &mut Report) {
    let due: Vec<(String, String)> = ledger
        .sent
        .iter()
        .filter(|(_, s)| s.patient_id.is_none())
        .filter_map(|(id, s)| {
            let now = manifest.base_for(&s.key, &(s.age..=s.age))?;
            (s.stable.as_deref() != Some(now.url.as_str())).then(|| (id.clone(), now.url))
        })
        .collect();
    for (id, url) in due {
        let set = BTreeMap::from([("stable".to_string(), url.clone())]);
        let name = ledger.sent[&id].name.clone();
        match door.replace(token, &id, &set) {
            Ok(FillReply::Filled(f)) if f.added > 0 && f.rejected.is_empty() => {
                if let Some(s) = ledger.sent.get_mut(&id) {
                    s.stable = Some(url.clone());
                }
                r.say(format!("replaced the face of {name} on waiting pack {} with {url}", &id[..12]));
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
        // A child's face is also asked how old it looks; only an answer inside the door's band
        // for the drawn age passes, because the painter renders "eight" as four unless told
        // otherwise and the door's band at eight is 6–10.
        let looks = if ok && age < prompts::CHILD_UNDER {
            let answer = tools.ask(&cfg.vertex_project, &cfg.judge_model, &webp, "image/webp", prompts::AGE)?;
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
    manifest.record_variant(&slot, "stable", &sibling_url(&url));
    // record_base files a batch-age face under the bare key; a remake is always its own entry.
    if !manifest.entries.contains_key(&slot) {
        if let Some(mut e) = manifest.entries.remove(&key) {
            e.portrait.retain(|k, _| k == "stable");
            manifest.entries.insert(slot.clone(), e);
        }
    }
    manifest.save(&cfg.manifest_path())?;
    r.say(format!("{slot}: {url} recorded; the old face and its states are off the file"));
    r.faces_made = 1;
    Ok(url)
}

/// Which manifest entry a patient on the board was given her face from.
fn entry_for(manifest: &Manifest, who: &Person, p: &crate::door::BoardPatient) -> Option<String> {
    let stable = p.portraits.get("stable").or(p.portrait.as_ref());
    if let Some(url) = stable {
        if let Some((k, _)) = manifest.entry_with_stable(url) {
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
    stable: String,
    /// On file already: push these.
    from_manifest: BTreeMap<String, String>,
    /// Not on file: make these.
    to_make: Vec<&'static str>,
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
        let Some(stable) = p.portraits.get("stable").or(p.portrait.as_ref()).cloned().or_else(|| entry.portrait.get("stable").cloned()) else {
            continue;
        };
        let has = |st: &str| p.portraits.contains_key(st);
        let record = entry.portrait.get("stable") == Some(&stable);
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
        if from_manifest.is_empty() && to_make.is_empty() {
            continue;
        }
        out.push(Gap { patient_id: p.patient_id, who: who.clone(), key, stable, from_manifest, to_make, age: p.age, record });
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
                r.say(format!("would make {:?} for patient {} ({}) from {} with {} and push them", g.to_make, g.patient_id, g.who.name, g.stable, cfg.model));
                made_one = true;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn complete_faces(cfg: &Config, door: &dyn Door, tools: &dyn Tools, token: &Token, ward: &WardView, pool: &[Person], manifest: &mut Manifest, r: &mut Report) {
    let mut made_one = false;
    let found = gaps(ward, pool, manifest, r);
    for g in found {
        let mut set = g.from_manifest.clone();
        if !g.to_make.is_empty() {
            if made_one {
                r.say(format!("patient {} ({}) still lacks {:?}; next tick", g.patient_id, g.who.name, g.to_make));
            } else {
                made_one = true;
                let (new, failed) = make_states(cfg, tools, &g, manifest, r);
                set.extend(new);
                for (st, e) in failed {
                    r.fail(format!("patient {} ({}): {st} could not be made: {e}", g.patient_id, g.who.name));
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
                            r.say("the door does not take 256 px keys yet; her siblings wait for a later tick");
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
fn make_states(cfg: &Config, tools: &dyn Tools, g: &Gap, manifest: &mut Manifest, r: &mut Report) -> (BTreeMap<String, String>, Vec<(&'static str, String)>) {
    let mut out = BTreeMap::new();
    let mut failed = Vec::new();
    let base = match g.stable.rsplit('/').next().and_then(|n| n.strip_suffix(".webp")) {
        Some(sha) if cfg.face_path(sha).exists() => std::fs::read(cfg.face_path(sha)).map_err(|e| e.to_string()),
        _ => tools.fetch(&g.stable),
    };
    let base = match base {
        Ok(b) => b,
        Err(e) => {
            failed.push(("stable", format!("her base could not be read: {e}")));
            return (out, failed);
        }
    };
    let mime = if base.starts_with(b"RIFF") { "image/webp" } else { "image/png" };
    for st in &g.to_make {
        let one = || -> Result<String, String> {
            let child = g.age.is_some_and(|a| a < prompts::CHILD_UNDER);
            let prompt = prompts::state_for(st, g.who.sex, child).ok_or_else(|| format!("no prompt for {st}"))?;
            let png = tools.edit(&cfg.vertex_project, &cfg.model, &base, mime, &prompt)?;
            let webp = tools.webp(&png, WEBP_QUALITY)?;
            publish(cfg, tools, &webp)
        };
        match one() {
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
                out.insert(format!("{st}_256"), sibling_url(&url));
                out.insert(st.to_string(), url);
            }
            Err(e) => failed.push((st, e)),
        }
    }
    (out, failed)
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
