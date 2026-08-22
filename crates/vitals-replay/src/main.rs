//! Replay EP1 — "The Last Bite" — two ways, and reduce each run to a leaf.
//!
//! Same scenario, same engine, two different players. One gives adrenaline IM and lays the
//! patient flat; the other stands her up. Anaphylaxis with an empty ventricle does not forgive
//! being stood up, and the automaton knows that without anyone scripting a failure.

use std::path::PathBuf;
use vitals_replay::{hex, leaf, replay, sce_hash, Replay, Step};

fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../conformance/sce-anaphylaxis-ep1.json")
}

fn main() {
    let sce_path = scenario_path();
    let sce_json = std::fs::read_to_string(&sce_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", sce_path.display()));
    let h = sce_hash(&sce_json);

    println!("scenario  ep1-anaphylaxis  ({} bytes)", sce_json.len());
    println!("sce_hash  {}\n", hex(&h));

    // The run that works: recognise it, adrenaline IM early, oxygen, flat, fluids, then keep her
    // for observation rather than sending her home into a biphasic relapse.
    let treated = vec![
        Step::Tick(30.0),
        Step::Do("adrenaline im".into()),
        Step::Do("oxygen".into()),
        Step::Do("supine".into()),
        Step::Tick(60.0),
        Step::Do("normal saline bolus".into()),
        Step::Tick(300.0),
        Step::Do("admit for observation".into()),
        Step::Tick(600.0),
    ];

    // Same outcome, different conduct: adrenaline given, then she is stood up. Anaphylaxis with
    // an empty ventricle does not forgive being stood up. She survives here — but the harm is on
    // the record, and the record is what differs.
    let stood_up = vec![
        Step::Tick(30.0),
        Step::Do("adrenaline im".into()),
        Step::Tick(30.0),
        Step::Do("let her stand up".into()),
        Step::Do("oxygen".into()),
        Step::Do("normal saline bolus".into()),
        Step::Tick(300.0),
        Step::Do("admit for observation".into()),
        Step::Tick(600.0),
    ];

    // The run where nobody reaches for the adrenaline. Antihistamine and steroids are what people
    // give when they have not recognised what they are looking at; neither one saves her.
    let missed = vec![
        Step::Tick(60.0),
        Step::Do("chlorpheniramine".into()),
        Step::Tick(120.0),
        Step::Do("hydrocortisone".into()),
        Step::Tick(300.0),
        Step::Tick(600.0),
    ];

    let a = show("treated", &sce_json, &h, &treated);
    let b = show("stood up", &sce_json, &h, &stood_up);
    let c = show("adrenaline never given", &sce_json, &h, &missed);

    println!("── determinism ─────────────────────────────────────────");
    let again = replay(&sce_json, &treated).expect("replay");
    let a2 = leaf(&h, &treated, &again);
    println!("  same tape replayed   {}", hex(&a2));
    println!("  matches first run    {}", if a2 == a { "yes" } else { "NO — investigate" });
    println!("  three tapes          {}", if a != b && b != c && a != c { "three distinct leaves" } else { "COLLISION — investigate" });
    println!("  same-outcome runs    {}", if a != b { "treated and stood-up differ even where the outcome matched" } else { "COLLIDED" });
    println!("\nthe leaf is what goes onchain. anyone holding the tape and the scenario can");
    println!("recompute it, and nothing in that path asks a model for an opinion.");
}

fn show(label: &str, sce_json: &str, h: &[u8; 32], tape: &[Step]) -> [u8; 32] {
    let r: Replay = replay(sce_json, tape).expect("replay");
    println!("── {label} ─────────────────────────────────────────");
    for s in tape {
        match s {
            Step::Tick(dt) => println!("   ·  +{dt:.0}s"),
            Step::Do(t) => println!("   >  {t}"),
        }
    }
    println!("   beats    {}", if r.beats.is_empty() { "—".into() } else { r.beats.join(" → ") });
    println!("   harm     {}", if r.harm_events.is_empty() { "none".into() } else { r.harm_events.join(", ") });
    println!("   outcome  {}", r.outcome.clone().unwrap_or_else(|| "(no terminal state reached)".into()));
    let l = leaf(h, tape, &r);
    println!("   leaf     {}\n", hex(&l));
    l
}
