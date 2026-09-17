//! One tick, against a door and tools that are not there.
//!
//! The door here does what the real one does with a page of packs — validates each with the
//! ward's own `validate_pack`, keeps each under its content address, answers the four numbers —
//! and the tools record what they were asked to make instead of making it. What is under test is
//! the tick's own promises: the queue is topped up to depth and no further; a re-run after a
//! crash queues nobody twice; a closed door builds nothing; one patient's faces are completed per
//! tick; a dry run touches nothing; and the token appears in no line of the report.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use vitals_factory::cases::fits;
use vitals_factory::door::{parse_cases, Door, FillReply, Filled, Outbound, Pushed, Queued, Token, WardCase, WardView};
use vitals_factory::ledger::Ledger;
use vitals_factory::manifest::{batch_age, Manifest};
use vitals_factory::pool::{read_pool, Person};
use vitals_factory::tick::{backfill_variants, estimate_usd, remake_face, tick, Config, EDIT_USD, FACE_ATTEMPTS, JUDGE_USD, VARIANT_PX, VARIANT_QUALITY};
use vitals_factory::tools::Tools;
use vitals_web::ward::{Pack, PORTRAIT_LADDER};
use vitals_web::ward_chain::{is_portrait_url, pack_id, AGE_RANGE, PORTRAITS};

const POOL: &str = include_str!("../../vitals-web/data/personas.json");
const STAGING: &str = include_str!("fixtures/ward-staging-2026-09-16.json");
const CASES: &str = include_str!("fixtures/ward-cases-2026-09-16.json");

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn sha_url(bytes: &[u8]) -> String {
    let mut h = <sha2::Sha256 as sha2::Digest>::new();
    sha2::Digest::update(&mut h, bytes);
    let sha: String = sha2::Digest::finalize(h).iter().map(|b| format!("{b:02x}")).collect();
    format!("{PORTRAITS}/{sha}.webp")
}

/// A fresh world directory per test, so ledgers and manifests never meet.
fn world(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vitals-factory-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The seeded manifest: sixty faces at the batch ages.
/// The seeded manifest as it was made before 16 Sep: sixty faces at the batch ages, each under
/// `stable` and nothing under `base` — the face itself was the stable then.
fn seed_manifest(dir: &Path, pool: &[Person]) -> Manifest {
    let mut m = Manifest::default();
    for p in pool {
        let e = m.entries.entry(p.key.clone()).or_default();
        e.name = Some(p.name.clone());
        e.country = Some(p.country.clone());
        e.sex = Some(p.sex.letter().to_string());
        e.age = Some(batch_age(&p.key).unwrap());
        e.portrait.insert("stable".into(), sha_url(p.key.as_bytes()));
    }
    m.save(&dir.join("portraits.json")).unwrap();
    m
}

fn config(dir: &Path, depth: usize, bases: usize) -> Config {
    Config {
        ward: "https://ward.test".into(),
        queue_depth: depth,
        bases_per_tick: bases,
        repo: repo_root(),
        world_dir: dir.to_path_buf(),
        secret_project: "vitals-academy-dev".into(),
        vertex_project: "vitals-academy".into(),
        bucket: "vitals-world-portraits".into(),
        model: "gemini-2.5-flash-image".into(),
        judge_model: "gemini-2.5-flash".into(),
        edits_per_day: 40,
        dry_run: false,
        seed: 11,
        now: 1_789_500_000,
    }
}

/// `(case_id, difficulty)` as a pushed pack carried them.
type CaseFields = (Option<String>, Option<String>);

/// The door of 0543ed7: a pack names a World case from the list, a person of the case's sex at an
/// age inside its window, a real country, portraits at the bucket's address — and a season id is
/// refused in the door's own words.
fn validate(p: &Pack, cases: &[WardCase]) -> Result<(), String> {
    let Some(case) = cases.iter().find(|c| c.case_id == p.case) else {
        return Err(format!("{} is not a case this ward serves — queueing her would put a patient on the board that no shift can open", p.case));
    };
    if !p.persona.country_is_alpha3() {
        return Err(format!("{} is not an ISO 3166-1 alpha-3 country code", p.persona.country));
    }
    if p.persona.name.trim().is_empty() {
        return Err("a patient with no name".into());
    }
    if !AGE_RANGE.contains(&p.persona.age) {
        return Err(format!("nobody is {}", p.persona.age));
    }
    let sex = vitals_factory::sex::Sex::parse(&p.persona.sex).ok_or_else(|| format!("{} is not a sex the pack may carry", p.persona.sex))?;
    if !fits(case, sex, p.persona.age) {
        return Err(format!("{} is written for {:?}, and this pack says {} of {}", p.case, case.patient, p.persona.sex, p.persona.age));
    }
    for (state, src) in &p.portrait {
        if state == "dead" || !PORTRAIT_LADDER.contains(&state.as_str()) {
            return Err(format!("{state} is not a state the engine reports"));
        }
        if !is_portrait_url(src) {
            return Err(format!("a portrait must be {PORTRAITS}/<sha256>.webp, not {src}"));
        }
    }
    Ok(())
}

/// The door, as the ward runs it: validate, content-address, answer in numbers.
struct FakeDoor {
    ward: RefCell<WardView>,
    open: bool,
    /// Whether this door knows `<state>_256` keys and `<sha>-256.webp` addresses (7b's door does
    /// not yet, 16 Sep); a door that does not refuses a pack whole and a fill entry by entry.
    takes_256: bool,
    queue: RefCell<BTreeMap<String, Pack>>,
    /// The case door's list; empty for a ward built before it.
    cases: RefCell<Vec<WardCase>>,
    /// What each pushed pack said beside the pack itself: `(case_id, difficulty)`, by pack id.
    chosen: RefCell<BTreeMap<String, CaseFields>>,
    pushes: RefCell<Vec<usize>>,
    fills: RefCell<Vec<(u64, BTreeMap<String, String>)>>,
    replaces: RefCell<Vec<(String, BTreeMap<String, String>)>>,
    tokens_seen: RefCell<Vec<String>>,
}

impl FakeDoor {
    /// A ward with its case door open on the 16 Sep list, as the real one is.
    fn new(ward: WardView) -> FakeDoor {
        FakeDoor {
            ward: RefCell::new(ward), open: true, takes_256: true, queue: RefCell::new(BTreeMap::new()),
            cases: RefCell::new(parse_cases(CASES).unwrap()), chosen: RefCell::new(BTreeMap::new()),
            pushes: RefCell::new(vec![]), fills: RefCell::new(vec![]), replaces: RefCell::new(vec![]), tokens_seen: RefCell::new(vec![]),
        }
    }
    /// A ward that lists no cases.
    fn no_cases(ward: WardView) -> FakeDoor {
        let d = FakeDoor::new(ward);
        d.cases.borrow_mut().clear();
        d
    }
}

impl Door for FakeDoor {
    fn read_ward(&self) -> Result<WardView, String> {
        Ok(self.ward.borrow().clone())
    }
    fn read_cases(&self) -> Result<Vec<WardCase>, String> {
        Ok(self.cases.borrow().clone())
    }
    fn push(&self, token: &Token, packs: &[Outbound]) -> Result<Pushed, String> {
        self.tokens_seen.borrow_mut().push(token.bearer());
        self.pushes.borrow_mut().push(packs.len());
        if !self.open {
            return Ok(Pushed::Closed { why: "the ward is not open yet".into() });
        }
        let mut q = self.queue.borrow_mut();
        let mut out = Queued { queued: 0, duplicates: 0, rejected: vec![], depth: 0 };
        for o in packs {
            let p = &o.pack;
            self.chosen.borrow_mut().insert(pack_id(p), (o.case_id.clone(), o.difficulty.clone()));
            let mut plain = p.clone();
            plain.portrait.retain(|k, _| !k.ends_with("_256"));
            if !self.takes_256 {
                if let Some(k) = p.portrait.keys().find(|k| k.ends_with("_256")) {
                    out.rejected.push(format!("{k} is not a state the engine reports, so nothing would ever draw it. The keys are [\"recovered\", …]"));
                    continue;
                }
            } else if let Some(bad) = p.portrait.iter().find(|(k, v)| k.ends_with("_256") && !v.ends_with("-256.webp")) {
                out.rejected.push(format!("{}: a 256 px portrait must be {PORTRAITS}/<sha256>-256.webp", bad.0));
                continue;
            }
            if let Err(why) = validate(&plain, &self.cases.borrow()) {
                out.rejected.push(why);
                continue;
            }
            match q.entry(pack_id(p)) {
                std::collections::btree_map::Entry::Occupied(_) => out.duplicates += 1,
                std::collections::btree_map::Entry::Vacant(v) => {
                    v.insert(p.clone());
                    out.queued += 1;
                }
            }
        }
        out.depth = q.len();
        Ok(Pushed::Queued(out))
    }
    fn fill(&self, token: &Token, patient_id: u64, set: &BTreeMap<String, String>) -> Result<FillReply, String> {
        self.tokens_seen.borrow_mut().push(token.bearer());
        if !self.open {
            return Ok(FillReply::Closed { why: "not open".into() });
        }
        let mut ward = self.ward.borrow_mut();
        let Some(p) = ward.patients.iter_mut().find(|p| p.patient_id == patient_id) else {
            return Ok(FillReply::Refused { error: "no such patient".into() });
        };
        let mut f = Filled { added: 0, kept: 0, rejected: vec![], states: vec![] };
        for (k, v) in set {
            if k.ends_with("_256") && !self.takes_256 { f.rejected.push(format!("{k} is not a state the engine reports")); continue; }
            if p.portraits.contains_key(k) { f.kept += 1 } else { p.portraits.insert(k.clone(), v.clone()); f.added += 1 }
        }
        f.states = p.portraits.keys().cloned().collect();
        self.fills.borrow_mut().push((patient_id, set.clone()));
        Ok(FillReply::Filled(f))
    }
    /// e56946b: a waiting pack's faces may be replaced, by pack id; an admitted one's may not.
    fn replace(&self, token: &Token, pack_id: &str, set: &BTreeMap<String, String>) -> Result<FillReply, String> {
        self.tokens_seen.borrow_mut().push(token.bearer());
        self.replaces.borrow_mut().push((pack_id.to_string(), set.clone()));
        let mut q = self.queue.borrow_mut();
        let mut f = Filled { added: 0, kept: 0, rejected: vec![], states: vec![] };
        let Some(p) = q.get_mut(pack_id) else {
            f.rejected.push(format!("no pack {pack_id} is waiting — she may be in a bed already, and a patient's faces are added through her own door and never replaced"));
            return Ok(FillReply::Filled(f));
        };
        for (k, v) in set {
            if k.ends_with("_256") {
                if !self.takes_256 { f.rejected.push(format!("{k} is not a state the engine reports")); continue; }
            } else if !vitals_web::ward_chain::is_portrait_url(v) { f.rejected.push(format!("{k}: a portrait must be {PORTRAITS}/<sha256>.webp")); continue; }
            p.portrait.insert(k.clone(), v.clone());
            f.added += 1;
        }
        f.states = p.portrait.keys().cloned().collect();
        Ok(FillReply::Filled(f))
    }
}

/// Tools that make nothing and remember everything.
#[derive(Default)]
struct FakeTools {
    token_fetches: RefCell<usize>,
    paints: RefCell<Vec<(String, PathBuf)>>,
    /// Seeds the painter was asked for, in order.
    seeds: RefCell<Vec<u64>>,
    /// Scripted answers to the photorealism question; empty means "yes".
    verdicts: RefCell<VecDeque<bool>>,
    judged: RefCell<Vec<String>>,
    /// Scripted answers to the age question; empty means "the age the prompt asked for".
    ages: RefCell<VecDeque<String>>,
    asked: RefCell<Vec<String>>,
    /// Words in a state prompt the editor refuses, as Vertex refused a child's "deteriorating".
    refuse_edits: RefCell<Vec<&'static str>>,
    resized: RefCell<Vec<(u8, u32)>>,
    /// Scripted answers to the two-picture question ("the same person?"); empty means yes.
    pair_verdicts: RefCell<VecDeque<bool>>,
    paired: RefCell<Vec<String>>,
    /// Scripted answers to "does this picture show a patient who is …?"; empty means yes. Kept
    /// apart from `verdicts` (the base gate) so a test of one does not eat the other's answers.
    state_verdicts: RefCell<VecDeque<bool>>,
    edits: RefCell<Vec<String>>,
    uploads: RefCell<Vec<String>>,
    fetches: RefCell<Vec<String>>,
}

impl Tools for FakeTools {
    fn secret_token(&self, _project: &str) -> Result<Token, String> {
        *self.token_fetches.borrow_mut() += 1;
        Ok(Token::new("sekrit-token-value".into()))
    }
    fn paint(&self, prompt: &str, seed: u64, out_png: &Path) -> Result<(), String> {
        std::fs::write(out_png, format!("PNG:{prompt}:{seed}")).unwrap();
        self.paints.borrow_mut().push((prompt.to_string(), out_png.to_path_buf()));
        self.seeds.borrow_mut().push(seed);
        Ok(())
    }
    fn judge(&self, _project: &str, model: &str, image: &[u8], _mime: &str, question: &str) -> Result<(bool, String), String> {
        self.judged.borrow_mut().push(format!("{model}|{question}|{}", image.len()));
        if question.contains("Does this picture show a patient who is") {
            let ok = self.state_verdicts.borrow_mut().pop_front().unwrap_or(true);
            return Ok((ok, if ok { "mask and pallor as described".into() } else { "she looks well".into() }));
        }
        let ok = self.verdicts.borrow_mut().pop_front().unwrap_or(true);
        Ok((ok, if ok { "natural proportions".into() } else { "oversized eyes".into() }))
    }
    fn edit(&self, _project: &str, _model: &str, base: &[u8], _mime: &str, prompt: &str) -> Result<Vec<u8>, String> {
        self.edits.borrow_mut().push(prompt.to_string());
        if let Some(refused) = self.refuse_edits.borrow().iter().find(|w| prompt.contains(*w)) {
            return Err(format!("Vertex returned no content (finishReason IMAGE_PROHIBITED_CONTENT) for {refused}"));
        }
        Ok(format!("PNG-EDIT:{prompt}:{}", String::from_utf8_lossy(base)).into_bytes())
    }
    fn judge_pair(&self, _project: &str, model: &str, a: &[u8], _ma: &str, b: &[u8], _mb: &str, question: &str) -> Result<(bool, String), String> {
        self.paired.borrow_mut().push(format!("{model}|{question}|{}|{}", a.len(), b.len()));
        let ok = self.pair_verdicts.borrow_mut().pop_front().unwrap_or(true);
        Ok((ok, if ok { "same face, same hair".into() } else { "a different jaw and eyes".into() }))
    }
    fn ask(&self, _project: &str, model: &str, image: &[u8], _mime: &str, question: &str) -> Result<String, String> {
        self.asked.borrow_mut().push(format!("{model}|{question}"));
        if let Some(a) = self.ages.borrow_mut().pop_front() {
            return Ok(a);
        }
        // The fake's "image" is the prompt it was painted from, so the age asked for is in it.
        let text = String::from_utf8_lossy(image).to_string();
        let aged = text.split("aged ").nth(1).and_then(|r| r.split(' ').next()).unwrap_or("30");
        Ok(aged.to_string())
    }
    fn webp(&self, png: &[u8], quality: u8) -> Result<Vec<u8>, String> {
        Ok(format!("WEBP{quality}:{}", String::from_utf8_lossy(png)).into_bytes())
    }
    fn webp_resized(&self, image: &[u8], quality: u8, size: u32) -> Result<Vec<u8>, String> {
        self.resized.borrow_mut().push((quality, size));
        Ok(format!("WEBP{quality}@{size}:{}", String::from_utf8_lossy(image)).into_bytes())
    }
    fn upload(&self, _local: &Path, object: &str) -> Result<(), String> {
        self.uploads.borrow_mut().push(object.to_string());
        Ok(())
    }
    fn fetch(&self, url: &str) -> Result<Vec<u8>, String> {
        self.fetches.borrow_mut().push(url.to_string());
        Ok(format!("BASE:{url}").into_bytes())
    }
}

#[test]
fn a_tick_tops_the_queue_up_to_depth_makes_at_most_so_many_faces_and_says_what_it_did() {
    let dir = world("fill");
    let pool = read_pool(POOL).unwrap();
    seed_manifest(&dir, &pool);
    let door = FakeDoor::new(WardView::parse(STAGING).unwrap());
    let tools = FakeTools::default();
    let cfg = config(&dir, 6, 2);

    let r = tick(&cfg, &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let q = door.queue.borrow();
    assert!(q.len() <= 6, "never past depth: {}", q.len());
    assert!(!q.is_empty(), "something was built");
    // Faces: at most two made this tick, each uploaded under its sha and recorded at her age.
    let paints = tools.paints.borrow();
    assert!(paints.len() <= 2, "{} faces made, two allowed", paints.len());
    let ups = tools.uploads.borrow();
    assert!(ups.len() >= 2 * paints.len(), "each face and its 256 px sibling, and each made stable and its sibling: {}", ups.len());
    for object in ups.iter() {
        assert!((object.len() == 69 || object.len() == 73) && object.ends_with(".webp"), "content-addressed, or the sibling of one: {object}");
        if object.len() == 69 {
            assert!(ups.contains(&format!("{}-256.webp", object.trim_end_matches(".webp"))), "{object} has its sibling");
        }
    }
    drop(ups);
    let man = Manifest::load(&dir.join("portraits.json")).unwrap();
    let made: Vec<_> = man.entries.iter().filter(|(k, _)| k.contains('@')).collect();
    assert_eq!(made.len(), paints.len(), "each face made is recorded under key@age");
    for (_, e) in &made {
        assert!(e.age.is_some() && e.portrait.contains_key("stable"));
    }
    for (prompt, _) in paints.iter() {
        assert!(prompt.contains("lying in a hospital bed") && prompt.contains("no flags"), "the batch's prompt: {prompt}");
    }
    // Every queued pack carries a face, and each is in the ledger as sent and unseen.
    let ledger = Ledger::load(&dir.join("factory-ledger.json")).unwrap();
    assert_eq!(ledger.sent.len(), q.len());
    for (id, p) in q.iter() {
        assert!(p.portrait.contains_key("stable"), "{}: sent with her face", p.persona.name);
        assert!(ledger.sent.contains_key(id), "the ledger holds the door's own id for her");
        assert!(ledger.sent[id].patient_id.is_none());
    }
    // The report says what happened in the door's words, and what each pack was drawn with.
    let text = r.lines.join("\n");
    assert!(text.contains("queued") && text.contains("depth"), "{text}");
    assert!(text.contains("people/doctor)"), "the weight beside each pack: {text}");
    assert!(text.contains("weights:") && text.contains("floor"), "the table the tick used: {text}");
    assert!(text.contains("deferred") || paints.len() < 2 || q.len() == 6, "faces past the cap wait for the next tick: {text}");
    assert!(!text.contains("sekrit"), "the token is in no line: {text}");
    assert_eq!(*tools.token_fetches.borrow(), 1, "fetched once per tick");
}

#[test]
fn a_second_tick_queues_nobody_twice_whether_or_not_the_door_kept_them() {
    let dir = world("twice");
    let pool = read_pool(POOL).unwrap();
    seed_manifest(&dir, &pool);
    let door = FakeDoor::new(WardView::parse(STAGING).unwrap());
    let tools = FakeTools::default();
    let cfg = config(&dir, 4, 1);

    let r1 = tick(&cfg, &door, &tools);
    assert!(r1.errors.is_empty(), "{:?}", r1.errors);
    let after_one: BTreeMap<String, Pack> = door.queue.borrow().clone();
    let sent_one = Ledger::load(&dir.join("factory-ledger.json")).unwrap().sent.len();
    assert_eq!(after_one.len(), sent_one);

    // The door kept them: the resend is all duplicates and nothing new is built past depth.
    let r2 = tick(&Config { seed: 12, ..cfg.clone() }, &door, &tools);
    assert!(r2.errors.is_empty(), "{:?}", r2.errors);
    assert!(r2.duplicates >= sent_one, "resent and recognised: {}", r2.duplicates);
    let q2 = door.queue.borrow().clone();
    assert!(q2.len() <= 4);
    for id in after_one.keys() {
        assert!(q2.contains_key(id), "still there, once");
    }

    // The door lost its queue (a store wiped, a redeploy): the resend puts each back, once, and
    // the same person is not built a second time under a new pack.
    door.queue.borrow_mut().clear();
    let r3 = tick(&Config { seed: 13, ..cfg.clone() }, &door, &tools);
    assert!(r3.errors.is_empty(), "{:?}", r3.errors);
    let q3 = door.queue.borrow().clone();
    for id in after_one.keys() {
        assert!(q3.contains_key(id), "put back under the same address");
    }
    let mut people: Vec<String> = q3.values().map(|p| format!("{}/{}", p.persona.name, p.persona.country)).collect();
    people.sort();
    people.dedup();
    assert_eq!(people.len(), q3.len(), "no person twice: {people:?}");
    assert_eq!(Ledger::load(&dir.join("factory-ledger.json")).unwrap().sent.len(), q3.len());
}

#[test]
fn a_closed_door_builds_nothing_and_says_so() {
    let dir = world("closed");
    let pool = read_pool(POOL).unwrap();
    seed_manifest(&dir, &pool);
    let mut door = FakeDoor::new(WardView::parse(STAGING).unwrap());
    door.open = false;
    let tools = FakeTools::default();
    let r = tick(&config(&dir, 6, 2), &door, &tools);
    assert!(door.queue.borrow().is_empty());
    assert!(tools.paints.borrow().is_empty() && tools.uploads.borrow().is_empty() && tools.edits.borrow().is_empty());
    assert!(r.lines.iter().any(|l| l.contains("closed")), "{:?}", r.lines);
    assert!(r.errors.is_empty(), "a closed door is a wait, not an error: {:?}", r.errors);
    assert!(!dir.join("factory-ledger.json").exists() || Ledger::load(&dir.join("factory-ledger.json")).unwrap().sent.is_empty());
}

#[test]
fn the_faces_of_one_patient_are_completed_per_tick_and_known_ones_are_not_made_again() {
    let dir = world("faces");
    let pool = read_pool(POOL).unwrap();
    let mut man = seed_manifest(&dir, &pool);
    // Ploy (THA-0) already has her full set on file; Budi (IDN-1) and Priya (IND-0) have only a base.
    let ploy = pool.iter().find(|p| p.key == "THA-0").unwrap();
    for st in ["improving", "deteriorating", "critical", "arrest", "recovered"] {
        man.record_state("THA-0", st, &sha_url(format!("THA-0/{st}").as_bytes()));
    }
    man.save(&dir.join("portraits.json")).unwrap();
    let base_of = |key: &str| man.entries[key].portrait["stable"].clone();

    let mut ward = WardView::parse(STAGING).unwrap();
    let put = |p: &mut vitals_factory::door::BoardPatient, who: &Person, case: &str, age: u16, stable: String| {
        p.name = Some(who.name.clone());
        p.country = Some(who.country.clone());
        p.case = Some(case.into());
        p.age = Some(age);
        p.portrait = Some(stable.clone());
        p.portraits = BTreeMap::from([("stable".to_string(), stable)]);
    };
    let budi = pool.iter().find(|p| p.key == "IDN-1").unwrap();
    let priya = pool.iter().find(|p| p.key == "IND-0").unwrap();
    put(&mut ward.patients[0], ploy, "world-cholecystitis-woman", 50, base_of("THA-0"));
    put(&mut ward.patients[1], budi, "world-stroke-man", 63, base_of("IDN-1"));
    put(&mut ward.patients[2], priya, "world-migraine-woman", 25, base_of("IND-0"));
    let door = FakeDoor::new(ward);
    let tools = FakeTools::default();

    let r = tick(&config(&dir, 0, 0), &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    // Ploy: filled from the manifest, nothing made.
    let fills = door.fills.borrow();
    let ploy_fill = fills.iter().find(|(id, _)| *id == 1789488342).expect("Ploy's set was pushed");
    assert_eq!(ploy_fill.1.len(), 6, "five states from the file, and the sibling of the stable the board shows");
    assert_eq!(ploy_fill.1["critical"], sha_url(b"THA-0/critical"));
    // One of the other two: five states made from her base, uploaded, recorded, pushed.
    let edits = tools.edits.borrow();
    assert_eq!(edits.len(), 5, "five states for one patient, not ten: {edits:?}");
    assert!(edits.iter().any(|e| e.contains("lying completely still")) && edits.iter().all(|e| e.contains("same person")));
    assert!(edits.iter().all(|e| !e.contains("dead")), "no picture of a dead patient is made");
    assert_eq!(tools.uploads.borrow().len(), 13, "five states and their five siblings, and the siblings of the three stables the board shows");
    let made_for: Vec<u64> = fills.iter().filter(|(id, _)| *id != 1789488342 && fills.iter().any(|(i2, set)| i2 == id && set.keys().any(|k| !k.ends_with("_256")))).map(|(id, _)| *id).collect();
    assert_eq!(made_for.len(), 1, "one patient per tick: {made_for:?}");
    let man2 = Manifest::load(&dir.join("portraits.json")).unwrap();
    let done = if made_for[0] == 1789490620 { "IDN-1" } else { "IND-0" };
    assert_eq!(man2.entries[done].portrait.len(), 6, "recorded under her key");
    let waiting = if done == "IDN-1" { "IND-0" } else { "IDN-1" };
    assert_eq!(man2.entries[waiting].portrait.len(), 1, "the other waits for the next tick");
    assert!(r.lines.iter().any(|l| l.contains("added")), "{:?}", r.lines);
    assert!(tools.fetches.borrow().iter().all(|u| !u.ends_with("-256.webp")), "only full pictures are ever fetched: {:?}", tools.fetches.borrow());
    drop(fills);
    drop(edits);

    // Next tick: the other patient's turn, and the first is not made again.
    let r = tick(&config(&dir, 0, 0), &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(tools.edits.borrow().len(), 10);
    let man3 = Manifest::load(&dir.join("portraits.json")).unwrap();
    assert_eq!(man3.entries["IDN-1"].portrait.len(), 6);
    assert_eq!(man3.entries["IND-0"].portrait.len(), 6);
    // And a third: nothing left to make, nothing pushed twice.
    let before = door.fills.borrow().len();
    let r = tick(&config(&dir, 0, 0), &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(tools.edits.borrow().len(), 10);
    assert_eq!(door.fills.borrow().len(), before, "every set is complete on the board");
}

#[test]
fn a_dry_run_reads_and_plans_and_touches_nothing() {
    let dir = world("dry");
    let pool = read_pool(POOL).unwrap();
    seed_manifest(&dir, &pool);
    let door = FakeDoor::new(WardView::parse(STAGING).unwrap());
    let tools = FakeTools::default();
    let cfg = Config { dry_run: true, ..config(&dir, 6, 2) };
    let r = tick(&cfg, &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(door.queue.borrow().is_empty(), "nothing pushed");
    assert!(door.pushes.borrow().is_empty(), "not even a probe");
    assert_eq!(*tools.token_fetches.borrow(), 0, "no token is fetched for a dry run");
    assert!(tools.paints.borrow().is_empty() && tools.uploads.borrow().is_empty());
    assert!(!dir.join("factory-ledger.json").exists());
    let text = r.lines.join("\n");
    assert!(text.contains("would"), "{text}");
    assert!(text.contains("world-"), "names the cases it would build: {text}");
    assert!(text.contains("dry run"), "{text}");
}

/// Founder, 16 Sep 2026: "ควรมีคนไข้จากทั่วโลกนะ". A dry run also prints the next twenty draws, each
/// with its country and its region, and one line on the spread — so the founder can be shown,
/// from any state of the ward, what the queue is about to look like. Twenty regardless of the
/// shortfall (here the queue wants six), and the spread rules hold: no country more than twice,
/// twelve countries, six regions.
#[test]
fn a_dry_run_prints_the_next_twenty_draws_with_country_and_region() {
    use std::collections::BTreeSet;
    use vitals_factory::plan::{MIN_COUNTRIES, MIN_REGIONS, QUEUE_CAP, QUEUE_WINDOW};
    use vitals_factory::region::{region_of, ALL};
    let dir = world("dry-twenty");
    let pool = read_pool(POOL).unwrap();
    seed_manifest(&dir, &pool);
    let door = FakeDoor::new(WardView::parse(STAGING).unwrap());
    let tools = FakeTools::default();
    let cfg = Config { dry_run: true, ..config(&dir, 6, 2) };
    let r = tick(&cfg, &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let text = r.lines.join("\n");
    let start = r.lines.iter().position(|l| l.starts_with(&format!("next {QUEUE_WINDOW} draws"))).unwrap_or_else(|| panic!("a heading for the next twenty draws:\n{text}"));
    let draws: Vec<&String> = r.lines[start + 1..].iter().take_while(|l| l.starts_with("  ")).collect();
    assert_eq!(draws.len(), QUEUE_WINDOW, "twenty draws under the heading:\n{text}");
    let mut countries: Vec<&str> = Vec::new();
    for (n, line) in draws.iter().enumerate() {
        // "  1. ETH · Sub-Saharan Africa · Tigist Alemu"
        let cells: Vec<&str> = line.trim().split(" · ").collect();
        assert!(cells.len() >= 3, "{line}");
        let (num, code) = cells[0].split_once(". ").unwrap_or_else(|| panic!("{line}"));
        assert_eq!(num.trim().parse::<usize>().ok(), Some(n + 1), "{line}");
        let region = region_of(code).unwrap_or_else(|| panic!("{code} in {line} has no region"));
        assert_eq!(cells[1], region.name(), "{line}");
        assert!(pool.iter().any(|p| p.country == code && p.name == cells[2]), "{line}: a person of the pool");
        countries.push(code);
    }
    let distinct: BTreeSet<&str> = countries.iter().copied().collect();
    assert!(distinct.len() >= MIN_COUNTRIES, "{countries:?}");
    assert!(distinct.iter().all(|c| countries.iter().filter(|x| x == &c).count() <= QUEUE_CAP), "{countries:?}");
    let regions: BTreeSet<&str> = countries.iter().map(|c| region_of(c).unwrap().name()).collect();
    assert!(regions.len() >= MIN_REGIONS && regions.len() <= ALL.len(), "{regions:?}");
    let spread = r.lines[start + 1 + draws.len()..].iter().find(|l| l.starts_with("spread:")).unwrap_or_else(|| panic!("a spread line after the draws:\n{text}"));
    assert!(spread.contains(&format!("{} countries", distinct.len())) && spread.contains(&format!("{} regions", regions.len())), "{spread}");
    // Still a dry run: nothing pushed, no token, no file.
    assert!(door.pushes.borrow().is_empty() && *tools.token_fetches.borrow() == 0 && !dir.join("factory-ledger.json").exists());
}

// ── the case door ────────────────────────────────────────────────────────────
// `GET /api/ward/cases` lists the cases the ward holds — the real payload is `{cases: [{archetype,
// case_id, country, difficulty, endemic, provisional, title, version, patient}], derivations}` —
// and the queue door refuses a season id. A pack's `case` is a World case_id from that list,
// chosen first; the person second, of the case's sex, at an age inside its window. The pack also
// carries `case_id` (the same id) and `difficulty` for the door that reads them.

/// Every pack built against the ward names a World case from its list, fits that case's patient
/// by sex and age, carries the case again as `case_id` with its level, and the ledger remembers.
#[test]
fn every_pack_built_names_a_world_case_that_fits_and_carries_its_level() {
    let dir = world("cases");
    let pool = read_pool(POOL).unwrap();
    seed_manifest(&dir, &pool);
    let door = FakeDoor::new(WardView::parse(STAGING).unwrap());
    let tools = FakeTools::default();
    let r = tick(&config(&dir, 6, 2), &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let listed = parse_cases(CASES).unwrap();
    let q = door.queue.borrow();
    assert!(!q.is_empty());
    for (id, pack) in q.iter() {
        let w = listed.iter().find(|c| c.case_id == pack.case).unwrap_or_else(|| panic!("{} is not on the ward's list", pack.case));
        assert!(!pack.case.starts_with("osce-"), "a season id");
        let sex = vitals_factory::sex::Sex::parse(&pack.persona.sex).unwrap();
        assert!(fits(w, sex, pack.persona.age), "{} on {} {} {}", pack.case, pack.persona.name, pack.persona.sex, pack.persona.age);
        assert!(w.country.is_none() || w.country.as_deref() == Some(pack.persona.country.as_str()), "{} is another country's case on {}", pack.case, pack.persona.name);
        let (case_id, difficulty) = door.chosen.borrow()[id].clone();
        assert_eq!(case_id.as_deref(), Some(pack.case.as_str()), "case_id is the case");
        assert_eq!(difficulty.as_deref(), Some(w.difficulty.as_str()));
    }
    let ledger = Ledger::load(&dir.join("factory-ledger.json")).unwrap();
    for s in ledger.sent.values() {
        assert_eq!(s.case_id.as_deref(), Some(s.case.as_str()));
        assert!(s.difficulty.is_some());
    }
    let text = r.lines.join("\n");
    assert!(text.contains("case door: 18 cases listed"), "the withdrawn row is not counted: {text}");
    assert!(text.lines().filter(|l| l.starts_with("queued")).all(|l| l.contains(" · student") || l.contains(" · intern") || l.contains(" · resident")), "every queued line names the level:\n{text}");
}

/// The endemic list is the ward's, not a file: a checkout with the pool and the physicians series
/// and nothing else — no endemic.json, no scenarios — is a checkout the factory runs from, and a
/// withdrawn row that reaches the factory is never chosen.
#[test]
fn no_endemic_file_is_needed_and_a_withdrawn_case_is_never_chosen() {
    let dir = world("no-endemic-file");
    let pool = read_pool(POOL).unwrap();
    seed_manifest(&dir, &pool);
    let bare = dir.join("repo");
    std::fs::create_dir_all(bare.join("crates/vitals-web/data")).unwrap();
    for f in ["personas.json", "physicians.json"] {
        std::fs::copy(repo_root().join("crates/vitals-web/data").join(f), bare.join("crates/vitals-web/data").join(f)).unwrap();
    }
    assert!(!bare.join("crates/vitals-web/data/endemic.json").exists());
    let door = FakeDoor::new(WardView::parse(STAGING).unwrap());
    let tools = FakeTools::default();
    let cfg = Config { repo: bare, ..config(&dir, 20, 20) };
    let r = tick(&cfg, &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(!r.lines.iter().any(|l| l.contains("endemic list")), "{:?}", r.lines);
    let q = door.queue.borrow();
    assert!(q.len() >= 10, "{}", q.len());
    assert!(q.values().all(|p| p.case != "world-withdrawn-angina"), "a withdrawn row, never chosen");
    let listed = parse_cases(CASES).unwrap();
    for p in q.values() {
        let w = listed.iter().find(|c| c.case_id == p.case).unwrap();
        assert_eq!(p.endemic, w.endemic && w.country.as_deref() == Some(p.persona.country.as_str()), "{}: the tag is the list's", p.case);
    }
    assert!(r.lines.iter().any(|l| l.contains("case door: 18 cases listed") && l.contains("countries with an endemic list")), "{:?}", r.lines);
}

/// A ward that lists no cases: nothing is built, nothing season-shaped is sent, and the tick says
/// so as an error — a factory that cannot read the case door cannot build a pack the door takes.
#[test]
fn a_ward_that_lists_no_cases_builds_nothing_and_says_so() {
    let dir = world("no-cases");
    let pool = read_pool(POOL).unwrap();
    seed_manifest(&dir, &pool);
    let door = FakeDoor::no_cases(WardView::parse(STAGING).unwrap());
    let tools = FakeTools::default();
    let r = tick(&config(&dir, 4, 2), &door, &tools);
    assert!(door.queue.borrow().is_empty(), "nothing built");
    assert!(r.errors.iter().any(|e| e.contains("no cases")), "{:?}", r.errors);
    assert!(tools.paints.borrow().is_empty(), "no face painted for a pack that cannot go");
}

/// A pack waiting in the ledger whose case the ward does not list — a season id from before
/// 0543ed7 — is already queued at the ward, and the ward's ticker places such a pack by the
/// patient's country and then by anything it holds, so it will be admitted with a World case as
/// beds free. It is left exactly as it is: not re-sent, not refused, not dropped; its face stays
/// reserved; one line per tick says how many are placed by the ward. The re-send path stays for
/// packs whose case the ward does list.
#[test]
fn a_waiting_pack_whose_case_the_ward_does_not_list_is_left_to_the_ward() {
    let dir = world("placed-by-ward");
    let pool = read_pool(POOL).unwrap();
    let man = seed_manifest(&dir, &pool);
    let anan = pool.iter().find(|p| p.key == "THA-1").unwrap();
    let ploy = pool.iter().find(|p| p.key == "THA-0").unwrap();
    let cfg = config(&dir, 3, 2);
    let mut ledger = Ledger::default();
    // One season-id pack, and one World-case pack the ward's queue has lost: only the second is re-sent.
    let old = vitals_factory::ledger::Sent::new("osce-a", anan, 70, false, Some(man.entries["THA-1"].portrait["stable"].clone()), cfg.now - 600, &cfg.ward);
    let old_id = pack_id(&old.to_pack());
    let mut listed = vitals_factory::ledger::Sent::new("world-copd-woman", ploy, 66, false, Some(man.entries["THA-0"].portrait["stable"].clone()), cfg.now - 500, &cfg.ward);
    listed.case_id = Some("world-copd-woman".into());
    listed.difficulty = Some("student".into());
    let listed_id = pack_id(&listed.to_pack());
    ledger.sent.insert(old_id.clone(), old);
    ledger.sent.insert(listed_id.clone(), listed);
    ledger.save(&dir.join("factory-ledger.json")).unwrap();
    let door = FakeDoor::new(WardView::parse(STAGING).unwrap());
    let tools = FakeTools::default();
    let r = tick(&cfg, &door, &tools);
    assert!(r.errors.is_empty(), "nothing refused, nothing dropped: {:?}", r.errors);
    let after = Ledger::load(&dir.join("factory-ledger.json")).unwrap();
    let kept = after.sent.get(&old_id).expect("left exactly as it was");
    assert!(kept.patient_id.is_none() && !kept.closed && kept.case == "osce-a" && kept.case_id.is_none(), "{kept:?}");
    assert!(!door.queue.borrow().contains_key(&old_id), "not re-sent");
    assert!(door.queue.borrow().contains_key(&listed_id), "the World-case pack was re-sent and put back");
    assert!(!after.sent.values().any(|s| s.key == "THA-1" && s.case != "osce-a"), "Anan's face stays reserved: he is not drawn again");
    assert!(door.queue.borrow().values().all(|p| p.persona.name != anan.name));
    let text = r.lines.join("\n");
    assert!(text.contains("1 waiting pack(s)") && text.contains("placed by the ward") && text.contains("osce-a"), "{text}");
    assert!(text.contains("resent 1 unseen pack(s)"), "the re-send counts the listed one only:\n{text}");
    // The queue is topped up with World packs around them.
    assert_eq!(after.sent.len(), 2 + 2, "two kept, two built to reach depth three with one put back: {:?}", after.sent.values().map(|s| s.case.clone()).collect::<Vec<_>>());
    assert!(after.sent.values().filter(|s| s.case != "osce-a").all(|s| s.case.starts_with("world-")));
}

/// Never the same case on the board twice: a case in a bed (`patients[].case`) is chosen for no
/// pack while it is there; the bed's `endemic` is about the draw, not the case, and is not read.
#[test]
fn a_case_in_a_bed_is_not_chosen_for_a_pack() {
    let dir = world("bed-case");
    let pool = read_pool(POOL).unwrap();
    seed_manifest(&dir, &pool);
    let mut ward = WardView::parse(STAGING).unwrap();
    ward.patients[0].case = Some("world-copd-woman".into());
    ward.patients[1].case = Some("world-cholecystitis-woman".into());
    ward.patients[1].endemic = true;
    ward.patients[2].case = Some("world-hip-fracture-woman".into());
    let door = FakeDoor::new(ward);
    let tools = FakeTools::default();
    let r = tick(&config(&dir, 12, 3), &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(door.queue.borrow().len() >= 6, "{}", door.queue.borrow().len());
    for pack in door.queue.borrow().values() {
        assert!(!["world-copd-woman", "world-cholecystitis-woman", "world-hip-fracture-woman"].contains(&pack.case.as_str()), "{} is in a bed, chosen for {}", pack.case, pack.persona.name);
    }
}

/// The dry run prints the case chosen for every draw it lists, in the build list and in the
/// twenty ahead, so the founder sees the case beside the country — and never "no case".
#[test]
fn a_dry_run_prints_the_case_chosen_for_each_draw() {
    let dir = world("dry-cases");
    let pool = read_pool(POOL).unwrap();
    seed_manifest(&dir, &pool);
    let door = FakeDoor::new(WardView::parse(STAGING).unwrap());
    let tools = FakeTools::default();
    let cfg = Config { dry_run: true, ..config(&dir, 6, 2) };
    let r = tick(&cfg, &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let listed = parse_cases(CASES).unwrap();
    let start = r.lines.iter().position(|l| l.starts_with("next 20 draws")).unwrap();
    let draws: Vec<&String> = r.lines[start + 1..].iter().take_while(|l| l.starts_with("  ")).collect();
    assert_eq!(draws.len(), 20);
    for line in &draws {
        // "  1. NER · Sub-Saharan Africa · Hadiza Moussa · case_id world-copd-woman (student)"
        let cells: Vec<&str> = line.trim().split(" · ").collect();
        assert_eq!(cells.len(), 4, "{line}");
        let (word, rest) = cells[3].split_once(' ').unwrap_or_else(|| panic!("{line}"));
        assert_eq!(word, "case_id", "{line}");
        let (case_id, difficulty) = rest.split_once(" (").unwrap_or_else(|| panic!("{line}"));
        let w = listed.iter().find(|c| c.case_id == case_id).unwrap_or_else(|| panic!("{case_id} in {line} is not on the list"));
        assert_eq!(w.difficulty, difficulty.trim_end_matches(')'), "{line}");
    }
    let build: Vec<&String> = r.lines.iter().filter(|l| l.starts_with("  world-")).collect();
    assert!(!build.is_empty(), "the build list names World cases:\n{}", r.lines.join("\n"));
    assert!(door.pushes.borrow().is_empty() && !dir.join("factory-ledger.json").exists(), "still a dry run");
}

/// Every face is judged before it is recorded or uploaded; the question is the one the brief
/// wrote, asked of the text model on Vertex with the image inline; a "no" is a new seed, up to
/// three; and a person whose three faces all failed gets no pack this tick — a pack is never built
/// on a rejected face.
#[test]
fn a_face_is_a_photograph_or_it_is_not_a_face() {
    let dir = world("gate");
    let pool = read_pool(POOL).unwrap();
    seed_manifest(&dir, &pool);
    let door = FakeDoor::new(WardView::parse(STAGING).unwrap());
    let tools = FakeTools::default();
    // The first face: no, no, yes. The second face: no, no, no. The cap counts paintings, and a
    // person started under the cap gets all her tries: with a cap of four, the second person
    // starts at three painted and the third is deferred at six.
    tools.verdicts.borrow_mut().extend([false, false, true, false, false, false]);
    let cfg = config(&dir, 20, 4);
    let r = tick(&cfg, &door, &tools);

    assert_eq!(FACE_ATTEMPTS, 3);
    let seeds = tools.seeds.borrow();
    assert_eq!(seeds.len(), 6, "three tries for each of the two faces, and nobody after them: {seeds:?}");
    assert_eq!(r.faces_tried, 6);
    assert!(seeds[0] != seeds[1] && seeds[1] != seeds[2], "each try is a new seed");
    assert!(seeds[3] != seeds[4] && seeds[4] != seeds[5]);
    let judged_all = tools.judged.borrow();
    let judged: Vec<&String> = judged_all.iter().filter(|j| !j.contains("Does this picture show")).collect();
    assert_eq!(judged.len(), 6, "every painted face was judged, none skipped");
    assert!(judged_all.iter().all(|j| j.starts_with("gemini-2.5-flash|")), "the judge model from the config, not the image model: {}", judged_all[0]);
    // Not the brief's sentence: that one made the model judge provenance and refuse every face we
    // have. This one judges style — see prompts::PHOTOREAL for the calibration.
    assert!(judged.iter().all(|j| j.contains("exactly ONE person") && j.contains("drawing, anime, cartoon, doll or stylised 3D render") && j.contains("Answer yes or no")), "{}", judged[0]);
    // Only the face that passed was uploaded and recorded (with its made stable and both siblings;
    // the packs with faces on file upload their made stables too).
    let man = Manifest::load(&dir.join("portraits.json")).unwrap();
    let made: Vec<(&String, &vitals_factory::manifest::Entry)> = man.entries.iter().filter(|(k, _)| k.contains('@')).collect();
    assert_eq!(made.len(), 1, "the rejected face is nowhere on file");
    let base = made[0].1.portrait.get("base").expect("the face that passed, under base");
    let ups = tools.uploads.borrow();
    assert!(ups.iter().any(|o| base.ends_with(o)), "the face that passed was uploaded");
    assert!(ups.iter().filter(|o| o.len() == 69).count() >= 1);
    drop(ups);
    // The log says so beside each face, and names the person whose face was given up on.
    let text = r.lines.join("\n");
    assert!(text.contains("photorealistic: no") && text.contains("(oversized eyes)"), "the verdict and its why, beside the face: {text}");
    assert!(text.contains("photorealistic: yes"), "{text}");
    assert_eq!(std::fs::read_dir(dir.join("work/refused")).map(|d| d.count()).unwrap_or(0), 5, "every refused face is kept locally for a person to look at");
    assert_eq!(r.errors.len(), 1, "{:?}", r.errors);
    assert!(r.errors[0].contains("three faces") && r.errors[0].contains("no pack"), "{}", r.errors[0]);
    // Her pack was not pushed, and she is not in the ledger.
    let ledger = Ledger::load(&dir.join("factory-ledger.json")).unwrap();
    let q = door.queue.borrow();
    for s in ledger.sent.values() {
        assert!(q.values().any(|p| p.persona.name == s.name), "ledger and queue agree");
    }
    let rejected_name = r.errors[0].clone();
    assert!(!q.values().any(|p| rejected_name.contains(&p.persona.name)), "no pack on a rejected face");
    assert_eq!(r.faces_made, 1);
}

/// A child's face asks for a child, in words the painter is known to need: a photographer's
/// opening and no negatives — the words "anime" and "doll" in a prompt summon what they name.
#[test]
fn a_childs_face_is_asked_for_as_a_photograph_of_a_child() {
    let dir = world("child");
    let pool = read_pool(POOL).unwrap();
    seed_manifest(&dir, &pool);
    let door = FakeDoor::new(WardView::parse(STAGING).unwrap());
    let tools = FakeTools::default();
    let r = tick(&config(&dir, 20, 20), &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let paints = tools.paints.borrow();
    // Adults are "a 57-year-old woman"; children are "a schoolgirl aged 8".
    let age_in = |p: &str| -> u16 {
        if let Some(rest) = p.split("aged ").nth(1) { return rest.split(' ').next().unwrap().parse().unwrap(); }
        p.split("-year-old").next().unwrap().rsplit(' ').next().unwrap().parse().unwrap()
    };
    let ages: Vec<u16> = paints.iter().map(|(p, _)| age_in(p)).collect();
    assert!(ages.iter().any(|a| *a < 16) && ages.iter().any(|a| *a >= 16), "the draw made both a child and an adult: {ages:?}");
    for (prompt, _) in paints.iter() {
        let age = age_in(prompt);
        let child_words = prompt.starts_with("Documentary photograph, 35mm film") && prompt.contains(&format!("aged {age} from"));
        assert_eq!(child_words, age < 16, "{age}: {prompt}");
        if age < 16 {
            let word = if age < 6 { "little" } else if age < 10 { "school" } else if age < 13 { "adolescent" } else { "teenage" };
            assert!(prompt.contains(word), "{age}: a {word} child: {prompt}");
        }
        assert!(!prompt.contains("anime") && !prompt.contains("doll") && !prompt.contains("not a"), "no negatives, ever: {prompt}");
        assert!(prompt.contains("no text, no logos, no flags"), "{prompt}");
    }
}

/// One face, remade on request through the same gate — for a face already on file that a person
/// looked at and refused. The old picture leaves the manifest; the new sha is what is printed.
#[test]
fn a_face_can_be_remade_through_the_gate_and_the_old_one_leaves_the_file() {
    let dir = world("remake");
    let pool = read_pool(POOL).unwrap();
    let mut man = seed_manifest(&dir, &pool);
    let kor0 = pool.iter().find(|p| p.key == "KOR-0").unwrap();
    man.record_base("KOR-0", 8, &sha_url(b"a doll"), kor0);
    man.record_state("KOR-0@8", "critical", &sha_url(b"a doll, worse"));
    man.save(&dir.join("portraits.json")).unwrap();
    let tools = FakeTools::default();
    tools.verdicts.borrow_mut().extend([false, true]);
    let cfg = config(&dir, 20, 2);

    let (url, r) = remake_face(&cfg, &tools, "KOR-0@8").expect("remade");
    assert!(url.starts_with(PORTRAITS) && url.ends_with(".webp") && url != sha_url(b"a doll"));
    assert_eq!(tools.seeds.borrow().len(), 2, "one refusal, one pass");
    assert!(tools.seeds.borrow()[0] != tools.seeds.borrow()[1]);
    let man2 = Manifest::load(&dir.join("portraits.json")).unwrap();
    let e = &man2.entries["KOR-0@8"];
    assert_eq!(e.portrait.get("base"), Some(&url), "the remade face is the reference, under base");
    assert!(!e.portrait.contains_key("stable"), "her stable is made when a pack needs it, from this base");
    assert!(!e.portrait.contains_key("critical"), "states edited from the refused face go with it");
    assert_eq!(e.age, Some(8));
    assert!(r.lines.iter().any(|l| l.contains("photorealistic: no")) && r.lines.iter().any(|l| l.contains("photorealistic: yes")), "{:?}", r.lines);
    assert!(!r.lines.iter().any(|l| l.contains("natural child proportions")), "the prompt itself is not logged");
    assert_eq!(tools.uploads.borrow().len(), 2, "the face and its sibling");

    assert!(remake_face(&cfg, &tools, "XXX-9@40").is_err(), "nobody by that key");
    assert!(remake_face(&cfg, &tools, "KOR-0").is_err(), "a face has an age");
    // Three refusals: the error names her, and the verdicts come back with it.
    tools.verdicts.borrow_mut().extend([false, false, false]);
    let (e, rep) = *remake_face(&cfg, &tools, "KOR-0@8").expect_err("three refusals");
    assert!(e.contains("Park Ji-woo") && e.contains("three faces"), "{e}");
    assert_eq!(rep.lines.iter().filter(|l| l.contains("photorealistic: no")).count(), 3);
}

/// A face remade after her pack was queued reaches the queue: the ledger sees that the made stable
/// for her key and age is no longer the one it sent, replaces it through the pack door, and
/// records the new address. Once per face; and never for a patient already in a bed, whose faces
/// are added through her own door and never replaced.
#[test]
fn a_remade_face_replaces_the_one_on_her_waiting_pack_once() {
    let dir = world("replace");
    let pool = read_pool(POOL).unwrap();
    let mut man = seed_manifest(&dir, &pool);
    let kor0 = pool.iter().find(|p| p.key == "KOR-0").unwrap();
    let doll = sha_url(b"a doll");
    man.record_base("KOR-0", 8, &doll, kor0);
    man.save(&dir.join("portraits.json")).unwrap();
    let door = FakeDoor::new(WardView::parse(STAGING).unwrap());
    let tools = FakeTools::default();
    let cfg = config(&dir, 0, 0);

    // Her pack went out with the doll as its stable, as packs were sent before the rule.
    let mut sent = vitals_factory::ledger::Sent::new("world-asthma-child", kor0, 8, false, Some(doll.clone()), cfg.now, &cfg.ward);
    sent.sex = "f".into();
    let pack = sent.to_pack();
    let id = pack_id(&pack);
    door.push(&Token::new("t".into()), std::slice::from_ref(&Outbound::plain(pack.clone()))).unwrap();
    let mut ledger = Ledger::default();
    ledger.sent.insert(id.clone(), sent);
    ledger.save(&dir.join("factory-ledger.json")).unwrap();

    // First tick: a stable is made from the doll base and replaces the doll on the waiting pack.
    let r = tick(&cfg, &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let made_from_doll = Manifest::load(&dir.join("portraits.json")).unwrap().entries["KOR-0@8"].made_stable().cloned().expect("a stable was made");
    assert_eq!(door.replaces.borrow().len(), 1);
    assert_eq!(door.queue.borrow()[&id].portrait["stable"], made_from_doll);
    assert_eq!(tools.edits.borrow().len(), 1);
    let r = tick(&cfg, &door, &tools);
    assert!(r.errors.is_empty());
    assert_eq!(door.replaces.borrow().len(), 1, "once: the manifest and the ledger agree");
    assert_eq!(tools.edits.borrow().len(), 1);

    // The base is remade (a person refused the doll): the next tick makes a stable from the new
    // base and replaces it on the waiting pack, once.
    let mut man2 = Manifest::load(&dir.join("portraits.json")).unwrap();
    man2.entries.remove("KOR-0@8");
    man2.record_base("KOR-0", 8, &sha_url(b"a child, photographed"), kor0);
    man2.save(&dir.join("portraits.json")).unwrap();
    let r = tick(&cfg, &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let made_from_child = Manifest::load(&dir.join("portraits.json")).unwrap().entries["KOR-0@8"].made_stable().cloned().expect("a new stable");
    assert_ne!(made_from_child, made_from_doll);
    {
        let reps = door.replaces.borrow();
        assert_eq!(reps.len(), 2);
        assert_eq!(reps[1].0, id);
        assert_eq!(reps[1].1.get("stable"), Some(&made_from_child));
        assert_eq!(reps[1].1.get("stable_256").map(String::as_str), Some(sibling(&made_from_child).as_str()));
    }
    assert_eq!(door.queue.borrow()[&id].portrait["stable"], made_from_child, "the waiting pack carries the new face's stable");
    assert_eq!(Ledger::load(&dir.join("factory-ledger.json")).unwrap().sent[&id].stable.as_deref(), Some(made_from_child.as_str()));
    assert!(r.lines.iter().any(|l| l.contains("replaced") && l.contains("Park Ji-woo")), "{:?}", r.lines);
    let r = tick(&cfg, &door, &tools);
    assert_eq!(door.replaces.borrow().len(), 2, "once");
    assert!(r.errors.is_empty());

    // She is admitted (the pack left the queue): the door refuses, the ledger keeps what it has,
    // and the line says why.
    let mut man3 = Manifest::load(&dir.join("portraits.json")).unwrap();
    man3.entries.remove("KOR-0@8");
    man3.record_base("KOR-0", 8, &sha_url(b"a third face"), kor0);
    man3.save(&dir.join("portraits.json")).unwrap();
    door.queue.borrow_mut().remove(&id);
    let r = tick(&cfg, &door, &tools);
    assert_eq!(door.replaces.borrow().len(), 3);
    assert!(r.lines.iter().any(|l| l.contains("in a bed already") || l.contains("never replaced")), "{:?}", r.lines);
    assert_eq!(Ledger::load(&dir.join("factory-ledger.json")).unwrap().sent[&id].stable.as_deref(), Some(made_from_child.as_str()), "not recorded as replaced");
}

/// A child's face is also asked how old it looks, and only a face inside the door's band for the
/// drawn age passes — the painter renders "eight" as four unless told otherwise, and the door's
/// band at eight is 6–10. A refusal on age is a new seed against the same three tries. Adults are
/// never asked.
#[test]
fn a_childs_face_must_look_her_age_and_an_adults_is_not_asked() {
    let dir = world("age");
    let pool = read_pool(POOL).unwrap();
    seed_manifest(&dir, &pool);
    let tools = FakeTools::default();
    let cfg = config(&dir, 20, 2);

    tools.ages.borrow_mut().extend(["4".to_string(), "She looks about 3.".to_string(), "7".to_string()]);
    let (url, r) = remake_face(&cfg, &tools, "KOR-0@8").expect("the third face looks her age");
    assert!(url.ends_with(".webp"));
    assert_eq!(tools.seeds.borrow().len(), 3, "two refused on age, the third passed");
    assert_eq!(tools.judged.borrow().len(), 3, "the style question first, every time");
    assert_eq!(tools.asked.borrow().len(), 3, "then the age question, for a child");
    assert!(tools.asked.borrow().iter().all(|a| a.ends_with("|About how old does this child look? Answer with one number.")), "{:?}", tools.asked.borrow());
    let text = r.lines.join("\n");
    assert!(text.contains("looks 4 (band 6\u{2013}10) — a new seed"), "{text}");
    assert!(text.contains("looks 3 (band 6\u{2013}10) — a new seed"), "a number inside a sentence is still a number: {text}");
    assert!(text.contains("looks 7 (band 6\u{2013}10)"), "{text}");
    assert_eq!(tools.uploads.borrow().len(), 2, "the face and its sibling");

    // Three faces that look wrong: given up, and nothing uploaded.
    tools.ages.borrow_mut().extend(["3".to_string(), "12".to_string(), "4".to_string()]);
    let (e, rep) = *remake_face(&cfg, &tools, "VNM-0@8").expect_err("three refusals on age");
    assert!(e.contains("Nguyen Thi Lan") && e.contains("three faces"), "{e}");
    assert_eq!(rep.lines.iter().filter(|l| l.contains("— a new seed")).count(), 3);
    assert_eq!(tools.uploads.borrow().len(), 2, "nothing more uploaded");

    // An adult: the style question only.
    let asked_before = tools.asked.borrow().len();
    remake_face(&cfg, &tools, "PAK-0@57").expect("an adult passes on style alone");
    assert_eq!(tools.asked.borrow().len(), asked_before, "adults are not asked their age");
}

/// The first real tick, 17 Sep 2026 11:00–11:06, SSD-3 at 12, prompt "a schoolboy aged 12": the
/// judge's three verdicts, verbatim from the log. Two faces were photographs of a child who looked
/// five (band 10–14); one was refused on style. The refused pictures are in work/refused — a boy
/// of about five to seven in every one. "Schoolboy" pins the look at primary-school age whatever
/// the number after it says, as "an 8-year-old girl" pinned it at a toddler on 16 Sep.
const KUOL_VERDICTS: [(bool, &str, Option<&str>); 3] = [
    (true, "this is a photograph-style picture of exactly one person with natural human proportions, natural skin, and natural eyes. The image depicts a child lying in a hospital bed, and the rendering style is realistic.", Some("5")),
    (true, "this is a photograph-style picture of exactly one person with natural human proportions, natural skin, and natural eyes. The image depicts a single child in what appears to be a hospital bed, with realistic features and lighting.", Some("5")),
    (false, "The image is AI-generated and has an unnatural, overly smooth skin texture and a slightly doll-like quality to the eyes, which deviates from natural human proportions and appearance.", None),
];

/// A twelve-year-old is asked for as a young adolescent, not a schoolboy — the recorded verdicts
/// are the reason — and when three faces fail the gate the sentence says what the verdicts said:
/// how many looked the wrong age and at what, and how many were not photographs of a person.
#[test]
fn a_twelve_year_old_is_asked_for_as_a_young_adolescent_and_a_refusal_says_what_the_judge_said() {
    use vitals_factory::prompts::{base, child_phrase};
    use vitals_factory::sex::Sex;
    // The brackets, from the calibration points: 6 and 8 as schoolchildren came out 6 and 7; 12
    // as a schoolboy came out 5, 5 and 5-ish. Ten to twelve is adolescence's edge, and the word
    // for it is the cue the painter needs.
    assert_eq!(child_phrase(12, Sex::M), "a young adolescent boy aged 12");
    assert_eq!(child_phrase(10, Sex::F), "a young adolescent girl aged 10");
    assert_eq!(child_phrase(8, Sex::F), "a schoolgirl aged 8", "the 16 Sep calibration stands");
    assert_eq!(child_phrase(6, Sex::F), "a schoolgirl aged 6");
    assert_eq!(child_phrase(9, Sex::M), "a schoolboy aged 9");
    assert_eq!(child_phrase(13, Sex::M), "a teenage boy aged 13");
    assert_eq!(child_phrase(3, Sex::F), "a little girl aged 3");
    assert!(base(12, Sex::M, "South Sudan").contains("a young adolescent boy aged 12 from South Sudan"));
    assert!(!base(12, Sex::M, "South Sudan").contains("schoolboy"));

    // The three verdicts replayed: the sentence names two wrong ages and one style refusal.
    let dir = world("kuol");
    let pool = read_pool(POOL).unwrap();
    seed_manifest(&dir, &pool);
    let tools = FakeTools::default();
    let cfg = config(&dir, 20, 2);
    for (ok, _, looks) in KUOL_VERDICTS {
        tools.verdicts.borrow_mut().push_back(ok);
        if let Some(n) = looks {
            tools.ages.borrow_mut().push_back(n.to_string());
        }
    }
    let (e, rep) = *remake_face(&cfg, &tools, "SSD-3@12").expect_err("three refusals");
    assert!(e.contains("Kuol Mayen") && e.contains("SSD-3 at 12"), "{e}");
    assert!(e.contains("three faces in a row failed the gate"), "{e}");
    assert!(e.contains("2 looked the wrong age (5, 5; band 10\u{2013}14)"), "{e}");
    assert!(e.contains("1 was not a photograph of a person"), "{e}");
    assert!(!e.contains("three faces in a row were not photographs"), "the old sentence blamed style for all three: {e}");
    assert_eq!(tools.seeds.borrow().len(), 3);
    assert_eq!(tools.asked.borrow().len(), 2, "the age question is asked only of a face that passed on style");
    assert!(rep.lines.iter().filter(|l| l.contains("looks 5 (band 10\u{2013}14) — a new seed")).count() == 2, "{:?}", rep.lines);
    assert!(rep.lines.iter().any(|l| l.contains("photorealistic: no — a new seed")), "{:?}", rep.lines);
    assert!(tools.paints.borrow().iter().all(|(p, _)| p.contains("a young adolescent boy aged 12")), "{:?}", tools.paints.borrow());
    assert!(tools.uploads.borrow().is_empty(), "nothing of a refused face is uploaded");
    // Three refusals on style alone say so, in the old words.
    for _ in 0..3 {
        tools.verdicts.borrow_mut().push_back(false);
    }
    let (e, _) = *remake_face(&cfg, &tools, "KOR-0@8").expect_err("three refusals");
    assert!(e.contains("3 were not photographs of a person") && !e.contains("wrong age"), "{e}");
}

/// A face remade after she was admitted stays old on the board (add only), so her states are
/// edited from the face the board shows and pushed — and not recorded under the new face's entry.
#[test]
fn states_for_a_face_the_board_kept_are_pushed_and_not_filed_under_the_new_one() {
    let dir = world("kept");
    let pool = read_pool(POOL).unwrap();
    let mut man = seed_manifest(&dir, &pool);
    let kor0 = pool.iter().find(|p| p.key == "KOR-0").unwrap();
    let on_board = sha_url(b"the face she was admitted with");
    man.record_base("KOR-0", 8, &sha_url(b"the face remade after"), kor0);
    man.save(&dir.join("portraits.json")).unwrap();
    let mut ward = WardView::parse(STAGING).unwrap();
    let p = &mut ward.patients[0];
    p.name = Some(kor0.name.clone()); p.country = Some("KOR".into()); p.case = Some("world-asthma-child".into()); p.age = Some(8);
    p.portrait = Some(on_board.clone()); p.portraits = BTreeMap::from([("stable".to_string(), on_board.clone())]);
    let door = FakeDoor::new(ward);
    let tools = FakeTools::default();
    let r = tick(&config(&dir, 0, 0), &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(tools.fetches.borrow().iter().all(|u| u == &on_board), "edited from the face the board shows: {:?}", tools.fetches.borrow());
    assert_eq!(tools.edits.borrow().len(), 5);
    let fills = door.fills.borrow();
    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].1.len(), 11, "pushed to her patient door, each state with its sibling, and the board's stable's sibling");
    let man2 = Manifest::load(&dir.join("portraits.json")).unwrap();
    assert_eq!(man2.entries["KOR-0@8"].portrait.len(), 1, "the new face's entry holds no states of the old face");
    assert!(r.lines.iter().any(|l| l.contains("pushed, not recorded")), "{:?}", r.lines);
}

/// The editor refusing one state does not lose the others: what was made is pushed, the refusal
/// is an error naming the state, and the board shows the nearest milder picture for the rest.
#[test]
fn a_state_the_editor_refuses_costs_only_that_state() {
    let dir = world("refused-state");
    let pool = read_pool(POOL).unwrap();
    let man = seed_manifest(&dir, &pool);
    let kor0 = pool.iter().find(|p| p.key == "KOR-0").unwrap();
    let stable = man.entries["KOR-0"].portrait["stable"].clone();
    let mut ward = WardView::parse(STAGING).unwrap();
    let p = &mut ward.patients[0];
    p.name = Some(kor0.name.clone()); p.country = Some("KOR".into()); p.case = Some("world-cholecystitis-woman".into()); p.age = Some(28);
    p.portrait = Some(stable.clone()); p.portraits = BTreeMap::from([("stable".to_string(), stable)]);
    let door = FakeDoor::new(ward);
    let tools = FakeTools::default();
    tools.refuse_edits.borrow_mut().extend(["oxygen mask over the nose", "lying completely still"]);
    let r = tick(&config(&dir, 0, 0), &door, &tools);
    assert_eq!(r.errors.len(), 2, "{:?}", r.errors);
    assert!(r.errors.iter().any(|e| e.contains("deteriorating could not be made")) && r.errors.iter().any(|e| e.contains("arrest could not be made")), "{:?}", r.errors);
    assert_eq!(r.states_made, 3, "recovered, improving and critical were made");
    let fills = door.fills.borrow();
    assert_eq!(fills.len(), 1, "and pushed");
    assert_eq!(fills[0].1.len(), 7, "each with its 256 px sibling, and the board's stable gets its sibling too");
    assert!(!fills[0].1.contains_key("deteriorating"));
    let man2 = Manifest::load(&dir.join("portraits.json")).unwrap();
    assert_eq!(man2.entries["KOR-0"].portrait.len(), 4, "the three made are on file with the base; the refused two are not");
}

/// Since the one-feature rule every state is asked for in clinical but gentle words for
/// everyone — a mask, a cannula, eyes closed, the blanket — and never with the colour of skin or
/// lips: the image editor refused the old adult wording of "deteriorating" for a child as
/// prohibited content (16 Sep), and the colour words were also where identity failed on arrest.
#[test]
fn a_childs_worse_states_are_asked_for_gently_and_an_adults_as_before() {
    let dir = world("gentle");
    let pool = read_pool(POOL).unwrap();
    let man = seed_manifest(&dir, &pool);
    let kor0 = pool.iter().find(|p| p.key == "KOR-0").unwrap();
    let mut ward = WardView::parse(STAGING).unwrap();
    let put = |p: &mut vitals_factory::door::BoardPatient, who: &Person, case: &str, age: u16, stable: String| {
        p.name = Some(who.name.clone()); p.country = Some(who.country.clone()); p.case = Some(case.into()); p.age = Some(age);
        p.portrait = Some(stable.clone()); p.portraits = BTreeMap::from([("stable".to_string(), stable)]);
    };
    put(&mut ward.patients[0], kor0, "world-asthma-child", 8, man.entries["KOR-0"].portrait["stable"].clone());
    let door = FakeDoor::new(ward);
    let tools = FakeTools::default();
    let r = tick(&config(&dir, 0, 0), &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let edits = tools.edits.borrow();
    assert_eq!(edits.len(), 5);
    for e in edits.iter() {
        assert!(e.starts_with("Edit this photo, keeping exactly the same person"), "identity first, always: {e}");
        for word in ["grey", "dusky", "ashen", "blue", "sweaty", "cardiac arrest", "critically"] {
            assert!(!e.contains(word), "a state carries no {word}: {e}");
        }
    }
    assert!(edits.iter().any(|e| e.contains("oxygen mask over the nose")) && edits.iter().any(|e| e.contains("lying completely still")), "{edits:?}");
    drop(edits);

    // An adult on the same tick path reads the same features: one text for editor and judge.
    let dir2 = world("gentle-adult");
    let man2 = seed_manifest(&dir2, &pool);
    let pak1 = pool.iter().find(|p| p.key == "PAK-1").unwrap();
    let mut ward2 = WardView::parse(STAGING).unwrap();
    put(&mut ward2.patients[0], pak1, "world-stroke-man", 62, man2.entries["PAK-1"].portrait["stable"].clone());
    let door2 = FakeDoor::new(ward2);
    let tools2 = FakeTools::default();
    let r = tick(&config(&dir2, 0, 0), &door2, &tools2);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let edits = tools2.edits.borrow();
    assert!(edits.iter().any(|e| e.contains("He is now eyes closed, no mask, lying completely still")), "{edits:?}");
    assert!(edits.iter().all(|e| !e.contains("ashen")), "{edits:?}");
}

// ── 256 px ───────────────────────────────────────────────────────────────────
// Every portrait the factory uploads also gets a 256 px sibling — `<sha>-256.webp`, the same sha
// as the full one so the pair is addressable, quality 80 — for the six state keys, and the pack
// carries `<state>_256` beside `<state>`. The door that takes those keys is 7b's to build; until
// it does, the factory sends them, reads the refusal, and sends without — once per tick.

fn sibling(url: &str) -> String {
    url.replace(".webp", "-256.webp")
}

#[test]
fn every_portrait_uploaded_gets_a_256_px_sibling() {
    let dir = world("v256");
    let pool = read_pool(POOL).unwrap();
    seed_manifest(&dir, &pool);
    let door = FakeDoor::new(WardView::parse(STAGING).unwrap());
    let tools = FakeTools::default();
    let r = tick(&config(&dir, 6, 2), &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let ups = tools.uploads.borrow();
    let fulls: Vec<&String> = ups.iter().filter(|o| !o.contains("-256")).collect();
    assert!(!fulls.is_empty());
    for full in &fulls {
        assert!(ups.contains(&format!("{}-256.webp", full.trim_end_matches(".webp"))), "{full} has its sibling");
    }
    assert!(tools.resized.borrow().iter().all(|(q, px)| *q == VARIANT_QUALITY && *px == VARIANT_PX));
    assert_eq!((VARIANT_QUALITY, VARIANT_PX), (80, 256));
    let man = Manifest::load(&dir.join("portraits.json")).unwrap();
    for (k, e) in man.entries.iter().filter(|(k, _)| k.contains('@')) {
        assert_eq!(e.portrait_256.get("stable").map(String::as_str), Some(sibling(&e.portrait["stable"]).as_str()), "{k}: the sibling is on file beside the face");
    }
    // The packs carry both keys where the file has the sibling (the faces made this tick do; the
    // seeded ones get theirs from the backfill), and the sibling's address is the full one's with -256.
    let mut with = 0;
    for p in door.queue.borrow().values() {
        let Some(s) = p.portrait.get("stable") else { continue };
        let on_file = man.entry_with_stable(s).and_then(|(_, e)| e.portrait_256.get("stable").cloned());
        assert_eq!(p.portrait.get("stable_256"), on_file.as_ref(), "{}: stable_256 beside stable exactly when the file has it", p.persona.name);
        if on_file.is_some() { with += 1; assert_eq!(on_file.as_deref(), Some(sibling(s).as_str())); }
    }
    assert!(with >= 1, "at least the faces made this tick carry their sibling");
}

#[test]
fn a_door_that_does_not_take_256_gets_the_pack_without_it_once_per_tick() {
    let dir = world("v256-old-door");
    let pool = read_pool(POOL).unwrap();
    seed_manifest(&dir, &pool);
    let mut door = FakeDoor::new(WardView::parse(STAGING).unwrap());
    door.takes_256 = false;
    let tools = FakeTools::default();
    let r = tick(&config(&dir, 6, 2), &door, &tools);
    assert!(r.errors.is_empty(), "a door that is not there yet is not an error: {:?}", r.errors);
    assert_eq!(r.rejected, 0, "a refusal of the sibling is not a rejected pack");
    let q = door.queue.borrow();
    assert!(q.len() >= 2, "{}", q.len());
    assert!(q.values().all(|p| !p.portrait.keys().any(|k| k.ends_with("_256"))), "sent without the sibling");
    let pushes = door.pushes.borrow();
    assert_eq!(pushes.len(), q.len() + 1 + 1, "every pack once, the empty probe, and exactly one retry — the door's answer is remembered for the tick: {pushes:?}");
    assert_eq!(r.lines.iter().filter(|l| l.contains("does not take 256")).count(), 1, "{:?}", r.lines);
    // The siblings were still made and uploaded: the bucket is ready for the door that takes them.
    assert!(tools.uploads.borrow().iter().any(|o| o.ends_with("-256.webp")));
}

#[test]
fn the_siblings_of_faces_already_on_file_are_made_once_and_carried_to_the_ward() {
    let dir = world("v256-backfill");
    let pool = read_pool(POOL).unwrap();
    let mut man = seed_manifest(&dir, &pool);
    for st in ["improving", "critical"] {
        man.record_state("THA-0", st, &sha_url(format!("THA-0/{st}").as_bytes()));
    }
    man.save(&dir.join("portraits.json")).unwrap();
    let tools = FakeTools::default();
    let cfg = config(&dir, 0, 0);

    let r = backfill_variants(&cfg, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(tools.uploads.borrow().len(), pool.len() + 2, "every base and two states, one sibling each");
    assert_eq!(tools.fetches.borrow().len(), pool.len() + 2, "each full picture fetched from the bucket once");
    let man2 = Manifest::load(&dir.join("portraits.json")).unwrap();
    assert_eq!(man2.entries["THA-0"].portrait_256.len(), 3);
    assert_eq!(man2.entries["THA-0"].portrait_256["critical"], sibling(&man2.entries["THA-0"].portrait["critical"]));
    let r = backfill_variants(&cfg, &tools);
    assert!(r.errors.is_empty());
    assert_eq!(tools.uploads.borrow().len(), pool.len() + 2, "a second run makes nothing");

    // On the ward: an admitted patient whose board lacks the siblings gets them through her own
    // door (add only) and a waiting pack through the replace door — when the door takes them.
    let ploy = pool.iter().find(|p| p.key == "THA-0").unwrap();
    let stable = man2.entries["THA-0"].portrait["stable"].clone();
    let mut ward = WardView::parse(STAGING).unwrap();
    let p = &mut ward.patients[0];
    p.name = Some(ploy.name.clone()); p.country = Some("THA".into()); p.case = Some("world-cholecystitis-woman".into()); p.age = Some(50);
    p.portrait = Some(stable.clone());
    p.portraits = BTreeMap::from([("stable".to_string(), stable.clone()), ("improving".to_string(), man2.entries["THA-0"].portrait["improving"].clone()), ("critical".to_string(), man2.entries["THA-0"].portrait["critical"].clone()),
        ("recovered".to_string(), sha_url(b"r")), ("deteriorating".to_string(), sha_url(b"d")), ("arrest".to_string(), sha_url(b"a"))]);
    let door = FakeDoor::new(ward);
    let anan = pool.iter().find(|p| p.key == "THA-1").unwrap();
    let mut sent = vitals_factory::ledger::Sent::new("world-acs-elderly-man", anan, 70, false, Some(man2.entries["THA-1"].portrait["stable"].clone()), cfg.now, &cfg.ward);
    sent.sex = "m".into();
    let pack = sent.to_pack();
    let id = pack_id(&pack);
    door.push(&Token::new("t".into()), std::slice::from_ref(&Outbound::plain(pack.clone()))).unwrap();
    let mut ledger = Ledger::default();
    ledger.sent.insert(id.clone(), sent);
    ledger.save(&dir.join("factory-ledger.json")).unwrap();
    let r = tick(&cfg, &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let fills = door.fills.borrow();
    let ploy_fill = fills.iter().find(|(pid, _)| *pid == 1789488342).expect("Ploy's siblings were pushed");
    assert_eq!(ploy_fill.1.keys().cloned().collect::<Vec<_>>(), vec!["arrest_256", "critical_256", "deteriorating_256", "improving_256", "recovered_256", "stable_256"],
        "the siblings she has on file, and the rest made from the pictures the board shows");
    let man3 = Manifest::load(&dir.join("portraits.json")).unwrap();
    let anan_stable = man3.entries["THA-1"].made_stable().expect("Anan's stable was made for his waiting pack").clone();
    assert_eq!(door.queue.borrow()[&id].portrait.get("stable_256").map(String::as_str), Some(sibling(&anan_stable).as_str()), "Anan's waiting pack carries his made stable's sibling");
    assert!(Ledger::load(&dir.join("factory-ledger.json")).unwrap().sent[&id].variants_sent, "and the ledger says so, so it is not sent again");
}

/// Since 4920a43 the board's `portrait` is the 256 px sibling when one exists. The face the other
/// states are edited from must be the full-size one: from `portraits.stable`, or the manifest's
/// stable — never the thumbnail, which would make every state after it softer than the first.
#[test]
fn the_states_are_edited_from_the_full_face_never_the_thumbnail_the_board_shows() {
    let dir = world("thumb");
    let pool = read_pool(POOL).unwrap();
    let man = seed_manifest(&dir, &pool);
    let anan = pool.iter().find(|p| p.key == "THA-1").unwrap();
    let full = man.entries["THA-1"].portrait["stable"].clone();
    let small = sibling(&full);
    let mut ward = WardView::parse(STAGING).unwrap();
    let p = &mut ward.patients[0];
    p.name = Some(anan.name.clone()); p.country = Some("THA".into()); p.case = Some("world-acs-elderly-man".into()); p.age = Some(70);
    p.portrait = Some(small.clone());
    p.portraits = BTreeMap::from([("stable".to_string(), full.clone()), ("stable_256".to_string(), small.clone())]);
    let door = FakeDoor::new(ward);
    let tools = FakeTools::default();
    let r = tick(&config(&dir, 0, 0), &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(tools.fetches.borrow().as_slice(), std::slice::from_ref(&full), "the full face, not the -256");
    assert_eq!(tools.edits.borrow().len(), 5);
    let man2 = Manifest::load(&dir.join("portraits.json")).unwrap();
    assert_eq!(man2.entries["THA-1"].portrait.len(), 6, "recorded under her entry: the reference matched the file");

    // A board that publishes only the thumbnail as `portrait`, with no set: the manifest's full
    // face is the reference, and the thumbnail is never fetched.
    let dir2 = world("thumb-only");
    let man3 = seed_manifest(&dir2, &pool);
    let full2 = man3.entries["THA-2"].portrait["stable"].clone();
    let kanya = pool.iter().find(|p| p.key == "THA-2").unwrap();
    let mut ward2 = WardView::parse(STAGING).unwrap();
    let p = &mut ward2.patients[0];
    p.name = Some(kanya.name.clone()); p.country = Some("THA".into()); p.case = Some("world-sepsis-woman".into()); p.age = Some(58);
    p.portrait = Some(sibling(&full2)); p.portraits = BTreeMap::new();
    let door2 = FakeDoor::new(ward2);
    let tools2 = FakeTools::default();
    let r = tick(&config(&dir2, 0, 0), &door2, &tools2);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(tools2.fetches.borrow().as_slice(), std::slice::from_ref(&full2));
    assert!(tools2.fetches.borrow().iter().all(|u| !u.ends_with("-256.webp")));
}

/// A patient whose pictures are on the board but not on file — states edited from a face the
/// board kept after a remake — still gets her 256 px siblings: made from the full pictures the
/// board shows, uploaded under their sha, and pushed through her own door. No model is called,
/// so this is not the one-patient-per-tick step.
#[test]
fn siblings_are_made_for_pictures_the_board_has_and_the_file_does_not() {
    let dir = world("board-siblings");
    let pool = read_pool(POOL).unwrap();
    let mut man = seed_manifest(&dir, &pool);
    let kor0 = pool.iter().find(|p| p.key == "KOR-0").unwrap();
    man.record_base("KOR-0", 8, &sha_url(b"the face remade after"), kor0);
    man.save(&dir.join("portraits.json")).unwrap();
    let states = ["stable", "recovered", "improving", "deteriorating", "critical", "arrest"];
    let mut ward = WardView::parse(STAGING).unwrap();
    let p = &mut ward.patients[0];
    p.name = Some(kor0.name.clone()); p.country = Some("KOR".into()); p.case = Some("world-asthma-child".into()); p.age = Some(8);
    p.portraits = states.iter().map(|st| (st.to_string(), sha_url(format!("board/{st}").as_bytes()))).collect();
    p.portrait = p.portraits.get("stable").cloned();
    let door = FakeDoor::new(ward);
    let tools = FakeTools::default();
    let r = tick(&config(&dir, 0, 0), &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(tools.edits.borrow().is_empty(), "nothing is edited: every state is already on the board");
    assert_eq!(tools.fetches.borrow().len(), 6, "each full picture fetched once");
    assert_eq!(tools.uploads.borrow().len(), 6, "six siblings uploaded");
    assert!(tools.uploads.borrow().iter().all(|o| o.ends_with("-256.webp")));
    let fills = door.fills.borrow();
    assert_eq!(fills.len(), 1);
    let keys: Vec<&String> = fills[0].1.keys().collect();
    assert_eq!(keys.len(), 6);
    assert!(keys.iter().all(|k| k.ends_with("_256")), "{keys:?}");
    for st in states {
        assert_eq!(fills[0].1[&format!("{st}_256")], sibling(&sha_url(format!("board/{st}").as_bytes())), "{st}: the sibling of the picture the board shows");
    }
    // The manifest is untouched: these pictures are not on file, and the siblings of pictures not on file are not either.
    let man2 = Manifest::load(&dir.join("portraits.json")).unwrap();
    assert!(man2.entries["KOR-0@8"].portrait_256.is_empty());
    // A second tick makes nothing: the board now carries them.
    let r = tick(&config(&dir, 0, 0), &door, &tools);
    assert!(r.errors.is_empty());
    assert_eq!(tools.uploads.borrow().len(), 6);
}

// ── stable is a made state, and the base is only the reference ───────────────
// Founder: รูปควรเป็นรูปที่เห็นเหมือนคนป่วย. A patient admitted vomiting blood does not smile in
// her "stable" picture. So the painted face is the reference — kept on file as `base`, never
// sent — and `stable` is made from it by the same edit path as the other states: the same
// person, in the bed, unwell and tired, eyes open, no smile. The smile belongs to `recovered`.

#[test]
fn stable_is_made_from_the_base_and_the_base_is_never_sent() {
    let dir = world("stable-made");
    let pool = read_pool(POOL).unwrap();
    seed_manifest(&dir, &pool);
    let door = FakeDoor::new(WardView::parse(STAGING).unwrap());
    let tools = FakeTools::default();
    let r = tick(&config(&dir, 3, 1), &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let man = Manifest::load(&dir.join("portraits.json")).unwrap();
    let q = door.queue.borrow();
    assert!(!q.is_empty());
    for p in q.values() {
        let stable = &p.portrait["stable"];
        let (key, e) = man.entry_with_stable(stable).unwrap_or_else(|| panic!("{}: her stable is on file", p.persona.name));
        let base = e.portrait.get("base").unwrap_or_else(|| panic!("{key}: the base is kept under its own key"));
        assert_ne!(base, stable, "{key}: stable is a made picture, not the base");
        assert_eq!(e.portrait_256.get("stable").map(String::as_str), Some(sibling(stable).as_str()), "{key}: with its sibling");
        assert!(!p.portrait.values().any(|v| v == base), "{key}: the base is never sent");
        assert_eq!(p.portrait.get("stable_256").map(String::as_str), Some(sibling(stable).as_str()));
    }
    // The stable edit is the founder's sentence; recovered keeps the smile.
    let edits = tools.edits.borrow();
    let stable_prompts: Vec<&String> = edits.iter().filter(|e| e.contains("not smiling")).collect();
    assert!(!stable_prompts.is_empty(), "stable was made by an edit: {edits:?}");
    for e in &stable_prompts {
        assert!(e.starts_with("Edit this photo, keeping exactly the same person"), "{e}");
        assert!(e.contains("unwell") && e.contains("not smiling") && e.contains("eyes open"), "{e}");
    }
    assert!(edits.iter().all(|e| !e.contains("smil") || e.contains("not smiling") || e.contains("looking well")), "the smile belongs to recovered: {edits:?}");
    // Every stable was judged twice: the same person as the base, and looking the state.
    assert!(tools.paired.borrow().iter().all(|j| j.contains("same person as the reference picture")), "{:?}", tools.paired.borrow());
    assert!(tools.judged.borrow().iter().any(|j| j.contains("Does this picture show a patient who is")), "{:?}", tools.judged.borrow());
}

/// A face the file already holds as a bare base (the seeded sixty, every face made before this
/// rule) gets its stable made the first time a pack needs it; a waiting pack that was sent with
/// the base as its stable has it replaced through the replace door, and the ledger records the
/// new address so it is done once.
#[test]
fn a_waiting_pack_sent_with_the_base_gets_a_made_stable_once() {
    let dir = world("stable-replace");
    let pool = read_pool(POOL).unwrap();
    let man = seed_manifest(&dir, &pool);
    let anan = pool.iter().find(|p| p.key == "THA-1").unwrap();
    let base = man.entries["THA-1"].portrait["stable"].clone();
    let door = FakeDoor::new(WardView::parse(STAGING).unwrap());
    let tools = FakeTools::default();
    let cfg = config(&dir, 0, 0);
    let mut sent = vitals_factory::ledger::Sent::new("world-acs-elderly-man", anan, 70, false, Some(base.clone()), cfg.now, &cfg.ward);
    sent.sex = "m".into();
    let pack = sent.to_pack();
    let id = pack_id(&pack);
    door.push(&Token::new("t".into()), std::slice::from_ref(&Outbound::plain(pack.clone()))).unwrap();
    let mut ledger = Ledger::default();
    ledger.sent.insert(id.clone(), sent);
    ledger.save(&dir.join("factory-ledger.json")).unwrap();

    let r = tick(&cfg, &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let man2 = Manifest::load(&dir.join("portraits.json")).unwrap();
    let e = &man2.entries["THA-1"];
    assert_eq!(e.portrait.get("base"), Some(&base), "the face moves under base");
    let made = e.portrait.get("stable").expect("a stable was made");
    assert_ne!(made, &base);
    assert_eq!(tools.edits.borrow().len(), 1, "one edit: his stable");
    let reps = door.replaces.borrow();
    assert_eq!(reps.len(), 1, "{reps:?}");
    assert_eq!(reps[0].1.get("stable"), Some(made));
    assert_eq!(reps[0].1.get("stable_256").map(String::as_str), Some(sibling(made).as_str()));
    assert_eq!(door.queue.borrow()[&id].portrait["stable"], *made, "the waiting pack carries the made stable");
    assert!(!door.queue.borrow()[&id].portrait.values().any(|v| v == &base), "and not the base");
    let l2 = Ledger::load(&dir.join("factory-ledger.json")).unwrap();
    assert_eq!(l2.sent[&id].stable.as_deref(), Some(made.as_str()));
    drop(reps);
    let r = tick(&cfg, &door, &tools);
    assert!(r.errors.is_empty());
    assert_eq!(door.replaces.borrow().len(), 1, "once");
    assert_eq!(tools.edits.borrow().len(), 1, "not made again");
}

// ── the state gate ───────────────────────────────────────────────────────────
// Two questions per made state, to the same judge: the same person as the reference (both
// pictures inline), and does it show a patient who is <the state's own sentence>. A no is one
// re-edit; a second no leaves that state out — the ladder falls back to the nearest milder one —
// and the tick's "rejected" counts states the judge refused, the ledger recording which.

#[test]
fn every_made_state_is_judged_twice_and_a_refusal_costs_one_re_edit_then_the_state() {
    let dir = world("state-gate");
    let pool = read_pool(POOL).unwrap();
    let man = seed_manifest(&dir, &pool);
    let pak1 = pool.iter().find(|p| p.key == "PAK-1").unwrap();
    let base = man.entries["PAK-1"].portrait["stable"].clone();
    let mut ward = WardView::parse(STAGING).unwrap();
    let p = &mut ward.patients[0];
    p.name = Some(pak1.name.clone()); p.country = Some("PAK".into()); p.case = Some("world-stroke-man".into()); p.age = Some(62);
    p.portrait = Some(base.clone()); p.portraits = BTreeMap::from([("stable".to_string(), base.clone())]);
    let door = FakeDoor::new(ward);
    let tools = FakeTools::default();
    let cfg = config(&dir, 0, 0);
    // Her pack is in the ledger, admitted, so the refusal has somewhere to be recorded.
    let mut sent = vitals_factory::ledger::Sent::new("world-stroke-man", pak1, 62, false, Some(base.clone()), cfg.now, &cfg.ward);
    sent.sex = "m".into();
    sent.patient_id = Some(1789488342);
    let id = pack_id(&sent.to_pack());
    let mut ledger = Ledger::default();
    ledger.sent.insert(id.clone(), sent);
    ledger.save(&dir.join("factory-ledger.json")).unwrap();

    // States are made in ladder order: recovered, improving, deteriorating, critical, arrest.
    // improving: not the same person once, then fine. critical: shows the state? no, twice.
    tools.pair_verdicts.borrow_mut().extend([true, false, true, true, true, true, true]);
    tools.state_verdicts.borrow_mut().extend([true, true, true, false, false, true]);
    let r = tick(&cfg, &door, &tools);
    assert!(r.errors.is_empty(), "a refused state is not an error: {:?}", r.errors);
    let edits = tools.edits.borrow();
    assert_eq!(edits.len(), 7, "five states, one re-edit for improving, one for critical: {}", edits.len());
    let paired = tools.paired.borrow();
    assert_eq!(paired.len(), 7, "every edit judged for identity");
    assert!(paired.iter().all(|j| j.contains("Is this the same person as the reference picture?")));
    let judged = tools.judged.borrow();
    assert_eq!(judged.len(), 6, "the state question is asked only of a picture that is the same person: {judged:?}");
    assert!(judged.iter().any(|j| j.contains("Does this picture show a patient who is") && j.contains("sitting up in the bed")), "{judged:?}");
    let fills = door.fills.borrow();
    assert_eq!(fills.len(), 1);
    let keys: Vec<&String> = fills[0].1.keys().collect();
    assert!(keys.iter().any(|k| *k == "improving"), "improving passed on the re-edit: {keys:?}");
    assert!(!keys.iter().any(|k| *k == "critical"), "critical was refused twice and left out: {keys:?}");
    assert!(keys.iter().any(|k| *k == "arrest") && keys.iter().any(|k| *k == "recovered") && keys.iter().any(|k| *k == "deteriorating"));
    assert_eq!(r.rejected, 1, "rejected counts states the judge refused");
    assert_eq!(r.states_made, 4);
    let text = r.lines.join("\n");
    assert!(text.contains("critical") && text.contains("left out"), "{text}");
    assert!(text.contains("re-edit"), "{text}");
    let l2 = Ledger::load(&dir.join("factory-ledger.json")).unwrap();
    assert!(l2.sent[&id].refused.iter().any(|w| w.starts_with("critical")), "the ledger records which: {:?}", l2.sent[&id].refused);
    let man2 = Manifest::load(&dir.join("portraits.json")).unwrap();
    assert!(!man2.entries["PAK-1"].portrait.contains_key("critical"), "a refused state is not on file");
    assert_eq!(man2.entries["PAK-1"].portrait.len(), 5, "base, recovered, improving, deteriorating, arrest — stable stays the base for an admitted patient");
}

// ── one feature per state, read by the editor and the judge alike ────────────
// The judge reads a sentence literally and the editor renders what it is told, so both read one
// text: prompts::feature(state) is the one observable thing the state shows, the edit prompt is
// KEEP + "She is now <feature>." and the judge's second question carries the same words. No
// interpretive words — "critically ill", "struggling" — anywhere in them. Identity may differ in
// skin colour and tone, which is where arrest changes the face most.
#[test]
fn the_editor_and_the_judge_read_one_feature_per_state() {
    use vitals_factory::sex::Sex;
    use vitals_factory::prompts::{feature, sentence, shows, state_for, KEEP, SAME_PERSON, STATES};
    for st in STATES.iter().chain(["stable"].iter()) {
        let f = feature(st).unwrap_or_else(|| panic!("{st} has a feature"));
        assert_eq!(sentence(st), Some(f), "{st}: the judge's sentence is the feature");
        assert!(shows(st).unwrap().contains(f), "{st}: the second question carries the feature verbatim");
        for (sex, child) in [(Sex::F, false), (Sex::M, false), (Sex::F, true)] {
            let p = state_for(st, sex, child).unwrap();
            assert!(p.starts_with(KEEP), "{st}: identity first");
            assert!(p.ends_with(&format!("is now {f}.")), "{st}: the editor renders the feature in the same words: {p}");
        }
        for word in ["critically ill", "struggling", "ashen", "dusky", "grey", "sweaty"] {
            assert!(!f.contains(word), "{st}: no interpretive or colour words: {f}");
        }
    }
    assert!(feature("improving").unwrap().contains("eyes open"), "{}", feature("improving").unwrap());
    assert!(feature("recovered").unwrap().contains("sitting up") && feature("recovered").unwrap().contains("no oxygen mask"));
    assert!(feature("deteriorating").unwrap().contains("oxygen mask"));
    assert!(feature("arrest").unwrap().contains("no mask") && feature("arrest").unwrap().contains("still"));
    assert!(feature("stable").unwrap().contains("not smiling"), "stable's stays as it is");
    // Recovered is the only face that smiles: improving is better, not well, and the three worse
    // states never smile — the arrest and critical faces of the first patient through the gate kept
    // a faint smile because the features said nothing about the expression.
    for st in ["improving", "deteriorating", "critical", "arrest"] {
        assert!(feature(st).unwrap().ends_with(", not smiling"), "{st}: not smiling, and the editor told so in the same words: {}", feature(st).unwrap());
    }
    assert!(!feature("recovered").unwrap().contains("not smiling"), "the smile is recovered's");
    assert!(SAME_PERSON.contains("skin colour and tone"), "identity may differ in skin colour and tone: {SAME_PERSON}");
    assert_eq!(feature("dead"), None);
}

// ── the daily edit budget ────────────────────────────────────────────────────
// A patient costs about a stable, five states and their re-edits — 7–10 edits — and the project's
// alert is 50 USD a month with Cloud Run inside it. EDITS_PER_DAY (default 40) is counted from the
// ledger across ticks by UTC day; when it is spent the tick still paints and gates bases (Flex is
// local) and defers states to the next tick with a "deferred: budget" line. The tick line and the
// ledger carry the cost, estimated from list price and labelled so.
#[test]
fn the_edit_budget_is_counted_across_ticks_and_states_wait_when_it_is_spent() {
    let dir = world("budget");
    let pool = read_pool(POOL).unwrap();
    let man = seed_manifest(&dir, &pool);
    let pak1 = pool.iter().find(|p| p.key == "PAK-1").unwrap();
    let base = man.entries["PAK-1"].portrait["stable"].clone();
    let mut ward = WardView::parse(STAGING).unwrap();
    let p = &mut ward.patients[0];
    p.name = Some(pak1.name.clone()); p.country = Some("PAK".into()); p.case = Some("world-stroke-man".into()); p.age = Some(62);
    p.portrait = Some(base.clone()); p.portraits = BTreeMap::from([("stable".to_string(), base.clone())]);
    let door = FakeDoor::new(ward);
    let tools = FakeTools::default();
    let mut cfg = config(&dir, 0, 0);
    cfg.edits_per_day = 3;

    // Thirty-eight edits already spent today, by earlier ticks.
    let today = vitals_factory::ledger::utc_day(cfg.now);
    let mut ledger = Ledger::default();
    ledger.spend.entry(today.clone()).or_default().edits = 1;
    ledger.save(&dir.join("factory-ledger.json")).unwrap();

    let r = tick(&cfg, &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(tools.edits.borrow().len(), 2, "one already spent, three allowed: two edits, then the budget");
    assert_eq!(r.edits, 2);
    assert!(r.judge_calls >= 4, "each edit judged twice: {}", r.judge_calls);
    {
        let fills = door.fills.borrow();
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].1.keys().filter(|k| !k.ends_with("_256")).count(), 2, "the two states made were pushed");
    }
    let text = r.lines.join("\n");
    assert!(text.contains("deferred: budget"), "{text}");
    assert!(text.contains("estimated from list price"), "{text}");
    assert!(!text.contains("measured"), "{text}");
    let l2 = Ledger::load(&dir.join("factory-ledger.json")).unwrap();
    assert_eq!(l2.spend[&today].edits, 3, "the ledger counts across ticks");
    assert!(l2.spend[&today].judge_calls >= 4);
    assert!(l2.spend[&today].deferred >= 3, "the states left for tomorrow are counted: {}", l2.spend[&today].deferred);
    assert!((l2.spend[&today].usd - estimate_usd(3, l2.spend[&today].judge_calls)).abs() < 1e-9);
    assert!((estimate_usd(10, 20) - (10.0 * EDIT_USD + 20.0 * JUDGE_USD)).abs() < 1e-9);
    assert_eq!((EDIT_USD, JUDGE_USD), (0.039, 0.0005));

    // The next tick, same day: nothing left, every state deferred, no edit made — but a base is
    // still painted and gated, because Flex is local.
    let r = tick(&Config { queue_depth: 1, bases_per_tick: 1, ..cfg.clone() }, &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(tools.edits.borrow().len(), 2, "no edit on a spent budget");
    assert_eq!(r.edits, 0);
    assert!(r.lines.iter().any(|l| l.contains("deferred: budget")), "{:?}", r.lines);
    assert!(r.lines.iter().any(|l| l.contains("goes out without a picture")), "a new pack whose stable waits for budget goes out without one: {:?}", r.lines);
    assert_eq!(door.queue.borrow().len(), 1, "the pack still went out");

    // Tomorrow the budget is new.
    let r = tick(&Config { now: cfg.now + 86_400, ..cfg.clone() }, &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(tools.edits.borrow().len() > 2, "a new day, new edits");
}
