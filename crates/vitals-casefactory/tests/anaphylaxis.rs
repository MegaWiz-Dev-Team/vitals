//! Anaphylaxis: adrenaline IM inside the window is the whole case; the antihistamine-first reflex
//! and an IV push of adrenaline are the harms.

mod common;

use vitals_casefactory::{compile, Source};
use vitals_replay::{replay, Step};

const ANA: &str = include_str!("fixtures/synthetic-anaphylaxis.json");

fn pack() -> vitals_casefactory::Pack {
    compile(ANA, Source::of("embla-cases", "test", ANA)).unwrap_or_else(|e| panic!("{}", e.reason))
}

#[test]
fn the_synthetic_reaction_compiles_under_anaphylaxis_and_adrenaline_im_turns_it() {
    let p = pack();
    assert_eq!(p.archetype, "anaphylaxis");
    let turn: Vec<String> = p.sce["states"][0]["transitions"].as_array().unwrap().iter()
        .filter_map(|t| t.get("to_state").map(|_| t["when"].to_string())).collect();
    assert!(turn.iter().any(|w| w.contains("tx_adrenaline_im")), "{turn:?}");
    assert!(p.replay.untreated_death_sec >= 240.0 && p.replay.untreated_death_sec <= 960.0, "{}", p.replay.untreated_death_sec);
    assert_eq!(p.replay.win_outcome, "win_icu");
}

#[test]
fn antihistamine_first_while_the_pressure_falls_is_harm_and_an_iv_push_is_harm() {
    let p = pack();
    let sce = p.sce.to_string();
    let r = replay(&sce, &[Step::Tick(30.0), Step::Act { text: "chlorpheniramine".into(), id: "tx_antihistamine".into() }, Step::Tick(150.0)]).unwrap();
    assert!(r.harm_events.iter().any(|h| h.contains("antihistamine")), "{:?}", r.harm_events);
    let r = replay(&sce, &[Step::Tick(30.0), Step::Act { text: "adrenaline iv push".into(), id: "tx_adrenaline_iv_push".into() }, Step::Tick(1.0)]).unwrap();
    assert!(r.harm_events.iter().any(|h| h.to_lowercase().contains("push")), "{:?}", r.harm_events);
    // and the typed words reach the right intervention: an IV push never lands on the IM order
    let (st, _) = vitals_replay::resume(&sce, &[Step::Tick(1.0), Step::Do("adrenaline iv push".into())]).unwrap();
    assert!(st.events().iter().any(|e| e.kind == "action" && e.text == "tx_adrenaline_iv_push"), "{:?}", st.events());
}

#[test]
fn adrenaline_im_is_a_timed_item_and_the_no_harm_items_price_the_reflexes() {
    let p = pack();
    let items = p.rubric["items"].as_array().unwrap();
    assert!(items.iter().any(|i| i["type"] == "action_by" && i["needle"] == "tx_adrenaline_im"), "{items:?}");
    let harms: Vec<&str> = items.iter().filter(|i| i["type"] == "no_harm").map(|i| i["needle"].as_str().unwrap()).collect();
    assert!(harms.iter().any(|h| h.contains("antihistamine")), "{harms:?}");
}

#[test]
fn the_library_anaphylaxis_cases_with_vitals_compile() {
    let Some(dir) = common::embla_dir() else { return common::skip("embla-cases not present"); };
    for id in ["ddx-anaphylaxis-3", "ddx-anaphylaxis-4"] {
        let Some(json) = common::library_case(&dir, id) else { continue };
        let p = compile(&json, Source::of("embla-cases", "worktree", &json)).unwrap_or_else(|e| panic!("{id}: {}", e.reason));
        assert_eq!(p.archetype, "anaphylaxis", "{id}");
        eprintln!("{id}: dies untreated {} s, wins {} at {} s", p.replay.untreated_death_sec, p.replay.win_outcome, p.replay.win_sec);
    }
}
