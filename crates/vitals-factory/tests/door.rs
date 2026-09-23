//! The ward's answers, parsed as they are.
//!
//! The factory is a job on another machine with nobody watching it, so everything it learns it
//! learns from the door's own sentences: `queued`, `duplicates`, `rejected`, `depth`, `door:
//! closed`. A client that read those loosely — an "ok" where the door said "rejected", a zero
//! where the door said nothing — would be a factory that looks like it is working.
//!
//! The first fixture is the staging ward's real answer on 16 Sep 2026, before the queue block
//! shipped there; the second is the shape on `cwf/ward`. Both must parse, because the factory
//! meets both.

use std::collections::BTreeMap;
use vitals_factory::door::{push_body, FillReply, Outbound, Pushed, Queue, Token, WardView};
use vitals_web::ward::{Pack, Persona};

const STAGING: &str = include_str!("fixtures/ward-staging-2026-09-16.json");

/// The `cwf/ward` shape: a queue block, packs joined on, a portrait set per patient.
fn on_branch() -> String {
    r#"{
      "as_of_slot": 500000000, "source": "devnet:4Ypy", "readable": true,
      "census": {"admitted": 5, "on_ward": 2, "went_home": 2, "died": 1, "shifts": 9, "keys": 4},
      "week": {"admitted": 5, "on_ward": 2, "went_home": 2, "died": 1, "shifts": 9, "keys": 4, "since_slot": 1},
      "policy": {"beds": 3, "catalogue": ["osce-a", "osce-b"], "difficulty": {"student": 1, "intern": 1, "resident": 0}},
      "queue": {"waiting": 4, "beds": 3, "door": "open", "filled_by": "a ticker"},
      "patients": [
        {"patient_id": 1789488342, "state": "on_shift", "on_shift_since": 1789490000, "bed": 1, "shifts": 2,
         "admitted_slot": 1, "closed_slot": null, "name": "Ploy Siriwattana", "age": 66, "country": "THA",
         "case": "osce-a2", "difficulty": "student", "endemic": false,
         "portrait": "https://storage.googleapis.com/vitals-world-portraits/AAAA.webp",
         "portraits": {"stable": "https://storage.googleapis.com/vitals-world-portraits/AAAA.webp",
                       "improving": "https://storage.googleapis.com/vitals-world-portraits/BBBB.webp"}},
        {"patient_id": 1789490620, "state": "on_ward", "on_shift_since": null, "bed": 2, "shifts": 0,
         "admitted_slot": 2, "closed_slot": null, "name": null, "age": null, "country": null, "case": null,
         "difficulty": null, "endemic": false, "portrait": null, "portraits": {}},
        {"patient_id": 1789400000, "state": "went_home", "on_shift_since": null, "bed": null, "shifts": 3,
         "admitted_slot": 0, "closed_slot": 3, "name": "Budi Santoso", "age": 45, "country": "IDN",
         "case": "osce-b", "difficulty": "intern", "endemic": false, "portrait": null, "portraits": {}}
      ]
    }"#
    .to_string()
}

#[test]
fn the_staging_answer_of_16_sep_parses_with_no_queue_and_nobody_named() {
    let w = WardView::parse(STAGING).expect("the real answer parses");
    assert!(w.readable);
    assert_eq!(w.source, "devnet:4YpyZ2oM8jtxM9GwC61kUsnhMFvWkYatrWVZpiafqypz");
    assert!(w.queue.is_none(), "that build published no queue block, and the client says so rather than reading 0");
    assert_eq!(w.beds, 3);
    assert_eq!(w.catalogue.len(), 16);
    assert_eq!(w.patients.len(), 3);
    assert!(w.patients.iter().all(|p| p.name.is_none() && p.case.is_none() && p.portraits.is_empty()));
    assert!(w.patients.iter().all(|p| p.is_open()), "three on_ward patients are open beds");
    assert_eq!(w.open().count(), 3);
}

#[test]
fn the_branch_answer_parses_with_the_queue_the_packs_and_the_portrait_sets() {
    let w = WardView::parse(&on_branch()).expect("parses");
    let q = w.queue.as_ref().expect("a queue block");
    assert_eq!((q.waiting, q.beds, q.door.as_str()), (4, 3, "open"));
    assert_eq!(w.catalogue, vec!["osce-a", "osce-b"]);

    let ploy = &w.patients[0];
    assert_eq!(ploy.patient_id, 1789488342);
    assert!(ploy.is_open(), "on_shift is an open bed");
    assert_eq!(ploy.name.as_deref(), Some("Ploy Siriwattana"));
    assert_eq!(ploy.age, Some(66));
    assert_eq!(ploy.country.as_deref(), Some("THA"));
    assert_eq!(ploy.case.as_deref(), Some("osce-a2"));
    assert_eq!(ploy.portraits.len(), 2);
    assert!(ploy.portraits.contains_key("improving"));

    let bare = &w.patients[1];
    assert!(bare.is_open() && bare.name.is_none() && bare.portraits.is_empty());

    let home = &w.patients[2];
    assert!(!home.is_open(), "went_home is not a bed");
    assert_eq!(w.open().count(), 2);
}

#[test]
fn an_unreadable_ward_is_not_a_ward_with_nobody_on_it() {
    let body = r#"{"readable": false, "source": "devnet:x", "why": "the RPC timed out", "policy": {"beds": 3, "catalogue": []}}"#;
    let w = WardView::parse(body).expect("the shape parses");
    assert!(!w.readable);
    assert_eq!(w.why.as_deref(), Some("the RPC timed out"));
    assert!(w.patients.is_empty());
}

#[test]
fn the_eternal_entry_is_not_the_ward_and_the_client_says_where_the_ward_is() {
    let body = r#"{"ward": "not on this host", "the_ward_is": "https://world.vitals.academy/api/ward", "why": "..."}"#;
    let err = WardView::parse(body).expect_err("not a ward");
    assert!(err.contains("world.vitals.academy"), "names the ward: {err}");
}

#[test]
fn the_doors_four_numbers_are_read_and_a_closed_door_is_not_a_refusal() {
    let queued = r#"{"queued": 3, "duplicates": 2, "rejected": ["x is not a case this ward serves"], "depth": 21}"#;
    match Pushed::parse(200, queued).expect("parses") {
        Pushed::Queued(q) => {
            assert_eq!((q.queued, q.duplicates, q.depth), (3, 2, 21));
            assert_eq!(q.rejected, vec!["x is not a case this ward serves"]);
        }
        other => panic!("{other:?}"),
    }
    let closed = r#"{"door": "closed", "why": "the ward is not open yet", "queued": 0, "duplicates": 0, "rejected": [], "depth": 0}"#;
    match Pushed::parse(503, closed).expect("parses") {
        Pushed::Closed { why } => assert!(why.contains("not open yet")),
        other => panic!("{other:?}"),
    }
    let refused = r#"{"error": "that is not a page of packs: missing field `case`", "shape": {}}"#;
    match Pushed::parse(200, refused).expect("parses") {
        Pushed::Refused { error } => assert!(error.contains("missing field")),
        other => panic!("{other:?}"),
    }
    let not_here = r#"{"ward": "not on this host", "the_ward_is": "https://world.vitals.academy/api/ward/queue"}"#;
    assert!(Pushed::parse(200, not_here).is_err(), "the Eternal entry is never pushed to");
    assert!(Pushed::parse(500, "<html>upstream error</html>").is_err(), "a non-answer is an error, not zero packs queued");
}

#[test]
fn a_portrait_push_is_answered_in_added_kept_and_rejected() {
    let ok = r#"{"added": 2, "kept": 1, "rejected": [], "states": ["critical", "improving", "stable"]}"#;
    match FillReply::parse(200, ok).expect("parses") {
        FillReply::Filled(f) => {
            assert_eq!((f.added, f.kept), (2, 1));
            assert_eq!(f.states, vec!["critical", "improving", "stable"]);
        }
        other => panic!("{other:?}"),
    }
    let closed = r#"{"door": "closed", "why": "the ward is not open yet, so the factory has nothing to do here either"}"#;
    assert!(matches!(FillReply::parse(503, closed), Ok(FillReply::Closed { .. })));
    let bad = r#"{"error": "that is not a set of portraits: ...", "shape": {}}"#;
    assert!(matches!(FillReply::parse(200, bad), Ok(FillReply::Refused { .. })));
    let not_a_patient = r#"{"error": "that is not a patient id"}"#;
    assert!(matches!(FillReply::parse(404, not_a_patient), Ok(FillReply::Refused { .. })));
}

#[test]
fn a_page_of_packs_is_the_shape_the_door_documents() {
    let pack = Pack {
        difficulty: None,
        case: "osce-a".into(),
        persona: Persona { name: "Anan Thepwong".into(), age: 70, country: "THA".into(), sex: "m".into() },
        portrait: BTreeMap::from([("stable".to_string(), "https://storage.googleapis.com/vitals-world-portraits/a.webp".to_string())]),
        endemic: false,
    };
    let body: serde_json::Value = serde_json::from_str(&push_body(&[Outbound::plain(pack)])).expect("json");
    let packs = body["packs"].as_array().expect("a packs array");
    assert_eq!(packs.len(), 1);
    assert_eq!(packs[0]["case"], "osce-a");
    assert_eq!(packs[0]["persona"]["name"], "Anan Thepwong");
    assert_eq!(packs[0]["persona"]["age"], 70);
    assert_eq!(packs[0]["persona"]["country"], "THA");
    assert_eq!(packs[0]["persona"]["sex"], "m", "since 949a76b the door checks it against the case");
    assert_eq!(packs[0]["portrait"]["stable"], "https://storage.googleapis.com/vitals-world-portraits/a.webp");
    assert_eq!(packs[0]["endemic"], false);
    assert_eq!(body.as_object().unwrap().len(), 1, "packs and nothing else");
}

#[test]
fn the_token_is_never_printed() {
    let t = Token::new("sekrit-value-1234".into());
    let shown = format!("{t:?}");
    assert!(!shown.contains("sekrit"), "debug output leaks the token: {shown}");
    assert!(shown.contains("redacted"));
    assert_eq!(t.bearer(), "Bearer sekrit-value-1234", "the header is the one place it is spelled out");
}

/// The ward's door has three states since 17 Sep: `open` (packs taken, patients admitted),
/// `preview` (packs taken, nobody admitted — the queue fills while the founder looks), and
/// `closed`. The factory sends on the first two and waits on the third.
#[test]
fn preview_takes_packs_like_open_and_closed_does_not() {
    for (door, takes) in [("open", true), ("preview", true), ("closed", false), ("", false), ("shut", false)] {
        let q = Queue { waiting: 4, beds: 3, door: door.into(), waiting_patients: vec![] };
        assert_eq!(q.takes_packs(), takes, "{door:?}");
    }
    let body = r#"{"readable": true, "source": "devnet:x", "policy": {"beds": 3, "catalogue": []}, "queue": {"waiting": 7, "beds": 3, "door": "preview"}, "patients": []}"#;
    let w = WardView::parse(body).unwrap();
    let q = w.queue.as_ref().unwrap();
    assert!(q.takes_packs() && q.waiting == 7, "{q:?}");
}

/// The factory speaks to four routes and no other: it never takes, admits, or touches a bed, so
/// a 409 from a take-style route is never its to see. Held here the way a grep would hold it.
#[test]
fn the_factory_speaks_to_four_routes_and_never_takes() {
    let src = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/door.rs")).unwrap();
    let mut routes: Vec<String> = Vec::new();
    for line in src.lines().filter(|l| !l.trim_start().starts_with("//")) {
        let mut rest = line;
        while let Some(i) = rest.find("/api/ward") {
            let tail = &rest[i..];
            let end = tail.find(['"', '{', ' ', ')', ':']).unwrap_or(tail.len());
            routes.push(tail[..end].trim_end_matches('/').to_string());
            rest = &tail[end..];
        }
    }
    routes.sort();
    routes.dedup();
    assert_eq!(routes, vec!["/api/ward", "/api/ward/cases?placeable=1", "/api/ward/pack", "/api/ward/queue"], "{routes:?}");
    assert!(!src.contains("/take") && !src.contains("/admit"), "no take-style route, ever");
}

/// The queue block lists who is waiting — `waiting_patients`, one row per pack, as the ward's
/// `waiting_rows` writes them: the pack id she is addressed by, who she is, and the one face her
/// pack shows. No patient id, because she is not on the chain: her pictures go through the same
/// route as a bed's (`POST /api/ward/pack/<id>`), which reads a 64-hex id as a waiting pack and
/// digits as a patient. A block from before the rows parses with nobody listed, not as an error.
#[test]
fn the_queue_block_lists_who_is_waiting_by_pack_id() {
    let body = r#"{"readable": true, "source": "devnet:x", "policy": {"beds": 3, "catalogue": []}, "patients": [],
      "queue": {"waiting": 2, "beds": 3, "door": "preview", "filled_by": "a ticker", "waiting_unknown_because": null,
        "waiting_patients": [
          {"pack": "0900aa0fcc205a7bcd8511d542de7942e27511495ee55d99911d6e2985a81f0d", "name": "Eric Habimana", "age": 60, "sex": "m",
           "country": "RWA", "difficulty": "resident", "endemic": false, "case_title": "Cirrhotic patient with confusion and low-grade fever",
           "portrait": "https://storage.googleapis.com/vitals-world-portraits/7133df8cad5160d12f9bf62cbcda3ac16a7cb14219b806b886cb4589115c946a.webp"},
          {"pack": "112bc97c1abe361d4129f87f48ec5edff8120b8266171a98736685b82a805224", "name": "Juma Shabani", "age": 40, "sex": "m",
           "country": "TZA", "difficulty": null, "endemic": true, "case_title": null, "portrait": null}
        ]}}"#;
    let w = WardView::parse(body).expect("parses");
    let q = w.queue.as_ref().expect("a queue block");
    assert_eq!((q.waiting, q.door.as_str()), (2, "preview"));
    assert_eq!(q.waiting_patients.len(), 2);
    let eric = &q.waiting_patients[0];
    assert_eq!(eric.pack, "0900aa0fcc205a7bcd8511d542de7942e27511495ee55d99911d6e2985a81f0d", "the address her pictures go to");
    assert_eq!((eric.name.as_deref(), eric.age, eric.country.as_deref()), (Some("Eric Habimana"), Some(60), Some("RWA")));
    assert!(eric.portrait.as_deref().is_some_and(|u| u.ends_with("115c946a.webp")), "the one face the pack shows");
    let juma = &q.waiting_patients[1];
    assert!(juma.portrait.is_none() && juma.endemic && juma.difficulty.is_none());
    assert_eq!(w.waiting().map(|p| p.pack.as_str()).collect::<Vec<_>>(), vec![eric.pack.as_str(), juma.pack.as_str()], "in the order the ward lists them");
    // Before the rows shipped (17 Sep): a block with no list is nobody listed, and still a queue.
    let older = r#"{"readable": true, "source": "devnet:x", "policy": {"beds": 3, "catalogue": []}, "queue": {"waiting": 7, "beds": 3, "door": "preview"}, "patients": []}"#;
    let w = WardView::parse(older).unwrap();
    let q = w.queue.as_ref().unwrap();
    assert!(q.waiting_patients.is_empty() && q.waiting == 7 && w.waiting().count() == 0, "{q:?}");
}

/// The ward of 22 Sep (b9b72af): the queue block says `beds_kept_free` and no `beds`, and the
/// policy's `beds` is the census rather than a cap. A factory that requires the old field reads
/// nothing — and read nothing for fourteen hours on 22–23 Sep, "queue: missing field `beds`",
/// while the ward drained to one patient. The queue's bed figure is a courtesy for the tick
/// line, never a fact the plan depends on; a ward that stops publishing it must still parse.
#[test]
fn the_ward_of_22_sep_parses_without_beds_in_its_queue_block() {
    let body = on_branch()
        .replace(r#""queue": {"waiting": 4, "beds": 3, "door": "open", "filled_by": "a ticker"}"#,
                 r#""queue": {"waiting": 4, "beds_kept_free": 3, "door": "open", "filled_by": "a ticker on the ward host"}"#);
    assert!(body.contains("beds_kept_free"), "the fixture was rewritten");
    let w = WardView::parse(&body).expect("a queue block without `beds` still parses");
    let q = w.queue.as_ref().expect("a queue block");
    assert_eq!((q.waiting, q.door.as_str()), (4, "open"));
    assert_eq!(q.beds, 3, "the floor the ward keeps free is read from beds_kept_free");
    assert_eq!(w.beds, 3, "the plan's bed figure still comes from the policy");
}

/// **The contract is linked, not copied.**
///
/// `on_branch()` above is a hand-written string that claims to be "the shape on `cwf/ward`". It
/// says `"queue": {"waiting": 4, "beds": 3, …}` and `"policy": {"beds": 3, …}`, and it has said so
/// since 16 Sep. On 22 Sep, 95963be renamed that published field to `beds_kept_free` and changed
/// what `policy.beds` *means* — it is the census now, not the floor. The fixture was not updated,
/// because nothing makes anybody update it. So this file went on passing, describing a ward that
/// had stopped existing, while every factory tick failed at the read for twelve hours: "the ward
/// could not be read, so nothing was built: queue: missing field `beds`". Zero queued on eighty
/// ticks, the production queue drained to one pack, and the census fell from twenty to one.
///
/// A fixture is a contract somebody has to remember, and remembering is the failure we keep having.
/// So this test does not describe the ward's shape. It **asks the ward for it** — `vitals-factory`
/// already depends on `vitals-web`, so the producer's own builders are one call away — and hands
/// what comes back to the consumer's own parser. A rename on either side now fails here, in the
/// ward's gates, with nothing for anybody to refresh.
///
/// Both halves of the 22 Sep failure are asserted, and the second is the one no fixture would have
/// caught: `Queue.beds` was a *loud* failure, a missing required field. `WardView.beds` never fails
/// at all — `parse` builds it from `policy` with a fallback, so a rename there yields a wrong
/// number in silence, and the factory sized its country cap by the census with nothing anywhere
/// saying so. The compile catches the first kind. Only an assertion catches the second.
#[test]
fn the_wards_own_blocks_parse_in_the_factory_that_reads_them() {
    // The door has to be open or the ward publishes no queue at all, which is its own correct
    // behaviour and not the thing under test here.
    std::env::set_var("VITALS_WARD_DOOR", "open");
    let dir = std::env::temp_dir().join(format!("vitals-contract-{}", std::process::id()));
    let store = vitals_web::store::Store::open(dir.clone()).expect("a store to build a ward from");

    // The producer's own words, built by the code that serves them rather than typed here.
    let queue = vitals_web::ward_chain::queue_block(&store);
    let policy = vitals_web::ward::policy(None, None, Some(7));
    let body = serde_json::json!({
        "readable": true,
        "source": "devnet:test",
        "policy": policy,
        "queue": queue,
        "patients": [],
    })
    .to_string();

    let view = WardView::parse(&body).unwrap_or_else(|e| {
        panic!("the factory could not read the ward this repository serves: {e}\n{body}")
    });

    // The loud half: whatever the ward calls the fields inside its queue block, this parses.
    assert!(view.queue.is_some(), "an open ward publishes a queue and the factory reads it: {body}");

    // The silent half. The ward's floor is `beds_kept_free`; `policy.beds` is the census, and the
    // census here is 7 on purpose — larger than any bed count — so a factory that read the wrong
    // field would size itself by 7 and this assertion would say so out loud.
    assert_eq!(
        view.beds,
        vitals_web::ward::BEDS,
        "the factory's bed figure is the ward's floor and not its census; reading `policy.beds` \
         now yields however many patients happen to be on the ward: {policy:#}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
