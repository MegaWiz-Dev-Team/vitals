//! **Silence must never correlate with wrongness.** `docs/RISKS.md` §11.
//!
//! The engine emits exactly the beats a case declares, and a sealed reply strips the `harm:`
//! lines — so an intervention whose author wrote no `beat` comes back one line shorter than its
//! neighbours. When the silent orders are the traps, counting lines in `/api/step` is the answer
//! key. The leak was in the files, not the engine, so the pin is on the files: these tests read
//! every station off disk and fail the moment any station — today's or week nine's — declares an
//! order that can reply with nothing.
//!
//! Scoped to `demo/stations/`: the episodes play unsealed in practice, and EP1's file is frozen
//! by the conformance vectors. The authoring rule these tests enforce is written in
//! `conformance/README.md`, *Authoring rules that are not in the schema*.

use vitals_sce::schema::Effect;
use vitals_sce::{render_beat, Sce, SceState};

fn stations() -> Vec<(String, String)> {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../demo/stations");
    let mut v: Vec<(String, String)> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            let json = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{name}: {e}"));
            (name, json)
        })
        .collect();
    v.sort();
    assert!(v.len() >= 12, "only {} station files found — the shelf moved", v.len());
    v
}

/// The runtime half, measured the way the leak was measured: every station, every order, one
/// fresh run each. A sealed reply keeps every beat that does not start with `harm:` — the same
/// filter the server's view applies — and **no order may come back with nothing**, whether the
/// order was a trap or the oxygen. Harmful and harmless silence rates are both asserted to be
/// zero, which is the only pair of rates that carries no information at all.
#[test]
fn no_order_in_any_station_returns_a_silent_sealed_reply() {
    let (mut harm_n, mut harm_silent) = (0usize, 0usize);
    let (mut ok_n, mut ok_silent) = (0usize, 0usize);
    let mut silent: Vec<String> = Vec::new();
    for (name, json) in stations() {
        let ids: Vec<String> = Sce::from_json(&json)
            .unwrap_or_else(|e| panic!("{name}: {e}"))
            .interventions
            .iter()
            .map(|i| i.id.clone())
            .collect();
        for id in ids {
            let mut st = SceState::new(Sce::from_json(&json).unwrap());
            let sealed_visible = st
                .apply_id(&id)
                .iter()
                .map(render_beat)
                .filter(|b| !b.starts_with("harm:"))
                .count();
            let harmful = !st.harm_events.is_empty();
            if harmful { harm_n += 1 } else { ok_n += 1 };
            if sealed_visible == 0 {
                if harmful { harm_silent += 1 } else { ok_silent += 1 };
                silent.push(format!("{name} {id} (harmful: {harmful})"));
            }
        }
    }
    // Printed so a reviewer can read the two rates this file exists to hold together.
    println!("P(no beat | harmful)  = {harm_silent}/{harm_n}");
    println!("P(no beat | harmless) = {ok_silent}/{ok_n}");
    assert!(
        harm_silent == 0 && ok_silent == 0,
        "these orders reply with no beat, so counting lines in the reply reads as a verdict:\n{}",
        silent.join("\n")
    );
}

/// The authoring half: every intervention carries a `beat` on **every path** through its
/// effects. A beat inside one arm of a branch does not cover the other arm — the candidate who
/// takes the silent arm gets the short reply — so each path must either speak or end the run
/// (a terminal outcome rings the bell, and the bell has its own beat).
#[test]
fn every_intervention_speaks_on_every_path() {
    let mut mute: Vec<String> = Vec::new();
    for (name, json) in stations() {
        let sce = Sce::from_json(&json).unwrap_or_else(|e| panic!("{name}: {e}"));
        for iv in &sce.interventions {
            if has_silent_path(&iv.effects) {
                mute.push(format!("{name} {}", iv.id));
            }
        }
    }
    assert!(
        mute.is_empty(),
        "these interventions have an effect path that emits no beat — the rule is in \
         conformance/README.md, `Authoring rules that are not in the schema`:\n{}",
        mute.join("\n")
    );
}

/// A path is silent when nothing on it emits a beat and nothing on it terminates the run.
fn has_silent_path(effects: &[Effect]) -> bool {
    // Each (beat, outcome) pair is one way execution can flow through `effects` so far.
    let mut paths = vec![(false, false)];
    for e in effects {
        match e {
            Effect::Beat { .. } => paths.iter_mut().for_each(|p| p.0 = true),
            Effect::Outcome { .. } => paths.iter_mut().for_each(|p| p.1 = true),
            Effect::Branch { branch, els } => {
                let mut split: Vec<(bool, bool)> = Vec::new();
                for arm in branch {
                    split.extend(fold(&arm.then));
                }
                split.extend(fold(els));
                let mut next = Vec::new();
                for (b, o) in &paths {
                    for (sb, so) in &split {
                        next.push((b | sb, o | so));
                    }
                }
                paths = next;
            }
            _ => {}
        }
    }
    paths.iter().any(|(beat, outcome)| !beat && !outcome)
}

/// The (beat, outcome) summaries of every path through one effect list.
fn fold(effects: &[Effect]) -> Vec<(bool, bool)> {
    let mut paths = vec![(false, false)];
    for e in effects {
        match e {
            Effect::Beat { .. } => paths.iter_mut().for_each(|p| p.0 = true),
            Effect::Outcome { .. } => paths.iter_mut().for_each(|p| p.1 = true),
            Effect::Branch { branch, els } => {
                let mut split: Vec<(bool, bool)> = Vec::new();
                for arm in branch {
                    split.extend(fold(&arm.then));
                }
                split.extend(fold(els));
                let mut next = Vec::new();
                for (b, o) in &paths {
                    for (sb, so) in &split {
                        next.push((b | sb, o | so));
                    }
                }
                paths = next;
            }
            _ => {}
        }
    }
    paths
}

/// Harm does not only arrive from an order: a trigger can record one off the clock. The rule in
/// `conformance/README.md` walks those too — every `harm` a trigger fires must leave a narrative
/// beat behind it in the same effect list, so the transcript at the bell reads whole and the
/// sealed feed is a strict subsequence of a feed that was never quiet in a marked place.
#[test]
fn a_trigger_that_records_harm_also_narrates() {
    let mut mute: Vec<String> = Vec::new();
    for (name, json) in stations() {
        let sce = Sce::from_json(&json).unwrap_or_else(|e| panic!("{name}: {e}"));
        for t in &sce.triggers {
            if fires(&t.doo, |e| matches!(e, Effect::Harm { .. }))
                && !fires(&t.doo, |e| matches!(e, Effect::Beat { .. }))
            {
                mute.push(format!("{name} {}", t.id));
            }
        }
    }
    assert!(
        mute.is_empty(),
        "these triggers record a harm and narrate nothing:\n{}",
        mute.join("\n")
    );
}

/// Does any effect in the tree (branches included) match?
fn fires(effects: &[Effect], want: impl Fn(&Effect) -> bool + Copy) -> bool {
    effects.iter().any(|e| {
        want(e)
            || match e {
                Effect::Branch { branch, els } => {
                    branch.iter().any(|a| fires(&a.then, want)) || fires(els, want)
                }
                _ => false,
            }
    })
}
