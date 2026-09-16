//! One tick, against a door and tools that are not there.
//!
//! The door here does what the real one does with a page of packs — validates each with the
//! ward's own `validate_pack`, keeps each under its content address, answers the four numbers —
//! and the tools record what they were asked to make instead of making it. What is under test is
//! the tick's own promises: the queue is topped up to depth and no further; a re-run after a
//! crash queues nobody twice; a closed door builds nothing; one patient's faces are completed per
//! tick; a dry run touches nothing; and the token appears in no line of the report.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use vitals_factory::door::{Door, FillReply, Filled, Pushed, Queued, Token, WardView};
use vitals_factory::ledger::Ledger;
use vitals_factory::manifest::{batch_age, Manifest};
use vitals_factory::pool::{read_pool, Person};
use vitals_factory::tick::{tick, Config};
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
        project: "vitals-academy".into(),
        bucket: "vitals-world-portraits".into(),
        model: "gemini-2.5-flash-image".into(),
        dry_run: false,
        seed: 11,
        now: 1_789_500_000,
    }
}

/// The door, as the ward runs it: validate, content-address, answer in numbers.
struct FakeDoor {
    ward: RefCell<WardView>,
    open: bool,
    queue: RefCell<BTreeMap<String, Pack>>,
    pushes: RefCell<Vec<usize>>,
    fills: RefCell<Vec<(u64, BTreeMap<String, String>)>>,
    tokens_seen: RefCell<Vec<String>>,
}

impl FakeDoor {
    fn new(ward: WardView) -> FakeDoor {
        FakeDoor {
            ward: RefCell::new(ward), open: true, queue: RefCell::new(BTreeMap::new()),
            pushes: RefCell::new(vec![]), fills: RefCell::new(vec![]), tokens_seen: RefCell::new(vec![]),
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
            if let Err(why) = validate_pack(p) {
                out.rejected.push(why);
                continue;
            }
            let id = pack_id(p);
            if q.contains_key(&id) {
                out.duplicates += 1;
            } else {
                q.insert(id, p.clone());
                out.queued += 1;
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
            if p.portraits.contains_key(k) { f.kept += 1 } else { p.portraits.insert(k.clone(), v.clone()); f.added += 1 }
        }
        f.states = p.portraits.keys().cloned().collect();
        self.fills.borrow_mut().push((patient_id, set.clone()));
        Ok(FillReply::Filled(f))
    }
}

/// Tools that make nothing and remember everything.
#[derive(Default)]
struct FakeTools {
    token_fetches: RefCell<usize>,
    paints: RefCell<Vec<(String, PathBuf)>>,
    edits: RefCell<Vec<String>>,
    uploads: RefCell<Vec<String>>,
    fetches: RefCell<Vec<String>>,
}

impl Tools for FakeTools {
    fn secret_token(&self, _project: &str) -> Result<Token, String> {
        *self.token_fetches.borrow_mut() += 1;
        Ok(Token::new("sekrit-token-value".into()))
    }
    fn paint(&self, prompt: &str, _seed: u64, out_png: &Path) -> Result<(), String> {
        std::fs::write(out_png, format!("PNG:{prompt}")).unwrap();
        self.paints.borrow_mut().push((prompt.to_string(), out_png.to_path_buf()));
        Ok(())
    }
    fn edit(&self, _project: &str, _model: &str, base: &[u8], _mime: &str, prompt: &str) -> Result<Vec<u8>, String> {
        self.edits.borrow_mut().push(prompt.to_string());
        Ok(format!("PNG-EDIT:{prompt}:{}", base.len()).into_bytes())
    }
    fn webp(&self, png: &[u8], quality: u8) -> Result<Vec<u8>, String> {
        Ok(format!("WEBP{quality}:{}", String::from_utf8_lossy(png)).into_bytes())
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
    assert_eq!(tools.uploads.borrow().len(), paints.len());
    for object in tools.uploads.borrow().iter() {
        assert!(object.len() == 69 && object.ends_with(".webp"), "content-addressed: {object}");
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
    // The report says what happened in the door's words.
    let text = r.lines.join("\n");
    assert!(text.contains("queued") && text.contains("depth"), "{text}");
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
    assert_eq!(tools.uploads.borrow().len(), 5);
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
