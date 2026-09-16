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
use vitals_factory::door::{Door, FillReply, Filled, Pushed, Queued, Token, WardView};
use vitals_factory::ledger::Ledger;
use vitals_factory::manifest::{batch_age, Manifest};
use vitals_factory::pool::{read_pool, Person};
use vitals_factory::tick::{backfill_variants, remake_face, tick, Config, FACE_ATTEMPTS, VARIANT_PX, VARIANT_QUALITY};
use vitals_factory::tools::Tools;
use vitals_web::ward::Pack;
use vitals_web::ward_chain::{pack_id, validate_pack, PORTRAITS};

const POOL: &str = include_str!("../../vitals-web/data/personas.json");
const STAGING: &str = include_str!("fixtures/ward-staging-2026-09-16.json");

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
fn seed_manifest(dir: &Path, pool: &[Person]) -> Manifest {
    let mut m = Manifest::default();
    for p in pool {
        m.record_base(&p.key, batch_age(&p.key).unwrap(), &sha_url(p.key.as_bytes()), p);
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
        dry_run: false,
        seed: 11,
        now: 1_789_500_000,
    }
}

/// The door, as the ward runs it: validate, content-address, answer in numbers.
struct FakeDoor {
    ward: RefCell<WardView>,
    open: bool,
    /// Whether this door knows `<state>_256` keys and `<sha>-256.webp` addresses (7b's door does
    /// not yet, 16 Sep); a door that does not refuses a pack whole and a fill entry by entry.
    takes_256: bool,
    queue: RefCell<BTreeMap<String, Pack>>,
    pushes: RefCell<Vec<usize>>,
    fills: RefCell<Vec<(u64, BTreeMap<String, String>)>>,
    replaces: RefCell<Vec<(String, BTreeMap<String, String>)>>,
    tokens_seen: RefCell<Vec<String>>,
}

impl FakeDoor {
    fn new(ward: WardView) -> FakeDoor {
        FakeDoor {
            ward: RefCell::new(ward), open: true, takes_256: true, queue: RefCell::new(BTreeMap::new()),
            pushes: RefCell::new(vec![]), fills: RefCell::new(vec![]), replaces: RefCell::new(vec![]), tokens_seen: RefCell::new(vec![]),
        }
    }
}

impl Door for FakeDoor {
    fn read_ward(&self) -> Result<WardView, String> {
        Ok(self.ward.borrow().clone())
    }
    fn push(&self, token: &Token, packs: &[Pack]) -> Result<Pushed, String> {
        self.tokens_seen.borrow_mut().push(token.bearer());
        self.pushes.borrow_mut().push(packs.len());
        if !self.open {
            return Ok(Pushed::Closed { why: "the ward is not open yet".into() });
        }
        let mut q = self.queue.borrow_mut();
        let mut out = Queued { queued: 0, duplicates: 0, rejected: vec![], depth: 0 };
        for p in packs {
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
            if let Err(why) = validate_pack(&plain) {
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
        let ok = self.verdicts.borrow_mut().pop_front().unwrap_or(true);
        Ok((ok, if ok { "natural proportions".into() } else { "oversized eyes".into() }))
    }
    fn edit(&self, _project: &str, _model: &str, base: &[u8], _mime: &str, prompt: &str) -> Result<Vec<u8>, String> {
        self.edits.borrow_mut().push(prompt.to_string());
        if let Some(refused) = self.refuse_edits.borrow().iter().find(|w| prompt.contains(*w)) {
            return Err(format!("Vertex returned no content (finishReason IMAGE_PROHIBITED_CONTENT) for {refused}"));
        }
        Ok(format!("PNG-EDIT:{prompt}:{}", base.len()).into_bytes())
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
    assert_eq!(tools.uploads.borrow().len(), 2 * paints.len(), "each face and its 256 px sibling");
    for object in tools.uploads.borrow().iter() {
        assert!((object.len() == 69 || object.len() == 73) && object.ends_with(".webp"), "content-addressed, or the sibling of one: {object}");
    }
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
    put(&mut ward.patients[0], ploy, "osce-c2", 50, base_of("THA-0"));
    put(&mut ward.patients[1], budi, "osce-d", 63, base_of("IDN-1"));
    put(&mut ward.patients[2], priya, "osce-b", 25, base_of("IND-0"));
    let door = FakeDoor::new(ward);
    let tools = FakeTools::default();

    let r = tick(&config(&dir, 0, 0), &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    // Ploy: filled from the manifest, nothing made.
    let fills = door.fills.borrow();
    let ploy_fill = fills.iter().find(|(id, _)| *id == 1789488342).expect("Ploy's set was pushed");
    assert_eq!(ploy_fill.1.len(), 5);
    assert_eq!(ploy_fill.1["critical"], sha_url(b"THA-0/critical"));
    // One of the other two: five states made from her base, uploaded, recorded, pushed.
    let edits = tools.edits.borrow();
    assert_eq!(edits.len(), 5, "five states for one patient, not ten: {edits:?}");
    assert!(edits.iter().any(|e| e.contains("cardiac arrest")) && edits.iter().all(|e| e.contains("same person")));
    assert!(edits.iter().all(|e| !e.contains("dead")), "no picture of a dead patient is made");
    assert_eq!(tools.uploads.borrow().len(), 10, "five states and their five siblings");
    let made_for: Vec<u64> = fills.iter().filter(|(id, _)| *id != 1789488342).map(|(id, _)| *id).collect();
    assert_eq!(made_for.len(), 1, "one patient per tick: {made_for:?}");
    let man2 = Manifest::load(&dir.join("portraits.json")).unwrap();
    let done = if made_for[0] == 1789490620 { "IDN-1" } else { "IND-0" };
    assert_eq!(man2.entries[done].portrait.len(), 6, "recorded under her key");
    let waiting = if done == "IDN-1" { "IND-0" } else { "IDN-1" };
    assert_eq!(man2.entries[waiting].portrait.len(), 1, "the other waits for the next tick");
    assert!(r.lines.iter().any(|l| l.contains("added")), "{:?}", r.lines);
    assert_eq!(tools.fetches.borrow().len(), 1, "her base was fetched once");
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
    assert!(text.contains("osce-"), "names the cases it would build: {text}");
    assert!(text.contains("dry run"), "{text}");
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
    let judged = tools.judged.borrow();
    assert_eq!(judged.len(), 6, "every painted face was judged, none skipped");
    assert!(judged.iter().all(|j| j.starts_with("gemini-2.5-flash|")), "the judge model from the config, not the image model: {}", judged[0]);
    // Not the brief's sentence: that one made the model judge provenance and refuse every face we
    // have. This one judges style — see prompts::PHOTOREAL for the calibration.
    assert!(judged.iter().all(|j| j.contains("exactly ONE person") && j.contains("drawing, anime, cartoon, doll or stylised 3D render") && j.contains("Answer yes or no")), "{}", judged[0]);
    // Only the face that passed was uploaded and recorded.
    assert_eq!(tools.uploads.borrow().len(), 2, "one face passed: it and its sibling were uploaded");
    let man = Manifest::load(&dir.join("portraits.json")).unwrap();
    assert_eq!(man.entries.iter().filter(|(k, _)| k.contains('@')).count(), 1, "the rejected face is nowhere on file");
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
            let word = if age < 6 { "little" } else if age < 13 { "school" } else { "teenage" };
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
    assert_eq!(e.portrait.get("stable"), Some(&url));
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

/// A face remade after her pack was queued reaches the queue: the ledger sees that the manifest's
/// face for her key and age is no longer the one it sent, replaces it through the pack door, and
/// records the new address. Once, not every tick; and never for a patient already in a bed, whose
/// faces are added through her own door and never replaced.
#[test]
fn a_remade_face_replaces_the_one_on_her_waiting_pack_once() {
    let dir = world("replace");
    let pool = read_pool(POOL).unwrap();
    let mut man = seed_manifest(&dir, &pool);
    let kor0 = pool.iter().find(|p| p.key == "KOR-0").unwrap();
    let old = sha_url(b"a doll");
    man.record_base("KOR-0", 8, &old, kor0);
    man.save(&dir.join("portraits.json")).unwrap();
    let door = FakeDoor::new(WardView::parse(STAGING).unwrap());
    let tools = FakeTools::default();
    let cfg = config(&dir, 0, 0);

    // Her pack goes out with the doll, by hand, as the ledger would have sent it.
    let mut sent = vitals_factory::ledger::Sent::new("osce-c", kor0, 8, false, Some(old.clone()), cfg.now, &cfg.ward);
    sent.sex = "f".into();
    let pack = sent.to_pack();
    let id = pack_id(&pack);
    door.push(&Token::new("t".into()), std::slice::from_ref(&pack)).unwrap();
    let mut ledger = Ledger::default();
    ledger.sent.insert(id.clone(), sent);
    ledger.save(&dir.join("factory-ledger.json")).unwrap();

    // Nothing to do while the manifest and the ledger agree.
    let r = tick(&cfg, &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(door.replaces.borrow().is_empty(), "the face she was sent with is the face on file");

    // The face is remade; the next tick replaces it on the waiting pack, once.
    let new = sha_url(b"a child, photographed");
    man.record_base("KOR-0", 8, &new, kor0);
    man.save(&dir.join("portraits.json")).unwrap();
    let r = tick(&cfg, &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    {
        let reps = door.replaces.borrow();
        assert_eq!(reps.len(), 1);
        assert_eq!(reps[0].0, id);
        assert_eq!(reps[0].1, BTreeMap::from([("stable".to_string(), new.clone())]));
    }
    assert_eq!(door.queue.borrow()[&id].portrait["stable"], new, "the waiting pack carries the new face");
    assert_eq!(Ledger::load(&dir.join("factory-ledger.json")).unwrap().sent[&id].stable.as_deref(), Some(new.as_str()));
    assert!(r.lines.iter().any(|l| l.contains("replaced") && l.contains("Park Ji-woo")), "{:?}", r.lines);
    let r = tick(&cfg, &door, &tools);
    assert_eq!(door.replaces.borrow().len(), 1, "once");
    assert!(r.errors.is_empty());

    // She is admitted (the pack left the queue): the door refuses, the ledger keeps what it has,
    // and the line says why.
    let newer = sha_url(b"a third face");
    man.record_base("KOR-0", 8, &newer, kor0);
    man.save(&dir.join("portraits.json")).unwrap();
    door.queue.borrow_mut().remove(&id);
    let r = tick(&cfg, &door, &tools);
    assert_eq!(door.replaces.borrow().len(), 2);
    assert!(r.lines.iter().any(|l| l.contains("in a bed already") || l.contains("never replaced")), "{:?}", r.lines);
    assert_eq!(Ledger::load(&dir.join("factory-ledger.json")).unwrap().sent[&id].stable.as_deref(), Some(new.as_str()), "not recorded as replaced");
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
    p.name = Some(kor0.name.clone()); p.country = Some("KOR".into()); p.case = Some("osce-c".into()); p.age = Some(8);
    p.portrait = Some(on_board.clone()); p.portraits = BTreeMap::from([("stable".to_string(), on_board.clone())]);
    let door = FakeDoor::new(ward);
    let tools = FakeTools::default();
    let r = tick(&config(&dir, 0, 0), &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(tools.fetches.borrow().as_slice(), std::slice::from_ref(&on_board), "edited from the face the board shows");
    assert_eq!(tools.edits.borrow().len(), 5);
    let fills = door.fills.borrow();
    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].1.len(), 10, "pushed to her patient door, each state with its sibling");
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
    p.name = Some(kor0.name.clone()); p.country = Some("KOR".into()); p.case = Some("osce-c2".into()); p.age = Some(28);
    p.portrait = Some(stable.clone()); p.portraits = BTreeMap::from([("stable".to_string(), stable)]);
    let door = FakeDoor::new(ward);
    let tools = FakeTools::default();
    tools.refuse_edits.borrow_mut().extend(["deteriorating", "cardiac arrest"]);
    let r = tick(&config(&dir, 0, 0), &door, &tools);
    assert_eq!(r.errors.len(), 2, "{:?}", r.errors);
    assert!(r.errors.iter().any(|e| e.contains("deteriorating could not be made")) && r.errors.iter().any(|e| e.contains("arrest could not be made")), "{:?}", r.errors);
    assert_eq!(r.states_made, 3, "recovered, improving and critical were made");
    let fills = door.fills.borrow();
    assert_eq!(fills.len(), 1, "and pushed");
    assert_eq!(fills[0].1.len(), 6, "each with its 256 px sibling");
    assert!(!fills[0].1.contains_key("deteriorating"));
    let man2 = Manifest::load(&dir.join("portraits.json")).unwrap();
    assert_eq!(man2.entries["KOR-0"].portrait.len(), 4, "the three made are on file with the base; the refused two are not");
}

/// A child's worse states are asked for in clinical but gentle words — a mask, a cannula, eyes
/// closed, the blanket — and never with the colour of skin or lips: the image editor refused the
/// adult wording for a child as prohibited content (16 Sep), and the gentle wording, tried once
/// per state on one child, was drawn. Adults keep the words the founder's first nine sets used.
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
    put(&mut ward.patients[0], kor0, "osce-c", 8, man.entries["KOR-0"].portrait["stable"].clone());
    let door = FakeDoor::new(ward);
    let tools = FakeTools::default();
    let r = tick(&config(&dir, 0, 0), &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let edits = tools.edits.borrow();
    assert_eq!(edits.len(), 5);
    for e in edits.iter() {
        assert!(e.starts_with("Edit this photo, keeping exactly the same person"), "identity first, always: {e}");
        for word in ["grey", "dusky", "ashen", "blue", "sweaty", "cardiac arrest"] {
            assert!(!e.contains(word), "a child's state carries no {word}: {e}");
        }
    }
    assert!(edits.iter().any(|e| e.contains("oxygen mask")) && edits.iter().any(|e| e.contains("nasal cannula")), "{edits:?}");
    drop(edits);

    // An adult on the same tick path keeps the founder's wording.
    let dir2 = world("gentle-adult");
    let man2 = seed_manifest(&dir2, &pool);
    let pak1 = pool.iter().find(|p| p.key == "PAK-1").unwrap();
    let mut ward2 = WardView::parse(STAGING).unwrap();
    put(&mut ward2.patients[0], pak1, "osce-d", 62, man2.entries["PAK-1"].portrait["stable"].clone());
    let door2 = FakeDoor::new(ward2);
    let tools2 = FakeTools::default();
    let r = tick(&config(&dir2, 0, 0), &door2, &tools2);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let edits = tools2.edits.borrow();
    assert!(edits.iter().any(|e| e.contains("ashen grey skin")) && edits.iter().any(|e| e.contains("cardiac arrest")), "{edits:?}");
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
    p.name = Some(ploy.name.clone()); p.country = Some("THA".into()); p.case = Some("osce-c2".into()); p.age = Some(50);
    p.portrait = Some(stable.clone());
    p.portraits = BTreeMap::from([("stable".to_string(), stable.clone()), ("improving".to_string(), man2.entries["THA-0"].portrait["improving"].clone()), ("critical".to_string(), man2.entries["THA-0"].portrait["critical"].clone()),
        ("recovered".to_string(), sha_url(b"r")), ("deteriorating".to_string(), sha_url(b"d")), ("arrest".to_string(), sha_url(b"a"))]);
    let door = FakeDoor::new(ward);
    let anan = pool.iter().find(|p| p.key == "THA-1").unwrap();
    let mut sent = vitals_factory::ledger::Sent::new("osce-a", anan, 70, false, Some(man2.entries["THA-1"].portrait["stable"].clone()), cfg.now, &cfg.ward);
    sent.sex = "m".into();
    let pack = sent.to_pack();
    let id = pack_id(&pack);
    door.push(&Token::new("t".into()), std::slice::from_ref(&pack)).unwrap();
    let mut ledger = Ledger::default();
    ledger.sent.insert(id.clone(), sent);
    ledger.save(&dir.join("factory-ledger.json")).unwrap();
    let r = tick(&cfg, &door, &tools);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let fills = door.fills.borrow();
    let ploy_fill = fills.iter().find(|(pid, _)| *pid == 1789488342).expect("Ploy's siblings were pushed");
    assert_eq!(ploy_fill.1.keys().cloned().collect::<Vec<_>>(), vec!["critical_256", "improving_256", "stable_256"], "the siblings she has on file, and nothing the file lacks");
    assert_eq!(door.queue.borrow()[&id].portrait.get("stable_256").map(String::as_str), Some(sibling(&man2.entries["THA-1"].portrait["stable"]).as_str()), "Anan's waiting pack carries his");
    assert!(Ledger::load(&dir.join("factory-ledger.json")).unwrap().sent[&id].variants_sent, "and the ledger says so, so it is not sent again");
}
